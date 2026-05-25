use crate::syscall;
use core::fmt;

pub const STDIN_FD: u8 = 0;
pub const STDOUT_FD: u8 = 1;
pub const STDERR_FD: u8 = 2;

pub struct Stdout;

pub struct Stdin;

pub struct Stderr;

impl Stdout {
    pub fn write(&self, buf: &[u8]) -> Result<usize, i64> {
        syscall::sys_write(STDOUT_FD, buf)
    }

    pub fn write_str(&self, s: &str) -> Result<usize, i64> {
        self.write(s.as_bytes())
    }

    pub fn flush(&self) {}
}

impl Stdin {
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, i64> {
        syscall::sys_read(STDIN_FD, buf)
    }
}

impl Stderr {
    pub fn write(&self, buf: &[u8]) -> Result<usize, i64> {
        syscall::sys_write(STDERR_FD, buf)
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

pub fn stdout() -> Stdout {
    Stdout
}

pub fn stdin() -> Stdin {
    Stdin
}

pub fn stderr() -> Stderr {
    Stderr
}

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
    let mut buf = StackBuf::<PRINT_BUF_SIZE> {
        buf: [0u8; PRINT_BUF_SIZE],
        pos: 0,
    };
    let _ = buf.write_fmt(args);
    syscall::sys_write(STDOUT_FD, &buf.buf[..buf.pos]).ok();
}

pub fn _eprint(args: fmt::Arguments) {
    use fmt::Write;
    let mut buf = StackBuf::<PRINT_BUF_SIZE> {
        buf: [0u8; PRINT_BUF_SIZE],
        pos: 0,
    };
    let _ = buf.write_fmt(args);
    syscall::sys_write(STDERR_FD, &buf.buf[..buf.pos]).ok();
}
