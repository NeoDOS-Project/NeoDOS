use crate::export;
use crate::syscall;

const OB_FS_PREFIX: &[u8] = b"\\Global\\FileSystem\\";

pub struct File {
    fd: u8,
}

impl File {
    /// Open a file by VFS path (e.g. `C:\\System\\Locale\\en-US\\app.nlt`).
    /// Internally converts to an Ob namespace path `\Global\FileSystem\...`.
    pub fn open(path: &str) -> Result<File, i64> {
        let bytes = path.as_bytes();
        let total = OB_FS_PREFIX.len() + bytes.len();
        if total >= 255 { return Err(syscall::EINVAL); }
        let mut buf = [0u8; 256];
        buf[..OB_FS_PREFIX.len()].copy_from_slice(OB_FS_PREFIX);
        buf[OB_FS_PREFIX.len()..total].copy_from_slice(bytes);
        let ret = (export::get_table().file_open)(buf.as_ptr());
        if ret < 0 { Err(ret) } else { Ok(File { fd: ret as u8 }) }
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, i64> {
        let ret = (export::get_table().file_read)(self.fd, buf.as_mut_ptr(), buf.len());
        if ret < 0 { Err(ret) } else { Ok(ret as usize) }
    }

    pub fn write(&mut self, buf: &[u8]) -> Result<usize, i64> {
        let ret = (export::get_table().file_write)(self.fd, buf.as_ptr(), buf.len());
        if ret < 0 { Err(ret) } else { Ok(ret as usize) }
    }

    pub fn fd(&self) -> u8 { self.fd }
}
