#![no_std]
#![no_main]

use libneodos::syscall;

const LINE_BUF_SIZE: usize = 256;
const HISTORY_SIZE: usize = 32;
const PROMPT: &str = "[ns] ";

/// Convert byte slice to uppercase in-place (ASCII only)
fn make_ascii_uppercase(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        if *b >= b'a' && *b <= b'z' {
            *b -= 32;
        }
    }
}

fn trim_ascii(s: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < s.len() && (s[start] == b' ' || s[start] == b'\t' || s[start] == b'\r' || s[start] == b'\n') {
        start += 1;
    }
    let mut end = s.len();
    while end > start && (s[end - 1] == b' ' || s[end - 1] == b'\t' || s[end - 1] == b'\r' || s[end - 1] == b'\n') {
        end -= 1;
    }
    &s[start..end]
}

/// Get first whitespace-delimited token from a byte slice
fn first_token(s: &[u8]) -> &[u8] {
    let trimmed = trim_ascii(s);
    for i in 0..trimmed.len() {
        if trimmed[i] == b' ' || trimmed[i] == b'\t' {
            return &trimmed[..i];
        }
    }
    trimmed
}

/// Get the rest after the first token
fn after_first_token(s: &[u8]) -> &[u8] {
    let trimmed = trim_ascii(s);
    for i in 0..trimmed.len() {
        if trimmed[i] == b' ' || trimmed[i] == b'\t' {
            let rest = trim_ascii(&trimmed[i + 1..]);
            return rest;
        }
    }
    &[]
}

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
}

struct Shell {
    line: [u8; LINE_BUF_SIZE],
    pos: usize,
    history: [[u8; LINE_BUF_SIZE]; HISTORY_SIZE],
    history_len: [usize; HISTORY_SIZE],
    history_count: usize,
    history_pos: usize,
}

impl Shell {
    fn new() -> Self {
        Self {
            line: [0u8; LINE_BUF_SIZE],
            pos: 0,
            history: [[0u8; LINE_BUF_SIZE]; HISTORY_SIZE],
            history_len: [0; HISTORY_SIZE],
            history_count: 0,
            history_pos: 0,
        }
    }

    /// Print the prompt: [ns] C:\> 
    fn prompt(&self) {
        let mut cwd_buf = [0u8; 256];
        let cwd = match syscall::sys_getcwd(&mut cwd_buf) {
            Ok(n) if n > 0 => {
                let s = core::str::from_utf8(&cwd_buf[..n - 1]).unwrap_or("C:\\");
                s.as_bytes()
            }
            _ => b"C:\\",
        };
        write_str(PROMPT.as_bytes());
        write_str(cwd);
        write_str(b"> ");
    }

    /// Read one line from stdin, character by character
    fn readline(&mut self) {
        self.pos = 0;
        loop {
            let mut byte = [0u8; 1];
            let n = match syscall::sys_read(0, &mut byte) {
                Ok(n) => n,
                Err(_) => continue,
            };
            if n == 0 {
                continue;
            }
            match byte[0] {
                b'\r' | b'\n' => {
                    write_str(b"\r\n");
                    return;
                }
                0x08 | 0x7F => {
                    if self.pos > 0 {
                        self.pos -= 1;
                        write_str(b"\x08 \x08");
                    }
                }
                0x01 => {
                    // up arrow
                    if self.history_count > 0 && self.history_pos > 0 {
                        self.history_pos -= 1;
                        self.load_history();
                    }
                }
                0x02 => {
                    // down arrow
                    if self.history_pos < self.history_count {
                        self.history_pos += 1;
                        if self.history_pos >= self.history_count {
                            self.clear_echo();
                            self.pos = 0;
                        } else {
                            self.load_history();
                        }
                    }
                }
                c if c >= 0x20 => {
                    if self.pos < LINE_BUF_SIZE - 1 {
                        self.line[self.pos] = c;
                        self.pos += 1;
                        write_str(&[c]);
                    }
                }
                _ => {}
            }
        }
    }

