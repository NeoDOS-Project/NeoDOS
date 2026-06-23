#![no_std]
#![no_main]

use libneodos::syscall;

const LINE_BUF_SIZE: usize = 256;
const HISTORY_SIZE: usize = 32;
const MAX_ENV: usize = 16;
const ARGS_ADDR: u64 = 0x41F000;
static BUILTINS: &[&[u8]] = &[
    b"CWD",
    b"SET", b"EXIT", b"POWEROFF", b"CALL",
];

fn make_ascii_uppercase(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        if *b >= b'a' && *b <= b'z' {
            *b -= 32;
        }
    }
}

const MAX_PIPELINE: usize = 16;

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

/// Find pipe (`|`) positions in a trimmed line. Returns number of pipes found.
/// pipe_pos is filled with byte offsets into `line`.
fn parse_pipeline(line: &[u8], pipe_pos: &mut [usize; MAX_PIPELINE]) -> usize {
    let mut count = 0;
    for i in 0..line.len() {
        if line[i] == b'|' && count < MAX_PIPELINE {
            pipe_pos[count] = i;
            count += 1;
        }
    }
    count
}

fn is_builtin(upper_cmd: &[u8]) -> bool {
    for b in BUILTINS {
        if *b == upper_cmd { return true; }
    }
    false
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

fn to_ob_path<'a>(vfs: &'a str, buf: &'a mut [u8; 512]) -> &'a str {
    let prefix = b"\\Global\\FileSystem\\";
    let vfs_bytes = vfs.as_bytes();
    let total = prefix.len() + vfs_bytes.len();
    if total > 510 { return vfs; }
    buf[..prefix.len()].copy_from_slice(prefix);
    buf[prefix.len()..total].copy_from_slice(vfs_bytes);
    buf[total] = 0;
    unsafe { core::str::from_utf8_unchecked(&buf[..total]) }
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
                let s = core::str::from_utf8(&cwd_buf[..n]).unwrap_or("C:\\");
                write_str(s.as_bytes());
            }
            _ => {
                write_str(b"C:\\");
            }
        }
        write_str(b"> ");
    }

    fn write_cursor(&self) {
        write_str(b"\x5F");
    }

    fn erase_cursor(&self) {
        write_str(b"\x08 \x08");
    }

    fn readline(&mut self) {
        self.pos = 0;
        let _ = syscall::sys_cursor_blink(true);
        self.write_cursor();
        let mut done = false;
        while !done {
            let mut byte = [0u8; 1];
            let n = match syscall::sys_read(0, &mut byte) {
                Ok(n) => n,
                Err(_) => continue,
            };
            if n == 0 {
                continue;
            }
            self.erase_cursor();
            match byte[0] {
                b'\r' | b'\n' => {
                    write_str(b"\r\n");
                    done = true;
                }
                0x08 | 0x7F => {
                    if self.pos > 0 {
                        self.pos -= 1;
                        write_str(b"\x08 \x08");
                    }
                }
                0x09 => {
                    let line_copy = {
                        let t = trim_ascii(&self.line[..self.pos]);
                        let mut buf = [0u8; LINE_BUF_SIZE];
                        let n = t.len().min(LINE_BUF_SIZE - 1);
                        buf[..n].copy_from_slice(&t[..n]);
                        (buf, n)
                    };
                    if line_copy.1 > 0 {
                        let word = first_token(&line_copy.0[..line_copy.1]);
                        let word_len = word.len();
                        let is_first = !line_copy.0[..word_len]
                            .iter()
                            .any(|&b| b == b' ' || b == b'\t');
                        if is_first {
                            self.try_complete_cmd(word, word_len);
                            self.write_cursor();
                            continue;
                        }
                    }
                    self.write_cursor();
                }
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
                c if c >= 0x20 => {
                    if self.pos < LINE_BUF_SIZE - 1 {
                        self.line[self.pos] = c;
                        self.pos += 1;
                        write_str(&[c]);
                    }
                }
                _ => {}
            }
            if !done {
                self.write_cursor();
            }
        }
        let _ = syscall::sys_cursor_blink(false);
    }

    fn scan_path_for_completion(&self, word_upper: &[u8], word_upper_len: usize, matches: &mut [[u8; 32]; 64], match_lens: &mut [usize; 64], match_count: &mut usize) {
        let path_val = self.env_get(b"PATH").unwrap_or(b"\\Programs");
        let drive = self.get_drive_letter();
        let mut start = 0usize;
        loop {
            while start < path_val.len() && path_val[start] == b';' {
                start += 1;
            }
            if start >= path_val.len() { break; }
            let mut end = start;
            while end < path_val.len() && path_val[end] != b';' {
                end += 1;
            }
            let dir = &path_val[start..end];
            let mut dir_path = [0u8; 260];
            let mut pos = 0;
            dir_path[pos] = drive; pos += 1;
            dir_path[pos] = b':'; pos += 1;
            for &b in dir {
                if pos < 255 { dir_path[pos] = b; pos += 1; }
            }
            let dir_str = core::str::from_utf8(&dir_path[..pos]).unwrap_or("");
            let mut ob_buf = [0u8; 512];
            let ob_path = to_ob_path(dir_str, &mut ob_buf);
            if let Ok(fd) = syscall::sys_ob_open(ob_path, libneodos::syscall::ob_access::READ) {
                let mut entry = syscall::DirEntry {
                    inode: 0, mode: 0, size: 0, name: [0u8; 260],
                };
                loop {
                    match syscall::sys_readdir(fd, &mut entry) {
                        Ok(1) => {
                            let name = entry.name_str();
                            let name_bytes = name.as_bytes();
                            if name_bytes.len() > 4
                                && name_bytes[name_bytes.len()-4..].eq_ignore_ascii_case(b".NXE")
                                && name_bytes.len() - 4 >= word_upper_len
                            {
                                let stem_upper = {
                                    let mut b = [0u8; 32];
                                    let n = (name_bytes.len() - 4).min(31);
                                    b[..n].copy_from_slice(&name_bytes[..n]);
                                    make_ascii_uppercase(&mut b[..n]);
                                    b
                                };
                                if stem_upper[..word_upper_len] == word_upper[..word_upper_len] {
                                    if *match_count < 64 {
                                        let stem = &name_bytes[..name_bytes.len() - 4];
                                        let n = stem.len().min(31);
                                        matches[*match_count][..n].copy_from_slice(stem);
                                        match_lens[*match_count] = n;
                                        *match_count += 1;
                                    }
                                }
                            }
                        }
                        _ => break,
                    }
                }
                let _ = syscall::sys_close(fd);
            }
            start = end + 1;
        }
    }

    fn try_complete_cmd(&mut self, word: &[u8], word_len: usize) {
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

        self.scan_path_for_completion(&word_upper, word_upper_len, &mut matches, &mut match_lens, &mut match_count);

        if match_count == 0 {
            return;
        }

        if match_count == 1 {
            for _ in 0..word_len {
                if self.pos > 0 {
                    self.pos -= 1;
                    write_str(b"\x08 \x08");
                }
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
            self.insert_char_no_cursor(b' ');
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

    fn insert_char_no_cursor(&mut self, c: u8) {
        if self.pos < LINE_BUF_SIZE - 1 {
            self.line[self.pos] = c;
            self.pos += 1;
            write_str(&[c]);
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
            let mut ob_buf = [0u8; 512];
            let ob_path = to_ob_path(path_str, &mut ob_buf);
            let fd = syscall::sys_ob_open(ob_path, libneodos::syscall::ob_access::READ);
            if fd.is_ok() {
                let _ = syscall::sys_close(fd.unwrap());
                return Ok(full);
            }
            start = end + 1;
        }
        Err(())
    }

    fn execute(&mut self) {
        let line_len = self.pos;
        if line_len == 0 {
            return;
        }
        let mut line_buf = [0u8; LINE_BUF_SIZE];
        let n = line_len.min(LINE_BUF_SIZE - 1);
        line_buf[..n].copy_from_slice(&self.line[..n]);
        let trimmed = trim_ascii(&line_buf[..n]);
        if trimmed.is_empty() {
            return;
        }
        self.execute_line(trimmed);
    }

    fn execute_pipeline(&mut self, line: &[u8], pipe_pos: &[usize], num_pipes: usize) {
        let num_cmds = num_pipes + 1;
        let mut read_fds = [0u8; MAX_PIPELINE];
        let mut write_fds = [0u8; MAX_PIPELINE];

        // Create all pipes upfront
        for i in 0..num_pipes {
            let mut fds = [0u64; 2];
            if syscall::sys_pipe(&mut fds).is_err() {
                write_err(b"\r\nPipe error\r\n");
                return;
            }
            read_fds[i] = fds[0] as u8;
            write_fds[i] = fds[1] as u8;
        }

        let mut error = false;
        let mut cmd_start = 0;
        for cmd_idx in 0..num_cmds {
            let cmd_end = if cmd_idx < num_pipes { pipe_pos[cmd_idx] } else { line.len() };
            let cmd_slice = trim_ascii(&line[cmd_start..cmd_end]);
            cmd_start = cmd_end + 1;

            if cmd_slice.is_empty() {
                write_err(b"\r\nInvalid pipe syntax\r\n");
                error = true;
                break;
            }

            let cmd_name = first_token(cmd_slice);
            let cmd_args = after_first_token(cmd_slice);

            let mut cmd_upper = [0u8; 32];
            let cmd_upper_len = {
                let n = cmd_name.len().min(31);
                cmd_upper[..n].copy_from_slice(&cmd_name[..n]);
                make_ascii_uppercase(&mut cmd_upper[..n]);
                n
            };

            if is_builtin(&cmd_upper[..cmd_upper_len]) {
                write_err(b"\r\nCannot pipe built-in commands\r\n");
                error = true;
                break;
            }

            match self.resolve_command_path(&cmd_upper[..cmd_upper_len]) {
                Ok(full_path) => {
                    let path_str = core::str::from_utf8(
                        &full_path[..full_path.iter().position(|&b| b == 0).unwrap_or(full_path.len())]
                    ).unwrap_or("");

                    unsafe {
                        let dst = ARGS_ADDR as *mut u8;
                        let copy_len = cmd_args.len().min(255);
                        dst.write_bytes(0, 256);
                        core::ptr::copy_nonoverlapping(cmd_args.as_ptr(), dst, copy_len);
                        dst.add(copy_len).write(0);
                    }

                    let stdin_fd = if cmd_idx == 0 { 0xFF } else { read_fds[cmd_idx - 1] };
                    let stdout_fd = if cmd_idx == num_cmds - 1 { 0xFF } else { write_fds[cmd_idx] };

                    match syscall::sys_spawn(path_str, stdin_fd, stdout_fd, 0xFF) {
                        Ok(pid) => {
                            write_str(b"\r\n[PID ");
                            write_u64(pid as u64);
                            write_str(b"] ");
                            write_str(cmd_name);
                            write_str(b"\r\n");
                        }
                        Err(_) => {
                            write_err(b"\r\nBad command or file name\r\n");
                            error = true;
                            break;
                        }
                    }
                }
                Err(_) => {
                    write_err(b"\r\nBad command or file name\r\n");
                    error = true;
                    break;
                }
            }

            // Close pipe fds: previous command's read end (consumed) and
            // this command's write end (signals EOF to next reader).
            // Must happen before spawning the next command so the pipe
            // is write-closed for the next reader.
            if cmd_idx > 0 {
                let _ = syscall::sys_close(read_fds[cmd_idx - 1]);
            }
            if cmd_idx < num_pipes {
                let _ = syscall::sys_close(write_fds[cmd_idx]);
            }
        }

        // On error, close any remaining pipe fds (safe on already-closed handles)
        if error {
            for i in 0..num_pipes {
                let _ = syscall::sys_close(read_fds[i]);
                let _ = syscall::sys_close(write_fds[i]);
            }
        }
    }

    fn execute_line(&mut self, line: &[u8]) {
        let trimmed = trim_ascii(line);
        if trimmed.is_empty() {
            return;
        }

        let mut pipe_pos = [0usize; MAX_PIPELINE];
        let num_pipes = parse_pipeline(trimmed, &mut pipe_pos);
        if num_pipes > 0 {
            self.execute_pipeline(trimmed, &pipe_pos[..num_pipes], num_pipes);
            return;
        }

        if trimmed.len() == 2 && trimmed[1] == b':' {
            let drive_char = if trimmed[0] >= b'a' && trimmed[0] <= b'z' { trimmed[0] - 32 } else { trimmed[0] };
            let mut path = [0u8; 4];
            path[0] = drive_char;
            path[1] = b':';
            path[2] = b'\\';
            if syscall::sys_chdir(core::str::from_utf8(&path[..3]).unwrap_or("C:\\")).is_err() {
                write_err(b"\r\nInvalid drive\r\n");
            }
            return;
        }

        let upper = first_token(trimmed);
        let mut cmd_upper = [0u8; 32];
        let cmd_upper_len = {
            let n = upper.len().min(31);
            cmd_upper[..n].copy_from_slice(&upper[..n]);
            make_ascii_uppercase(&mut cmd_upper[..n]);
            n
        };

        match &cmd_upper[..cmd_upper_len] {
            b"CWD" => self.cmd_cwd(),
            b"SET" => self.cmd_set_line(trimmed),
            b"EXIT" => self.cmd_exit(),
            b"POWEROFF" => self.cmd_poweroff(),
            b"CALL" => self.cmd_call(trimmed),
            _ => {
                write_str(b"\r\n");
                let rest = after_first_token(trimmed);
                unsafe {
                    let dst = ARGS_ADDR as *mut u8;
                    let copy_len = rest.len().min(255);
                    dst.write_bytes(0, 256);
                    core::ptr::copy_nonoverlapping(rest.as_ptr(), dst, copy_len);
                    dst.add(copy_len).write(0);
                }
                match self.resolve_command_path(&cmd_upper[..cmd_upper_len]) {
                    Ok(full_path) => {
                        let path_str = core::str::from_utf8(
                            &full_path[..full_path.iter().position(|&b| b == 0).unwrap_or(full_path.len())]
                        ).unwrap_or("");
                        let is_cd_tool = path_str.ends_with("\\CD.NXE")
                            || path_str.eq_ignore_ascii_case("CD.NXE");
                        match syscall::sys_spawn(path_str, 0xFF, 0xFF, 0xFF) {
                            Ok(pid) => {
                                write_str(b"[PID ");
                                write_u64(pid as u64);
                                write_str(b"] ");
                                write_str(upper);
                                write_str(b"\r\n");
                                if syscall::sys_waitpid(pid).is_err() {
                                    write_err(b"waitpid error\r\n");
                                } else if is_cd_tool {
                                    let mut buf = [0u8; 256];
                                    unsafe {
                                        core::ptr::copy_nonoverlapping(ARGS_ADDR as *const u8, buf.as_mut_ptr(), buf.len());
                                    }
                                    let result = trim_ascii(&buf);
                                    if rest.is_empty() {
                                        if !result.is_empty() {
                                            write_str(b"\r\n");
                                            write_str(result);
                                            write_str(b"\r\n");
                                        }
                                    } else if !result.is_empty() {
                                        let path = core::str::from_utf8(result).unwrap_or("");
                                        if syscall::sys_chdir(path).is_err() {
                                            write_err(b"cd: directory not found\r\n");
                                        }
                                    }
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

    fn cmd_call(&mut self, line: &[u8]) {
        let rest = after_first_token(line);
        if rest.is_empty() {
            write_str(b"\r\nUsage: CALL batchfile\r\n");
            return;
        }
        let drive = self.get_drive_letter();
        let mut full_path = [0u8; 260];
        let mut pos = 0;
        full_path[pos] = drive; pos += 1;
        full_path[pos] = b':'; pos += 1;
        if rest[0] != b'\\' && rest[0] != b'/' {
            let mut cwd_buf = [0u8; 256];
            if let Ok(n) = syscall::sys_getcwd(&mut cwd_buf) {
                if n > 0 {
                    let cwd = &cwd_buf[..n - 1];
                    if cwd.len() > 2 {
                        for &b in cwd.iter().skip(2) {
                            if pos < 255 { full_path[pos] = b; pos += 1; }
                        }
                    }
                    if pos > 2 && full_path[pos - 1] != b'\\' {
                        if pos < 255 { full_path[pos] = b'\\'; pos += 1; }
                    }
                }
            }
        }
        for &b in rest {
            if pos < 255 { full_path[pos] = b; pos += 1; }
        }
        let path_str = core::str::from_utf8(&full_path[..pos]).unwrap_or("");
        let mut ob_buf = [0u8; 512];
        let ob_path = to_ob_path(path_str, &mut ob_buf);
        let fd = match syscall::sys_ob_open(ob_path, libneodos::syscall::ob_access::READ) {
            Ok(fd) => fd,
            Err(_) => {
                write_err(b"\r\nBatch file not found\r\n");
                return;
            }
        };
        let mut content = [0u8; 4096];
        let read_len = match syscall::sys_readfile(fd, &mut content) {
            Ok(n) => n,
            Err(_) => {
                let _ = syscall::sys_close(fd);
                write_err(b"\r\nError reading batch file\r\n");
                return;
            }
        };
        let _ = syscall::sys_close(fd);
        let content = &content[..read_len];

        let mut line_start = 0usize;
        while line_start < content.len() {
            let mut line_end = line_start;
            while line_end < content.len() && content[line_end] != b'\n' {
                line_end += 1;
            }
            let raw_line = &content[line_start..line_end];
            let trimmed = trim_ascii(raw_line);
            line_start = line_end + 1;

            if trimmed.is_empty() {
                continue;
            }
            if trimmed[0] == b':' || trimmed[0] == b'@' {
                continue;
            }
            if trimmed.eq_ignore_ascii_case(b"pause") {
                write_str(b"Press any key to continue . . .\r\n");
                let mut byte = [0u8; 1];
                let _ = syscall::sys_read(0, &mut byte);
                continue;
            }
            self.execute_line(trimmed);
        }
    }

    fn cmd_cwd(&self) {
        let mut buf = [0u8; 256];
        match syscall::sys_getcwd(&mut buf) {
            Ok(n) if n > 0 => {
                write_str(b"\r\n");
                write_str(&buf[..n]);
                write_str(b"\r\n");
            }
            _ => {
                write_str(b"\r\nC:\\\r\n");
            }
        }
    }

    fn cmd_set_line(&mut self, line: &[u8]) {
        let rest_raw = after_first_token(line);
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
        let mut ver = [0u8; 32];
        let ver_str = match syscall::sys_get_version(&mut ver) {
            Ok(n) if n > 0 => {
                let end = ver.iter().position(|&b| b == 0).unwrap_or(n.min(ver.len()));
                core::str::from_utf8(&ver[..end]).unwrap_or("?.?.?")
            }
            _ => "?.?.?",
        };
        write_str(ver_str.as_bytes());
        write_str(b" - RING3\r\n");
        write_str(b"Type HELP for a list of commands.\r\n");

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
