//! Internationalization runtime for NeoDOS (Ring 3).
//!
//! This module is a thin loader/orchestrator on top of [`libnlt`], which owns
//! the NLT binary format (v2 compatibility + v3 features). The old, duplicated
//! NLTv2 parser that used to live here was removed in favour of the shared
//! library so there is a single source of truth.
//!
//! Format reference: `docs/userland/nlt.md`.

use core::str;
use core::sync::atomic::{AtomicUsize, Ordering};

use libnlt::{self, lang, plural, region, Header};

use crate::{res, syscall};

// ── Limits ─────────────────────────────────────────────────────────────

const MAX_TABLES: usize = 8;
/// Maximum raw `.nlt` file size (payload + optional region/signature blocks).
const MAX_FILE: usize = 18432;
const MAX_APP_NAME: usize = 32;
const MAX_LANG: usize = 16;

/// Why a locale table could not be loaded (#576).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadError {
    /// The `.nlt` file / app table was not found.
    NotFound,
    /// The file exists but is not a valid NLT table.
    BadFormat,
    /// The decoded payload exceeds [`MAX_FILE`].
    TooBig,
    /// All [`MAX_TABLES`] slots are occupied.
    TableFull,
    /// Signature verification failed (or a signature was required and absent).
    Unsigned,
    /// Underlying syscall error.
    Io(i64),
}

impl LoadError {
    /// Short human-readable description.
    pub fn as_str(self) -> &'static str {
        match self {
            LoadError::NotFound => "translation table not found",
            LoadError::BadFormat => "invalid NLT table",
            LoadError::TooBig => "translation table too large",
            LoadError::TableFull => "no free translation-table slot",
            LoadError::Unsigned => "translation table signature rejected",
            LoadError::Io(_) => "I/O error loading translation table",
        }
    }
}

/// Runtime table-pool capacity `(max_tables, max_bytes_per_table)` (#576).
///
/// The pool is static; exceeding either limit yields
/// [`LoadError::TableFull`] / [`LoadError::TooBig`].
pub const fn i18n_capacity() -> (usize, usize) {
    (MAX_TABLES, MAX_FILE)
}

const REG_LOCALE_KEY: &str =
    "\\Registry\\Machine\\System\\CurrentControlSet\\Control\\Locale";
const REG_LANG_VALUE: &str = "Language";
const DEFAULT_LANG: &str = "en-US";

/// When the `i18n-signatures` feature is enabled, reject unsigned tables if
/// `i18n-require-signed` (which implies `i18n-signatures`) is also enabled.
/// Off by default so development images keep loading unsigned tables;
/// production images flip `i18n-require-signed` on (#582).
#[cfg(feature = "i18n-signatures")]
const REQUIRE_SIGNED: bool = cfg!(feature = "i18n-require-signed");

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