    /// Clear the current line visually (overwrite with spaces)
    fn clear_echo(&self) {
        write_str(b"\r");
        for _ in 0..self.pos {
            write_str(b" ");
        }
        write_str(b"\r");
    }

    /// Load an entry from history into the line buffer
    fn load_history(&mut self) {
        self.clear_echo();
        self.pos = 0;
        let idx = self.history_pos;
        let len = self.history_len[idx];
        if len > 0 {
            self.line[..len].copy_from_slice(&self.history[idx][..len]);
            self.pos = len;
            write_str(&self.history[idx][..len]);
        }
    }

    /// Add a line to history
    fn add_history(&mut self, line: &[u8]) {
        let trimmed = trim_ascii(line);
        if trimmed.is_empty() {
            return;
        }
        // Avoid duplicate of last entry
        if self.history_count > 0 {
            let last = self.history_len[self.history_count - 1];
            if last == trimmed.len() && &self.history[self.history_count - 1][..last] == trimmed {
                self.history_pos = self.history_count;
                return;
            }
        }
        if self.history_count >= HISTORY_SIZE {
            // shift all entries up (drop oldest)
            for i in 1..HISTORY_SIZE {
                self.history[i - 1] = self.history[i];
                self.history_len[i - 1] = self.history_len[i];
            }
            self.history_count = HISTORY_SIZE - 1;
        }
        let n = trimmed.len().min(LINE_BUF_SIZE - 1);
        self.history[self.history_count][..n].copy_from_slice(&trimmed[..n]);
        self.history_len[self.history_count] = n;
        self.history_count += 1;
        self.history_pos = self.history_count;
    }

    /// Get the trimmed input line
    fn line_trimmed(&self) -> &[u8] {
        trim_ascii(&self.line[..self.pos])
    }

    /// Execute the typed command
    fn execute(&self) {
        let line = self.line_trimmed();
        if line.is_empty() {
            return;
        }
        let upper = first_token(line);

        // Drive change (e.g. "C:")
        if upper.len() == 2 && upper[1] == b':' {
            let drive_char = if upper[0] >= b'a' && upper[0] <= b'z' {
                upper[0] - 32
            } else {
                upper[0]
            };
            let mut path = [0u8; 4];
            path[0] = drive_char;
            path[1] = b':';
            path[2] = b'\\';
            if syscall::sys_chdir(core::str::from_utf8(&path[..3]).unwrap_or("C:\\")).is_err() {
                write_err(b"\r\nneoshell: invalid drive\r\n");
            }
            return;
        }

        let cmd_upper = {
            let mut buf = [0u8; 32];
            let n = upper.len().min(31);
            buf[..n].copy_from_slice(&upper[..n]);
            make_ascii_uppercase(&mut buf[..n]);
            buf
        };

        match &cmd_upper[..upper.len().min(31)] {
            b"HELP" => self.cmd_help(),
            b"CLS" => self.cmd_cls(),
            b"ECHO" => self.cmd_echo(),
            b"VER" => self.cmd_ver(),
            b"CD" => self.cmd_cd(),
            b"CWD" => self.cmd_cwd(),
            b"DIR" => self.cmd_dir_stub(),
            b"EXIT" => self.cmd_exit(),
            b"POWEROFF" => self.cmd_poweroff(),
            _ => {
                write_err(b"\r\nneoshell: '");
                write_err(upper);
                write_err(b"' is not recognized\r\n");
            }
        }
    }

    fn cmd_help(&self) {
        write_str(b"\r\nneoshell built-in commands:\r\n");
        write_str(b"  HELP    Show this help\r\n");
        write_str(b"  CLS     Clear screen\r\n");
        write_str(b"  ECHO    Print text\r\n");
        write_str(b"  VER     Show version\r\n");
        write_str(b"  CD      Change directory\r\n");
        write_str(b"  CWD     Show current directory\r\n");
        write_str(b"  DIR     List directory (coming in A4.6)\r\n");
        write_str(b"  POWEROFF  Power off machine\r\n");
        write_str(b"  EXIT    Return to Ring 0 shell\r\n");
        write_str(b"\r\nExternal commands via PATH.NXE (coming in A4.6)\r\n");
    }

