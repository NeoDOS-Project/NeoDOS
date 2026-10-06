//! Ob namespace — path normalization and name-key helpers.

use alloc::string::ToString;
use alloc::vec::Vec;

pub(super) const MAX_NAME_LEN: usize = 24;
pub(super) const MAX_PATH_LEN: usize = 255;
pub(super) const MAX_SYMLINK_HOPS: u32 = 10;

pub const OB_INVALID_PATH: &str = "OB_INVALID_PATH";
pub const OB_PATH_TOO_LONG: &str = "OB_PATH_TOO_LONG";
pub const OB_NAME_TOO_LONG: &str = "OB_NAME_TOO_LONG";
pub const OB_NOT_FOUND: &str = "OB_NOT_FOUND";
pub const OB_ALREADY_EXISTS: &str = "OB_ALREADY_EXISTS";
pub const OB_CANNOT_CREATE_ROOT: &str = "OB_CANNOT_CREATE_ROOT";
pub const OB_SAME_NAME: &str = "OB_SAME_NAME";
pub const OB_SYMLINK_LOOP: &str = "OB_SYMLINK_LOOP";
pub const OB_PROTECTED: &str = "OB_PROTECTED";

pub(super) fn name_to_key(name: &str) -> [u8; MAX_NAME_LEN] {
    let mut key = [0u8; MAX_NAME_LEN];
    let bytes = name.as_bytes();
    let len = bytes.len().min(MAX_NAME_LEN - 1);
    for i in 0..len {
        key[i] = bytes[i].to_ascii_lowercase();
    }
    key
}

pub(super) fn key_to_str(key: &[u8; MAX_NAME_LEN]) -> &str {
    let len = key.iter().position(|&b| b == 0).unwrap_or(MAX_NAME_LEN);
    core::str::from_utf8(&key[..len]).unwrap_or("<?>")
}

pub fn normalize_path(path: &str) -> alloc::string::String {
    if path.is_empty() {
        return "\\".to_string();
    }
    let mut result = alloc::string::String::new();
    result.push('\\');
    let has_drive = path.len() >= 2 && path.as_bytes()[1] == b':';
    let path_body = if has_drive { &path[2..] } else { path };
    let trimmed = path_body.trim_start_matches('\\');
    if trimmed.is_empty() {
        return result;
    }
    let parts: Vec<&str> = trimmed.split('\\').collect();
    let mut out_parts: Vec<&str> = Vec::new();
    for part in parts {
        match part {
            "" | "." => continue,
            ".." => {
                out_parts.pop();
            }
            _ => {
                out_parts.push(part);
            }
        }
    }
    if has_drive {
        let drive_letter = path.as_bytes()[0].to_ascii_uppercase();
        result.push(drive_letter as char);
        result.push(':');
    }
    for (i, p) in out_parts.iter().enumerate() {
        if i > 0 || has_drive {
            result.push('\\');
        }
        result.push_str(p);
    }
    result
}
