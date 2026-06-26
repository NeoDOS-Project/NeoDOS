#![no_std]
#![no_main]

use core::arch::asm;
use core::sync::atomic::AtomicU32;

// ── Constants ──────────────────────────────────

const INPUT_MAX: usize = 256;
const HISTORY_MAX: usize = 32;
const HISTORY_LINE_MAX: usize = 128;

// ── Syscall wrappers ───────────────────────────

unsafe fn sys_write(fd: u8, buf: *const u8, len: usize) -> i64 {
    let r: i64;
    asm!(
        "push rbx", "push rcx", "push rdx",
        "mov rax, 1",
        "mov rbx, {fd}",
        "mov rcx, {buf}",
        "mov rdx, {len}",
        "int 0x80",
        "pop rdx", "pop rcx", "pop rbx",
        fd = in(reg) fd as u64,
        buf = in(reg) buf as u64,
        len = in(reg) len as u64,
        out("rax") r,
    );
    r
}

fn write_str(s: &[u8]) {
    if !s.is_empty() {
        unsafe { sys_write(1, s.as_ptr(), s.len()); }
    }
}

fn read_byte() -> i32 {
    let mut c: u8 = 0;
    let r: u64;
    unsafe {
        asm!(
            "push rbx", "push rcx", "push rdx",
            "mov rax, 4",
            "mov rbx, 0",
            "mov rcx, {buf}",
            "mov rdx, 1",
            "int 0x80",
            "pop rdx", "pop rcx", "pop rbx",
            buf = in(reg) (&mut c as *mut u8),
            out("rax") r,
        );
    }
    if r & 0x8000_0000_0000_0000 != 0 { -1 } else { c as i32 }
}

// ── History state ──────────────────────────────

struct History {
    entries: [[u8; HISTORY_LINE_MAX]; HISTORY_MAX],
    count: u16,
    browse_pos: i16,
    head: u16,
    pending: [u8; INPUT_MAX],
    pending_len: u16,
}

static mut HIST: History = History {
    entries: [[0; HISTORY_LINE_MAX]; HISTORY_MAX],
    count: 0,
    browse_pos: -1,
    head: 0,
    pending: [0; INPUT_MAX],
    pending_len: 0,
};

fn entry_index(logical: u16) -> usize {
    unsafe {
        if HIST.count < HISTORY_MAX as u16 {
            logical as usize
        } else {
            ((HIST.head as u16 + 1 + logical) % HISTORY_MAX as u16) as usize
        }
    }
}

fn cstr_len(s: *const u8, max: usize) -> usize {
    unsafe {
        for i in 0..max {
            if *s.add(i) == 0 { return i; }
        }
    }
    max
}

fn u64_to_str(mut n: u64, buf: &mut [u8]) -> usize {
    if n == 0 {
        if !buf.is_empty() { buf[0] = b'0'; return 1; }
        return 0;
    }
    let mut digits = [0u8; 20];
    let mut i = 0;
    while n > 0 {
        if i < 20 { digits[i] = b'0' + (n % 10) as u8; }
        n /= 10;
        i += 1;
    }
    for j in 0..i {
        if j < buf.len() { buf[j] = digits[i - 1 - j]; }
    }
    core::cmp::min(i, buf.len())
}

fn trim_input(s: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < s.len() && (s[start] == b' ' || s[start] == b'\t') { start += 1; }
    let mut end = s.len();
    while end > start && (s[end - 1] == b' ' || s[end - 1] == b'\t') { end -= 1; }
    &s[start..end]
}

// ── History API ────────────────────────────────

#[no_mangle]
pub extern "C" fn history_add(text: *const u8) {
    let len = core::cmp::min(cstr_len(text, HISTORY_LINE_MAX - 1), HISTORY_LINE_MAX - 1);
    if len == 0 { return; }
    unsafe {
        if HIST.count > 0 {
            let last = &HIST.entries[HIST.head as usize];
            let same = (0..len).all(|i| i < HISTORY_LINE_MAX && last[i] == *text.add(i)) && last[len] == 0;
            if same { return; }
        }
        let idx = (HIST.head as usize + 1) % HISTORY_MAX;
        let entry = &mut HIST.entries[idx];
        entry.fill(0);
        core::ptr::copy_nonoverlapping(text, entry.as_mut_ptr(), len);
        HIST.head = idx as u16;
        if HIST.count < HISTORY_MAX as u16 { HIST.count += 1; }
        HIST.browse_pos = -1;
    }
}

