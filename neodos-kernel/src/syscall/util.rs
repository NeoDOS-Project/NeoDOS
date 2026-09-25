//! Syscall utilities — extracted from mod.rs (mechanical split)
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;
use x86_64::structures::paging::PageTableFlags;

// P0.2: dedicated lock for user-memory address space (heap/mmap) to make
// copy_user_string atomic against munmap/brk/terminate without deadlocking
// on SCHEDULER. Order: SCHEDULER -> USER_MEMORY_LOCK (always).
pub(crate) static USER_MEMORY_LOCK: Mutex<()> = Mutex::new(());

/// Check if a user page is present (PTE PRESENT bit). For 4KB pages (heap/mmap/NXL)
/// we must walk the PT; for huge pages (USER window 0..4GiB identity-mapped) the
/// walk returns None and we treat as present (huge pages are always PRESENT).
#[inline]
fn is_page_present(virt: u64) -> bool {
    if let Some(pte) = crate::hal::walk_ptes_4k(virt) {
        pte.flags().contains(PageTableFlags::PRESENT)
    } else {
        // Huge page (USER window, NXL, or unsplit heap/mmap). For heap/mmap the
        // region is always split at init, so None after split means not present
        // is handled by caller's range check, but we treat huge USER window as present.
        // Conservative: check if virt is in USER window huge pages -> true, else false.
        virt >= crate::arch::x64::paging::USER_BASE && virt < crate::arch::x64::paging::USER_LIMIT
    }
}

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

/// F-05: fault-safe copy_user_string — makes validation + dereference atomic
/// against concurrent unmap/free on another CPU (TOCTOU → Ring0 fault).
/// Uses bounded spin on the scheduler lock to avoid false -EFAULT under SMP
/// contention (F-DEV-03). If the lock is contended beyond spin budget, we return
/// Fault (safe) and let the syscall be retried — availability vs safety trade-off
/// (NT STATUS_DEVICE_BUSY analog).
///
/// F-DEV-01: is_valid_locked now checks PTE PRESENT for heap/mmap via walk_ptes_4k
/// to avoid #PF at DISPATCH (INV-14). USER window huge pages stay unconditionally
/// present (identity-mapped 0..4GiB, paging.rs:239); heap/mmap require PRESENT.
pub(crate) fn copy_user_string(ptr: u64) -> Result<String, ()> {
    if ptr == 0 {
        return Err(());
    }
    let mut buf = [0u8; 256];
    let mut len = 0usize;

    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };

    // F-DEV-03: bounded spin with cpu_relax instead of immediate try_lock failure.
    // Reduces false -EFAULT under SMP 8-core syscall storm while preserving
    // non-blocking guarantee (no deadlock at DISPATCH).
    let sched_guard = {
        let mut guard = None;
        for _ in 0..1000 {
            if let Some(g) = crate::scheduler::current_scheduler().try_lock() {
                guard = Some(g);
                break;
            }
            core::hint::spin_loop();
        }
        guard
    };

    let result = if let Some(sched) = sched_guard {
        // We hold SCHEDULER — now try USER_MEMORY_LOCK with same bounded spin
        // (order SCHEDULER -> USER_MEMORY_LOCK)
        let _mem_guard = {
            let mut guard = None;
            for _ in 0..1000 {
                if let Some(g) = USER_MEMORY_LOCK.try_lock() {
                    guard = Some(g);
                    break;
                }
                core::hint::spin_loop();
            }
            match guard {
                Some(g) => g,
                None => {
                    unsafe { crate::hal::irql::lower_irql(old_irql); }
                    return Err(());
                }
            }
        };
        // We hold both locks — is_valid and read are atomic against free/unmap.
        // F-DEV-01: heap/mmap must check PTE PRESENT (walk_ptes_4k) to avoid #PF at DISPATCH.
        let is_valid_locked = |p: u64| -> bool {
            if p >= crate::arch::x64::paging::USER_BASE && p < crate::arch::x64::paging::USER_LIMIT {
                // USER window is identity-mapped huge pages PRESENT|USER — always present
                return true;
            }
            if p >= 0x1E000000 && p < 0x1E200000 {
                // NXL region is split at init_nxl_region, check PRESENT
                return is_page_present(p);
            }
            let pid = sched.current_pid();
            if let Some(ep) = sched.find_eprocess(pid) {
                if ep.heap_base != 0 && p >= ep.heap_base && p < ep.heap_break {
                    // Heap pages are 4KB demand-paged — must be PRESENT
                    return is_page_present(p);
                }
                for r in &ep.mmap_regions {
                    if p >= r.base && p < r.base + r.len {
                        // mmap pages are 4KB demand-paged — must be PRESENT
                        return is_page_present(p);
                    }
                }
            }
            false
        };
        let r = (|| {
            unsafe {
                while len < 255 {
                    let cur = ptr + len as u64;
                    if !is_valid_locked(cur) {
                        return Err(());
                    }
                    let byte = (cur as *const u8).read();
                    if byte == 0 { break; }
                    buf[len] = byte;
                    len += 1;
                }
            }
            Ok(())
        })();
        // sched_guard and _mem_guard dropped here
        r
    } else {
        // Lock contended or already held — return Fault (safe, reintentable) to avoid TOCTOU.
        // Previously we did a lock-free fallback with is_user_ptr_valid + read, but that
        // reintroduces the TOCTOU (another CPU could free between check and read -> Ring0 #PF).
        // Returning Fault is safe and lets the syscall be retried after the lock is free.
        unsafe { crate::hal::irql::lower_irql(old_irql); }
        return Err(());
    };

    unsafe { crate::hal::irql::lower_irql(old_irql); }

    if result.is_err() {
        return Err(());
    }
    core::str::from_utf8(&buf[..len]).map(|s| s.to_string()).map_err(|_| ())
}