fn try_load_table(app: &str, locale: &str) -> Result<(), LoadError> {
    let path_buf = build_nlt_path(app, locale).map_err(|_| LoadError::NotFound)?;
    let path_str =
        str::from_utf8(&path_buf.0[..path_buf.1]).map_err(|_| LoadError::BadFormat)?;

    const FS_PREFIX: &str = "\\Global\\FileSystem\\";
    let mut ob_buf = [0u8; 512];
    let ob_bytes = FS_PREFIX.as_bytes();
    let vfs_bytes = path_str.as_bytes();
    let total = ob_bytes.len() + vfs_bytes.len();
    if total >= 510 {
        return Err(LoadError::BadFormat);
    }
    ob_buf[..ob_bytes.len()].copy_from_slice(ob_bytes);
    ob_buf[ob_bytes.len()..total].copy_from_slice(vfs_bytes);
    let ob_path = unsafe { str::from_utf8_unchecked(&ob_buf[..total]) };

    let fd = syscall::sys_ob_open(ob_path, syscall::ob_access::READ).map_err(LoadError::Io)?;
    let n = match syscall::sys_ob_query_info(
        fd,
        syscall::ObInfoClass::ReadContent,
        unsafe { &mut *core::ptr::addr_of_mut!(LOAD_RAW) },
    ) {
        Ok(n) => n,
        Err(e) => {
            let _ = syscall::sys_close(fd);
            return Err(LoadError::Io(e));
        }
    };
    let _ = syscall::sys_close(fd);

    if n > MAX_FILE {
        return Err(LoadError::TooBig);
    }

    let data: &[u8] = unsafe { &LOAD_RAW[..n] };

    // Optional signature verification.
    if !verify_if_signed(data) {
        return Err(LoadError::Unsigned);
    }

    let header = libnlt::parse_header(data).ok_or(LoadError::BadFormat)?;
    let decoded = unsafe {
        libnlt::decode(
            data,
            &mut *core::ptr::addr_of_mut!(LOAD_SCRATCH),
            &mut *core::ptr::addr_of_mut!(LOAD_OUT),
        )
        .ok_or(LoadError::BadFormat)?
    };

    if !store_table(app, &header, decoded.payload, decoded.region_raw) {
        return Err(LoadError::TableFull);
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
/// Negotiation chain (#580): exact tag → language-only → canonical tag for the
/// language → `en-US`. Supports NLTv2 and NLTv3 (compressed / UTF-16 / plural /
/// region / signed).
pub fn i18n_load(app: &str) -> Result<(), LoadError> {
    if find_table_idx(app).is_some() {
        return Ok(());
    }
    let (chain, n) = locale_chain(lang_str());
    let mut last = LoadError::NotFound;
    let mut i = 0usize;
    while i < n {
        match try_load_table(app, chain[i]) {
            Ok(()) => return Ok(()),
            Err(e) => last = e,
        }
        i += 1;
    }
    Err(last)
}

/// Fallback locales for `tag`, most specific first, deduplicated (#580).
///
/// `"es-MX"` → `["es-MX", "es", "es-ES", "en-US"]`.
pub fn locale_chain(tag: &str) -> ([&str; 4], usize) {
    libnlt::lang::fallback_chain(tag)
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

/// Look up `id`, falling back to a `#<id>` marker. **Never panics.**
///
/// See `missing_marker` for the marker buffer's lifetime contract.
pub fn i18n_get_id(id: u32) -> &'static str {
    match i18n_try_get_id(id) {
        Some(s) => s,
        None => missing_marker(id),
    }
}

// ── Missing-key markers (#574) ─────────────────────────────────────────
//
// A missing key returns `#<id>` from a small ring of static buffers, so the
// failure is identifiable without grepping ids by hand. The ring index is
// atomic; a returned marker stays valid for the next ~16 misses.

const MARKER_SLOTS: usize = 16;
const MARKER_LEN: usize = 16;

static mut MARKER_BUF: [[u8; MARKER_LEN]; MARKER_SLOTS] = [[0; MARKER_LEN]; MARKER_SLOTS];
static MARKER_NEXT: AtomicUsize = AtomicUsize::new(0);

/// Write `#<id>` into `out`; returns the number of bytes written.
fn write_marker(id: u32, out: &mut [u8]) -> usize {
    let mut tmp = [0u8; 12];
    let mut n = 0usize;
    let mut v = id;
    if v == 0 {
        tmp[0] = b'0';
        n = 1;
    } else {
        while v > 0 && n < tmp.len() {
            tmp[n] = b'0' + (v % 10) as u8;
            v /= 10;
            n += 1;
        }
    }
    let mut pos = 0usize;
    if pos < out.len() {
        out[pos] = b'#';
        pos += 1;
    }
    let mut i = n;
    while i > 0 {
        i -= 1;
        if pos < out.len() {
            out[pos] = tmp[i];
            pos += 1;
        }
    }
    pos
}

/// Return a `#<id>` marker from the shared ring.
fn missing_marker(id: u32) -> &'static str {
    let slot = MARKER_NEXT.fetch_add(1, Ordering::Relaxed) % MARKER_SLOTS;
    unsafe {
        let buf = &mut *core::ptr::addr_of_mut!(MARKER_BUF[slot]);
        let n = write_marker(id, &mut buf[..]);
        str::from_utf8(&buf[..n]).unwrap_or("?")
    }
}

// ── Symbolic key resolution (#578) ─────────────────────────────────────

/// Resolve a symbolic key (`"<app>.<NAME>"`) to its numeric id.
///
/// `const`-evaluable: used by `tr!` inside a `const`, an unknown key is a
/// **compile-time error**.
pub const fn key_id(key: &str) -> u32 {
    libnlt::keys::require(crate::i18n_keymap::NLT_KEYMAP, key)
}

/// Runtime variant of [`key_id`]; `None` for an unknown key.
pub fn try_key_id(key: &str) -> Option<u32> {
    libnlt::keys::find(crate::i18n_keymap::NLT_KEYMAP, key)
}

/// Look up a plural group and select the form for `n`.
///
/// Falls back to a `#<id>` marker when the group is absent.
pub fn i18n_plural(id: u32, n: u64) -> &'static str {
    unsafe {
        for i in 0..NLT_COUNT {
            let payload = table_payload(i);
            if let Some((off, flags)) =
                libnlt::find_entry(payload, NLT_VERSION[i], NLT_ENTRIES[i], id)
            {
                if flags & libnlt::ENTRY_FLAG_PLURAL != 0 {
                    return plural::select(payload, off, NLT_LANG[i], n)
                        .unwrap_or_else(|| missing_marker(id));
                }
                return libnlt::str_at(payload, off).unwrap_or_else(|| missing_marker(id));
            }
        }
    }
    missing_marker(id)
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

// ── Formatting (#575) ──────────────────────────────────────────────────

const FMT_SLOTS: usize = 8;
const FMT_LEN: usize = 256;
static mut FMT_BUF: [[u8; FMT_LEN]; FMT_SLOTS] = [[0; FMT_LEN]; FMT_SLOTS];
static FMT_NEXT: AtomicUsize = AtomicUsize::new(0);

/// Format a translated template into a **caller-owned** buffer (#575).
///
/// Fully reentrant/thread-safe: unlike [`i18n_format`] there is no shared
/// buffer, so the result can be held while formatting others.
pub fn i18n_format_into<'a>(id: u32, args: &[&str], out: &'a mut [u8]) -> &'a str {
    match i18n_try_get_id(id) {
        Some(tmpl) => i18n_format_str_into(tmpl, args, out),
        None => {
            let n = write_marker(id, out);
            str::from_utf8(&out[..n]).unwrap_or("?")
        }
    }
}