    fn cmd_cls(&self) {
        write_str(b"\x1b[2J\x1b[H");
    }

    fn cmd_echo(&self) {
        write_str(b"\r\n");
        let rest = after_first_token(self.line_trimmed());
        write_str(rest);
        write_str(b"\r\n");
    }

    fn cmd_ver(&self) {
        write_str(b"\r\nneoshell v0.1.0 (Ring 3)\r\n");
    }

    fn cmd_cd(&self) {
        let raw = self.line_trimmed();
        let rest = after_first_token(raw);
        if rest.is_empty() {
            let _ = syscall::sys_chdir("\\");
            return;
        }
        // Build path as a stack-allocated buffer, then convert to &str
        let mut path_buf = [0u8; 260];
        let path_len: usize;
        if rest[0] != b'\\' && rest[0] != b'/' && !rest.contains(&b':') {
            // relative: prepend CWD
            let mut cwd_buf = [0u8; 256];
            let cwd = match syscall::sys_getcwd(&mut cwd_buf) {
                Ok(n) if n > 0 => core::str::from_utf8(&cwd_buf[..n - 1]).unwrap_or("C:\\"),
                _ => "C:\\",
            };
            let mut pos = 0;
            for &b in cwd.as_bytes() {
                if pos < 259 { path_buf[pos] = b; pos += 1; }
            }
            if !cwd.ends_with('\\') && !cwd.ends_with('/') {
                if pos < 259 { path_buf[pos] = b'\\'; pos += 1; }
            }
            for &b in rest {
                if pos < 259 { path_buf[pos] = b; pos += 1; }
            }
            path_len = pos;
        } else {
            let mut pos = 0;
            for &b in rest {
                if pos < 259 { path_buf[pos] = b; pos += 1; }
            }
            path_len = pos;
        }
        let path = core::str::from_utf8(&path_buf[..path_len]).unwrap_or("\\");
        match syscall::sys_chdir(path) {
            Ok(_) => {}
            Err(_) => {
                write_err(b"\r\nneoshell: CD: directory not found\r\n");
            }
        }
    }

    fn cmd_cwd(&self) {
        let mut buf = [0u8; 256];
        match syscall::sys_getcwd(&mut buf) {
            Ok(n) if n > 0 => {
                write_str(b"\r\n");
                write_str(&buf[..n - 1]);
                write_str(b"\r\n");
            }
            _ => {
                write_str(b"\r\nC:\\\r\n");
            }
        }
    }

    fn cmd_dir_stub(&self) {
        write_str(b"\r\nneoshell: DIR requires sys_readdir (A4.6)\r\n");
    }

    fn cmd_poweroff(&self) -> ! {
        write_str(b"\r\nneoshell: powering off...\r\n");
        syscall::sys_poweroff()
    }

    fn cmd_exit(&self) -> ! {
        write_str(b"\r\nneoshell: returning to Ring 0 shell...\r\n");
        syscall::sys_exit(0)
    }

    fn run(&mut self) -> ! {
        write_str(b"\r\n");
        write_str(b"neoshell v0.1.0 (Ring 3)\r\n");
        write_str(b"Type HELP for commands.\r\n\r\n");

        loop {
            self.prompt();
            self.readline();
            let trimmed = {
                let t = self.line_trimmed();
                let mut buf = [0u8; LINE_BUF_SIZE];
                let n = t.len().min(LINE_BUF_SIZE - 1);
                buf[..n].copy_from_slice(&t[..n]);
                (buf, n)
            };
            if trimmed.1 > 0 {
                self.add_history(&trimmed.0[..trimmed.1]);
            }
            self.execute();
        }
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut shell = Shell::new();
    shell.run()
}