#[no_mangle]
pub extern "C" fn history_prev() -> *const u8 {
    unsafe {
        if HIST.count == 0 { return core::ptr::null(); }
        if HIST.browse_pos == -1 {
            HIST.browse_pos = (HIST.count - 1) as i16;
        } else if HIST.browse_pos > 0 {
            HIST.browse_pos -= 1;
        } else { return core::ptr::null(); }
        let idx = entry_index(HIST.browse_pos as u16);
        HIST.entries[idx].as_ptr()
    }
}

#[no_mangle]
pub extern "C" fn history_next() -> *const u8 {
    unsafe {
        if HIST.count == 0 || HIST.browse_pos == -1 { return core::ptr::null(); }
        if HIST.browse_pos < (HIST.count - 1) as i16 {
            HIST.browse_pos += 1;
            let idx = entry_index(HIST.browse_pos as u16);
            HIST.entries[idx].as_ptr()
        } else {
            HIST.browse_pos = -1;
            core::ptr::null()
        }
    }
}

#[no_mangle]
pub extern "C" fn history_reset() {
    unsafe { HIST.browse_pos = -1; }
}

#[no_mangle]
pub extern "C" fn history_get_count() -> i32 {
    unsafe { HIST.count as i32 }
}

#[no_mangle]
pub extern "C" fn history_get_entry(idx: i32) -> *const u8 {
    unsafe {
        if idx < 0 || idx >= HIST.count as i32 { return core::ptr::null(); }
        let i = entry_index(idx as u16);
        HIST.entries[i].as_ptr()
    }
}

// ── Completion handler ─────────────────────────

type CompletionFn = extern "C" fn(*const u8, i32, *mut u8, i32) -> i32;

static mut COMPLETION_HANDLER: Option<CompletionFn> = None;

#[no_mangle]
pub extern "C" fn completion_register(handler: Option<CompletionFn>) {
    unsafe { COMPLETION_HANDLER = handler; }
}

// ── Readline ───────────────────────────────────

fn clear_and_rewrite(prompt: &[u8], buf: &[u8]) {
    let mut line = [0u8; 200];
    let mut p = 0usize;
    line[p] = b'\r'; p += 1;
    for &b in prompt.iter().chain(buf.iter()) {
        if p < 79 { line[p] = b; p += 1; }
    }
    // Clear to end of visible line with spaces
    while p < 79 { line[p] = b' '; p += 1; }
    // Pad with backspaces (\x08) to reach 200 bytes for QEMU flush.
    // \x08 moves cursor left on terminal — invisible, but triggers flush.
    while p < 200 { line[p] = 0x08; p += 1; }
    unsafe { sys_write(1, line.as_ptr(), p); }
}

fn find_word_start(buf: &[u8], pos: usize) -> usize {
    let mut p = pos;
    while p > 0 && buf[p - 1] != b' ' { p -= 1; }
    p
}

