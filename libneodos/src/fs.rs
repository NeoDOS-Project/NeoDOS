use crate::export;
use crate::syscall;

pub struct File {
    fd: u8,
}

impl File {
    pub fn open(path: &str) -> Result<File, i64> {
        let bytes = path.as_bytes();
        let mut buf = [0u8; 256];
        if bytes.len() >= 255 { return Err(syscall::EINVAL); }
        buf[..bytes.len()].copy_from_slice(bytes);
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
