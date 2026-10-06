//! Interactive readline with history browsing and TAB completion.

use core::arch::asm;
use crate::io::{cstr_len, read_byte, sys_write, trim_input, write_str};
use crate::history::{entry_index, history_add, HIST};
use crate::{INPUT_MAX, HISTORY_LINE_MAX};

// ── Completion handler ─────────────────────────

pub(crate) type CompletionFn = extern "C" fn(*const u8, i32, *mut u8, i32) -> i32;

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
pub unsafe extern "C" fn console_readline(prompt: *const u8, output: *mut u8, max_out: i32) -> i32 {
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
                unsafe { asm!("mov rax, 1", "int 0x80"); }
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
            0x08 | 0x7F if pos > 0 && len > 0 => {
                for i in pos..len { buf[i - 1] = buf[i]; }
                pos -= 1;
                len -= 1;
                clear_and_rewrite(prompt_slice, &buf[..len]);
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
            c if (0x20..=0x7E).contains(&c) && len < maxlen => {
                let cb = c as u8;
                for i in (pos..len).rev() { buf[i + 1] = buf[i]; }
                buf[pos] = cb;
                pos += 1;
                len += 1;
                clear_and_rewrite(prompt_slice, &buf[..len]);
            }
            _ => {}
        }
    }
}
