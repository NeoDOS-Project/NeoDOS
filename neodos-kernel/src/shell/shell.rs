// src/shell/shell.rs

use crate::drivers::ata::AtaDriver;
use crate::buffer::block_cache::BlockCache;
use crate::fs::neodos_fs::NeoDosFs;
use crate::shell::environment::Environment;
use crate::input;
use crate::println;
use crate::print;

pub struct DosShell<'a> {
    pub current_dir: [u8; 128],
    pub current_dir_len: usize,
    pub environment: Environment,
    pub fs: &'a mut NeoDosFs,
    pub cache: &'a mut BlockCache,
    pub ata: &'a mut AtaDriver,
    pub running: bool,
}

impl<'a> DosShell<'a> {
    pub fn new(fs: &'a mut NeoDosFs, cache: &'a mut BlockCache, ata: &'a mut AtaDriver) -> Self {
        let mut shell = DosShell {
            current_dir: [0; 128],
            current_dir_len: 1,
            environment: Environment::new(),
            fs,
            cache,
            ata,
            running: true,
        };
        shell.current_dir[0] = b'\\';
        shell.environment.set("PATH", "C:\\");
        shell.environment.set("PROMPT", "$P$G");
        shell
    }

    pub fn run(&mut self) -> ! {
        println!("NeoDOS v0.5 - Shell Started");
        println!("Type HELP for a list of commands.");
        println!();

        // Check for AUTOEXEC.BAT
        self.check_autoexec();

        while self.running {
            self.print_prompt();
            let mut line_buffer = [0u8; 128];
            let mut line_len = 0;

            let mut blink_counter = 0;
            let mut cursor_visible = false;

            // Simple line reader from input buffer
            loop {
                blink_counter += 1;
                if blink_counter > 100000 { // Adjust for speed
                    blink_counter = 0;
                    cursor_visible = !cursor_visible;
                    crate::vga::draw_cursor(cursor_visible);
                }

                // Poll keyboard manually since we don't have interrupts yet
                if let Some(scancode) = crate::drivers::keyboard::KeyboardDriver::read_scancode() {
                    if let Some(ascii) = crate::drivers::keyboard::KeyboardDriver::scancode_to_ascii(scancode) {
                        input::push_byte(ascii);
                    }
                }

                if let Some(byte) = input::pop_byte() {
                    // Erase cursor before processing input
                    crate::vga::draw_cursor(false);
                    cursor_visible = false;

                    match byte {
                        b'\n' => {
                            println!();
                            break;
                        }
                        b'\x08' => { // Backspace
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
        loop { unsafe { core::arch::asm!("hlt") }; }
    }

    fn print_prompt(&mut self) {
        if let Ok(dir) = core::str::from_utf8(&self.current_dir[..self.current_dir_len]) {
            print!("C:{}> ", dir);
        } else {
            print!("C:\\> ");
        }
    }

    pub fn execute_line(&mut self, line: &str) {
        let trimmed = line.trim();
        if trimmed.is_empty() { return; }

        let mut parts = trimmed.split_whitespace();
        let cmd_raw = parts.next().unwrap();
        
        // Convert to uppercase manually for no_std
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
}