#[no_mangle]
pub extern "C" fn console_readline(prompt: *const u8, output: *mut u8, max_out: i32) -> i32 {
    let plen = core::cmp::min(cstr_len(prompt, 128), 128);
    let prompt_slice = unsafe { core::slice::from_raw_parts(prompt, plen) };
    let maxlen = if max_out < 1 { 1 } else { max_out as usize - 1 };
    let maxlen = core::cmp::min(maxlen, INPUT_MAX);

    let mut buf = [0u8; INPUT_MAX];
    let mut len: usize = 0;
    let mut pos: usize = 0;

    clear_and_rewrite(prompt_slice, &[]);

    loop {
        let key = read_byte();
        match key {
            -1 => {
                unsafe { asm!("mov rax, 2", "int 0x80"); }
                continue;
            }
            0x0D | 0x0A => {
                write_str(b"\r\n");
                if len > 0 {
                    let trimmed = trim_input(&buf[..len]);
                    if !trimmed.is_empty() {
                        let mut entry = [0u8; HISTORY_LINE_MAX];
                        let elen = core::cmp::min(trimmed.len(), HISTORY_LINE_MAX - 1);
                        entry[..elen].copy_from_slice(&trimmed[..elen]);
                        entry[elen] = 0;
                        history_add(entry.as_ptr());
                    }
                }
                unsafe {
                    core::ptr::copy_nonoverlapping(buf.as_ptr(), output, len);
                    *output.add(len) = 0;
                    HIST.browse_pos = -1;
                }
                return len as i32;
            }
            0x08 | 0x7F => {
                if pos > 0 && len > 0 {
                    for i in pos..len { buf[i - 1] = buf[i]; }
                    pos -= 1;
                    len -= 1;
                    clear_and_rewrite(prompt_slice, &buf[..len]);
                }
            }
            0x01 => {
                unsafe {
                    if HIST.count == 0 { continue; }
                    if HIST.browse_pos == -1 {
                        let plen = core::cmp::min(len, INPUT_MAX - 1);
                        HIST.pending[..plen].copy_from_slice(&buf[..plen]);
                        HIST.pending[plen] = 0;
                        HIST.pending_len = plen as u16;
                        HIST.browse_pos = (HIST.count - 1) as i16;
                    } else if HIST.browse_pos > 0 {
                        HIST.browse_pos -= 1;
                    } else { continue; }
                    let idx = entry_index(HIST.browse_pos as u16);
                    let entry = &HIST.entries[idx];
                    let entry_len = entry.iter().position(|&b| b == 0).unwrap_or(HISTORY_LINE_MAX);
                    len = core::cmp::min(entry_len, maxlen);
                    buf[..len].copy_from_slice(&entry[..len]);
                    pos = len;
                    clear_and_rewrite(prompt_slice, &buf[..len]);
                }
            }
            0x02 => {
                unsafe {
                    if HIST.count == 0 || HIST.browse_pos == -1 { continue; }
                    if HIST.browse_pos < (HIST.count - 1) as i16 {
                        HIST.browse_pos += 1;
                        let idx = entry_index(HIST.browse_pos as u16);
                        let entry = &HIST.entries[idx];
                        let entry_len = entry.iter().position(|&b| b == 0).unwrap_or(HISTORY_LINE_MAX);
                        len = core::cmp::min(entry_len, maxlen);
                        buf[..len].copy_from_slice(&entry[..len]);
                    } else {
                        HIST.browse_pos = -1;
                        let plen = HIST.pending_len as usize;
                        len = plen;
                        buf[..len].copy_from_slice(&HIST.pending[..len]);
                    }
                    pos = len;
                    clear_and_rewrite(prompt_slice, &buf[..len]);
                }
            }
            0x09 => {
                unsafe {
                    if let Some(handler) = COMPLETION_HANDLER {
                        let mut cand_buf = [0u8; 512];
                        let n = handler(buf.as_ptr(), pos as i32, cand_buf.as_mut_ptr(), 512);
                        if n <= 0 { continue; }
                        let mut first_len = 0;
                        while first_len < 512 && cand_buf[first_len] != 0 { first_len += 1; }
                        if n == 1 {
                            let word_start = find_word_start(&buf[..len], pos);
                            let suffix_len = len - pos;
                            let new_len = word_start + first_len + suffix_len;
                            if new_len <= maxlen {
                                for i in (0..suffix_len).rev() {
                                    let dst = word_start + first_len + i;
                                    if dst < maxlen { buf[dst] = buf[pos + i]; }
                                }
                                buf[word_start..word_start + first_len].copy_from_slice(&cand_buf[..first_len]);
                                len = new_len;
                                pos = word_start + first_len;
                                clear_and_rewrite(prompt_slice, &buf[..len]);
                            }
                        } else {
                            let mut display = [0u8; 512];
                            let mut dpos = 0usize;
                            let mut ci = 0usize;
                            while ci < 512 && cand_buf[ci] != 0 {
                                if dpos > 0 { dpos += 2; }
                                let start = ci;
                                while ci < 512 && cand_buf[ci] != 0 { ci += 1; }
                                let wlen = (ci - start).min(510 - dpos);
                                if wlen > 0 {
                                    if dpos > 0 { display[dpos-2] = b' '; display[dpos-1] = b' '; }
                                    let mut k = 0;
                                    while k < wlen { display[dpos + k] = cand_buf[start + k]; k += 1; }
                                    dpos += wlen;
                                }
                                ci += 1;
                            }
                            write_str(b"\r\n");
                            write_str(&display[..dpos]);
                            clear_and_rewrite(prompt_slice, &buf[..len]);
                        }
                    }
                }
            }
            0x1B => {
                let n1 = read_byte();
                if n1 == b'[' as i32 {
                    let n2 = read_byte();
                    if n2 == b'A' as i32 || n2 == b'B' as i32 { continue; }
                }
            }
            c if c >= 0x20 && c <= 0x7E => {
                if len < maxlen {
                    let cb = c as u8;
                    for i in (pos..len).rev() { buf[i + 1] = buf[i]; }
                    buf[pos] = cb;
                    pos += 1;
                    len += 1;
                    clear_and_rewrite(prompt_slice, &buf[..len]);
                }
            }
            _ => {}
        }
    }
}

