#![no_std]
#![no_main]

use libneodos::syscall;

const LINE_BUF_SIZE: usize = 256;
const HISTORY_SIZE: usize = 32;
const MAX_ENV: usize = 16;
static BUILTINS: &[&[u8]] = &[
    b"CLS", b"ECHO", b"CD", b"CWD",
    b"SET", b"EXIT", b"POWEROFF",
];

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

fn first_token(s: &[u8]) -> &[u8] {
    let trimmed = trim_ascii(s);
    for i in 0..trimmed.len() {
        if trimmed[i] == b' ' || trimmed[i] == b'\t' {
            return &trimmed[..i];
        }
    }
    trimmed
}

fn after_first_token(s: &[u8]) -> &[u8] {
    let trimmed = trim_ascii(s);
    for i in 0..trimmed.len() {
        if trimmed[i] == b' ' || trimmed[i] == b'\t' {
            return trim_ascii(&trimmed[i + 1..]);
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

fn write_u64(v: u64) {
    let mut buf = [0u8; 20];
    let mut i = 19;
    let mut n = v;
    if n == 0 {
        write_str(b"0");
        return;
    }
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if i == 0 { break; }
        i -= 1;
    }
    write_str(&buf[i..=19]);
}

#[derive(Copy, Clone)]
struct EnvVar {
    key: [u8; 32],
    key_len: usize,
    val: [u8; 128],
    val_len: usize,
}

struct Shell {
    line: [u8; LINE_BUF_SIZE],
    pos: usize,
    history: [[u8; LINE_BUF_SIZE]; HISTORY_SIZE],
    history_len: [usize; HISTORY_SIZE],
    history_count: usize,
    history_pos: usize,
    env: [EnvVar; MAX_ENV],
    env_count: usize,
}

impl Shell {
    fn new() -> Self {
        let mut s = Self {
            line: [0u8; LINE_BUF_SIZE],
            pos: 0,
            history: [[0u8; LINE_BUF_SIZE]; HISTORY_SIZE],
            history_len: [0; HISTORY_SIZE],
            history_count: 0,
            history_pos: 0,
            env: [EnvVar { key: [0u8; 32], key_len: 0, val: [0u8; 128], val_len: 0 }; MAX_ENV],
            env_count: 0,
        };
        s.env_set(b"PATH", b"\\Programs");
        s
    }

    fn env_get(&self, key: &[u8]) -> Option<&[u8]> {
        for i in 0..self.env_count {
            if self.env[i].key_len == key.len() && &self.env[i].key[..key.len()] == key {
                return Some(&self.env[i].val[..self.env[i].val_len]);
            }
        }
        None
    }

    fn env_set(&mut self, key: &[u8], val: &[u8]) {
        let klen = key.len().min(31);
        let vlen = val.len().min(127);
        for i in 0..self.env_count {
            if self.env[i].key_len == klen && &self.env[i].key[..klen] == key {
                self.env[i].val[..vlen].copy_from_slice(&val[..vlen]);
                self.env[i].val_len = vlen;
                return;
            }
        }
        if self.env_count < MAX_ENV {
            let idx = self.env_count;
            self.env[idx].key[..klen].copy_from_slice(&key[..klen]);
            self.env[idx].key_len = klen;
            self.env[idx].val[..vlen].copy_from_slice(&val[..vlen]);
            self.env[idx].val_len = vlen;
            self.env_count += 1;
        }
    }

    fn prompt(&self) {
        let mut cwd_buf = [0u8; 256];
        match syscall::sys_getcwd(&mut cwd_buf) {
            Ok(n) if n > 0 => {
                let s = core::str::from_utf8(&cwd_buf[..n - 1]).unwrap_or("C:\\");
                write_str(s.as_bytes());
            }
            _ => {
                write_str(b"C:\\");
            }
        }
        write_str(b"> ");
    }

    fn backspace(&mut self) {
        if self.pos > 0 {
            self.pos -= 1;
            write_str(b"\x08 \x08");
        }
    }

    fn insert_char(&mut self, c: u8) {
        if self.pos < LINE_BUF_SIZE - 1 {
            self.line[self.pos] = c;
            self.pos += 1;
            write_str(&[c]);
        }
    }

    fn try_complete(&mut self) {
        let trimmed = trim_ascii(&self.line[..self.pos]);
        if trimmed.is_empty() {
            return;
        }
        let word = first_token(trimmed);
        let word_len = word.len();
        let is_first = !trimmed[..word_len]
            .iter()
            .any(|&b| b == b' ' || b == b'\t');
        if !is_first {
            return;
        }
        let mut word_upper = [0u8; 32];
        let word_upper_len = {
            let n = word_len.min(31);
            word_upper[..n].copy_from_slice(&word[..n]);
            make_ascii_uppercase(&mut word_upper[..n]);
            n
        };
        let mut matches: [[u8; 32]; 64] = [[0u8; 32]; 64];
        let mut match_lens: [usize; 64] = [0; 64];
        let mut match_count = 0usize;

        for builtin in BUILTINS {
            if builtin.len() >= word_upper_len && &builtin[..word_upper_len] == &word_upper[..word_upper_len] {
                if match_count < 64 {
                    let n = builtin.len().min(31);
                    matches[match_count][..n].copy_from_slice(&builtin[..n]);
                    match_lens[match_count] = n;
                    match_count += 1;
                }
            }
        }

        if match_count == 0 {
            return;
        }

        if match_count == 1 {
            for _ in 0..word_len {
                self.backspace();
            }
            self.pos = 0;
            let m = &matches[0];
            let ml = match_lens[0];
            let mut lower = [0u8; 32];
            lower[..ml].copy_from_slice(&m[..ml]);
            for b in lower[..ml].iter_mut() {
                if *b >= b'A' && *b <= b'Z' {
                    *b += 32;
                }
            }
            for &c in lower[..ml].iter() {
                if self.pos < LINE_BUF_SIZE - 1 {
                    self.line[self.pos] = c;
                    self.pos += 1;
                    write_str(&[c]);
                }
            }
            self.insert_char(b' ');
            return;
        }

        write_str(b"\r\n");
        for i in 0..match_count {
            write_str(&matches[i][..match_lens[i]]);
            write_str(b"  ");
        }
        write_str(b"\r\n");
        self.prompt();
        write_str(&self.line[..self.pos]);
    }

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
                0x08 | 0x7F => self.backspace(),
                0x09 => self.try_complete(),
                0x01 => {
                    if self.history_count > 0 && self.history_pos > 0 {
                        self.history_pos -= 1;
                        self.load_history();
                    }
                }
                0x02 => {
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
                c if c >= 0x20 => self.insert_char(c),
                _ => {}
            }
        }
    }

    fn clear_echo(&self) {
        write_str(b"\r");
        for _ in 0..self.pos {
            write_str(b" ");
        }
        write_str(b"\r");
    }

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

    fn add_history(&mut self, line: &[u8]) {
        let trimmed = trim_ascii(line);
        if trimmed.is_empty() {
            return;
        }
        if self.history_count > 0 {
            let last = self.history_len[self.history_count - 1];
            if last == trimmed.len() && &self.history[self.history_count - 1][..last] == trimmed {
                self.history_pos = self.history_count;
                return;
            }
        }
        if self.history_count >= HISTORY_SIZE {
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

    fn line_trimmed(&self) -> &[u8] {
        trim_ascii(&self.line[..self.pos])
    }

    fn get_drive_letter(&self) -> u8 {
        let mut cwd_buf = [0u8; 256];
        match syscall::sys_getcwd(&mut cwd_buf) {
            Ok(n) if n > 0 && cwd_buf[1] == b':' => cwd_buf[0],
            _ => b'C',
        }
    }

    fn resolve_command_path(&self, cmd_upper: &[u8]) -> Result<[u8; 260], ()> {
        let path_val = self.env_get(b"PATH").unwrap_or(b"\\Programs");
        let drive = self.get_drive_letter();
        let mut start = 0usize;
        loop {
            while start < path_val.len() && path_val[start] == b';' {
                start += 1;
            }
            if start >= path_val.len() {
                break;
            }
            let mut end = start;
            while end < path_val.len() && path_val[end] != b';' {
                end += 1;
            }
            let dir = &path_val[start..end];
            let mut full = [0u8; 260];
            let mut pos = 0;
            full[pos] = drive; pos += 1;
            full[pos] = b':'; pos += 1;
            for &b in dir {
                if pos < 255 { full[pos] = b; pos += 1; }
            }
            if pos > 0 && full[pos - 1] != b'\\' {
                if pos < 255 { full[pos] = b'\\'; pos += 1; }
            }
            for &b in cmd_upper {
                if pos < 255 { full[pos] = b; pos += 1; }
            }
            if pos + 4 < 260 {
                full[pos] = b'.'; full[pos + 1] = b'N'; full[pos + 2] = b'X'; full[pos + 3] = b'E';
                pos += 4;
            }
            let path_str = core::str::from_utf8(&full[..pos]).unwrap_or("");
            let fd = syscall::sys_open(path_str);
            if fd.is_ok() {
                let _ = syscall::sys_close(fd.unwrap());
                return Ok(full);
            }
            start = end + 1;
        }
        Err(())
    }

    fn execute(&mut self) {
        let line = self.line_trimmed();
        if line.is_empty() {
            return;
        }

        if line.len() == 2 && line[1] == b':' {
            let drive_char = if line[0] >= b'a' && line[0] <= b'z' { line[0] - 32 } else { line[0] };
            let mut path = [0u8; 4];
            path[0] = drive_char;
            path[1] = b':';
            path[2] = b'\\';
            if syscall::sys_chdir(core::str::from_utf8(&path[..3]).unwrap_or("C:\\")).is_err() {
                write_err(b"\r\nInvalid drive\r\n");
            }
            return;
        }

        let upper = first_token(line);
        let mut cmd_upper = [0u8; 32];
        let cmd_upper_len = {
            let n = upper.len().min(31);
            cmd_upper[..n].copy_from_slice(&upper[..n]);
            make_ascii_uppercase(&mut cmd_upper[..n]);
            n
        };

        match &cmd_upper[..cmd_upper_len] {
            b"CLS" => self.cmd_cls(),
            b"ECHO" => self.cmd_echo(),
            b"CD" => self.cmd_cd(),
            b"CWD" => self.cmd_cwd(),
            b"SET" => self.cmd_set(),
            b"EXIT" => self.cmd_exit(),
            b"POWEROFF" => self.cmd_poweroff(),
            _ => {
                write_str(b"\r\n");
                match self.resolve_command_path(&cmd_upper[..cmd_upper_len]) {
                    Ok(full_path) => {
                        let path_str = core::str::from_utf8(
                            &full_path[..full_path.iter().position(|&b| b == 0).unwrap_or(full_path.len())]
                        ).unwrap_or("");
                        match syscall::sys_spawn(path_str, 0xFF, 0xFF, 0xFF) {
                            Ok(pid) => {
                                write_str(b"[PID ");
                                write_u64(pid as u64);
                                write_str(b"] ");
                                write_str(upper);
                                write_str(b"\r\n");
                                if syscall::sys_waitpid(pid).is_err() {
                                    write_err(b"waitpid error\r\n");
                                }
                            }
                            Err(_) => {
                                write_err(b"Bad command or file name\r\n");
                            }
                        }
                    }
                    Err(_) => {
                        write_err(b"Bad command or file name\r\n");
                    }
                }
            }
        }
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

    fn cmd_cd(&mut self) {
        let raw = self.line_trimmed();
        let rest = after_first_token(raw);
        if rest.is_empty() {
            let _ = syscall::sys_chdir("\\");
            return;
        }
        let mut path_buf = [0u8; 260];
        let path_len: usize;
        if rest[0] != b'\\' && rest[0] != b'/' && !rest.contains(&b':') {
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

    fn cmd_set(&mut self) {
        let trimmed = trim_ascii(&self.line[..self.pos]);
        let rest_raw = after_first_token(trimmed);
        let mut rest_buf = [0u8; 128];
        let rest_len = rest_raw.len().min(127);
        rest_buf[..rest_len].copy_from_slice(&rest_raw[..rest_len]);
        let rest = &rest_buf[..rest_len];

        if rest.is_empty() {
            write_str(b"\r\n");
            for i in 0..self.env_count {
                write_str(&self.env[i].key[..self.env[i].key_len]);
                write_str(b"=");
                write_str(&self.env[i].val[..self.env[i].val_len]);
                write_str(b"\r\n");
            }
            return;
        }
        if let Some(eq_pos) = rest.iter().position(|&b| b == b'=') {
            let key = &rest[..eq_pos];
            let val = &rest[eq_pos + 1..];
            let mut key_upper = [0u8; 32];
            let key_len = key.len().min(31);
            key_upper[..key_len].copy_from_slice(&key[..key_len]);
            make_ascii_uppercase(&mut key_upper[..key_len]);
            self.env_set(&key_upper[..key_len], val);
            write_str(b"\r\n");
        } else {
            let mut key_upper = [0u8; 32];
            let key_len = rest.len().min(31);
            key_upper[..key_len].copy_from_slice(&rest[..key_len]);
            make_ascii_uppercase(&mut key_upper[..key_len]);
            match self.env_get(&key_upper[..key_len]) {
                Some(v) => { write_str(b"\r\n"); write_str(v); write_str(b"\r\n"); }
                None => { write_str(b"\r\n"); }
            }
        }
    }

    fn cmd_poweroff(&self) -> ! {
        write_str(b"\r\npowering off...\r\n");
        syscall::sys_poweroff()
    }

    fn cmd_exit(&self) -> ! {
        syscall::sys_exit(0)
    }

    fn run(&mut self) -> ! {
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
