// src/shell/shell.rs

use crate::buffer::block_cache::BlockCache;
use crate::drivers::ata::AtaDriver;
use crate::fs::drive_manager::{DriveManager, DriveManagerError, FsInstanceId, InternalPath};
use crate::fs::neodos_fs::{FsError, NeoDosFs, ROOT_INODE};
use crate::input;
use crate::print;
use crate::println;
use crate::shell::environment::Environment;

/// Logical path passed to [`NeoDosFs::resolve_directory_path`] (no `X:` prefix).
pub(crate) enum VfsPath<'a> {
    Borrowed(&'a str),
    Internal(InternalPath),
}

impl VfsPath<'_> {
    pub(crate) fn as_str(&self) -> Result<&str, DriveManagerError> {
        match self {
            VfsPath::Borrowed(s) => Ok(s),
            VfsPath::Internal(p) => p.as_str(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ShellPathError {
    Drive(DriveManagerError),
    UnsupportedVolume,
}

/// Normalize user input using only [`DriveManager`] (no `&mut` shell), so callers can
/// run VFS operations afterward without borrow conflicts.
pub(crate) fn vfs_path_from_drive_manager<'a>(
    drive_manager: &DriveManager,
    path: &'a str,
) -> Result<VfsPath<'a>, ShellPathError> {
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        let Some(&letter_byte) = bytes.first() else {
            return Err(ShellPathError::Drive(DriveManagerError::InvalidPath));
        };
        if !letter_byte.is_ascii_alphabetic() {
            return Err(ShellPathError::Drive(DriveManagerError::InvalidDriveLetter));
        }
        let (fs_id, internal) = drive_manager
            .resolve_dos_path(path)
            .map_err(ShellPathError::Drive)?;
        if fs_id != FsInstanceId::PRIMARY {
            return Err(ShellPathError::UnsupportedVolume);
        }
        Ok(VfsPath::Internal(internal))
    } else {
        Ok(VfsPath::Borrowed(path))
    }
}

pub struct DosShell<'a> {
    pub current_dir: [u8; 128],
    pub current_dir_len: usize,
    pub current_dir_inode: u32,
    /// Active DOS drive letter (`b'A'`..=b'Z'`).
    pub current_drive: u8,
    pub drive_manager: DriveManager,
    pub environment: Environment,
    pub fs: &'a mut NeoDosFs,
    pub cache: &'a mut BlockCache,
    pub ata: &'a mut AtaDriver,
    pub running: bool,
}

impl<'a> DosShell<'a> {
    pub fn new(fs: &'a mut NeoDosFs, cache: &'a mut BlockCache, ata: &'a mut AtaDriver) -> Self {
        let mut drive_manager = DriveManager::new();
        let _ = drive_manager.mount('C', FsInstanceId::PRIMARY);

        let mut shell = DosShell {
            current_dir: [0; 128],
            current_dir_len: 1,
            current_dir_inode: 0,
            current_drive: b'C',
            drive_manager,
            environment: Environment::new(),
            fs,
            cache,
            ata,
            running: true,
        };
        shell.current_dir[0] = b'\\';
        shell.environment.set("PATH", "\\BIN;\\SYSTEM");
        shell.environment.set("PROMPT", "$P$G");
        shell
    }

