//! DOS-style drive letters mapped to filesystem instances.
//!
//! The filesystem implementation must not encode drive letters; only this module
//! translates `C:\path` → `(FsInstanceId, internal "/path")`.
//!
//! ## Example (kernel bootstrap)
//!
//! ```ignore
//! let mut dm = DriveManager::new();
//! dm.mount('C', FsInstanceId::PRIMARY)?;
//! let (fs_id, path) = dm.resolve_dos_path("C:\\SYSTEM\\CONFIG.SYS")?;
//! // Dispatch to NeoDosFs instance for fs_id using `path` as UNIX-like logical path.
//! ```
/// Kernel-owned identifier for a mounted filesystem backend (ATA volume, RAM disk, …).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FsInstanceId(pub u8);

impl FsInstanceId {
    pub const PRIMARY: FsInstanceId = FsInstanceId(0);
    pub const FAT32_ESP: FsInstanceId = FsInstanceId(1);
}

/// One drive letter assignment to a filesystem instance.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Drive {
    pub letter: u8,
    pub fs: FsInstanceId,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DriveManagerError {
    InvalidDriveLetter,
    InvalidPath,
    DriveNotMounted,
    DriveAlreadyMounted,
    PathTooLong,
}

/// Normalized path inside the filesystem (leading `'/'`, backslashes converted).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct InternalPath {
    buf: [u8; InternalPath::CAPACITY],
    len: usize,
}

impl InternalPath {
    pub const CAPACITY: usize = 260;

    pub fn empty() -> Self {
        InternalPath {
            buf: [0; Self::CAPACITY],
            len: 0,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    pub fn as_str(&self) -> Result<&str, DriveManagerError> {
        core::str::from_utf8(self.as_bytes()).map_err(|_| DriveManagerError::InvalidPath)
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.len
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn push_byte(&mut self, b: u8) -> Result<(), DriveManagerError> {
        if self.len >= Self::CAPACITY {
            return Err(DriveManagerError::PathTooLong);
        }
        self.buf[self.len] = b;
        self.len += 1;
        Ok(())
    }

    fn push_slash_normalized(&mut self, b: u8) -> Result<(), DriveManagerError> {
        let b = if b == b'\\' { b'/' } else { b };
        if b != b'/' {
            return self.push_byte(b);
        }
        if self.len == 0 || self.buf[self.len - 1] != b'/' {
            self.push_byte(b'/')?;
        }
        Ok(())
    }

    fn trim_trailing_slashes(&mut self) {
        while self.len > 1 && self.buf[self.len - 1] == b'/' {
            self.len -= 1;
        }
    }
}

/// Fixed-size drive letter → filesystem mapping (no heap).
#[derive(Clone, Copy)]
pub struct DriveManager {
    /// For each slot `i`, letter is `b'A' + i` when `Some`.
    slots: [Option<Drive>; DriveManager::LETTER_COUNT],
}

impl DriveManager {
    const LETTER_COUNT: usize = 26;

    pub fn new() -> Self {
        DriveManager {
            slots: [None; Self::LETTER_COUNT],
        }
    }

    fn letter_index(ch: char) -> Result<usize, DriveManagerError> {
        match ch {
            'A'..='Z' => Ok((ch as u8 - b'A') as usize),
            'a'..='z' => Ok((ch as u8 - b'a') as usize),
            _ => Err(DriveManagerError::InvalidDriveLetter),
        }
    }

    fn letter_byte(idx: usize) -> Result<u8, DriveManagerError> {
        if idx >= Self::LETTER_COUNT {
            return Err(DriveManagerError::InvalidDriveLetter);
        }
        Ok(b'A' + idx as u8)
    }

    /// Register `letter` → `fs`. Fails if letter invalid or already used.
    pub fn mount(&mut self, letter: char, fs: FsInstanceId) -> Result<(), DriveManagerError> {
        let idx = Self::letter_index(letter)?;
        if self.slots[idx].is_some() {
            return Err(DriveManagerError::DriveAlreadyMounted);
        }
        let upper = Self::letter_byte(idx)?;
        self.slots[idx] = Some(Drive {
            letter: upper,
            fs,
        });
        Ok(())
    }

    #[allow(dead_code)]
    pub fn unmount(&mut self, letter: char) -> Result<(), DriveManagerError> {
        let idx = Self::letter_index(letter)?;
        if self.slots[idx].is_none() {
            return Err(DriveManagerError::DriveNotMounted);
        }
        self.slots[idx] = None;
        Ok(())
    }

    pub fn set_primary(&mut self, letter: char) -> Result<(), DriveManagerError> {
        let idx = Self::letter_index(letter)?;
        
        let drive = match self.slots[idx] {
            Some(d) => d,
            None => {
                self.slots[idx] = Some(Drive {
                    letter: b'A' + idx as u8,
                    fs: FsInstanceId::PRIMARY,
                });
                return Ok(());
            }
        };
        
        for i in 0..Self::LETTER_COUNT {
            if let Some(d) = &mut self.slots[i] {
                if d.fs == FsInstanceId::PRIMARY {
                    d.fs = drive.fs;
                    break;
                }
            }
        }
        self.slots[idx] = Some(Drive {
            letter: drive.letter,
            fs: FsInstanceId::PRIMARY,
        });
        Ok(())
    }

    pub fn get(&self, letter: char) -> Option<Drive> {
        let idx = Self::letter_index(letter).ok()?;
        self.slots[idx]
    }

    /// Resolve `C:\folder\file.txt` → `(FsInstanceId, "/folder/file.txt")`.
    pub fn resolve_dos_path(&self, input: &str) -> Result<(FsInstanceId, InternalPath), DriveManagerError> {
        let bytes = input.as_bytes();
        if bytes.len() < 2 {
            return Err(DriveManagerError::InvalidPath);
        }

        let letter = match input.chars().next() {
            Some(c) => c,
            None => return Err(DriveManagerError::InvalidPath),
        };
        let idx = Self::letter_index(letter)?;

        if bytes.get(1) != Some(&b':') {
            return Err(DriveManagerError::InvalidPath);
        }

        let drive = self.slots[idx].ok_or(DriveManagerError::DriveNotMounted)?;

        for &b in bytes.iter().skip(2) {
            if b >= 0x80 {
                return Err(DriveManagerError::InvalidPath);
            }
        }

        let mut path = InternalPath::empty();
        path.push_byte(b'/')?;

        let mut i = 2usize;
        while i < bytes.len() && (bytes[i] == b'\\' || bytes[i] == b'/') {
            i += 1;
        }

        while i < bytes.len() {
            let b = bytes[i];
            if b == b'\\' || b == b'/' {
                path.push_slash_normalized(b)?;
                i += 1;
                while i < bytes.len() && (bytes[i] == b'\\' || bytes[i] == b'/') {
                    i += 1;
                }
                continue;
            }

            if b < 0x20 || b == 0x7f {
                return Err(DriveManagerError::InvalidPath);
            }

            path.push_byte(b)?;
            i += 1;
        }

        path.trim_trailing_slashes();

        Ok((drive.fs, path))
    }
}
