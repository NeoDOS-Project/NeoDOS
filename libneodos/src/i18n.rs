//! Internationalization runtime for NeoDOS (Ring 3).
//!
//! This module is a thin loader/orchestrator on top of [`libnlt`], which owns
//! the NLT binary format (v2 compatibility + v3 features). The old, duplicated
//! NLTv2 parser that used to live here was removed in favour of the shared
//! library so there is a single source of truth.
//!
//! Format reference: `docs/userland/nlt.md`.

use core::str;

use libnlt::{self, lang, plural, region, Header};

use crate::{res, syscall};

// ── Limits ─────────────────────────────────────────────────────────────

const MAX_TABLES: usize = 8;
/// Maximum raw `.nlt` file size (payload + optional region/signature blocks).
const MAX_FILE: usize = 18432;
const MAX_APP_NAME: usize = 32;
const MAX_LANG: usize = 16;

const REG_LOCALE_KEY: &str =
    "\\Registry\\Machine\\System\\CurrentControlSet\\Control\\Locale";
const REG_LANG_VALUE: &str = "Language";
const DEFAULT_LANG: &str = "en-US";

/// When the `i18n-signatures` feature is enabled, reject unsigned tables.
/// Left `false` so development images keep loading unsigned tables; production
/// images flip this to enforce signatures.
#[cfg(feature = "i18n-signatures")]
const REQUIRE_SIGNED: bool = false;

/// Development public key embedded in builds that enable `i18n-signatures`.
#[cfg(feature = "i18n-signatures")]
pub const NLT_DEV_PUBLIC_KEY: [u8; 32] = [
    0xA3, 0x16, 0x89, 0xB7, 0x75, 0x2B, 0x7B, 0xEE, 0x2E, 0xDD, 0x66, 0xA2,
    0xD5, 0x48, 0x5C, 0xBE, 0x04, 0x32, 0x0B, 0x39, 0x53, 0x78, 0x41, 0x69,
    0x37, 0x4E, 0xE5, 0x1C, 0xB9, 0x61, 0x89, 0xF8,
];

// ── Static state ───────────────────────────────────────────────────────

static mut NLT_NAMES: [[u8; MAX_APP_NAME]; MAX_TABLES] = [[0; MAX_APP_NAME]; MAX_TABLES];
static mut NLT_NAME_LENS: [usize; MAX_TABLES] = [0; MAX_TABLES];
static mut NLT_DATA: [[u8; MAX_FILE]; MAX_TABLES] = [[0; MAX_FILE]; MAX_TABLES];
static mut NLT_DATA_LENS: [usize; MAX_TABLES] = [0; MAX_TABLES];
static mut NLT_VERSION: [u16; MAX_TABLES] = [0; MAX_TABLES];
static mut NLT_ENTRIES: [u32; MAX_TABLES] = [0; MAX_TABLES];
static mut NLT_LANG: [u32; MAX_TABLES] = [0; MAX_TABLES];
static mut NLT_REGION_LENS: [usize; MAX_TABLES] = [0; MAX_TABLES];
static mut NLT_COUNT: usize = 0;

static mut LOAD_RAW: [u8; MAX_FILE] = [0; MAX_FILE];
static mut LOAD_SCRATCH: [u8; MAX_FILE] = [0; MAX_FILE];
static mut LOAD_OUT: [u8; MAX_FILE] = [0; MAX_FILE];

static mut LANG_BUF: [u8; MAX_LANG] = [0; MAX_LANG];
static mut LANG_LEN: usize = 0;
static mut INITIALIZED: bool = false;

// ── Language state ─────────────────────────────────────────────────────

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

fn active_lang_id() -> u32 {
    lang::lang_to_id(lang_str())
}

// ── Table storage ──────────────────────────────────────────────────────

unsafe fn table_name(idx: usize) -> &'static str {
    let ptr = core::ptr::addr_of!(NLT_NAMES[idx]) as *const u8;
    let s = core::slice::from_raw_parts(ptr, NLT_NAME_LENS[idx]);
    str::from_utf8(s).unwrap_or("")
}

unsafe fn table_payload(idx: usize) -> &'static [u8] {
    let ptr = core::ptr::addr_of!(NLT_DATA[idx]) as *const u8;
    core::slice::from_raw_parts(ptr, NLT_DATA_LENS[idx])
}