    pub(crate) fn split_parent_and_leaf<'b>(&self, path: &'b str) -> (&'b str, &'b str) {
        if let Some(idx) = path
            .as_bytes()
            .iter()
            .rposition(|b| *b == b'\\' || *b == b'/')
        {
            (&path[..idx], &path[idx + 1..])
        } else {
            ("", path)
        }
    }

    pub(crate) fn resolve_directory_arg_from_vfs(
        &mut self,
        vfs: VfsPath<'_>,
    ) -> Result<(u32, [u8; 128], usize), FsError> {
        let s = vfs.as_str().map_err(|_| FsError::FileNotFound)?;
        self.fs.resolve_directory_path(
            self.current_dir_inode,
            &self.current_dir[..self.current_dir_len],
            self.current_dir_len,
            s,
            self.cache,
            self.ata,
        )
    }

    pub(crate) fn resolve_directory_arg(
        &mut self,
        path: &str,
    ) -> Result<(u32, [u8; 128], usize), FsError> {
        let dm = self.drive_manager;
        let vfs = vfs_path_from_drive_manager(&dm, path).map_err(|_| FsError::FileNotFound)?;
        self.resolve_directory_arg_from_vfs(vfs)
    }

    pub(crate) fn resolve_file_inode(&mut self, path: &str) -> Result<u32, FsError> {
        let (parent_path, leaf) = self.split_parent_and_leaf(path);
        if leaf.is_empty() || leaf == "." || leaf == ".." {
            return Err(FsError::FileNotFound);
        }

        let parent_inode = if parent_path.is_empty() {
            self.current_dir_inode
        } else {
            let dm = self.drive_manager;
            let parent_vfs =
                vfs_path_from_drive_manager(&dm, parent_path).map_err(|_| FsError::FileNotFound)?;
            self.resolve_directory_arg_from_vfs(parent_vfs)?.0
        };
        self.fs
            .find_file_in_directory(parent_inode, leaf, self.cache, self.ata)
    }

    /// Drive letter to show in `DIR` header when user passes a drive-qualified path.
    pub(crate) fn dir_display_drive(&self, path_arg: Option<&str>) -> char {
        match path_arg {
            Some(p) => {
                let b = p.as_bytes();
                if b.len() >= 2 && b[1] == b':' {
                    p.chars()
                        .next()
                        .map(|c| c.to_ascii_uppercase())
                        .unwrap_or(self.current_drive as char)
                } else {
                    self.current_drive as char
                }
            }
            None => self.current_drive as char,
        }
    }

    pub fn run(&mut self) -> ! {
        println!("NeoDOS v0.5 - Shell Started");
        println!("Type HELP for a list of commands.");
        println!();

        self.check_autoexec();

        while self.running {
            self.print_prompt();
            let mut line_buffer = [0u8; 128];
            let mut line_len = 0;

            let mut blink_counter = 0;
            let mut cursor_visible = false;

            loop {
                blink_counter += 1;
                if blink_counter > 100000 {
                    blink_counter = 0;
                    cursor_visible = !cursor_visible;
                    crate::vga::draw_cursor(cursor_visible);
                }

                if let Some(scancode) = crate::drivers::keyboard::KeyboardDriver::read_scancode() {
                    if let Some(ascii) =
                        crate::drivers::keyboard::KeyboardDriver::scancode_to_ascii(scancode)
                    {
                        input::push_byte(ascii);
                    }
                }

                if let Some(byte) = input::pop_byte() {
                    crate::vga::draw_cursor(false);
                    cursor_visible = false;

                    match byte {
                        b'\n' => {
                            println!();
                            break;
                        }
                        b'\x08' => {
                            if line_len > 0 {
                                line_len -= 1;
                                print!("\x08");
                            }
                        }
                        c if line_len < 127 => {
                            line_buffer[line_len] = c;
                            line_len += 1;
                            print!("{}", c as char);
                        }
                        _ => {}
                    }
                }
            }

            if line_len > 0 {
                if let Ok(line) = core::str::from_utf8(&line_buffer[..line_len]) {
                    self.execute_line(line);
                }
            }
        }

        println!("Returning to BIOS...");
        loop {
            unsafe { core::arch::asm!("hlt") };
        }
    }

    fn print_prompt(&mut self) {
        if let Ok(dir) = core::str::from_utf8(&self.current_dir[..self.current_dir_len]) {
            print!("{}:{}> ", self.current_drive as char, dir);
        } else {
            print!("{}:\\> ", self.current_drive as char);
        }
    }

    pub(crate) fn volume_label(&self) -> &'static str {
        "NEODOS"
    }

    pub fn execute_line(&mut self, line: &str) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return;
        }

        let mut parts = trimmed.split_whitespace();
        let cmd_raw = parts.next().unwrap();

        let mut cmd_buf = [0u8; 32];
        let mut cmd_len = 0;
        for (i, b) in cmd_raw.as_bytes().iter().enumerate() {
            if i < 32 {
                let mut c = *b;
                if c >= b'a' && c <= b'z' {
                    c -= 32;
                }
                cmd_buf[i] = c;
                cmd_len += 1;
            }
        }

        let cmd = core::str::from_utf8(&cmd_buf[..cmd_len]).unwrap_or("");

        let mut args_buf = [""; 16];
        let mut arg_count = 0;
        for part in parts {
            if arg_count < 16 {
                args_buf[arg_count] = part;
                arg_count += 1;
            }
        }

        self.dispatch_command(cmd, &args_buf[..arg_count]);
    }

    pub fn check_autoexec(&mut self) {
        match self.fs.find_file("AUTOEXEC.BAT", self.cache, self.ata) {
            Ok(inode_num) => {
                let mut buf = [0u8; 4096];
                match self.fs.read_file_to_buf(inode_num, &mut buf, self.cache, self.ata) {
                    Ok(read) => {
                        if let Ok(content) = core::str::from_utf8(&buf[..read]) {
                            println!("Executing AUTOEXEC.BAT...");
                            self.execute_batch(content);
                            println!();
                        }
                    }
                    Err(_) => {}
                }
            }
            Err(_) => {}
        }
    }

    pub fn navigate_to_path(&mut self, path: &str) -> Result<u32, FsError> {
        let dm = self.drive_manager;
        let vfs = vfs_path_from_drive_manager(&dm, path).map_err(|_| FsError::FileNotFound)?;
        let s = vfs.as_str().map_err(|_| FsError::FileNotFound)?;
        let base_inode = if s.starts_with('\\') || s.starts_with('/') {
            ROOT_INODE
        } else {
            self.current_dir_inode
        };

        let base_len = if base_inode == ROOT_INODE {
            1
        } else {
            self.current_dir_len
        };
        let base_path = if base_inode == ROOT_INODE {
            &self.current_dir[..1]
        } else {
            &self.current_dir[..self.current_dir_len]
        };

        self.fs
            .resolve_directory_path(base_inode, base_path, base_len, s, self.cache, self.ata)
            .map(|(inode, _, _)| inode)
    }
}