/// Format an arbitrary template into a caller-owned buffer (#575).
pub fn i18n_format_str_into<'a>(template: &str, args: &[&str], out: &'a mut [u8]) -> &'a str {
    let n = libnlt::format::format_template(template, args, out);
    str::from_utf8(&out[..n]).unwrap_or("?")
}

fn fmt_ring_slot() -> &'static mut [u8] {
    let slot = FMT_NEXT.fetch_add(1, Ordering::Relaxed) % FMT_SLOTS;
    unsafe {
        let p = core::ptr::addr_of_mut!(FMT_BUF[slot]) as *mut u8;
        core::slice::from_raw_parts_mut(p, FMT_LEN)
    }
}

/// Format a translated template identified by `id` with `{0}`, `{0:d}`, …
///
/// Ring-backed for backwards compatibility; prefer [`i18n_format_into`] when
/// the result must outlive the next call.
pub fn i18n_format(id: u32, args: &[&str]) -> &'static str {
    match i18n_try_get_id(id) {
        Some(template) => i18n_format_str(template, args),
        None => missing_marker(id),
    }
}

/// Format an arbitrary template (`{0}`, `{0:d}`, `{0:x}`, `{0:X}`, `{{`, `}}`).
///
/// Ring-backed; see [`i18n_format_str_into`] for a reentrant form.
pub fn i18n_format_str(template: &str, args: &[&str]) -> &'static str {
    let out = fmt_ring_slot();
    let n = libnlt::format::format_template(template, args, out);
    str::from_utf8(&out[..n]).unwrap_or("?")
}

/// Plural selection + formatting in one call (#579).
pub fn i18n_plural_format(id: u32, n: u64, args: &[&str]) -> &'static str {
    i18n_format_str(i18n_plural(id, n), args)
}

// ── Regional formatting (#579) ─────────────────────────────────────────

fn write_i64(value: i64, out: &mut [u8]) -> usize {
    let neg = value < 0;
    let mut mag = if neg {
        (value as i128).unsigned_abs() as u64
    } else {
        value as u64
    };
    let mut tmp = [0u8; 24];
    let mut n = 0usize;
    if mag == 0 {
        tmp[0] = b'0';
        n = 1;
    } else {
        while mag > 0 {
            tmp[n] = b'0' + (mag % 10) as u8;
            mag /= 10;
            n += 1;
        }
    }
    let mut pos = 0usize;
    if neg && pos < out.len() {
        out[pos] = b'-';
        pos += 1;
    }
    let mut i = n;
    while i > 0 {
        i -= 1;
        if pos < out.len() {
            out[pos] = tmp[i];
            pos += 1;
        }
    }
    pos
}

fn put_str(out: &mut [u8], pos: usize, s: &str) -> usize {
    let mut p = pos;
    for &b in s.as_bytes() {
        if p < out.len() {
            out[p] = b;
            p += 1;
        }
    }
    p
}

fn put_u8_pad2(out: &mut [u8], pos: usize, v: u8) -> usize {
    let b = [b'0' + (v / 10) % 10, b'0' + v % 10];
    put_str(out, pos, str::from_utf8(&b).unwrap_or("00"))
}