unsafe fn table_region(idx: usize) -> Option<region::Region<'static>> {
    let len = NLT_REGION_LENS[idx];
    if len == 0 {
        return None;
    }
    // The region block is stored immediately after the payload.
    let ptr = (core::ptr::addr_of!(NLT_DATA[idx]) as *const u8).add(NLT_DATA_LENS[idx]);
    region::parse(core::slice::from_raw_parts(ptr, len))
}

fn find_table_idx(app: &str) -> Option<usize> {
    unsafe {
        for i in 0..NLT_COUNT {
            if table_name(i) == app {
                return Some(i);
            }
        }
    }
    None
}

/// Store a decoded table in the next free slot. Returns false when full or the
/// payload does not fit.
fn store_table(app: &str, header: &Header, payload: &[u8], region_raw: Option<&[u8]>) -> bool {
    unsafe {
        let idx = NLT_COUNT;
        if idx >= MAX_TABLES || payload.len() > MAX_FILE {
            return false;
        }
        let app_bytes = app.as_bytes();
        if app_bytes.len() > MAX_APP_NAME {
            return false;
        }
        let region_len = region_raw.map(|r| r.len()).unwrap_or(0);
        if payload.len() + region_len > MAX_FILE {
            return false;
        }

        NLT_DATA[idx][..payload.len()].copy_from_slice(payload);
        if region_len > 0 {
            NLT_DATA[idx][payload.len()..payload.len() + region_len]
                .copy_from_slice(region_raw.unwrap());
        }
        NLT_DATA_LENS[idx] = payload.len();
        NLT_REGION_LENS[idx] = region_len;
        NLT_VERSION[idx] = header.version;
        NLT_ENTRIES[idx] = header.entry_count;
        NLT_LANG[idx] = header.language_id;

        NLT_NAMES[idx][..app_bytes.len()].copy_from_slice(app_bytes);
        NLT_NAME_LENS[idx] = app_bytes.len();
        NLT_COUNT = idx + 1;
    }
    true
}

fn try_load_table(app: &str, locale: &str) -> Result<(), ()> {
    let path_buf = build_nlt_path(app, locale)?;
    let path_str = str::from_utf8(&path_buf.0[..path_buf.1]).map_err(|_| ())?;

    const FS_PREFIX: &str = "\\Global\\FileSystem\\";
    let mut ob_buf = [0u8; 512];
    let ob_bytes = FS_PREFIX.as_bytes();
    let vfs_bytes = path_str.as_bytes();
    let total = ob_bytes.len() + vfs_bytes.len();
    if total >= 510 {
        return Err(());
    }
    ob_buf[..ob_bytes.len()].copy_from_slice(ob_bytes);
    ob_buf[ob_bytes.len()..total].copy_from_slice(vfs_bytes);
    let ob_path = unsafe { str::from_utf8_unchecked(&ob_buf[..total]) };

    let fd = syscall::sys_ob_open(ob_path, syscall::ob_access::READ).map_err(|_| ())?;
    let n = unsafe {
        match syscall::sys_ob_query_info(
            fd,
            syscall::ObInfoClass::ReadContent,
            &mut *core::ptr::addr_of_mut!(LOAD_RAW),
        ) {
            Ok(n) => n,
            Err(_) => {
                let _ = syscall::sys_close(fd);
                return Err(());
            }
        }
    };
    let _ = syscall::sys_close(fd);

    if n > MAX_FILE {
        return Err(());
    }

    let data: &[u8] = unsafe { &LOAD_RAW[..n] };

    // Optional signature verification.
    if !verify_if_signed(data) {
        return Err(());
    }

    let header = libnlt::parse_header(data).ok_or(())?;
    let decoded = unsafe {
        libnlt::decode(
            data,
            &mut *core::ptr::addr_of_mut!(LOAD_SCRATCH),
            &mut *core::ptr::addr_of_mut!(LOAD_OUT),
        )
        .ok_or(())?
    };

    if !store_table(app, &header, decoded.payload, decoded.region_raw) {
        return Err(());
    }
    Ok(())
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

// ── Public API ─────────────────────────────────────────────────────────

/// Initialise the i18n subsystem and read the active locale from the Registry.
pub fn i18n_init() {
    unsafe {
        if INITIALIZED {
            return;
        }
        INITIALIZED = true;
    }
    if let Ok(fd) = syscall::sys_cm_open_key(REG_LOCALE_KEY) {
        let mut buf = [0u8; 128];
        if let Ok(size) = syscall::sys_cm_query_value(fd, REG_LANG_VALUE, &mut buf) {
            if size > 8 {
                let data_len = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
                let end = buf.len().min(8 + data_len);
                let data = &buf[8..end];
                let trimmed = match data.iter().position(|&b| b == 0) {
                    Some(z) => &data[..z],
                    None => data,
                };
                if let Ok(l) = str::from_utf8(trimmed) {
                    if !l.is_empty() {
                        set_lang(l);
                    }
                }
            }
        }
        let _ = syscall::sys_close(fd);
    }
    unsafe {
        if LANG_LEN == 0 {
            set_lang(DEFAULT_LANG);
        }
    }
}

/// Current language tag, e.g. `"es-ES"`.
pub fn i18n_language() -> &'static str {
    lang_str()
}

