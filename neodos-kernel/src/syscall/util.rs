//! Syscall utilities — extracted from mod.rs (mechanical split)
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub(crate) fn is_user_ptr_valid(ptr: u64, len: u64) -> bool {
    if ptr >= crate::arch::x64::paging::USER_BASE && ptr.saturating_add(len) <= crate::arch::x64::paging::USER_LIMIT {
        return true;
    }
    if ptr >= 0x1E000000 && ptr.saturating_add(len) <= 0x1E200000 {
        return true;
    }
    let (heap_base, heap_break) = crate::scheduler::current_process_heap_range();
    if heap_base != 0 && ptr >= heap_base && ptr.saturating_add(len) <= heap_break {
        return true;
    }
    let regions = crate::scheduler::current_process_mmap_regions();
    for r in &regions {
        if ptr >= r.base && ptr.saturating_add(len) <= r.base + r.len {
            return true;
        }
    }
    false
}

pub(crate) fn copy_user_string(ptr: u64) -> Result<String, ()> {
    if ptr == 0 {
        return Err(());
    }
    let mut buf = [0u8; 256];
    let mut len = 0usize;
    unsafe {
        while len < 255 {
            let cur = ptr + len as u64;
            if !is_user_ptr_valid(cur, 1) {
                return Err(());
            }
            let byte = (cur as *const u8).read();
            if byte == 0 { break; }
            buf[len] = byte;
            len += 1;
            // Validate that next byte will still be in user range before next iteration
            // (also ensures we don't cross into unmapped kernel space)
            if len == 255 {
                break;
            }
        }
    }
    core::str::from_utf8(&buf[..len]).map(|s| s.to_string()).map_err(|_| ())
}
