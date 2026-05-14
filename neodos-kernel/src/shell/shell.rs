// src/shell/shell.rs

use crate::buffer::block_cache::BlockCache;
use crate::drivers::ata::{AtaChannel, AtaDriver};
use crate::drivers::fat32::Fat32Driver;
use crate::drivers::usb_hid;
use crate::fs::drive_manager::{DriveManager, DriveManagerError, FsInstanceId, InternalPath};
use crate::fs::neodos_fs::{FsError, NeoDosFs, ROOT_INODE};
use crate::fs::volume::Volume;
use crate::input;
use crate::print;
use crate::println;
use crate::shell::environment::Environment;
use alloc::string::String;

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

/// Normalize user input using only [`DriveManager`] (no `&mut` shell), so callers can
/// run VFS operations afterward without borrow conflicts.
pub(crate) fn vfs_path_from_drive_manager<'a>(
    drive_manager: &DriveManager,
    path: &'a str,
) -> Result<(FsInstanceId, VfsPath<'a>), DriveManagerError> {
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        let Some(&letter_byte) = bytes.first() else {
            return Err(DriveManagerError::InvalidPath);
        };
        if !letter_byte.is_ascii_alphabetic() {
            return Err(DriveManagerError::InvalidDriveLetter);
        }
        let (fs_id, internal) = drive_manager.resolve_dos_path(path)?;
        Ok((fs_id, VfsPath::Internal(internal)))
    } else {
        Ok((FsInstanceId::PRIMARY, VfsPath::Borrowed(path)))
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
    pub ata_secondary: &'a mut AtaDriver,
    pub fat32: Option<Fat32Driver>,
    pub extra_volumes: [Option<Volume>; 3],
    pub running: bool,
}

