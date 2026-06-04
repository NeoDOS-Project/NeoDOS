use crate::export;
use core::fmt;

pub const STDIN_FD: u8 = 0;
pub const STDOUT_FD: u8 = 1;
pub const STDERR_FD: u8 = 2;

pub struct Stdout;
pub struct Stdin;
pub struct Stderr;

impl Stdout {
    pub fn write(&self, buf: &[u8]) -> Result<usize, i64> {
        let ptr = buf.as_ptr();
        let len = buf.len();
        let ret = (export::get_table().stdout_write)(ptr, len);
        if ret < 0 { Err(ret) } else { Ok(ret as usize) }
    }

    pub fn write_str(&self, s: &str) -> Result<usize, i64> {
        self.write(s.as_bytes())
    }

    pub fn flush(&self) {}
}

impl Stdin {
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, i64> {
        let ptr = buf.as_mut_ptr();
        let len = buf.len();
        let ret = (export::get_table().stdin_read)(ptr, len);
        if ret < 0 { Err(ret) } else { Ok(ret as usize) }
    }
}

impl Stderr {
    pub fn write(&self, buf: &[u8]) -> Result<usize, i64> {
        let ptr = buf.as_ptr();
        let len = buf.len();
        let ret = (export::get_table().stderr_write)(ptr, len);
        if ret < 0 { Err(ret) } else { Ok(ret as usize) }
    }

    pub fn write_str(&self, s: &str) -> Result<usize, i64> {
        self.write(s.as_bytes())
    }

    pub fn flush(&self) {}
}

impl fmt::Write for Stdout {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        Stdout::write_str(self, s).map(|_| ()).map_err(|_| fmt::Error)
    }
}

impl fmt::Write for Stderr {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        Stderr::write_str(self, s).map(|_| ()).map_err(|_| fmt::Error)
    }
}

pub fn stdout() -> Stdout { Stdout }
pub fn stdin() -> Stdin { Stdin }
pub fn stderr() -> Stderr { Stderr }

struct StackBuf<const N: usize> {
    buf: [u8; N],
    pos: usize,
}

impl<const N: usize> fmt::Write for StackBuf<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        let remaining = N.saturating_sub(self.pos);
        let to_copy = core::cmp::min(bytes.len(), remaining);
        self.buf[self.pos..self.pos + to_copy].copy_from_slice(&bytes[..to_copy]);
        self.pos += to_copy;
        Ok(())
    }
}

const PRINT_BUF_SIZE: usize = 1024;

pub fn _print(args: fmt::Arguments) {
    use fmt::Write;
    let mut buf = StackBuf::<PRINT_BUF_SIZE> { buf: [0u8; PRINT_BUF_SIZE], pos: 0 };
    let _ = buf.write_fmt(args);
    let ptr = &buf.buf[..buf.pos];
    (export::get_table().sys_write)(1, ptr.as_ptr(), ptr.len());
}

pub fn _eprint(args: fmt::Arguments) {
    use fmt::Write;
    let mut buf = StackBuf::<PRINT_BUF_SIZE> { buf: [0u8; PRINT_BUF_SIZE], pos: 0 };
    let _ = buf.write_fmt(args);
    let ptr = &buf.buf[..buf.pos];
    (export::get_table().sys_write)(2, ptr.as_ptr(), ptr.len());
}
