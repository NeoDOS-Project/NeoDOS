//! Registry (Cm) client — an RAII [`RegistryKey`] over the kernel Cm syscalls.
//!
//! Replaces the ad-hoc `[type][len][data]` parsing that was duplicated across
//! the network config, DNS, keyboard and i18n consumers. The handle is closed
//! automatically on drop.
//!
//! ```ignore
//! let key = RegistryKey::open(r"\Registry\Machine\System\CurrentControlSet\Control\Library")?;
//! let mut buf = [0u8; 260];
//! let n = key.query_string("math", &mut buf);
//! ```

use crate::syscall::{self, REG_DWORD, REG_SZ};

/// Registry (Cm) error — the negative errno returned by the kernel Cm syscall.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryError(pub i64);

impl From<i64> for RegistryError {
    fn from(code: i64) -> Self {
        RegistryError(code)
    }
}

impl RegistryError {
    /// Raw negative errno.
    pub fn code(self) -> i64 {
        self.0
    }
}

/// Owning handle to an open Registry key. Closes the key on drop.
pub struct RegistryKey {
    fd: u8,
}

impl RegistryKey {
    /// Open an existing key by full path.
    pub fn open(path: &str) -> Result<Self, i64> {
        Ok(RegistryKey {
            fd: syscall::sys_cm_open_key(path)?,
        })
    }

    /// Open `path`, creating it (and any missing ancestors) when absent.
    ///
    /// The Cm API creates a subkey given a parent handle, so ancestors are
    /// ensured recursively; the terminal component is created under its parent.
    pub fn create_tree(path: &str) -> Result<Self, i64> {
        if let Ok(k) = Self::open(path) {
            return Ok(k);
        }
        let bytes = path.as_bytes();
        let sep = match bytes.iter().rposition(|&b| b == b'\\') {
            Some(i) => i,
            None => return Err(-1),
        };
        let (parent, leaf) = (&path[..sep], &path[sep + 1..]);
        if parent.is_empty() || leaf.is_empty() {
            return Err(-1);
        }
        let pk = Self::create_tree(parent)?;
        let _ = syscall::sys_cm_create_key(pk.fd, leaf);
        Self::open(path)
    }

    /// Read a `REG_DWORD` value. `None` if missing or a different type.
    pub fn query_dword(&self, name: &str) -> Option<u32> {
        let mut buf = [0u8; 12];
        let total = syscall::sys_cm_query_value(self.fd, name, &mut buf).ok()?;
        if total < 12 {
            return None;
        }
        let value_type = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        if value_type != REG_DWORD {
            return None;
        }
        Some(u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]))
    }

    /// Read a `REG_SZ` value into `buf`; returns the length (0 if missing).
    pub fn query_string(&self, name: &str, buf: &mut [u8]) -> usize {
        let mut raw = [0u8; 260];
        let total = match syscall::sys_cm_query_value(self.fd, name, &mut raw) {
            Ok(n) => n,
            Err(_) => return 0,
        };
        if total < 8 {
            return 0;
        }
        let data_len = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]) as usize;
        let available = total.saturating_sub(8).min(raw.len() - 8);
        let src = &raw[8..8 + data_len.min(available)];
        let end = src.iter().position(|&b| b == 0).unwrap_or(src.len());
        let n = end.min(buf.len());
        buf[..n].copy_from_slice(&src[..n]);
        n
    }

    /// Write a `REG_DWORD` value.
    pub fn set_dword(&self, name: &str, value: u32) -> Result<(), i64> {
        syscall::sys_cm_set_value(self.fd, name, REG_DWORD, &value.to_le_bytes())
    }

    /// Write a `REG_SZ` value (raw bytes, no NUL added).
    pub fn set_string(&self, name: &str, value: &[u8]) -> Result<(), i64> {
        syscall::sys_cm_set_value(self.fd, name, REG_SZ, value)
    }

    /// Delete a subkey `name` under this key.
    pub fn delete_key(&self, name: &str) -> Result<(), RegistryError> {
        syscall::sys_ob_set_info(
            self.fd,
            syscall::ObSetInfoClass::RegistryDeleteKey,
            name.as_bytes(),
        )
        .map_err(RegistryError)
    }

    /// Delete a value `name` under this key.
    pub fn delete_value(&self, name: &str) -> Result<(), RegistryError> {
        syscall::sys_ob_set_info(
            self.fd,
            syscall::ObSetInfoClass::RegistryDeleteValue,
            name.as_bytes(),
        )
        .map_err(RegistryError)
    }

    /// Read a `REG_MULTI_SZ` value (NUL-separated strings) into `buf`; returns
    /// the raw byte length (internal NULs preserved, final terminator dropped).
    pub fn query_multi_string(&self, name: &str, buf: &mut [u8]) -> usize {
        let mut raw = [0u8; 260];
        let total = match syscall::sys_cm_query_value(self.fd, name, &mut raw) {
            Ok(n) => n,
            Err(_) => return 0,
        };
        if total < 8 {
            return 0;
        }
        let data_len = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]) as usize;
        let avail = total.saturating_sub(8).min(raw.len() - 8);
        let src = &raw[8..8 + data_len.min(avail)];
        let end = src
            .iter()
            .rposition(|&b| b != 0)
            .map(|i| i + 1)
            .unwrap_or(0);
        let n = end.min(buf.len());
        buf[..n].copy_from_slice(&src[..n]);
        n
    }

    /// Flush this key's hive to disk.
    pub fn flush(&self) -> Result<(), i64> {
        syscall::sys_cm_flush_key(self.fd)
    }
}

impl Drop for RegistryKey {
    fn drop(&mut self) {
        let _ = syscall::sys_close(self.fd);
    }
}
