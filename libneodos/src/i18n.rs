use core::str;
use crate::syscall;

// ── Constants ──────────────────────────────────────────────────────────

const MAX_TABLES: usize = 8;
const MAX_NLT_SIZE: usize = 8192;
const MAX_APP_NAME: usize = 32;
const MAX_LANG: usize = 16;

const REG_LOCALE_KEY: &str =
    "\\Registry\\Machine\\System\\CurrentControlSet\\Control\\Locale";
const REG_LANG_VALUE: &str = "Language";
const DEFAULT_LANG: &str = "en-US";

// ── Static state ───────────────────────────────────────────────────────
// Loaded NLT tables are stored as parallel arrays.  Immutable after load.
// SAFETY: accessed during i18n_init/load (single-threaded at app start)
// and during i18n_get/try_get (potentially multi-threaded, but only reads).

static mut NLT_NAMES: [[u8; MAX_APP_NAME]; MAX_TABLES] = [[0; MAX_APP_NAME]; MAX_TABLES];
static mut NLT_NAME_LENS: [usize; MAX_TABLES] = [0; MAX_TABLES];
static mut NLT_DATA: [[u8; MAX_NLT_SIZE]; MAX_TABLES] = [[0; MAX_NLT_SIZE]; MAX_TABLES];
static mut NLT_DATA_LENS: [usize; MAX_TABLES] = [0; MAX_TABLES];
static mut NLT_COUNT: usize = 0;

static mut LANG_BUF: [u8; MAX_LANG] = [0; MAX_LANG];
static mut LANG_LEN: usize = 0;
static mut INITIALIZED: bool = false;

// ── Internal helpers ───────────────────────────────────────────────────

fn lang_str() -> &'static str {
    unsafe {
        if LANG_LEN == 0 {
            return DEFAULT_LANG;
        }
        let s = core::slice::from_raw_parts(core::ptr::addr_of!(LANG_BUF) as *const u8, LANG_LEN);
        str::from_utf8(s).unwrap_or(DEFAULT_LANG)
    }
}

fn set_lang(s: &str) {
    unsafe {
        let bytes = s.as_bytes();
        let len = bytes.len().min(MAX_LANG - 1);
        LANG_BUF[..len].copy_from_slice(&bytes[..len]);
        LANG_LEN = len;
    }
}

/// Return a null-terminated string at offset `off` within `data`.
fn nlt_str_at<'a>(data: &'a [u8], off: u32) -> Option<&'a str> {
    let start = off as usize;
    if start >= data.len() {
        return None;
    }
    let end = data[start..].iter().position(|&b| b == 0)?;
    str::from_utf8(&data[start..start + end]).ok()
}

/// NLT format:
///   [0..4)   magic  "NLT\0"
///   [4..8)   version u32 LE  (must be 1)
///   [8..12)  count   u32 LE  (= N)
///   [12..12+4N)  key_off[0..N)  u32 LE each
///   [12+4N..12+8N)  val_off[0..N)  u32 LE each
///   [12+8N..)     key strings, val strings (null-terminated)
///
/// Returns `Some(&str)` pointing into `data` (zero-copy), or `None`.
fn nlt_lookup<'a>(data: &'a [u8], key: &str) -> Option<&'a str> {
    if data.len() < 12 {
        return None;
    }
    if &data[..4] != b"NLT\0" {
        return None;
    }
    let count = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;
    let keys_off = 12usize;
    let vals_off = 12usize + count * 4;

    for i in 0..count {
        let kp = keys_off + i * 4;
        let vp = vals_off + i * 4;
        if kp + 4 > data.len() || vp + 4 > data.len() {
            break;
        }
        let ko = u32::from_le_bytes([data[kp], data[kp + 1], data[kp + 2], data[kp + 3]]);
        let vo = u32::from_le_bytes([data[vp], data[vp + 1], data[vp + 2], data[vp + 3]]);
        if let Some(k) = nlt_str_at(data, ko) {
            if k == key {
                return nlt_str_at(data, vo);
            }
        }
    }
    None
}

/// Locate a table by app name.  Returns its index or `None`.
fn find_table_idx(app: &str) -> Option<usize> {
    unsafe {
        for i in 0..NLT_COUNT {
            let ptr = core::ptr::addr_of!(NLT_NAMES[i]) as *const u8;
            let name_slice = core::slice::from_raw_parts(ptr, NLT_NAME_LENS[i]);
            if let Ok(name) = str::from_utf8(name_slice) {
                if name == app {
                    return Some(i);
                }
            }
        }
    }
    None
}

/// Build `C:\System\Locale\{locale}\{app}.nlt` into a fixed buffer.
/// Returns (buffer, length) on success.
fn build_nlt_path(app: &str, locale: &str) -> Result<([u8; 256], usize), ()> {
    let prefix = b"C:\\System\\Locale\\";
    let sep = b"\\";
    let ext = b".nlt";
    let total = prefix.len() + locale.len() + sep.len() + app.len() + ext.len();
    if total > 255 || locale.len() > 128 || app.len() > 128 {
        return Err(());
    }
    let mut buf = [0u8; 256];
    let mut pos = 0;
    buf[pos..pos + prefix.len()].copy_from_slice(prefix);
    pos += prefix.len();
    buf[pos..pos + locale.len()].copy_from_slice(locale.as_bytes());
    pos += locale.len();
    buf[pos..pos + sep.len()].copy_from_slice(sep);
    pos += sep.len();
    buf[pos..pos + app.len()].copy_from_slice(app.as_bytes());
    pos += app.len();
    buf[pos..pos + ext.len()].copy_from_slice(ext);
    pos += ext.len();
    Ok((buf, pos))
}

