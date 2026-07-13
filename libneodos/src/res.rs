use core::str;
use crate::syscall;
use crate::syscall::ObInfoClass;

const RES_PATH_PREFIX: &str = "\\Global\\FileSystem\\C:\\Programs\\";

fn build_res_path(app: &str, path: &str) -> Result<([u8; 256], usize), ()> {
    let prefix = RES_PATH_PREFIX.as_bytes();
    let res = b"/resources/";
    let total = prefix.len() + app.len() + res.len() + path.len();
    if total > 255 || app.len() > 128 || path.len() > 128 {
        return Err(());
    }
    let mut buf = [0u8; 256];
    let mut pos = 0;
    buf[pos..pos + prefix.len()].copy_from_slice(prefix);
    pos += prefix.len();
    buf[pos..pos + app.len()].copy_from_slice(app.as_bytes());
    pos += app.len();
    buf[pos..pos + res.len()].copy_from_slice(res);
    pos += res.len();
    buf[pos..pos + path.len()].copy_from_slice(path.as_bytes());
    pos += path.len();
    Ok((buf, pos))
}

fn path_to_str(buf: &[u8; 256], len: usize) -> &str {
    str::from_utf8(&buf[..len]).unwrap_or("")
}

/// Open a resource file for the current application.
/// Path is relative to `<app>/resources/`.
pub fn res_open(path: &str) -> Result<u8, i64> {
    if let Some(app) = crate::i18n::current_app_name() {
        return res_open_app(app, path);
    }
    Err(-3)
}

/// Open a resource for a specific application.
pub fn res_open_app(app: &str, path: &str) -> Result<u8, i64> {
    let (buf, len) = build_res_path(app, path).map_err(|_| -3i64)?;
    let path_str = path_to_str(&buf, len);
    if path_str.is_empty() {
        return Err(-3);
    }
    syscall::sys_ob_open(path_str, syscall::ob_access::READ)
}

/// Open a localized resource.
/// Falls back: {lang} → {lang-only} → en-US → unlocalized.
pub fn res_open_locale(app: &str, path: &str) -> Result<u8, i64> {
    let lang = crate::i18n::i18n_language();

    // Try exact locale: locale/{lang}/{path}
    let locale_prefix = b"locale/";
    let sep = b"/";
    let total = locale_prefix.len() + lang.len() + sep.len() + path.len();
    if total <= 255 {
        let mut locale_buf = [0u8; 256];
        let mut pos = 0;
        locale_buf[pos..pos + locale_prefix.len()].copy_from_slice(locale_prefix);
        pos += locale_prefix.len();
        locale_buf[pos..pos + lang.len()].copy_from_slice(lang.as_bytes());
        pos += lang.len();
        locale_buf[pos..pos + sep.len()].copy_from_slice(sep);
        pos += sep.len();
        locale_buf[pos..pos + path.len()].copy_from_slice(path.as_bytes());
        pos += path.len();
        let locale_path = path_to_str(&locale_buf, pos);

        if let Ok(fd) = res_open_app(app, locale_path) {
            return Ok(fd);
        }
    }

    // Try language-only: locale/{lang-only}/{path}
    if let Some(dash) = lang.find('-') {
        let lang_only = &lang[..dash];
        let total2 = locale_prefix.len() + lang_only.len() + sep.len() + path.len();
        if total2 <= 255 {
            let mut buf2 = [0u8; 256];
            let mut pos2 = 0;
            buf2[pos2..pos2 + locale_prefix.len()].copy_from_slice(locale_prefix);
            pos2 += locale_prefix.len();
            buf2[pos2..pos2 + lang_only.len()].copy_from_slice(lang_only.as_bytes());
            pos2 += lang_only.len();
            buf2[pos2..pos2 + sep.len()].copy_from_slice(sep);
            pos2 += sep.len();
            buf2[pos2..pos2 + path.len()].copy_from_slice(path.as_bytes());
            pos2 += path.len();
            let lang_only_path = path_to_str(&buf2, pos2);

            if let Ok(fd) = res_open_app(app, lang_only_path) {
                return Ok(fd);
            }
        }
    }

    // Try en-US fallback
    if lang != "en-US" {
        let en_prefix = b"locale/en-US/";
        let total3 = en_prefix.len() + path.len();
        if total3 <= 255 {
            let mut buf3 = [0u8; 256];
            buf3[..en_prefix.len()].copy_from_slice(en_prefix);
            buf3[en_prefix.len()..total3].copy_from_slice(path.as_bytes());
            let en_path = path_to_str(&buf3, total3);

            if let Ok(fd) = res_open_app(app, en_path) {
                return Ok(fd);
            }
        }
    }

    // Try unlocalized fallback (path as-is)
    res_open_app(app, path)
}

/// Read resource content into buffer.
pub fn res_read(fd: u8, buf: &mut [u8]) -> Result<usize, i64> {
    syscall::sys_read(fd, buf)
}

/// Get resource file info (size) via ObQueryInfo.
pub fn res_size(fd: u8) -> Result<u64, i64> {
    let mut info_buf = [0u8; 32];
    syscall::sys_ob_query_info(fd, ObInfoClass::File, &mut info_buf)?;
    // File info struct: mode(2) | size(4) ...
    let size = u32::from_le_bytes([info_buf[2], info_buf[3], info_buf[4], info_buf[5]]);
    Ok(size as u64)
}

/// Read entire resource into a caller-provided buffer.
pub fn res_read_all(fd: u8, buf: &mut [u8]) -> Result<usize, i64> {
    let to_read = buf.len();
    let mut total = 0;
    while total < to_read {
        match res_read(fd, &mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) => return Err(e),
        }
    }
    Ok(total)
}