// ── Output API ─────────────────────────────────

#[no_mangle]
pub extern "C" fn console_write(text: *const u8, len: i32) -> i32 {
    if len <= 0 { return 0; }
    unsafe { sys_write(1, text, len as usize) as i32 }
}

#[no_mangle]
pub extern "C" fn console_write_line(text: *const u8, len: i32) -> i32 {
    if len > 0 { unsafe { sys_write(1, text, len as usize); } }
    write_str(b"\r\n");
    0
}

#[no_mangle]
pub extern "C" fn console_set_color(fg: u8, bg: u8) {
    let mut buf = [0u8; 16];
    buf[0] = 0x1B; buf[1] = b'[';
    let mut pos = 2;
    let fg_code = (fg & 0x0F).saturating_add(30);
    let bg_code = (bg & 0x0F).saturating_add(40);
    let mut tmp = [0u8; 4];
    let fl = u64_to_str(fg_code as u64, &mut tmp);
    buf[pos..pos+fl].copy_from_slice(&tmp[..fl]); pos += fl;
    buf[pos] = b';'; pos += 1;
    let bl = u64_to_str(bg_code as u64, &mut tmp);
    buf[pos..pos+bl].copy_from_slice(&tmp[..bl]); pos += bl;
    buf[pos] = b'm'; pos += 1;
    write_str(&buf[..pos]);
}

#[no_mangle]
pub extern "C" fn console_reset_color() { write_str(b"\x1b[0m"); }

#[no_mangle]
pub extern "C" fn console_clear_screen() { write_str(b"\x1b[2J"); }

#[no_mangle]
pub extern "C" fn console_cursor_home() { write_str(b"\x1b[H"); }

#[no_mangle]
pub extern "C" fn console_read_byte() -> i32 { read_byte() }

// ── Progress bars ──────────────────────────────

const MAX_BARS: usize = 8;
const BAR_WIDTH: usize = 20;
const TITLE_MAX: usize = 64;
const MSG_MAX: usize = 128;

const BLOCK_FILLED: &[u8; 3] = b"\xe2\x96\x93";
const BLOCK_EMPTY: &[u8; 3]  = b"\xe2\x96\x91";

#[repr(C)]
struct ProgressBar {
    id: i32,
    title: [u8; TITLE_MAX],
    title_len: u8,
    message: [u8; MSG_MAX],
    msg_len: u8,
    current: u64,
    total: u64,
    active: bool,
}

const fn empty_bar() -> ProgressBar {
    ProgressBar {
        id: 0, title: [0; TITLE_MAX], title_len: 0,
        message: [0; MSG_MAX], msg_len: 0,
        current: 0, total: 0, active: false,
    }
}

