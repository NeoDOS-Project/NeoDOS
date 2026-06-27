#![no_std]
#![no_main]

use libneodos::{console, syscall};

const LINE_BUF_SIZE: usize = 256;
const MAX_ENV: usize = 16;
const ARGS_ADDR: u64 = 0x41F000;
const MAX_PIPELINE: usize = 16;

static BUILTINS: &[&[u8]] = &[
    b"CWD", b"SET", b"EXIT", b"POWEROFF", b"CALL",
];

// ── Completion context ─────────────────────────

static mut COMPL_PATH: [u8; 256] = [0; 256];
static mut COMPL_PATH_LEN: usize = 0;

fn set_completion_ctx(_drive: u8, path: &[u8]) {
    unsafe {
        let n = path.len().min(255);
        COMPL_PATH[..n].copy_from_slice(&path[..n]);
        COMPL_PATH_LEN = n;
    }
}

// ── Utilities ──────────────────────────────────

fn make_ascii_uppercase(buf: &mut [u8]) {
    for b in buf.iter_mut() { if *b >= b'a' && *b <= b'z' { *b -= 32; } }
}

fn trim_ascii(s: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < s.len() && matches!(s[start], b' ' | b'\t' | b'\r' | b'\n') { start += 1; }
    let mut end = s.len();
    while end > start && matches!(s[end - 1], b' ' | b'\t' | b'\r' | b'\n') { end -= 1; }
    &s[start..end]
}

fn first_token(s: &[u8]) -> &[u8] {
    let t = trim_ascii(s);
    t.split(|&b| b == b' ' || b == b'\t').next().unwrap_or(t)
}

fn after_first_token(s: &[u8]) -> &[u8] {
    let t = trim_ascii(s);
    if let Some(p) = t.iter().position(|&b| b == b' ' || b == b'\t') {
        trim_ascii(&t[p + 1..])
    } else { &[] }
}

fn to_ob_path<'a>(vfs: &'a str, buf: &'a mut [u8; 512]) -> &'a str {
    let p = b"\\Global\\FileSystem\\";
    let vb = vfs.as_bytes();
    let t = p.len() + vb.len();
    if t > 510 { return vfs; }
    buf[..p.len()].copy_from_slice(p);
    buf[p.len()..t].copy_from_slice(vb);
    buf[t] = 0;
    unsafe { core::str::from_utf8_unchecked(&buf[..t]) }
}

fn write_str(s: &[u8]) { let _ = syscall::sys_write(1, s); }
fn write_err(s: &[u8]) { let _ = syscall::sys_write(2, s); }

fn write_u64(v: u64) {
    if v == 0 { write_str(b"0"); return; }
    let mut b = [0u8; 20]; let mut i = 19; let mut n = v;
    while n > 0 { b[i] = b'0' + (n % 10) as u8; n /= 10; if i == 0 { break; } i -= 1; }
    write_str(&b[i..=19]);
}

fn get_vt_num() -> u8 {
    let mut b = [0u8; 4];
    if let Ok(fd) = syscall::sys_ob_open("\\Global\\Info\\VtInfo", syscall::ob_access::READ) {
        if let Ok(n) = syscall::sys_ob_query_info(fd, syscall::ObInfoClass::ReadContent, &mut b) {
            if n >= 1 { let _ = syscall::sys_close(fd); return b[0]; }
        }
        let _ = syscall::sys_close(fd);
    }
    0
}

fn parse_pipeline(line: &[u8], pos: &mut [usize; MAX_PIPELINE]) -> usize {
    let mut c = 0;
    for i in 0..line.len() { if line[i] == b'|' && c < MAX_PIPELINE { pos[c] = i; c += 1; } }
    c
}

// ── TAB completion handler ─────────────────────