/// Active locale (alias of [`i18n_language`]).
pub fn i18n_active_locale() -> &'static str {
    lang_str()
}

/// Whether the active locale reads right-to-left.
pub fn i18n_is_rtl() -> bool {
    plural::is_rtl(active_lang_id())
}

/// Load the NLT file for `app` under the active language.
///
/// Fallback chain: `{lang}` → `{lang-only}` → `en-US`. Supports NLTv2 and
/// NLTv3 (compressed / UTF-16 / plural / region / signed).
pub fn i18n_load(app: &str) -> Result<(), ()> {
    if find_table_idx(app).is_some() {
        return Ok(());
    }
    let l = lang_str();
    if try_load_table(app, l).is_ok() {
        return Ok(());
    }
    if let Some(dash) = l.find('-') {
        let lang_only = &l[..dash];
        if lang_only != l && try_load_table(app, lang_only).is_ok() {
            return Ok(());
        }
    }
    if l != DEFAULT_LANG && try_load_table(app, DEFAULT_LANG).is_ok() {
        return Ok(());
    }
    Err(())
}

unsafe fn lookup_in(idx: usize, id: u32) -> Option<&'static str> {
    libnlt::lookup(table_payload(idx), NLT_VERSION[idx], NLT_ENTRIES[idx], id)
}

/// Look up `id` in all loaded tables. Returns `None` when missing.
pub fn i18n_try_get_id(id: u32) -> Option<&'static str> {
    unsafe {
        for i in 0..NLT_COUNT {
            if let Some(s) = lookup_in(i, id) {
                return Some(s);
            }
        }
    }
    None
}

/// Look up `id`, falling back to `"?"`. **Never panics.**
pub fn i18n_get_id(id: u32) -> &'static str {
    i18n_try_get_id(id).unwrap_or("?")
}

/// Look up a plural group and select the form for `n`.
pub fn i18n_plural(id: u32, n: u64) -> &'static str {
    unsafe {
        for i in 0..NLT_COUNT {
            let payload = table_payload(i);
            if let Some((off, flags)) =
                libnlt::find_entry(payload, NLT_VERSION[i], NLT_ENTRIES[i], id)
            {
                if flags & libnlt::ENTRY_FLAG_PLURAL != 0 {
                    return plural::select(payload, off, NLT_LANG[i], n).unwrap_or("?");
                }
                return libnlt::str_at(payload, off).unwrap_or("?");
            }
        }
    }
    "?"
}

/// Regional-format block of the first loaded table that has one.
pub fn i18n_region() -> Option<region::Region<'static>> {
    unsafe {
        for i in 0..NLT_COUNT {
            if let Some(r) = table_region(i) {
                return Some(r);
            }
        }
    }
    None
}

/// Format a translated template identified by `id` with `{0}`, `{0:d}`, …
pub fn i18n_format(id: u32, args: &[&str]) -> &'static str {
    let template = i18n_get_id(id);
    if template == "?" {
        return "?";
    }
    i18n_format_str(template, args)
}

