use crate::syscall;

pub struct File {
    fd: u8,
}

impl File {
    pub fn open(path: &str) -> Result<File, i64> {
        let fd = syscall::sys_open(path)?;
        Ok(File { fd })
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, i64> {
        syscall::sys_readfile(self.fd, buf)
    }

    pub fn write(&mut self, buf: &[u8]) -> Result<usize, i64> {
        syscall::sys_writefile(self.fd, buf)
    }

    pub fn fd(&self) -> u8 {
        self.fd
    }
}