extern "C" fn shell_complete(input: *const u8, cursor: i32, cand: *mut u8, max: i32) -> i32 {
    let t = unsafe { trim_ascii(core::slice::from_raw_parts(input, cursor.max(0) as usize)) };
    if t.is_empty() { return 0; }
    let word = first_token(t);
    let wup = { let mut b = [0u8; 32]; let n = word.len().min(31); b[..n].copy_from_slice(&word[..n]); make_ascii_uppercase(&mut b[..n]); b };
    let wlen = word.len().min(31);
    let max = max as usize;
    let mut p = 0usize; let mut cnt = 0i32;
    let mut add = |name: &[u8]| {
        let n = name.len().min(max - p - 1);
        if n > 0 && p + n + 1 <= max {
            unsafe { core::ptr::copy_nonoverlapping(name.as_ptr(), cand.add(p), n); *cand.add(p + n) = 0; }
            p += n + 1; cnt += 1;
        }
    };
    for builtin in BUILTINS {
        if builtin.len() >= wlen && builtin[..wlen] == wup[..wlen] { add(builtin); }
    }
    // Scan PATH
    let path_val = unsafe {
        let ptr = core::ptr::addr_of!(COMPL_PATH) as *const u8;
        core::slice::from_raw_parts(ptr, COMPL_PATH_LEN)
    };
    let store = if path_val.is_empty() { b"\\Programs" } else { path_val };
    let drive_byte = b'C'; // simplified
    let mut s = 0usize;
    loop {
        while s < store.len() && store[s] == b';' { s += 1; }
        if s >= store.len() { break; }
        let mut e = s;
        while e < store.len() && store[e] != b';' { e += 1; }
        let dir = &store[s..e];
        let mut dp = [0u8; 260]; let mut dp_pos = 0;
        dp[dp_pos] = drive_byte; dp_pos += 1; dp[dp_pos] = b':'; dp_pos += 1;
        for &b in dir { if dp_pos < 255 { dp[dp_pos] = b; dp_pos += 1; } }
        let ds = core::str::from_utf8(&dp[..dp_pos]).unwrap_or("");
        let mut ob = [0u8; 512];
        let obp = to_ob_path(ds, &mut ob);
        if let Ok(fd) = syscall::sys_ob_open(obp, syscall::ob_access::READ) {
            let mut ents: [syscall::ObEnumEntry; 64] = core::array::from_fn(|_| syscall::ObEnumEntry {
                id:0, obj_type:0, name:[0;32], mode:0, _pad:[0;2], size:0,
            });
            if let Ok(n) = syscall::sys_ob_enum(fd, &mut ents) {
                for i in 0..n {
                    let nm = ents[i].name_str(); let nb = nm.as_bytes();
                    if nb.len() > 4 && nb[nb.len()-4..].eq_ignore_ascii_case(b".NXE") && nb.len()-4 >= wlen {
                        let su = { let mut b=[0u8;32]; let n=(nb.len()-4).min(31); b[..n].copy_from_slice(&nb[..n]); make_ascii_uppercase(&mut b[..n]); b };
                        if su[..wlen] == wup[..wlen] { add(&nb[..nb.len()-4]); }
                    }
                }
            }
            let _ = syscall::sys_close(fd);
        }
        s = e + 1;
    }
    cnt
}

// ── Environment ────────────────────────────────

#[derive(Copy, Clone)]
struct EnvVar { key: [u8; 32], key_len: usize, val: [u8; 128], val_len: usize }

// ── Shell ──────────────────────────────────────

struct Shell {
    line: [u8; LINE_BUF_SIZE],
    pos: usize,
    pending: [u8; LINE_BUF_SIZE],
    pending_len: usize,
    env: [EnvVar; MAX_ENV],
    env_count: usize,
}

impl Shell {
    fn new() -> Self {
        let mut s = Self {
            line: [0; LINE_BUF_SIZE], pos: 0,
            pending: [0; LINE_BUF_SIZE], pending_len: 0,
            env: [EnvVar { key: [0;32], key_len:0, val:[0;128], val_len:0 }; MAX_ENV], env_count: 0,
        };
        s.env_set(b"PATH", b"\\Programs"); s
    }

    fn env_get(&self, key: &[u8]) -> Option<&[u8]> {
        for i in 0..self.env_count { if self.env[i].key_len == key.len() && &self.env[i].key[..key.len()] == key { return Some(&self.env[i].val[..self.env[i].val_len]); } }
        None
    }