/// Format an arbitrary template with `{0}`, `{0:d}`, `{0:x}`, `{0:X}`, `{{`, `}}`.
pub fn i18n_format_str(template: &str, args: &[&str]) -> &'static str {
    unsafe {
        static mut FORMAT_BUF: [u8; 256] = [0; 256];
        let n = libnlt::format::format_template(
            template,
            args,
            &mut *core::ptr::addr_of_mut!(FORMAT_BUF),
        );
        let s = core::slice::from_raw_parts(core::ptr::addr_of!(FORMAT_BUF) as *const u8, n);
        str::from_utf8(s).unwrap_or("?")
    }
}

/// Reorder a logical line into visual order for the active locale.
pub fn i18n_reorder_visual(input: &str, out: &mut [u8]) -> usize {
    let dir = if i18n_is_rtl() {
        libnlt::bidi::Direction::Rtl
    } else {
        libnlt::bidi::Direction::Ltr
    };
    libnlt::bidi::reorder_visual(input, dir, out)
}

/// Unload the table for `app`.
pub fn i18n_unload(app: &str) {
    unsafe {
        let idx = match find_table_idx(app) {
            Some(i) => i,
            None => return,
        };
        for i in idx..NLT_COUNT - 1 {
            NLT_NAMES[i] = NLT_NAMES[i + 1];
            NLT_NAME_LENS[i] = NLT_NAME_LENS[i + 1];
            NLT_DATA[i] = NLT_DATA[i + 1];
            NLT_DATA_LENS[i] = NLT_DATA_LENS[i + 1];
            NLT_VERSION[i] = NLT_VERSION[i + 1];
            NLT_ENTRIES[i] = NLT_ENTRIES[i + 1];
            NLT_LANG[i] = NLT_LANG[i + 1];
            NLT_REGION_LENS[i] = NLT_REGION_LENS[i + 1];
        }
        if NLT_COUNT > 0 {
            NLT_COUNT -= 1;
        }
    }
}

/// Reload all loaded tables (hot language switch).
pub fn i18n_set_language(tag: &str) {
    set_lang(tag);
}

/// Reload all loaded tables (hot language switch).
pub fn i18n_reload_all() {
    unsafe {
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
                let _ = i18n_load(app_name);
            }
        }
    }
}

/// Number of loaded tables (diagnostics).
pub fn i18n_loaded_count() -> usize {
    unsafe { NLT_COUNT }
}

/// Whether `app` has a loaded table.
pub fn i18n_is_loaded(app: &str) -> bool {
    find_table_idx(app).is_some()
}

// ── Current app name ───────────────────────────────────────────────────

static mut CURRENT_APP: [u8; MAX_APP_NAME] = [0; MAX_APP_NAME];
static mut CURRENT_APP_LEN: usize = 0;

/// Register the current application name for resource resolution.
pub fn i18n_set_app_name(app: &str) {
    unsafe {
        let bytes = app.as_bytes();
        let len = bytes.len().min(MAX_APP_NAME - 1);
        CURRENT_APP[..len].copy_from_slice(&bytes[..len]);
        CURRENT_APP_LEN = len;
    }
}

/// Current application name, if registered.
pub fn current_app_name() -> Option<&'static str> {
    unsafe {
        if CURRENT_APP_LEN == 0 {
            return None;
        }
        let s = core::slice::from_raw_parts(
            core::ptr::addr_of!(CURRENT_APP) as *const u8,
            CURRENT_APP_LEN,
        );
        str::from_utf8(s).ok()
    }
}

// ── Package resources ──────────────────────────────────────────────────