impl<'a> DosShell<'a> {
    pub fn new(
        fs: &'a mut NeoDosFs,
        cache: &'a mut BlockCache,
        ata: &'a mut AtaDriver,
        ata_secondary: &'a mut AtaDriver,
        fat32: Option<Fat32Driver>,
        extra_volumes: [Option<Volume>; 3],
    ) -> Self {
        let mut drive_manager = DriveManager::new();

        let mut system_drive = b'C';
        let environment = Environment::new();

        if let Some(drive_letter) = environment.get("SYSTEMDRIVE") {
            if let Some(first_char) = drive_letter.chars().next() {
                if first_char.is_ascii_uppercase() {
                    system_drive = first_char as u8;
                }
            }
        }

        let _ = drive_manager.mount(system_drive as char, FsInstanceId::PRIMARY);
        if fat32.is_some() {
            let _ = drive_manager.mount('A', FsInstanceId::FAT32_ESP);
        }

        // Mount extra volumes as D:, E:, F:
        let extra_ids = [FsInstanceId::VOLUME_1, FsInstanceId::VOLUME_2, FsInstanceId::VOLUME_3];
        for (i, vol) in extra_volumes.iter().enumerate() {
            if vol.is_some() {
                let letter = (b'D' + i as u8) as char;
                let _ = drive_manager.mount(letter, extra_ids[i]);
            }
        }

        let mut shell = DosShell {
            current_dir: [0; 128],
            current_dir_len: 1,
            current_dir_inode: 0,
            current_drive: system_drive,
            drive_manager,
            environment,
            fs,
            cache,
            ata,
            ata_secondary,
            fat32,
            extra_volumes,
            running: true,
        };
        shell.current_dir[0] = b'\\';
        shell.environment.set("PATH", "\\BIN;\\SYSTEM");
        shell.environment.set("PROMPT", "$P$G");
        if !shell.environment.get("SYSTEMDRIVE").is_some() {
            shell.environment.set("SYSTEMDRIVE", "C");
        }
        shell.environment.set("CURSOR", "18");
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

    /// Resolve a path for NeoDosFs only. Returns error for FAT32 ESP paths.
    pub(crate) fn resolve_directory_arg(
        &mut self,
        path: &str,
    ) -> Result<(u32, [u8; 128], usize), FsError> {
        let dm = self.drive_manager;
        let (fs_id, vfs) = vfs_path_from_drive_manager(&dm, path)
            .map_err(|_| FsError::FileNotFound)?;
        if fs_id != FsInstanceId::PRIMARY {
            return Err(FsError::FileNotFound);
        }
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
            let (fs_id, parent_vfs) = vfs_path_from_drive_manager(&dm, parent_path)
                .map_err(|_| FsError::FileNotFound)?;
            if fs_id != FsInstanceId::PRIMARY {
                return Err(FsError::FileNotFound);
            }
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
        println!("NeoDOS v{} - FS Started", env!("CARGO_PKG_VERSION"));
        println!("Type HELP for a list of commands.");

        self.check_config_sys();
        self.init_boot_drive_from_config();
        self.check_autoexec();

        let mut idle_hits: u64 = 0;

        while self.running {
            self.print_prompt(idle_hits);
            idle_hits = 0;
            let mut line_buffer = [0u8; 128];
            let mut line_len = 0;

            let mut blink_ticks = 0u64;
            let mut cursor_visible = false;

            let mut utf8_rem = 0usize;
            let mut utf8_cp = 0u32;

            let cursor_interval: u64 = self
                .environment
                .get("CURSOR")
                .and_then(|s| s.parse().ok())
                .unwrap_or(18);

            loop {
                let ticks = crate::scheduler::TIMER_TICKS.load(core::sync::atomic::Ordering::Relaxed);
                if ticks - blink_ticks >= cursor_interval {
                    blink_ticks = ticks;
                    cursor_visible = !cursor_visible;
                    crate::console::draw_cursor(cursor_visible);
                }

                // Poll USB keyboard if available
                if usb_hid::has_usb_keyboard() {
                    usb_hid::poll_usb_keyboard();
                }

                // Input comes from IRQ1 keyboard handler which fills the buffer
                if let Some(byte) = input::pop_byte() {
                    crate::console::draw_cursor(false);
                    cursor_visible = false;

                    match byte {
                        b'\n' => {
                            println!();
                            break;
                        }
                        b'\x08' => {
                            utf8_rem = 0;
                            if line_len > 0 {
                                let mut n = 1;
                                while n < line_len && (line_buffer[line_len - n] & 0xC0) == 0x80 {
                                    n += 1;
                                }
                                line_len -= n;
                                crate::console::write_char(b'\x08');
                                crate::serial_print!("\x08 \x08");
                            }
                        }
                        _ if line_len + 4 < 128 => {
                            if utf8_rem == 0 {
                                if byte < 0x80 {
                                    line_buffer[line_len] = byte;
                                    line_len += 1;
                                    crate::console::write_char(byte);
                                    crate::serial_print!("{}", byte as char);
                                } else if byte >= 0xC2 && byte <= 0xDF {
                                    utf8_rem = 1;
                                    utf8_cp = (byte & 0x1F) as u32;
                                    line_buffer[line_len] = byte;
                                    line_len += 1;
                                } else if byte >= 0xE0 && byte <= 0xEF {
                                    utf8_rem = 2;
                                    utf8_cp = (byte & 0x0F) as u32;
                                    line_buffer[line_len] = byte;
                                    line_len += 1;
                                }
                            } else if (byte & 0xC0) == 0x80 {
                                utf8_cp = (utf8_cp << 6) | (byte & 0x3F) as u32;
                                utf8_rem -= 1;
                                line_buffer[line_len] = byte;
                                line_len += 1;
                                if utf8_rem == 0 {
                                    crate::console::write_codepoint(utf8_cp);
                                    if let Some(ch) = core::char::from_u32(utf8_cp) {
                                        crate::serial_print!("{}", ch);
                                    }
                                }
                            } else {
                                utf8_rem = 0;
                            }
                        }
                        _ => { utf8_rem = 0; }
                    }
                } else {
                    crate::globals::flush_cache_if_needed();
                    unsafe { core::arch::asm!("hlt") };
                    idle_hits += 1;
                }
            }

            if line_len > 0 {
                match core::str::from_utf8(&line_buffer[..line_len]) {
                    Ok(line) => self.execute_line(line),
                    Err(_) => println!("?Invalid UTF-8 in command line"),
                }
            }
        }

        println!("Returning to BIOS...");
        loop {
            unsafe { core::arch::asm!("hlt") };
        }
    }

    fn print_prompt(&mut self, _idle_hits: u64) {
        if let Ok(dir) = core::str::from_utf8(&self.current_dir[..self.current_dir_len]) {
            crate::serial_print!("{}:{}> ", self.current_drive as char, dir);
            print!("{}:{}> ", self.current_drive as char, dir);
        } else {
            crate::serial_print!("{}:\\> ", self.current_drive as char);
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

        if !self.dispatch_command(cmd, &args_buf[..arg_count]) {
            self.run_path_command(cmd_raw, &args_buf[..arg_count]);
        }
    }

    pub fn run_path_command(&mut self, cmd_raw: &str, _args: &[&str]) {
        let path = self.environment.get("PATH").map(String::from).unwrap_or_default();
        if path.is_empty() {
            crate::println!("Bad command or file name");
            return;
        }

        if cmd_raw.contains('\\') || cmd_raw.contains('/') || cmd_raw.contains('.') {
            let run_args = [cmd_raw];
            if cmd_raw.len() > 4 && cmd_raw[cmd_raw.len()-4..].eq_ignore_ascii_case(".BAT")
            {
                self.cmd_call(&run_args);
            } else {
                self.cmd_run(&run_args);
            }
            return;
        }

        let mut path_buf = [0u8; 260];
        let cmd_upper = cmd_raw.to_ascii_uppercase();
        let cmd_bytes = cmd_upper.as_bytes();
        let extensions: &[&[u8]] = &[b"BIN", b"BAT"];

        for dir in path.split(';') {
            let dir = dir.trim();
            if dir.is_empty() { continue; }
            let dir_bytes = dir.as_bytes();

            for &ext in extensions {
                let mut pos = 0;
                path_buf[pos..pos + dir_bytes.len()].copy_from_slice(dir_bytes);
                pos += dir_bytes.len();
                if dir_bytes.last() != Some(&b'\\') {
                    path_buf[pos] = b'\\';
                    pos += 1;
                }
                path_buf[pos..pos + cmd_bytes.len()].copy_from_slice(cmd_bytes);
                pos += cmd_bytes.len();
                path_buf[pos] = b'.';
                pos += 1;
                path_buf[pos..pos + 3].copy_from_slice(ext);
                pos += 3;

                let full_path = core::str::from_utf8(&path_buf[..pos]).unwrap_or("");
                if self.resolve_file_inode(full_path).is_ok() {
                    let run_args = [full_path];
                    if ext == b"BAT" {
                        self.cmd_call(&run_args);
                    } else {
                        self.cmd_run(&run_args);
                    }
                    return;
                }
            }
        }
        crate::println!("Bad command or file name");
    }

    pub fn check_config_sys(&mut self) {
        self.try_load_config("CONFIG.SYS");
        self.try_load_config("SYSTEM\\CONFIG.SYS");
    }

    fn try_load_config(&mut self, path: &str) {
        match self.resolve_file_inode(path) {
            Ok(inode_num) => {
                let mut buf = [0u8; 4096];
                if let Ok(read) = self.fs.read_file_to_buf(inode_num, &mut buf, self.cache, self.ata) {
                    if let Ok(content) = core::str::from_utf8(&buf[..read]) {
                        for line in content.lines() {
                            let trimmed = line.trim();
                            if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
                                continue;
                            }
                            if let Some(eq) = trimmed.find('=') {
                                let key = trimmed[..eq].trim();
                                let value = trimmed[eq + 1..].trim();
                                if !key.is_empty() && !value.is_empty() {
                                    self.environment.set(key, value);
                                }
                            }
                        }
                    }
                }
            }
            Err(_) => {}
        }
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

    #[allow(dead_code)]
    pub fn navigate_to_path(&mut self, path: &str) -> Result<u32, FsError> {
        let dm = self.drive_manager;
        let (fs_id, vfs) = vfs_path_from_drive_manager(&dm, path)
            .map_err(|_| FsError::FileNotFound)?;
        if fs_id != FsInstanceId::PRIMARY {
            return Err(FsError::FileNotFound);
        }
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

    /// Execute a closure on the filesystem identified by `fs_id`.
    /// Temporarily sets ATA base_lba to the volume's partition and restores it after.
    /// Selects the correct ATA driver based on the volume's physical channel.
    pub(crate) fn with_volume<R>(
        &mut self,
        fs_id: FsInstanceId,
        f: impl FnOnce(&mut NeoDosFs, &mut BlockCache, &mut AtaDriver) -> R,
    ) -> Result<R, FsError> {
        match fs_id {
            FsInstanceId::PRIMARY => Ok(f(self.fs, self.cache, self.ata)),
            FsInstanceId::VOLUME_1 | FsInstanceId::VOLUME_2 | FsInstanceId::VOLUME_3 => {
                let idx = match fs_id {
                    FsInstanceId::VOLUME_1 => 0,
                    FsInstanceId::VOLUME_2 => 1,
                    FsInstanceId::VOLUME_3 => 2,
                    _ => unreachable!(),
                };
                let vol = self.extra_volumes[idx].as_mut().ok_or(FsError::FileNotFound)?;
                let ata = match vol.channel {
                    AtaChannel::Primary => &mut *self.ata,
                    AtaChannel::Secondary => &mut *self.ata_secondary,
                };
                let saved_base = ata.base_lba();
                ata.set_base_lba(vol.base_lba);
                let result = f(&mut vol.fs, &mut vol.cache, ata);
                ata.set_base_lba(saved_base);
                Ok(result)
            }
            _ => Err(FsError::FileNotFound),
        }
    }

    fn init_boot_drive_from_config(&mut self) {
        if let Some(boot_drive) = self.environment.get("BOOTDRIVE") {
            let drive_char = boot_drive.chars().next().map(|c| c.to_ascii_uppercase());
            if let Some(drive) = drive_char {
                if drive >= 'A' && drive <= 'Z' {
                    if drive as u8 != self.current_drive {
                        if self.drive_manager.set_primary(drive).is_ok() {
                            self.current_drive = drive as u8;
                        }
                    }
                }
            }
        }
    }
}