    fn env_set(&mut self, key: &[u8], val: &[u8]) {
        let kl = key.len().min(31); let vl = val.len().min(127);
        for i in 0..self.env_count {
            if self.env[i].key_len == kl && &self.env[i].key[..kl] == key { self.env[i].val[..vl].copy_from_slice(&val[..vl]); self.env[i].val_len = vl; return; }
        }
        if self.env_count < MAX_ENV {
            let i = self.env_count; self.env[i].key[..kl].copy_from_slice(&key[..kl]); self.env[i].key_len = kl;
            self.env[i].val[..vl].copy_from_slice(&val[..vl]); self.env[i].val_len = vl; self.env_count += 1;
        }
    }

    fn prompt(&self) {
        let mut b = [0u8; 256];
        match syscall::sys_getcwd(&mut b) {
            Ok(n) if n > 0 => write_str(&b[..n]),
            _ => write_str(b"C:\\"),
        }
        write_str(b"> ");
    }

    fn get_drive(&self) -> u8 {
        let mut b = [0u8; 256];
        match syscall::sys_getcwd(&mut b) { Ok(n) if n > 0 && b[1] == b':' => b[0], _ => b'C' }
    }

    // ── Readline ────────────────────────────

    fn readline(&mut self) {
        self.pos = 0;
        let _ = syscall::sys_cursor_blink(true);
        write_str(b"\x5F");
        loop {
            let b = console::read_byte();
            if b < 0 { continue; }
            write_str(b"\x08 \x08");
            match b as u8 {
                b'\r' | b'\n' => {
                    write_str(b"\r\n");
                    if self.pos > 0 {
                        let trimmed = trim_ascii(&self.line[..self.pos]);
                        if !trimmed.is_empty() {
                            let mut e = [0u8; 128];
                            let n = trimmed.len().min(127);
                            e[..n].copy_from_slice(&trimmed[..n]); e[n] = 0;
                            console::history_add_raw(e.as_ptr());
                        }
                    }
                    break;
                }
                0x08 | 0x7F => {
                    if self.pos > 0 { self.pos -= 1; write_str(b"\x08 \x08"); }
                }
                0x01 => { // Up
                    let ptr = console::history_prev();
                    if !ptr.is_null() {
                        // Clear echo
                        write_str(b"\r");
                        for _ in 0..self.pos { write_str(b" "); }
                        write_str(b"\r");
                        // Load
                        let s = unsafe {
                            let mut n = 0;
                            while n < LINE_BUF_SIZE - 1 && *ptr.add(n) != 0 { n += 1; }
                            core::slice::from_raw_parts(ptr, n)
                        };
                        self.pos = s.len().min(LINE_BUF_SIZE - 1);
                        self.line[..self.pos].copy_from_slice(&s[..self.pos]);
                        write_str(&self.line[..self.pos]);
                    }
                }
                0x02 => { // Down
                    let ptr = console::history_next();
                    // Clear echo
                    write_str(b"\r");
                    for _ in 0..self.pos { write_str(b" "); }
                    write_str(b"\r");
                    if !ptr.is_null() {
                        let s = unsafe {
                            let mut n = 0;
                            while n < LINE_BUF_SIZE - 1 && *ptr.add(n) != 0 { n += 1; }
                            core::slice::from_raw_parts(ptr, n)
                        };
                        self.pos = s.len().min(LINE_BUF_SIZE - 1);
                        self.line[..self.pos].copy_from_slice(&s[..self.pos]);
                    } else {
                        // Back to pending
                        self.pos = self.pending_len;
                        self.line[..self.pos].copy_from_slice(&self.pending[..self.pos]);
                    }
                    write_str(&self.line[..self.pos]);
                }
                0x09 => { // TAB
                    let t = trim_ascii(&self.line[..self.pos]);
                    if !t.is_empty() {
                        let word = first_token(t);
                        let wlen = word.len();
                        if !t[..wlen].contains(&(b' ')) && !t[..wlen].contains(&(b'\t')) {
                            // Call completion handler
                            let mut cbuf = [0u8; 512];
                            let n = shell_complete(self.line.as_ptr(), self.pos as i32, cbuf.as_mut_ptr(), 512);
                            if n > 0 {
                                let mut first = 0;
                                while first < 512 && cbuf[first] != 0 { first += 1; }
                                if n == 1 {
                                    // Replace word
                                    for _ in 0..wlen {
                                        if self.pos > 0 { self.pos -= 1; write_str(b"\x08 \x08"); }
                                    }
                                    let lower = {
                                        let mut b = [0u8; 32];
                                        b[..first].copy_from_slice(&cbuf[..first]);
                                        for c in b[..first].iter_mut() { if *c >= b'A' && *c <= b'Z' { *c += 32; } }
                                        b
                                    };
                                    self.line[self.pos..self.pos+first].copy_from_slice(&lower[..first]);
                                    self.pos += first;
                                    write_str(&lower[..first]);
                                    if self.pos < LINE_BUF_SIZE - 1 { self.line[self.pos] = b' '; self.pos += 1; write_str(b" "); }
                                } else {
                                    write_str(b"\r\n");
                                    let mut ci = 0usize;
                                    while ci < 512 && cbuf[ci] != 0 {
                                        while ci < 512 && cbuf[ci] != 0 { write_str(&[cbuf[ci]]); ci += 1; }
                                        ci += 1; write_str(b"  ");
                                    }
                                    write_str(b"\r\n");
                                    self.prompt();
                                    write_str(&self.line[..self.pos]);
                                }
                            }
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
            if self.pos > 0 { write_str(b"\x5F"); }
        }
        let _ = syscall::sys_cursor_blink(false);
    }

    // ── Command execution ───────────────────

    fn resolve_path(&self, cmd: &[u8]) -> Result<[u8; 260], ()> {
        let pv = self.env_get(b"PATH").unwrap_or(b"\\Programs");
        let dr = self.get_drive();
        let mut s = 0usize;
        loop {
            while s < pv.len() && pv[s] == b';' { s += 1; }
            if s >= pv.len() { break; }
            let mut e = s; while e < pv.len() && pv[e] != b';' { e += 1; }
            let dir = &pv[s..e];
            let mut f = [0u8; 260]; let mut p = 0;
            f[p] = dr; p += 1; f[p] = b':'; p += 1;
            for &b in dir { if p < 255 { f[p] = b; p += 1; } }
            if p > 0 && f[p-1] != b'\\' && p < 255 { f[p] = b'\\'; p += 1; }
            for &b in cmd { if p < 255 { f[p] = b; p += 1; } }
            if p+4 < 260 { f[p]=b'.'; f[p+1]=b'N'; f[p+2]=b'X'; f[p+3]=b'E'; p+=4; }
            let fs = core::str::from_utf8(&f[..p]).unwrap_or("");
            let mut ob = [0u8; 512];
            let obp = to_ob_path(fs, &mut ob);
            if syscall::sys_ob_open(obp, syscall::ob_access::READ).is_ok() { return Ok(f); }
            s = e + 1;
        }
        Err(())
    }

    fn execute_line(&mut self, line: &[u8]) {
        let trimmed = trim_ascii(line);
        if trimmed.is_empty() { return; }
        let mut pp = [0usize; MAX_PIPELINE];
        if parse_pipeline(trimmed, &mut pp) > 0 {
            self.execute_pipeline(trimmed, &pp); return;
        }
        if trimmed.len() == 2 && trimmed[1] == b':' {
            let dc = if trimmed[0] >= b'a' && trimmed[0] <= b'z' { trimmed[0] - 32 } else { trimmed[0] };
            let path = [dc, b':', b'\\'];
            if let Ok(fd) = syscall::sys_ob_open("\\Global\\Info\\Cwd", syscall::ob_access::WRITE) {
                let _ = syscall::sys_ob_set_info(fd, syscall::ob_set_info_class::SET_CWD, &path);
                let _ = syscall::sys_close(fd);
            } else { write_err(b"\r\nInvalid drive\r\n"); }
            return;
        }
        let up = first_token(trimmed);
        let mut cu = [0u8; 32];
        let cul = { let n = up.len().min(31); cu[..n].copy_from_slice(&up[..n]); make_ascii_uppercase(&mut cu[..n]); n };
        match &cu[..cul] {
            b"CWD" => self.cmd_cwd(),
            b"SET" => self.cmd_set(trimmed),
            b"EXIT" => self.cmd_exit(),
            b"POWEROFF" => self.cmd_poweroff(),
            b"CALL" => self.cmd_call(trimmed),
            _ => {
                write_str(b"\r\n");
                let rest = after_first_token(trimmed);
                unsafe { let d = ARGS_ADDR as *mut u8; d.write_bytes(0,256); let n=rest.len().min(255); core::ptr::copy_nonoverlapping(rest.as_ptr(),d,n); d.add(n).write(0); }
                match self.resolve_path(&cu[..cul]) {
                    Ok(full) => {
                        let fs = core::str::from_utf8(&full[..full.iter().position(|&b|b==0).unwrap_or(full.len())]).unwrap_or("");
                        let iscd = fs.ends_with("\\CD.NXE") || fs.eq_ignore_ascii_case("CD.NXE");
                        let pk = (0xFFu64)|((0xFFu64)<<8)|((0xFFu64)<<16);
                        let mut ob = [0u8; 512];
                        let obp = to_ob_path(fs, &mut ob);
                        match syscall::sys_ob_create(obp, syscall::ob_type::PROCESS, None, pk) {
                            Ok(fd) => {
                                write_str(b"[OB "); write_u64(fd as u64); write_str(b"] "); write_str(up); write_str(b"\r\n");
                                if syscall::sys_ob_wait(fd).is_err() { write_err(b"ob_wait error\r\n"); }
                                else if iscd {
                                    let mut rb = [0u8; 256];
                                    unsafe { core::ptr::copy_nonoverlapping(ARGS_ADDR as *const u8, rb.as_mut_ptr(), 256); }
                                    let r = trim_ascii(&rb);
                                    if rest.is_empty() { if !r.is_empty() { write_str(b"\r\n"); write_str(r); write_str(b"\r\n"); } }
                                    else if !r.is_empty() {
                                        let p = core::str::from_utf8(r).unwrap_or("");
                                        let pb = p.as_bytes();
                                        if let Ok(cf) = syscall::sys_ob_open("\\Global\\Info\\Cwd", syscall::ob_access::WRITE) {
                                            let _ = syscall::sys_ob_set_info(cf, syscall::ob_set_info_class::SET_CWD, pb);
                                            let _ = syscall::sys_close(cf);
                                        } else { write_err(b"cd: directory not found\r\n"); }
                                    }
                                }
                                let _ = syscall::sys_close(fd);
                            }
                            Err(_) => { write_err(b"Bad command or file name\r\n"); }
                        }
                    }
                    Err(_) => { write_err(b"Bad command or file name\r\n"); }
                }
            }
        }
    }

    fn execute_pipeline(&mut self, line: &[u8], pp: &[usize]) {
        let nc = pp.len() + 1;
        let mut rf = [0u8; MAX_PIPELINE];
        let mut wf = [0u8; MAX_PIPELINE];
        for i in 0..pp.len() {
            let mut fds = [0u64;2];
            let mut pn = [0u8;16]; pn[..7].copy_from_slice(b"\\Pipe/p");
            let mut pos = 7; let mut v = i as u64;
            if v==0 { pn[pos]=b'0'; pos+=1; }
            else { let mut d=[0u8;4]; let mut nd=0; while v>0&&nd<4 { d[nd]=b'0'+(v%10)as u8; v/=10; nd+=1; } for di in(0..nd).rev(){ pn[pos]=d[di]; pos+=1; } }
            pn[pos]=0;
            let ps = unsafe { core::str::from_utf8_unchecked(&pn[..pos]) };
            if syscall::sys_ob_create(ps,4,Some(&mut fds),0).is_err() { write_err(b"\r\nPipe error\r\n"); return; }
            rf[i]=fds[0] as u8; wf[i]=fds[1] as u8;
        }
        let mut err = false; let mut cs = 0;
        for ci in 0..nc {
            let ce = if ci<pp.len() { pp[ci] } else { line.len() };
            let sl = trim_ascii(&line[cs..ce]); cs = ce+1;
            if sl.is_empty() { write_err(b"\r\nInvalid pipe syntax\r\n"); err=true; break; }
            let cn = first_token(sl); let ca = after_first_token(sl);
            let mut cu=[0u8;32]; let cl={let n=cn.len().min(31); cu[..n].copy_from_slice(&cn[..n]); make_ascii_uppercase(&mut cu[..n]); n};
            if BUILTINS.iter().any(|&bi| bi == &cu[..cl]) { write_err(b"\r\nCannot pipe built-in\r\n"); err=true; break; }
            match self.resolve_path(&cu[..cl]) {
                Ok(full) => {
                    let fs = core::str::from_utf8(&full[..full.iter().position(|&b|b==0).unwrap_or(full.len())]).unwrap_or("");
                    unsafe { let d=ARGS_ADDR as *mut u8; d.write_bytes(0,256); let n=ca.len().min(255); core::ptr::copy_nonoverlapping(ca.as_ptr(),d,n); d.add(n).write(0); }
                    let si = if ci==0 { 0xFF } else { rf[ci-1] };
                    let so = if ci==nc-1 { 0xFF } else { wf[ci] };
                    let pk = (si as u64)|((so as u64)<<8)|((0xFFu64)<<16);
                    let mut ob=[0u8;512];
                    match syscall::sys_ob_create(to_ob_path(fs,&mut ob), syscall::ob_type::PROCESS, None, pk) {
                        Ok(_) => { write_str(b"\r\n["); write_u64(0); write_str(b"] "); write_str(cn); write_str(b"\r\n"); }
                        Err(_) => { write_err(b"\r\nBad command or file name\r\n"); err=true; break; }
                    }
                }
                Err(_) => { write_err(b"\r\nBad command or file name\r\n"); err=true; break; }
            }
            if ci>0 { let _=syscall::sys_close(rf[ci-1]); }
            if ci<pp.len() { let _=syscall::sys_close(wf[ci]); }
        }
        if err { for i in 0..pp.len() { let _=syscall::sys_close(rf[i]); let _=syscall::sys_close(wf[i]); } }
    }

    fn cmd_cwd(&self) {
        let mut b = [0u8; 256];
        match syscall::sys_getcwd(&mut b) { Ok(n) if n>0 => { write_str(b"\r\n"); write_str(&b[..n]); write_str(b"\r\n"); } _ => { write_str(b"\r\nC:\\\r\n"); } }
    }

    fn cmd_set(&mut self, line: &[u8]) {
        let r = after_first_token(line);
        let mut rb = [0u8; 128]; let rl = r.len().min(127); rb[..rl].copy_from_slice(&r[..rl]);
        let rs = &rb[..rl];
        if rs.is_empty() {
            write_str(b"\r\n");
            for i in 0..self.env_count { write_str(&self.env[i].key[..self.env[i].key_len]); write_str(b"="); write_str(&self.env[i].val[..self.env[i].val_len]); write_str(b"\r\n"); }
            return;
        }
        if let Some(e) = rs.iter().position(|&b| b==b'=') {
            let k = &rs[..e]; let v = &rs[e+1..];
            let mut ku = [0u8; 32]; let kl = k.len().min(31); ku[..kl].copy_from_slice(&k[..kl]); make_ascii_uppercase(&mut ku[..kl]);
            self.env_set(&ku[..kl], v); write_str(b"\r\n");
        } else {
            let mut ku = [0u8; 32]; let kl = rs.len().min(31); ku[..kl].copy_from_slice(&rs[..kl]); make_ascii_uppercase(&mut ku[..kl]);
            match self.env_get(&ku[..kl]) { Some(v) => { write_str(b"\r\n"); write_str(v); write_str(b"\r\n"); } None => { write_str(b"\r\n"); } }
        }
    }

    fn cmd_poweroff(&self) -> ! { write_str(b"\r\npowering off...\r\n"); syscall::sys_poweroff() }
    fn cmd_exit(&self) -> ! { syscall::sys_exit(0) }

    fn cmd_call(&mut self, line: &[u8]) {
        let r = after_first_token(line);
        if r.is_empty() { write_str(b"\r\nUsage: CALL batchfile\r\n"); return; }
        let dr = self.get_drive();
        let mut fp = [0u8; 260]; let mut pos = 0;
        fp[pos]=dr; pos+=1; fp[pos]=b':'; pos+=1;
        if r[0]!=b'\\'&&r[0]!=b'/' {
            let mut cb = [0u8; 256];
            if let Ok(n) = syscall::sys_getcwd(&mut cb) {
                if n>0 { let cwd=&cb[..n-1]; if cwd.len()>2 { for &b in cwd.iter().skip(2) { if pos<255 { fp[pos]=b; pos+=1; } } } if pos>2&&fp[pos-1]!=b'\\'&&pos<255 { fp[pos]=b'\\'; pos+=1; } }
            }
        }
        for &b in r { if pos<255 { fp[pos]=b; pos+=1; } }
        let fs = core::str::from_utf8(&fp[..pos]).unwrap_or("");
        let mut ob = [0u8; 512];
        let fd = match syscall::sys_ob_open(to_ob_path(fs,&mut ob), syscall::ob_access::READ) {
            Ok(f) => f, Err(_) => { write_err(b"\r\nBatch file not found\r\n"); return; }
        };
        let mut ct = [0u8; 4096];
        let rl = match syscall::sys_ob_query_info(fd, syscall::ObInfoClass::ReadContent, &mut ct) {
            Ok(n) => n, Err(_) => { let _=syscall::sys_close(fd); write_err(b"\r\nError reading batch\r\n"); return; }
        };
        let _ = syscall::sys_close(fd);
        let mut ls = 0usize;
        while ls < rl {
            let mut le = ls; while le < rl && ct[le]!=b'\n' { le+=1; }
            let rl2 = trim_ascii(&ct[ls..le]); ls = le+1;
            if rl2.is_empty() || rl2[0]==b':' || rl2[0]==b'@' { continue; }
            if rl2.eq_ignore_ascii_case(b"pause") { write_str(b"Press any key to continue . . .\r\n"); let _ = syscall::sys_read(0, &mut [0;1]); continue; }
            self.execute_line(rl2);
        }
    }

    fn run(&mut self) -> ! {
        let mut vb = [0u8; 128];
        let ver = if let Ok(fd) = syscall::sys_ob_open("\\Global\\Info\\Version", syscall::ob_access::READ) {
            let b = if let Ok(n) = syscall::sys_ob_query_info(fd, syscall::ObInfoClass::Version, &mut vb) {
                let e = vb.iter().position(|&b|b==0).unwrap_or(n.min(vb.len()));
                &vb[..e]
            } else { b"?.?.?" };
            let _ = syscall::sys_close(fd); b
        } else { b"?.?.?" };
        write_str(ver); write_str(b" [VT"); write_str(&[b'0'+get_vt_num()]); write_str(b"]\r\n");
        write_str(b"Type HELP for a list of commands.\r\n");

        console::register_completion(Some(shell_complete));

        loop {
            let drive = self.get_drive();
            let path = self.env_get(b"PATH").unwrap_or(b"\\Programs");
            set_completion_ctx(drive, path);

            self.prompt();
            self.readline();
            let mut lb = [0u8; LINE_BUF_SIZE];
            let n = self.pos.min(LINE_BUF_SIZE-1);
            lb[..n].copy_from_slice(&self.line[..n]);
            let trimmed = trim_ascii(&lb[..n]);
            if !trimmed.is_empty() { self.execute_line(trimmed); }
        }
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut shell = Shell::new();
    shell.run()
}
