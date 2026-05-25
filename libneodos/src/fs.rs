use crate::syscall;

pub struct File {
    handle: u64,
}

impl File {
    pub fn open(path: &str) -> Result<File, i64> {
        let handle = syscall::sys_open(path)?;
        Ok(File { handle })
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize, i64> {
        syscall::sys_readfile(self.handle, buf)
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize, i64> {
        syscall::sys_writefile(self.handle, buf)
    }
}
