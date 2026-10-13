//! Ob query — raw keyboard events (#466).
//!
//! `KeyboardRead` (44): request `{ count: u32, timeout_ms: u32 }`; drains up to
//! `count` [`KeyEvent`]s into the same buffer. Requires raw mode. When the ring
//! is empty it blocks once (indefinitely if `timeout_ms == 0`, else with a
//! deadline) and returns 0 — the caller loops, mirroring `ob_wait`.
//!
//! `KeyboardPoll` (45): non-blocking; drains up to `buf_size / 16` events and
//! returns `count * 16`.

use alloc::vec::Vec;
use crate::input::kbd::ring::KeyEvent;
use crate::object::types::ObInfoClass;
use crate::syscall::{copy_from_user, copy_to_user, err_to_u64, SyscallError};

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObInfoClass::KeyboardRead as u32
        || info_class == ObInfoClass::KeyboardPoll as u32
}

fn ev_bytes(ev: &KeyEvent) -> &[u8] {
    // SAFETY: KeyEvent is #[repr(C)] Copy; the slice covers exactly 16 bytes.
    unsafe {
        core::slice::from_raw_parts(ev as *const KeyEvent as *const u8, core::mem::size_of::<KeyEvent>())
    }
}

fn drain(ring: &crate::input::kbd::ring::KeyEventRing, max: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(max * core::mem::size_of::<KeyEvent>());
    let mut n = 0usize;
    while n < max {
        match ring.pop() {
            Some(ev) => {
                out.extend_from_slice(ev_bytes(&ev));
                n += 1;
            }
            None => break,
        }
    }
    out
}

pub(super) fn dispatch(
    info_class: u32,
    _fd: u8,
    _entry: crate::handle::HandleEntry,
    buf_ptr: u64,
    buf_size: usize,
) -> u64 {
    let vt = crate::scheduler::current_vt_num() as usize;
    let ring = match crate::input::manager::key_ring_for(vt) {
        Some(r) => r,
        None => return err_to_u64(SyscallError::Inval),
    };
    let evsz = core::mem::size_of::<KeyEvent>();

    if info_class == ObInfoClass::KeyboardRead as u32 {
        if buf_size < 8 {
            return err_to_u64(SyscallError::Inval);
        }
        // Raw mode is required so a client cannot consume a key twice.
        if !crate::input::manager::raw_mode(vt) {
            return err_to_u64(SyscallError::Again);
        }
        let mut hdr = [0u8; 8];
        if copy_from_user(&mut hdr, buf_ptr).is_err() {
            return err_to_u64(SyscallError::Fault);
        }
        let count = u32::from_ne_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as usize;
        let timeout_ms = u32::from_ne_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);
        let max = count.min(buf_size.saturating_sub(8) / evsz);
        if max == 0 {
            return err_to_u64(SyscallError::Inval);
        }
        let out = drain(ring, max);
        if out.is_empty() {
            // Block once; the caller re-issues the read after the wake.
            let reason = crate::kwait::WaitReason::Keyboard { vt: vt as u32 };
            if timeout_ms > 0 {
                crate::kwait::register_deadline(
                    reason.encode_magic(),
                    crate::net::tcp::tcp_deadline_from_ms(timeout_ms as u64),
                );
            }
            crate::kwait::kwait_block(reason);
            return 0;
        }
        if copy_to_user(buf_ptr, &out).is_err() {
            return err_to_u64(SyscallError::Fault);
        }
        out.len() as u64
    } else {
        // KeyboardPoll (45): non-blocking drain, events only.
        let max = buf_size / evsz;
        if max == 0 {
            return err_to_u64(SyscallError::Inval);
        }
        let out = drain(ring, max);
        if !out.is_empty() && copy_to_user(buf_ptr, &out).is_err() {
            return err_to_u64(SyscallError::Fault);
        }
        out.len() as u64
    }
}
