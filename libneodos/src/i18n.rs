use core::str;
use crate::syscall;

// ── NLTv2 Binary Format ───────────────────────────────────────────────
//
//   Offset  Size  Field
//   ──────  ────  ─────
//   0       4     Magic: "NLT2"
//   4       2     Version: u16 = 2
//   6       2     HeaderSize: u16
//   8       4     LanguageID: u32 LE
//   12      4     ApplicationID: u32 LE
//   16      4     StringCount: u32 LE  (= N)
//   20      4     Flags: u32 LE
//   24      4     Checksum: u32 LE
//   28      4     Reserved: u32
//   32      8*N   IndexTable: { id: u32 LE, offset: u32 LE }[N]
//   32+8*N  ~     StringData: UTF-8 null-terminated strings
//
//   The index table is sorted by ID for binary search.
//   Only NLTv2 is supported. NLTv1 (string-key) is NOT supported.

// ── Constants ──────────────────────────────────────────────────────────

const MAX_TABLES: usize = 8;
const MAX_NLT_SIZE: usize = 16384;
const MAX_APP_NAME: usize = 32;
const MAX_LANG: usize = 16;

const REG_LOCALE_KEY: &str =
    "\\Registry\\Machine\\System\\CurrentControlSet\\Control\\Locale";
const REG_LANG_VALUE: &str = "Language";
const DEFAULT_LANG: &str = "en-US";

const NLT2_MAGIC: [u8; 4] = [b'N', b'L', b'T', b'2'];
const NLT2_HEADER_SIZE: usize = 32;

// ── Static state ───────────────────────────────────────────────────────

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
fn nlt_str_at(data: &[u8], off: u32) -> Option<&str> {
    let start = off as usize;
    if start >= data.len() {
        return None;
    }
    let end = data[start..].iter().position(|&b| b == 0)?;
    str::from_utf8(&data[start..start + end]).ok()
}

fn validate_nltv2(data: &[u8]) -> Option<u32> {
    if data.len() < NLT2_HEADER_SIZE {
        return None;
    }
    if &data[..4] != NLT2_MAGIC {
        return None;
    }
    let ver = u16::from_le_bytes([data[4], data[5]]);
    if ver != 2 {
        return None;
    }
    let count = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
    let min_size = NLT2_HEADER_SIZE + count as usize * 8;
    if data.len() < min_size {
        return None;
    }
    Some(count)
}

/// Binary search for string ID in NLTv2 data.
/// Returns the string data slice or None.
fn nlt_lookup_id(data: &[u8], id: u32) -> Option<&str> {
    let count = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
    if count == 0 {
        return None;
    }

    let entry_size = 8usize;
    let index_start = NLT2_HEADER_SIZE;

    let mut lo = 0i32;
    let mut hi = count as i32 - 1;

    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        let off = index_start + mid as usize * entry_size;
        let mid_id = u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        if mid_id == id {
            let str_off = u32::from_le_bytes([data[off + 4], data[off + 5], data[off + 6], data[off + 7]]);
            return nlt_str_at(data, str_off);
        } else if mid_id < id {
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }
    None
}

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

fn try_load_table(app: &str, locale: &str) -> Result<(), ()> {
    let (path_buf, path_len) = build_nlt_path(app, locale)?;
    let path_slice = &path_buf[..path_len];
    let path_str = str::from_utf8(path_slice).map_err(|_| ())?;

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

    if validate_nltv2(&buf[..n]).is_none() {
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
/// Reads `Language` from Registry, falls back to `"en-US"`.
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

/// Load the NLTv2 translation file for `app` under the current language.
///
/// Fallback chain:
///   1. `C:\System\Locale\{lang}\{app}.nlt`
///   2. `C:\System\Locale\{lang-only}\{app}.nlt`   (e.g. `es`)
///   3. `C:\System\Locale\en-US\{app}.nlt`
///
/// Only NLTv2 format is supported.
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

/// Look up `id` in all loaded NLTv2 tables.
/// Returns the translated string or `None`.
pub fn i18n_try_get_id(id: u32) -> Option<&'static str> {
    unsafe {
        for i in 0..NLT_COUNT {
            let ptr = core::ptr::addr_of!(NLT_DATA[i]) as *const u8;
            let data = core::slice::from_raw_parts(ptr, NLT_DATA_LENS[i]);
            if let Some(val) = nlt_lookup_id(data, id) {
                return Some(val);
            }
        }
    }
    None
}

/// Look up `id` in all loaded NLTv2 tables.
/// Returns the translation if found, otherwise returns `"?"`.
/// **Never panics.**
pub fn i18n_get_id(id: u32) -> &'static str {
    match i18n_try_get_id(id) {
        Some(s) => s,
        None => "?",
    }
}

/// Unload the NLT table for `app`.
pub fn i18n_unload(app: &str) {
    unsafe {
        let idx = match find_table_idx(app) {
            Some(i) => i,
            None => return,
        };
        // Shift remaining tables down
        for i in idx..NLT_COUNT - 1 {
            NLT_NAMES[i] = NLT_NAMES[i + 1];
            NLT_NAME_LENS[i] = NLT_NAME_LENS[i + 1];
            NLT_DATA[i] = NLT_DATA[i + 1];
            NLT_DATA_LENS[i] = NLT_DATA_LENS[i + 1];
        }
        if NLT_COUNT > 0 {
            NLT_COUNT -= 1;
        }
    }
}

/// Reload all loaded NLT tables from disk (for hot language switching).
pub fn i18n_reload_all() {
    unsafe {
        // Snapshot loaded app names
        let mut apps: [([u8; MAX_APP_NAME], usize); MAX_TABLES] =
            [([0; MAX_APP_NAME], 0); MAX_TABLES];
        let count = NLT_COUNT;
        for i in 0..count {
            apps[i] = (NLT_NAMES[i], NLT_NAME_LENS[i]);
        }
        NLT_COUNT = 0;

        for i in 0..count {
            let ptr = core::ptr::addr_of!(apps[i].0) as *const u8;
            let name_slice = core::slice::from_raw_parts(ptr, apps[i].1);
            if let Ok(app_name) = str::from_utf8(name_slice) {
                // Re-read language from registry
                let _ = i18n_load(app_name);
            }
        }
    }
}

/// Return the active locale string (e.g. `"es-ES"`).
pub fn i18n_active_locale() -> &'static str {
    lang_str()
}

/// Return the number of currently loaded NLT tables (for diagnostics).
pub fn i18n_loaded_count() -> usize {
    unsafe { NLT_COUNT }
}

/// Check if a given app has its NLT table loaded.
pub fn i18n_is_loaded(app: &str) -> bool {
    find_table_idx(app).is_some()
}
