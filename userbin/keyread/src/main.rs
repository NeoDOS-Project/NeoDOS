#![no_std]
#![no_main]

//! `keyread` — read raw keyboard events (Ring 3, #466).
//!
//! Enables raw mode for the calling VT, prints up to 10 make/break events
//! (scancode, virtual key, pressed, modifiers, extended), then restores cooked
//! mode and exits. Demonstrates the `ObInfoClass::KeyboardRead` API and the
//! E0 extended-key mapping (#391).

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use libneodos::keyboard::{self, KeyEvent};
use libneodos::syscall;

struct SbrkAlloc;
unsafe impl GlobalAlloc for SbrkAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size().max(8) as i64;
        let ptr = libneodos::mem::sbrk(size).ok().unwrap_or(0) as *mut u8;
        if ptr.is_null() { core::ptr::null_mut() } else { ptr }
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}
#[global_allocator]
static ALLOC: SbrkAlloc = SbrkAlloc;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}
fn write_dec(mut v: u64) {
    let mut b = [0u8; 20];
    let mut i = b.len();
    if v == 0 { write_str(b"0"); return; }
    while v > 0 && i > 0 { i -= 1; b[i] = b'0' + (v % 10) as u8; v /= 10; }
    write_str(&b[i..]);
}

fn vk_name(vk: u16) -> &'static [u8] {
    match vk {
        keyboard::VK_UP => b"UP",
        keyboard::VK_DOWN => b"DOWN",
        keyboard::VK_LEFT => b"LEFT",
        keyboard::VK_RIGHT => b"RIGHT",
        keyboard::VK_HOME => b"HOME",
        keyboard::VK_END => b"END",
        keyboard::VK_PRIOR => b"PGUP",
        keyboard::VK_NEXT => b"PGDN",
        keyboard::VK_INSERT => b"INS",
        keyboard::VK_DELETE => b"DEL",
        keyboard::VK_ESCAPE => b"ESC",
        keyboard::VK_RETURN => b"ENTER",
        keyboard::VK_SPACE => b"SPACE",
        keyboard::VK_TAB => b"TAB",
        keyboard::VK_BACK => b"BACKSPACE",
        0 => b"-",
        _ => b"?",
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write_str(b"[keyread] raw mode ON; press keys (arrows/Ctrl/Esc/Space)...\r\n");
    if keyboard::kbd_set_raw(true).is_err() {
        write_str(b"[keyread] kbd_set_raw failed\r\n");
        syscall::sys_exit(1);
    }
    if keyboard::kbd_set_raw(true).is_err() {
        // idempotent; ignore
    }

    let mut evs = [KeyEvent::NONE; 8];
    let mut printed = 0u32;
    let mut idle = 0u32;
    // `kbd_read_event` blocks (with a deadline) and returns 0 on a wake or
    // timeout — like `ob_wait`, the caller re-issues. `idle` bounds the demo.
    while printed < 10 && idle < 20 {
        match keyboard::kbd_read_event(&mut evs, 500) {
            Ok(n) if n > 0 => {
                idle = 0;
                for e in evs.iter().take(n) {
                    write_str(b"  sc=");
                    write_dec(e.scancode as u64);
                    write_str(b" vk=");
                    write_dec(e.vk as u64);
                    write_str(b" ");
                    write_str(vk_name(e.vk));
                    write_str(b" ");
                    write_str(if e.is_pressed() { b"down" } else { b"up" });
                    write_str(b" mod=");
                    write_dec(e.modifiers as u64);
                    if e.is_extended() {
                        write_str(b" ext");
                    }
                    write_str(b"\r\n");
                    printed += 1;
                }
            }
            Ok(_) => {
                idle += 1;
            }
            Err(code) => {
                write_str(b"[keyread] read error ");
                write_dec((-code) as u64);
                write_str(b"\r\n");
                break;
            }
        }
    }
    if printed == 0 {
        write_str(b"[keyread] (no keys captured)\r\n");
    }

    let _ = keyboard::kbd_set_raw(false);
    write_str(b"[keyread] raw mode OFF, done\r\n");
    syscall::sys_exit(0)
}