fn put_i32_pad4(out: &mut [u8], pos: usize, v: i32) -> usize {
    let v = if v < 0 { 0u32 } else { v as u32 };
    let digits = [
        b'0' + ((v / 1000) % 10) as u8,
        b'0' + ((v / 100) % 10) as u8,
        b'0' + ((v / 10) % 10) as u8,
        b'0' + (v % 10) as u8,
    ];
    put_str(out, pos, core::str::from_utf8(&digits).unwrap_or("0000"))
}

/// Format an integer with the active region's grouping (#579).
///
/// Falls back to plain decimal when no region block is loaded.
pub fn i18n_format_number(value: i64, out: &mut [u8]) -> &str {
    match i18n_region() {
        Some(r) => {
            let n = region::format_number(value, &r, out);
            str::from_utf8(&out[..n]).unwrap_or("?")
        }
        None => {
            let n = write_i64(value, out);
            str::from_utf8(&out[..n]).unwrap_or("?")
        }
    }
}

/// Format a currency amount (in minor units) using the active region (#579).
pub fn i18n_format_currency(minor_units: i64, out: &mut [u8]) -> &str {
    match i18n_region() {
        Some(r) => {
            let n = region::format_currency(minor_units, 2, &r, out);
            str::from_utf8(&out[..n]).unwrap_or("?")
        }
        None => {
            let mut pos = write_i64(minor_units / 100, out);
            pos = put_str(out, pos, ".");
            pos = put_u8_pad2(out, pos, (minor_units.abs() % 100) as u8);
            let n = pos;
            str::from_utf8(&out[..n]).unwrap_or("?")
        }
    }
}

/// Format a date using the active region's short/long pattern (#579).
pub fn i18n_format_date(year: i32, month: u8, day: u8, long: bool, out: &mut [u8]) -> &str {
    match i18n_region() {
        Some(r) => {
            let n = region::format_date(year, month, day, long, &r, out);
            str::from_utf8(&out[..n]).unwrap_or("?")
        }
        None => {
            let mut pos = put_i32_pad4(out, 0, year);
            pos = put_str(out, pos, "-");
            pos = put_u8_pad2(out, pos, month);
            pos = put_str(out, pos, "-");
            pos = put_u8_pad2(out, pos, day);
            str::from_utf8(&out[..pos]).unwrap_or("?")
        }
    }
}