static mut BARS: [ProgressBar; MAX_BARS] = [
    empty_bar(), empty_bar(), empty_bar(), empty_bar(),
    empty_bar(), empty_bar(), empty_bar(), empty_bar(),
];
static NEXT_BAR_ID: AtomicU32 = AtomicU32::new(1);
static mut PREV_PROGRESS_ROWS: usize = 0;

fn bars_mut() -> &'static mut [ProgressBar; MAX_BARS] {
    unsafe { &mut *core::ptr::addr_of_mut!(BARS) }
}

fn bars_ref() -> &'static [ProgressBar; MAX_BARS] {
    unsafe { &*core::ptr::addr_of!(BARS) }
}

fn format_bar(current: u64, total: u64, buf: &mut [u8]) -> usize {
    let (pct, filled) = if total == 0 {
        (100, BAR_WIDTH)
    } else {
        let p = (current as u128).saturating_mul(100).saturating_div(total as u128);
        let f = (current as u128).saturating_mul(BAR_WIDTH as u128).saturating_div(total as u128);
        (core::cmp::min(p, 100) as u64, core::cmp::min(f, BAR_WIDTH as u128) as usize)
    };
    let empty = BAR_WIDTH - filled;
    let mut pos = 0usize;
    if pos < buf.len() { buf[pos] = b'['; pos += 1; }
    for _ in 0..filled {
        if pos + 3 <= buf.len() { buf[pos..pos+3].copy_from_slice(BLOCK_FILLED); pos += 3; }
    }
    for _ in 0..empty {
        if pos + 3 <= buf.len() { buf[pos..pos+3].copy_from_slice(BLOCK_EMPTY); pos += 3; }
    }
    if pos < buf.len() { buf[pos] = b']'; pos += 1; }
    if pos < buf.len() { buf[pos] = b' '; pos += 1; }
    let mut pct_buf = [0u8; 4];
    let pct_len = u64_to_str(pct, &mut pct_buf);
    if pos + pct_len + 1 <= buf.len() {
        buf[pos..pos+pct_len].copy_from_slice(&pct_buf[..pct_len]);
        pos += pct_len; buf[pos] = b'%'; pos += 1;
    }
    pos
}

fn progress_render() {
    let mut indices = [0usize; MAX_BARS];
    let mut count = 0;
    {
        let bars = bars_ref();
        for i in 0..MAX_BARS {
            if bars[i].active && bars[i].id > 0 { indices[count] = i; count += 1; }
        }
    }
    let mut new_rows = 0usize;
    {
        let bars = bars_ref();
        for i in 0..count {
            new_rows += 2;
            if bars[indices[i]].msg_len > 0 { new_rows += 1; }
        }
    }
    let prev = unsafe { PREV_PROGRESS_ROWS };
    for _ in 0..prev { write_str(b"\x1b[A"); }
    if new_rows < prev {
        let extra = prev - new_rows;
        for _ in 0..extra { write_str(b"\r\x1b[K\r\n"); }
        for _ in 0..extra { write_str(b"\x1b[A"); }
    }
    if count == 0 { unsafe { PREV_PROGRESS_ROWS = 0; } return; }
    let bars = bars_ref();
    for i in 0..count {
        let bar = &bars[indices[i]];
        write_str(b"\r\x1b[K");
        write_str(&bar.title[..bar.title_len as usize]);
        write_str(b"\r\n");
        write_str(b"\r\x1b[K");
        let mut bar_buf = [0u8; 128];
        let blen = format_bar(bar.current, bar.total, &mut bar_buf);
        write_str(&bar_buf[..blen]);
        write_str(b"\r\n");
        if bar.msg_len > 0 {
            write_str(b"\r\x1b[K");
            write_str(&bar.message[..bar.msg_len as usize]);
            write_str(b"\r\n");
        }
    }
    unsafe { PREV_PROGRESS_ROWS = new_rows; }
}