/// Load the NLT file from the current app's own package resources.
pub fn i18n_load_from_package() -> Result<(), ()> {
    let app = current_app_name().ok_or(())?;
    if i18n_is_loaded(app) {
        return Ok(());
    }
    let l = lang_str();

    let mut path = [0u8; 256];
    let mut pos = 0;
    for part in ["locale/", l, "/", app, ".nlt"] {
        let b = part.as_bytes();
        if pos + b.len() > path.len() {
            return i18n_load(app);
        }
        path[pos..pos + b.len()].copy_from_slice(b);
        pos += b.len();
    }
    let locale_path = str::from_utf8(&path[..pos]).map_err(|_| ())?;

    if let Ok(fd) = res::res_open_locale(app, locale_path) {
        let n = res::res_read_all(fd, unsafe { &mut *core::ptr::addr_of_mut!(LOAD_RAW) })
            .map_err(|_| ())?;
        let _ = syscall::sys_close(fd);
        let data = unsafe { &LOAD_RAW[..n] };
        let header = libnlt::parse_header(data).ok_or(())?;
        let decoded = unsafe {
            libnlt::decode(
            data,
            &mut *core::ptr::addr_of_mut!(LOAD_SCRATCH),
            &mut *core::ptr::addr_of_mut!(LOAD_OUT),
        )
        .ok_or(())?
        };
        if store_table(app, &header, decoded.payload, decoded.region_raw) {
            return Ok(());
        }
    }
    i18n_load(app)
}

// ── Available locales ──────────────────────────────────────────────────

/// Semicolon-separated list of locales found under `C:\System\Locale\`.
pub fn i18n_available_locales() -> &'static str {
    const LOCALE_PATH: &str = "\\Global\\FileSystem\\C:\\System\\Locale";

    let mut result = [0u8; 256];
    let mut result_len = 0usize;

    if let Ok(fd) = syscall::sys_ob_open(LOCALE_PATH, syscall::ob_access::READ) {
        let mut entries: [syscall::ObEnumEntry; 16] = core::array::from_fn(|_| {
            syscall::ObEnumEntry {
                id: 0,
                obj_type: 0,
                name: [0; 32],
                mode: 0,
                _pad: [0; 2],
                size: 0,
            }
        });
        loop {
            match syscall::sys_ob_enum(fd, &mut entries) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    for i in 0..count.min(entries.len()) {
                        if result_len > 0 && result_len < 255 {
                            result[result_len] = b';';
                            result_len += 1;
                        }
                        let name = entries[i].name_str();
                        let remaining = 255 - result_len;
                        let to_copy = name.len().min(remaining);
                        result[result_len..result_len + to_copy]
                            .copy_from_slice(name[..to_copy].as_bytes());
                        result_len += to_copy;
                    }
                }
            }
        }
        let _ = syscall::sys_close(fd);
    }

    unsafe {
        static mut LOCALE_RESULT: [u8; 256] = [0; 256];
        static mut LOCALE_RESULT_LEN: usize = 0;
        LOCALE_RESULT[..result_len].copy_from_slice(&result[..result_len]);
        LOCALE_RESULT_LEN = result_len;
        if result_len == 0 {
            return "";
        }
        let s = core::slice::from_raw_parts(
            core::ptr::addr_of!(LOCALE_RESULT) as *const u8,
            LOCALE_RESULT_LEN,
        );
        str::from_utf8(s).unwrap_or("")
    }
}

/// Alias of [`i18n_available_locales`].
pub fn available_locales() -> &'static str {
    i18n_available_locales()
}

// ── Signature verification (optional) ──────────────────────────────────

/// Verify the signature of a raw NLT file against `public_key`.
#[cfg(feature = "i18n-signatures")]
pub fn i18n_verify_signature(data: &[u8], public_key: &[u8; 32]) -> bool {
    let header = match libnlt::parse_header(data) {
        Some(h) => h,
        None => return false,
    };
    if !header.is_signed() || header.signature_size != 64 {
        return false;
    }
    let sig_start = header.signature_offset as usize;
    let sig_end = sig_start + 64;
    if sig_end > data.len() {
        return false;
    }
    let mut sig = [0u8; 64];
    sig.copy_from_slice(&data[sig_start..sig_end]);
    // The signature covers everything before the signature block.
    libnlt::signature::verify(public_key, &data[..sig_start], &sig)
}

#[cfg(feature = "i18n-signatures")]
fn verify_if_signed(data: &[u8]) -> bool {
    match libnlt::parse_header(data) {
        Some(h) if h.is_signed() => i18n_verify_signature(data, &NLT_DEV_PUBLIC_KEY),
        _ => !REQUIRE_SIGNED,
    }
}

#[cfg(not(feature = "i18n-signatures"))]
fn verify_if_signed(_data: &[u8]) -> bool {
    true
}