/// Format a time using the active region's time pattern (#579).
pub fn i18n_format_time(hour: u8, minute: u8, second: u8, out: &mut [u8]) -> &str {
    match i18n_region() {
        Some(r) => {
            let n = region::format_time(hour, minute, second, &r, out);
            str::from_utf8(&out[..n]).unwrap_or("?")
        }
        None => {
            let mut pos = put_u8_pad2(out, 0, hour);
            pos = put_str(out, pos, ":");
            pos = put_u8_pad2(out, pos, minute);
            pos = put_str(out, pos, ":");
            pos = put_u8_pad2(out, pos, second);
            str::from_utf8(&out[..pos]).unwrap_or("?")
        }
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
pub fn i18n_load_from_package() -> Result<(), LoadError> {
    let app = current_app_name().ok_or(LoadError::NotFound)?;
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
    let locale_path = str::from_utf8(&path[..pos]).map_err(|_| LoadError::BadFormat)?;

    if let Ok(fd) = res::res_open_locale(app, locale_path) {
        let n = res::res_read_all(fd, unsafe { &mut *core::ptr::addr_of_mut!(LOAD_RAW) })
            .map_err(|_| LoadError::BadFormat)?;
        let _ = syscall::sys_close(fd);
        let data = unsafe { &LOAD_RAW[..n] };
        let header = libnlt::parse_header(data).ok_or(LoadError::BadFormat)?;
        let decoded = unsafe {
            libnlt::decode(
                data,
                &mut *core::ptr::addr_of_mut!(LOAD_SCRATCH),
                &mut *core::ptr::addr_of_mut!(LOAD_OUT),
            )
            .ok_or(LoadError::BadFormat)?
        };
        if store_table(app, &header, decoded.payload, decoded.region_raw) {
            return Ok(());
        }
    }
    i18n_load(app)
}

// ── Available locales (#577) ───────────────────────────────────────────

/// Maximum locales retained.
pub const MAX_LOCALES: usize = 32;
/// Maximum bytes stored per locale tag.
pub const MAX_LOCALE_LEN: usize = 16;

static mut LOCALE_NAMES: [[u8; MAX_LOCALE_LEN]; MAX_LOCALES] =
    [[0; MAX_LOCALE_LEN]; MAX_LOCALES];
static mut LOCALE_NAME_LENS: [usize; MAX_LOCALES] = [0; MAX_LOCALES];
static mut LOCALE_COUNT: usize = 0;
static mut LOCALE_SCANNED: bool = false;

/// Enumerate `C:\System\Locale\` once.
///
/// `sys_ob_enum` is **stateless** — it always returns the first `max` entries,
/// so a single pass with a large-enough buffer is the only correct usage.
/// Looping over it expecting a continuation cursor hangs (this was the bug).
fn scan_locales_once() {
    const LOCALE_PATH: &str = "\\Global\\FileSystem\\C:\\System\\Locale";
    unsafe {
        if LOCALE_SCANNED {
            return;
        }
        LOCALE_SCANNED = true;
        LOCALE_COUNT = 0;

        let mut entries: [syscall::ObEnumEntry; 64] = core::array::from_fn(|_| {
            syscall::ObEnumEntry {
                id: 0,
                obj_type: 0,
                name: [0; 32],
                mode: 0,
                _pad: [0; 2],
                size: 0,
            }
        });
        if let Ok(fd) = syscall::sys_ob_open(LOCALE_PATH, syscall::ob_access::READ) {
            if let Ok(count) = syscall::sys_ob_enum(fd, &mut entries) {
                for e in entries.iter().take(count.min(MAX_LOCALES)) {
                    let name = e.name_str();
                    let len = name.len().min(MAX_LOCALE_LEN - 1);
                    let idx = LOCALE_COUNT;
                    LOCALE_NAMES[idx][..len].copy_from_slice(&name.as_bytes()[..len]);
                    LOCALE_NAME_LENS[idx] = len;
                    LOCALE_COUNT += 1;
                }
            }
            let _ = syscall::sys_close(fd);
        }
    }
}

/// Force the next locale query to re-scan the filesystem.
pub fn i18n_rescan_locales() {
    unsafe {
        LOCALE_SCANNED = false;
        LOCALE_COUNT = 0;
    }
}

/// Number of installed locales found under `C:\System\Locale\`.
pub fn i18n_locale_count() -> usize {
    scan_locales_once();
    unsafe { LOCALE_COUNT }
}

/// Locale tag at `index`, or `None` when out of range.
pub fn i18n_locale_at(index: usize) -> Option<&'static str> {
    scan_locales_once();
    unsafe {
        if index >= LOCALE_COUNT {
            return None;
        }
        let s = core::slice::from_raw_parts(
            core::ptr::addr_of!(LOCALE_NAMES[index]) as *const u8,
            LOCALE_NAME_LENS[index],
        );
        str::from_utf8(s).ok()
    }
}

/// Semicolon-separated list of installed locales.
///
/// Compatibility helper; prefer [`i18n_locale_count`]/[`i18n_locale_at`].
pub fn i18n_available_locales() -> &'static str {
    const CAP: usize = 256;
    static mut LOCALE_RESULT: [u8; CAP] = [0; CAP];
    static mut LOCALE_RESULT_LEN: usize = 0;

    scan_locales_once();
    unsafe {
        let mut pos = 0usize;
        let mut i = 0usize;
        while i < LOCALE_COUNT {
            if i > 0 && pos < CAP - 1 {
                LOCALE_RESULT[pos] = b';';
                pos += 1;
            }
            let len = LOCALE_NAME_LENS[i];
            let copy = len.min(CAP - pos);
            LOCALE_RESULT[pos..pos + copy].copy_from_slice(&LOCALE_NAMES[i][..copy]);
            pos += copy;
            i += 1;
        }
        LOCALE_RESULT_LEN = pos;
        if pos == 0 {
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

// ── Translator seam (#581) ─────────────────────────────────────────────

/// UI-agnostic translation seam: resolves a numeric message id.
///
/// The runtime provides [`I18nTranslator`]; host tests and alternate UIs can
/// implement their own table without linking the kernel-facing code.
pub trait Translator {
    /// Resolve `id`; must not panic (return a marker on miss).
    fn tr(&self, id: u32) -> &str;
}

/// Runtime-backed [`Translator`] over [`i18n_get_id`].
pub struct I18nTranslator;

impl Translator for I18nTranslator {
    fn tr(&self, id: u32) -> &str {
        i18n_get_id(id)
    }
}

/// Symbolic-key translation for code that prefers `<app>.<NAME>` keys (#578).
pub struct I18nKeyTranslator;

impl I18nKeyTranslator {
    /// Resolve a symbolic key; returns a marker on miss.
    pub fn tr_key(&self, key: &str) -> &'static str {
        match try_key_id(key) {
            Some(id) => i18n_get_id(id),
            None => "?",
        }
    }
}