#[no_mangle]
pub extern "C" fn progress_create(title: *const u8, total: u64) -> i32 {
    let id = {
        let bars = bars_mut();
        let mut slot = None;
        for i in 0..MAX_BARS { if !bars[i].active { slot = Some(i); break; } }
        let idx = match slot { Some(i) => i, None => return -1 };
        let id = NEXT_BAR_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed) as i32;
        let bar = &mut bars[idx];
        bar.id = id; bar.current = 0; bar.total = total; bar.msg_len = 0; bar.active = true;
        bar.title_len = {
            let len = core::cmp::min(cstr_len(title, TITLE_MAX - 1), TITLE_MAX - 1);
            unsafe { core::ptr::copy_nonoverlapping(title, bar.title.as_mut_ptr(), len); }
            bar.title[len] = 0; len as u8
        };
        id
    };
    progress_render(); id
}

#[no_mangle]
pub extern "C" fn progress_update(id: i32, current: u64) {
    for bar in bars_mut().iter_mut() { if bar.active && bar.id == id { bar.current = current; break; } }
    progress_render();
}

#[no_mangle]
pub extern "C" fn progress_set_message(id: i32, text: *const u8) {
    for bar in bars_mut().iter_mut() {
        if bar.active && bar.id == id {
            bar.msg_len = {
                let len = core::cmp::min(cstr_len(text, MSG_MAX - 1), MSG_MAX - 1);
                unsafe { core::ptr::copy_nonoverlapping(text, bar.message.as_mut_ptr(), len); }
                bar.message[len] = 0; len as u8
            }; break;
        }
    }
    progress_render();
}

#[no_mangle]
pub extern "C" fn progress_finish(id: i32) {
    for bar in bars_mut().iter_mut() { if bar.active && bar.id == id { bar.current = bar.total; break; } }
    progress_render();
    for bar in bars_mut().iter_mut() { if bar.id == id { bar.active = false; break; } }
}

// ── Export table ───────────────────────────────

#[repr(C)]
pub struct ConsoleAbiTable {
    pub version: u32,
    pub readline: extern "C" fn(*const u8, *mut u8, i32) -> i32,
    pub read_byte: extern "C" fn() -> i32,
    pub write: extern "C" fn(*const u8, i32) -> i32,
    pub write_line: extern "C" fn(*const u8, i32) -> i32,
    pub set_color: extern "C" fn(u8, u8),
    pub reset_color: extern "C" fn(),
    pub clear_screen: extern "C" fn(),
    pub cursor_home: extern "C" fn(),
    pub history_add: extern "C" fn(*const u8),
    pub history_prev: extern "C" fn() -> *const u8,
    pub history_next: extern "C" fn() -> *const u8,
    pub history_reset: extern "C" fn(),
    pub history_get_count: extern "C" fn() -> i32,
    pub history_get_entry: extern "C" fn(i32) -> *const u8,
    pub completion_register: extern "C" fn(Option<CompletionFn>),
    pub progress_create: extern "C" fn(*const u8, u64) -> i32,
    pub progress_update: extern "C" fn(i32, u64),
    pub progress_set_message: extern "C" fn(i32, *const u8),
    pub progress_finish: extern "C" fn(i32),
    _reserved: [u64; 8],
}

#[no_mangle]
#[link_section = ".export_table"]
pub static CONSOLE_EXPORT_TABLE: ConsoleAbiTable = ConsoleAbiTable {
    version: 2,
    readline: console_readline,
    read_byte: console_read_byte,
    write: console_write,
    write_line: console_write_line,
    set_color: console_set_color,
    reset_color: console_reset_color,
    clear_screen: console_clear_screen,
    cursor_home: console_cursor_home,
    history_add: history_add,
    history_prev: history_prev,
    history_next: history_next,
    history_reset: history_reset,
    history_get_count: history_get_count,
    history_get_entry: history_get_entry,
    completion_register: completion_register,
    progress_create: progress_create,
    progress_update: progress_update,
    progress_set_message: progress_set_message,
    progress_finish: progress_finish,
    _reserved: [0; 8],
};

// ── NXL boilerplate ────────────────────────────

#[no_mangle]
pub extern "C" fn nxl_entry() -> ! {
    loop { unsafe { asm!("hlt"); } }
}

#[panic_handler]
fn nxl_panic(_info: &core::panic::PanicInfo) -> ! {
    loop { unsafe { asm!("hlt"); } }
}