/// Try to load one NLT file for `app` in `locale`.  Validates magic + version.
/// Uses the Ob API directly (sys_ob_open + sys_ob_query_info).
fn try_load_table(app: &str, locale: &str) -> Result<(), ()> {
    let (path_buf, path_len) = build_nlt_path(app, locale)?;
    let path_slice = &path_buf[..path_len];
    let path_str = str::from_utf8(path_slice).map_err(|_| ())?;

    // Build Ob namespace path: \Global\FileSystem\C:\...
    const FS_PREFIX: &str = "\\Global\\FileSystem\\";
    let mut ob_buf = [0u8; 512];
    let ob_bytes = FS_PREFIX.as_bytes();
    let vfs_bytes = path_str.as_bytes();
    let total = ob_bytes.len() + vfs_bytes.len();
    if total >= 510 { return Err(()); }
    ob_buf[..ob_bytes.len()].copy_from_slice(ob_bytes);
    ob_buf[ob_bytes.len()..total].copy_from_slice(vfs_bytes);
    let ob_path = unsafe { core::str::from_utf8_unchecked(&ob_buf[..total]) };

    let fd = syscall::sys_ob_open(ob_path, syscall::ob_access::READ).map_err(|_| ())?;

    let mut buf = [0u8; MAX_NLT_SIZE];
    let n = syscall::sys_ob_query_info(fd, syscall::ObInfoClass::ReadContent, &mut buf)
        .map_err(|_| ())?;

    let _ = syscall::sys_close(fd);

    if n < 12 {
        return Err(());
    }
    if &buf[..4] != b"NLT\0" {
        return Err(());
    }
    let ver = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    if ver != 1 {
        return Err(());
    }
    unsafe {
        let idx = NLT_COUNT;
        if idx >= MAX_TABLES {
            return Err(());
        }
        let app_bytes = app.as_bytes();
        NLT_NAMES[idx][..app_bytes.len()].copy_from_slice(app_bytes);
        NLT_NAME_LENS[idx] = app_bytes.len();
        NLT_DATA[idx][..n].copy_from_slice(&buf[..n]);
        NLT_DATA_LENS[idx] = n;
        NLT_COUNT = idx + 1;
    }
    Ok(())
}

// ── Public API ─────────────────────────────────────────────────────────

/// Initialise the i18n subsystem.
///
/// Reads `Language` from
/// `\Registry\Machine\System\CurrentControlSet\Control\Locale`.
/// Falls back to `"en-US"` if the value is absent or unreadable.
/// Safe to call multiple times — subsequent calls are no-ops.
pub fn i18n_init() {
    unsafe {
        if INITIALIZED {
            return;
        }
        INITIALIZED = true;
    }
    match syscall::sys_cm_open_key(REG_LOCALE_KEY) {
        Ok(fd) => {
            let mut buf = [0u8; 128];
            if let Ok(size) = syscall::sys_cm_query_value(fd, REG_LANG_VALUE, &mut buf) {
                if size > 8 {
                    let data_len =
                        u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
                    let end = buf.len().min(8 + data_len);
                    let data = &buf[8..end];
                    let trimmed = match data.iter().position(|&b| b == 0) {
                        Some(z) => &data[..z],
                        None => data,
                    };
                    if let Ok(lang) = str::from_utf8(trimmed) {
                        if !lang.is_empty() {
                            set_lang(lang);
                        }
                    }
                }
            }
            let _ = syscall::sys_close(fd);
        }
        Err(_) => {}
    }
    unsafe {
        if LANG_LEN == 0 {
            set_lang(DEFAULT_LANG);
        }
    }
}

/// Return the current language string (e.g. `"es-ES"`).
pub fn i18n_language() -> &'static str {
    lang_str()
}

/// Load the NLT translation file for `app` under the current language.
///
/// Fallback chain (tries files in order):
///   1. `C:\System\Locale\{lang}\{app}.nlt`
///   2. `C:\System\Locale\{lang-only}\{app}.nlt`   (e.g. `es`)
///   3. `C:\System\Locale\en-US\{app}.nlt`
///
/// If no file is found, `tr!()` will return keys untranslated (never panics).
/// Idempotent: subsequent calls for the same app are no-ops.
pub fn i18n_load(app: &str) -> Result<(), ()> {
    if find_table_idx(app).is_some() {
        return Ok(());
    }
    let lang = lang_str();

    if try_load_table(app, lang).is_ok() {
        return Ok(());
    }
    if let Some(dash) = lang.find('-') {
        let lang_only = &lang[..dash];
        if lang_only != lang && try_load_table(app, lang_only).is_ok() {
            return Ok(());
        }
    }
    if lang != "en-US" {
        if try_load_table(app, "en-US").is_ok() {
            return Ok(());
        }
    }
    Err(())
}

/// Look up `key` in all loaded NLT tables.
///
/// Returns `Some(translation)` or `None` (no match in any table).
pub fn try_get(key: &str) -> Option<&'static str> {
    unsafe {
        for i in 0..NLT_COUNT {
            let ptr = core::ptr::addr_of!(NLT_DATA[i]) as *const u8;
            let data = core::slice::from_raw_parts(ptr, NLT_DATA_LENS[i]);
            if let Some(val) = nlt_lookup(data, key) {
                return Some(val);
            }
        }
    }
    None
}

/// Look up `key` in all loaded NLT tables.
///
/// Returns the translation if found, otherwise returns `key` unchanged.
/// **Never panics.**
pub fn i18n_get<'a>(key: &'a str) -> &'a str {
    match try_get(key) {
        Some(t) => t,
        None => key,
    }
}
