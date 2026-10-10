//! Syscall utilities — extracted from mod.rs (mechanical split)
use alloc::string::{String, ToString};
use spin::Mutex;
use x86_64::structures::paging::PageTableFlags;
use crate::scheduler::Scheduler;

// P0.2: dedicated lock for user-memory address space (heap/mmap) to make user
// copies atomic against munmap/brk/terminate without deadlocking on SCHEDULER.
// Order: SCHEDULER -> USER_MEMORY_LOCK (always). Every page-freer of user
// memory (brk shrink, munmap, process exit) MUST hold this lock while freeing,
// and every user copy holds it while validating + accessing, so a validated
// pointer cannot be freed mid-copy (NEODOS-04 / #634).
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

/// Is a single byte of user memory accessible by the current process? Must be
/// called with SCHEDULER + USER_MEMORY_LOCK held (F-DEV-01: heap/mmap require
/// the PTE to be PRESENT so we never fault at DISPATCH, INV-14).
fn is_valid_user_byte_locked(sched: &Scheduler, p: u64) -> bool {
    if p >= crate::arch::x64::paging::USER_BASE && p < crate::arch::x64::paging::USER_LIMIT {
        // USER window is identity-mapped huge pages PRESENT|USER — always present.
        return true;
    }
    if p >= 0x1E000000 && p < 0x1E200000 {
        // NXL region is split at init_nxl_region, check PRESENT.
        return is_page_present(p);
    }
    let pid = sched.current_pid();
    if let Some(ep) = sched.find_eprocess(pid) {
        if ep.heap_base != 0 && p >= ep.heap_base && p < ep.heap_break {
            // Heap pages are 4KB demand-paged — must be PRESENT.
            return is_page_present(p);
        }
        for r in &ep.mmap_regions {
            if p >= r.base && p < r.base + r.len {
                // mmap pages are 4KB demand-paged — must be PRESENT.
                return is_page_present(p);
            }
        }
    }
    false
}

/// Run `op` with SCHEDULER -> USER_MEMORY_LOCK held, so that validating a user
/// pointer and then accessing it is atomic against concurrent munmap/brk/exit.
///
/// Bounded spin (F-DEV-03): if either lock is contended beyond the budget we
/// return `Err(())` (safe, retryable — NT STATUS_DEVICE_BUSY analog) instead of
/// reintroducing a TOCTOU with a lock-free fallback.
fn with_user_memory_locked<R>(
    op: impl FnOnce(&Scheduler) -> Result<R, ()>,
) -> Result<R, ()> {
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };

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
    let sched = match sched_guard {
        Some(g) => g,
        None => {
            unsafe { crate::hal::irql::lower_irql(old_irql); }
            return Err(());
        }
    };

    let mem_guard = {
        let mut guard = None;
        for _ in 0..1000 {
            if let Some(g) = USER_MEMORY_LOCK.try_lock() {
                guard = Some(g);
                break;
            }
            core::hint::spin_loop();
        }
        guard
    };
    let _mem = match mem_guard {
        Some(g) => g,
        None => {
            drop(sched);
            unsafe { crate::hal::irql::lower_irql(old_irql); }
            return Err(());
        }
    };

    // SMAP: the kernel may only touch user pages with RFLAGS.AC set. The HAL
    // wraps the validated access so it stays fault-safe under SMAP (NEODOS-09 /
    // #639); on CPUs without SMAP this is a no-op.
    let result = crate::hal::safe::with_user_access(|| op(&sched));

    drop(_mem);
    drop(sched);
    unsafe { crate::hal::irql::lower_irql(old_irql); }
    result
}

/// Fault-safe copy of `src` (user VA) into `dst` (kernel buffer). Validates and
/// reads each byte under SCHEDULER -> USER_MEMORY_LOCK, so the range cannot be
/// unmapped/freed between validation and the read (NEODOS-04 / #634).
///
/// Returns `Ok(dst.len())` on success, `Err(())` (EFAULT) if any byte is not
/// accessible user memory or the locks are contended. Never faults in Ring 0.
pub(crate) fn copy_from_user(dst: &mut [u8], src: u64) -> Result<usize, ()> {
    if src == 0 {
        return Err(());
    }
    with_user_memory_locked(|sched| {
        for (i, slot) in dst.iter_mut().enumerate() {
            let cur = src + i as u64;
            if !is_valid_user_byte_locked(sched, cur) {
                return Err(());
            }
            *slot = unsafe { (cur as *const u8).read() };
        }
        Ok(dst.len())
    })
}

/// Fault-safe copy of `src` (kernel buffer) into `dst` (user VA). Validates and
/// writes each byte under SCHEDULER -> USER_MEMORY_LOCK (NEODOS-04 / #634).
pub(crate) fn copy_to_user(dst: u64, src: &[u8]) -> Result<(), ()> {
    if dst == 0 && !src.is_empty() {
        return Err(());
    }
    with_user_memory_locked(|sched| {
        for (i, b) in src.iter().enumerate() {
            let cur = dst + i as u64;
            if !is_valid_user_byte_locked(sched, cur) {
                return Err(());
            }
            unsafe { (cur as *mut u8).write(*b) };
        }
        Ok(())
    })
}

/// F-05: fault-safe copy_user_string — makes validation + dereference atomic
/// against concurrent unmap/free on another CPU (TOCTOU → Ring0 fault).
/// Uses bounded spin on the scheduler lock to avoid false -EFAULT under SMP
/// contention (F-DEV-03). If the lock is contended beyond spin budget, we return
/// Fault (safe) and let the syscall be retried — availability vs safety trade-off
/// (NT STATUS_DEVICE_BUSY analog).
///
/// F-DEV-01: heap/mmap are checked for PTE PRESENT via walk_ptes_4k to avoid
/// #PF at DISPATCH (INV-14). USER window huge pages stay unconditionally present.
pub(crate) fn copy_user_string(ptr: u64) -> Result<String, ()> {
    if ptr == 0 {
        return Err(());
    }
    let mut buf = [0u8; 256];
    let mut len = 0usize;

    with_user_memory_locked(|sched| {
        while len < 255 {
            let cur = ptr + len as u64;
            if !is_valid_user_byte_locked(sched, cur) {
                return Err(());
            }
            let byte = unsafe { (cur as *const u8).read() };
            if byte == 0 { break; }
            buf[len] = byte;
            len += 1;
        }
        Ok(())
    })?;

    core::str::from_utf8(&buf[..len]).map(|s| s.to_string()).map_err(|_| ())
}
