// src/shell/shell.rs

use crate::drivers::usb_hid;
use crate::input;
use crate::print;
use crate::println;
use crate::shell::environment::Environment;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

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
                        b'\t' => {
                            if line_len > 0 {
                                crate::console::draw_cursor(false);
                                cursor_visible = false;
                                let line_owned = {
                                    let s = core::str::from_utf8(&line_buffer[..line_len]);
                                    match s {
                                        Ok(s) => Some(alloc::string::String::from(s)),
                                        Err(_) => None,
                                    }
                                };
                                if let Some(ref line_str) = line_owned {
                                    self.try_complete(line_str, &mut line_buffer[..], &mut line_len);
                                }
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
            let drive = (trimmed.as_bytes()[0] as char).to_ascii_uppercase();
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
        let cmd_raw = match parts.next() {
            Some(c) => c,
            None => return,
        };

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

    pub fn try_complete(&mut self, line: &str, line_buffer: &mut [u8], line_len: &mut usize) {
        let trimmed = line.trim_start();
        let leading_ws = line.len() - trimmed.len();

        let word_start = trimmed
            .rfind(|c: char| c.is_whitespace())
            .map(|i| leading_ws + i + 1)
            .unwrap_or(leading_ws);

        let prefix = &line[word_start..];
        if prefix.is_empty() {
            return;
        }

        let is_first_word = line[..word_start].trim().is_empty();
        let mut matches: Vec<String> = Vec::new();

        if is_first_word {
            for name in crate::shell::handler::COMMANDS.names_starting_with(prefix) {
                matches.push(name.to_string());
            }

            if let Some(path) = self.environment.get("PATH") {
                let path_owned = alloc::string::String::from(path);
                for dir in path_owned.split(';') {
                    if dir.is_empty() {
                        continue;
                    }
                    let full_dir = if dir.starts_with('\\') || dir.starts_with('/') {
                        alloc::format!("{}:{}", self.current_drive, dir)
                    } else {
                        alloc::format!("{}:\\{}", self.current_drive, dir)
                    };

                    crate::globals::with_vfs(|vfs| {
                        if let Ok((drive_idx, node)) = vfs.resolve_path(&full_dir) {
                            let mut i = 0;
                            loop {
                                match vfs.readdir(drive_idx, node.inode, i) {
                                    Ok(Some(entry)) => {
                                        let name_upper = entry.name.to_ascii_uppercase();
                                        let p_upper = prefix.to_ascii_uppercase();
                                        if name_upper.starts_with(&p_upper)
                                            && (name_upper.ends_with(".BIN")
                                                || !entry.name.contains('.'))
                                        {
                                            let display_name = if name_upper.ends_with(".BIN") {
                                                entry.name[..entry.name.len() - 4].to_string()
                                            } else {
                                                entry.name.clone()
                                            };
                                            if !matches.contains(&display_name) {
                                                matches.push(display_name);
                                            }
                                        }
                                        i += 1;
                                    }
                                    Ok(None) => break,
                                    Err(_) => break,
                                }
                            }
                        }
                    });
                }
            }
        } else {
            let (search_dir, file_prefix) =
                if let Some(sep_idx) = prefix.rfind(|c| c == '\\' || c == '/') {
                    let dir_part = &prefix[..=sep_idx];
                    let file_part = &prefix[sep_idx + 1..];
                    let full_dir = self.resolve_absolute_path(dir_part);
                    (Some(full_dir), file_part)
                } else {
                    (None, prefix)
                };

            if file_prefix.is_empty() {
                return;
            }

            let cwd = if self.current_dir == "\\" {
                alloc::format!("{}:\\", self.current_drive)
            } else {
                alloc::format!("{}:{}", self.current_drive, self.current_dir)
            };
            let search = search_dir.unwrap_or(cwd);

            crate::globals::with_vfs(|vfs| {
                if let Ok((drive_idx, node)) = vfs.resolve_path(&search) {
                    let mut i = 0;
                    loop {
                        match vfs.readdir(drive_idx, node.inode, i) {
                            Ok(Some(entry)) => {
                                if entry.name.len() >= file_prefix.len()
                                    && entry.name[..file_prefix.len()]
                                        .eq_ignore_ascii_case(file_prefix)
                                {
                                    if !matches.contains(&entry.name) {
                                        matches.push(entry.name.clone());
                                    }
                                }
                                i += 1;
                            }
                            Ok(None) => break,
                            Err(_) => break,
                        }
                    }
                }
            });
        }

        if matches.is_empty() {
            return;
        }

        matches.sort();

        if matches.len() == 1 {
            let completion = &matches[0];

            let erase_len = *line_len - word_start;
            for _ in 0..erase_len {
                crate::console::write_char(b'\x08');
                crate::serial_print!("\x08 \x08");
            }

            *line_len = word_start;
            for b in completion.bytes() {
                if *line_len < 128 {
                    line_buffer[*line_len] = b;
                    *line_len += 1;
                }
                crate::console::write_char(b);
                crate::serial_print!("{}", b as char);
            }

            if is_first_word {
                if *line_len < 128 {
                    line_buffer[*line_len] = b' ';
                    *line_len += 1;
                }
                crate::console::write_char(b' ');
                crate::serial_print!(" ");
            }
        } else {
            println!();
            let mut col = 0usize;
            for m in &matches {
                if col + m.len() + 2 > 70 {
                    println!();
                    col = 0;
                }
                print!("{}  ", m);
                col += m.len() + 2;
            }
            println!();
            self.print_prompt(0);
            if *line_len > 0 {
                if let Ok(line_str) = core::str::from_utf8(&line_buffer[..*line_len]) {
                    for ch in line_str.chars() {
                        crate::console::write_codepoint(ch as u32);
                    }
                    crate::serial_print!("{}", line_str);
                }
            }
        }
    }
}
