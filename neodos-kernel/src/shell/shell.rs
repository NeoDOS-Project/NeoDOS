// src/shell/shell.rs

use crate::drivers::usb_hid;
use crate::input;
use crate::print;
use crate::println;
use crate::shell::environment::Environment;
use alloc::string::String;

pub struct DosShell {
    pub current_dir: String,
    pub current_dir_inode: u32,
    /// Active DOS drive letter ('A'..='Z').
    pub current_drive: char,
    pub environment: Environment,
    pub running: bool,
}

impl DosShell {
    pub fn new() -> Self {
        let mut system_drive = 'C';
        let environment = Environment::new();

        if let Some(drive_letter) = environment.get("SYSTEMDRIVE") {
            if let Some(first_char) = drive_letter.chars().next() {
                if first_char.is_ascii_uppercase() {
                    system_drive = first_char;
                }
            }
        }

        let mut shell = DosShell {
            current_dir: String::from("\\"),
            current_dir_inode: 0,
            current_drive: system_drive,
            environment,
            running: true,
        };
        shell.environment.set("PATH", "\\BIN;\\SYSTEM");
        shell.environment.set("PROMPT", "$P$G");
        if !shell.environment.get("SYSTEMDRIVE").is_some() {
            shell.environment.set("SYSTEMDRIVE", "C");
        }
        shell.environment.set("CURSOR", "18");
        shell
    }

    pub fn run(&mut self) -> ! {
        println!("NeoDOS v{} - VFS Active", env!("CARGO_PKG_VERSION"));
        println!("Type HELP for a list of commands.");

        self.check_config_sys();

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

                if usb_hid::has_usb_keyboard() {
                    usb_hid::poll_usb_keyboard();
                }

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
        print!("{}:{}> ", self.current_drive, self.current_dir);
        crate::serial_print!("{}:{}> ", self.current_drive, self.current_dir);
    }

    pub fn execute_line(&mut self, line: &str) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return;
        }

        // Handle drive change (e.g., "A:")
        if trimmed.len() == 2 && trimmed.ends_with(':') {
            let drive = trimmed.chars().next().unwrap().to_ascii_uppercase();
            crate::globals::with_vfs(|vfs| {
                if let Some(idx) = crate::fs::vfs::Vfs::drive_index(drive) {
                    if vfs.drives[idx].is_some() {
                        self.current_drive = drive;
                        self.current_dir = String::from("\\");
                        self.current_dir_inode = 0;
                    } else {
                        println!("Invalid drive");
                    }
                } else {
                    println!("Invalid drive");
                }
            });
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
            println!("Bad command or file name");
        }
    }

    #[allow(dead_code)]
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

    pub fn check_config_sys(&mut self) {
        self.try_load_config("C:\\CONFIG.SYS");
        self.try_load_config("C:\\SYSTEM\\CONFIG.SYS");
    }

    fn try_load_config(&mut self, path: &str) {
        crate::globals::with_vfs(|vfs| {
            if let Ok((drive_idx, node)) = vfs.resolve_path(path) {
                let mut buf = [0u8; 4096];
                if let Ok(read) = vfs.read(drive_idx, node.inode, 0, &mut buf) {
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
        });
    }

    #[allow(dead_code)]
    pub fn check_autoexec(&mut self) {
        // Placeholder
    }

    pub fn execute_batch(&mut self, content: &str) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with(':') || trimmed.starts_with('@') {
                continue;
            }
            if trimmed.eq_ignore_ascii_case("pause") {
                println!("Press any key to continue . . .");
                crate::drivers::keyboard::wait_for_key();
                continue;
            }
            self.execute_line(trimmed);
        }
    }

    pub(crate) fn resolve_absolute_path(&self, path: &str) -> String {
        let mut drive = self.current_drive;
        let rest = if path.starts_with('\\') || path.starts_with('/') {
            path
        } else if path.contains(':') {
            drive = path.chars().next().unwrap_or(self.current_drive).to_ascii_uppercase();
            &path[2..]
        } else {
            path
        };

        let mut abs = alloc::format!("{}:", drive);
        if rest.starts_with('\\') || rest.starts_with('/') {
            abs.push_str(rest);
        } else {
            abs.push_str(&self.current_dir);
            if !self.current_dir.ends_with('\\') && !self.current_dir.ends_with('/') {
                abs.push('\\');
            }
            abs.push_str(rest);
        }
        abs
    }
}
