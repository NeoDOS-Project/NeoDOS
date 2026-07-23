//! Non-Ob syscall handlers — process, I/O, filesystem, memory, thread lifecycle.
//! All functions are `pub(super)` for SSDT registration in `mod.rs`.

use crate::log::LogSubsys;
use crate::scheduler::{self, ThreadState};
use crate::net::types::Ipv4Addr;
use super::{err_to_u64, SyscallError, is_user_ptr_valid, copy_user_string,
           current_handle_entry, set_current_handle, set_need_resched};

// ── Poll struct ──

const POLLIN: i16 = 1;
const POLLOUT: i16 = 2;
const POLLERR: i16 = 4;
const POLLHUP: i16 = 8;

#[repr(C)]
#[derive(Clone, Copy)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

pub(super) fn handler_exit(regs: super::Registers) -> u64 {
    let code = regs.rbx;
    crate::hal::without_interrupts(|| {
        kdebug!(LogSubsys::Syscall, "enter code={}", code);

        let s = crate::scheduler::current_scheduler();
        let mut scheduler = s.lock();
        let tid = scheduler.current_tid;
        if tid > 0 {
            kdebug!(LogSubsys::Syscall, "tid={} start", tid);
            if let Some(k) = scheduler.current_kthread_mut() {
                k.state = ThreadState::Terminated;
            }
            kdebug!(LogSubsys::Syscall, "marked Terminated");
            let pid = scheduler.current_pid();
            kdebug!(LogSubsys::Syscall, "pid={}", pid);
            if pid > 0 {
                kdebug!(LogSubsys::Syscall, "getting eproc");
                let eproc = scheduler.current_eprocess_mut();
                kdebug!(LogSubsys::Syscall, "got eproc: {:?}", eproc.is_some());
                if let Some(ep) = eproc {
                    ep.thread_count = ep.thread_count.saturating_sub(1);
                    ep.exit_code = code as i64;
                    kdebug!(LogSubsys::Syscall, "thread_count={}", ep.thread_count);
                    if ep.thread_count == 0 {
                        kdebug!(LogSubsys::Syscall, "freeing resources");
                        if let Some(slot) = ep.user_slot.take() {
                            crate::arch::x64::paging::free_user_slot(slot);
                        }
                        if ep.heap_base != 0 {
                            crate::arch::x64::paging::heap_free_range(
                                ep.heap_base,
                                ep.heap_base + crate::arch::x64::paging::PROCESS_HEAP_SIZE,
                            );
                            let heap_idx = ((ep.heap_base
                                - crate::arch::x64::paging::PROCESS_HEAP_BASE)
                                / crate::arch::x64::paging::PROCESS_HEAP_SIZE) as u8;
                            crate::arch::x64::paging::free_heap_slot(heap_idx);
                            ep.heap_base = 0;
                            ep.heap_break = 0;
                        }
                        for r in ep.mmap_regions.iter() {
                            crate::arch::x64::paging::mmap_free_range(r.base, r.base + r.len);
                        }
                        ep.mmap_regions.clear();
                        ep.mmap_next = crate::arch::x64::paging::MMAP_BASE;
                        for i in 0..ep.handle_table.len() {
                            let h = ep.handle_table[i];
                            if h.is_pipe_read() {
                                crate::object::pipe::PIPE_MANAGER.dec_read_ref(h.native_id().unwrap_or(0) as u8);
                            } else if h.is_pipe_write() {
                                crate::object::pipe::PIPE_MANAGER.dec_write_ref(h.native_id().unwrap_or(0) as u8);
                            } else if h.has_ob_object() {
                                let _ = crate::object::ob_close_object(h.object_id);
                            }
                            ep.handle_table.set(i as u8, crate::handle::HandleEntry::closed());
                        }
                        scheduler.wake_waiters(pid);
                    }
                    kdebug!(LogSubsys::Syscall, "after resource freeing");
                }
            }
            kdebug!(LogSubsys::Syscall, "wake_thread_joiner via KWait (OB-031)");
            let tj_magic = crate::kwait::WaitReason::ThreadJoin { tid }.encode_magic();
            for k in scheduler.kthreads.iter_mut().flatten() {
                if k.waiting_for == Some(tj_magic) && matches!(k.state, ThreadState::Blocked { .. }) {
                    k.waiting_for = None;
                    k.state = ThreadState::Ready;
                    scheduler::Scheduler::enqueue_to_cpu_run_queue(k);
                    set_need_resched();
                }
            }
            kdebug!(LogSubsys::Syscall, "checking: pid={} thread_count", pid);
            if pid > 0 {
                let ce_magic = crate::kwait::WaitReason::ChildExit { pid }.encode_magic();
                for k in scheduler.kthreads.iter_mut().flatten() {
                    if k.waiting_for == Some(ce_magic) && matches!(k.state, ThreadState::Blocked { .. }) {
                        k.waiting_for = None;
                        k.state = ThreadState::Ready;
                        scheduler::Scheduler::enqueue_to_cpu_run_queue(k);
                        set_need_resched();
                    }
                }
            }
            if pid > 0 {
                let eproc = scheduler.current_eprocess();
                if eproc.is_none_or(|ep| ep.thread_count == 0)
                    && pid == crate::usermode::current_wait_pid() {
                    crate::usermode::request_exit_to_kernel();
                }
            }
            // OB-046 fix: defer EPROCESS slot recycling to work queue.
            // Previously done in handler_ob_wait (which ran before child
            // context switch, destroying child prematurely) or inline here
            // (which removes current thread from kthreads while it's still
            // running, causing use-after-free in syscall return path).
            // Work queue items fire at safe points (syscall return / idle).
            let do_cleanup = pid > 0 && {
                let eproc_ref = scheduler.current_eprocess();
                eproc_ref.is_some_and(|ep| ep.thread_count == 0)
            };
            if do_cleanup {
                // Heap-allocate the pid so it outlives this stack frame
                let pid_box = alloc::boxed::Box::new(pid);
                let pid_ptr = alloc::boxed::Box::into_raw(pid_box) as *mut u8;
                crate::work_queue::WORK_QUEUE.push_high(
                    |data| {
                        let pid_box = unsafe { alloc::boxed::Box::from_raw(data as *mut u32) };
                        crate::scheduler::cleanup_terminated_process(*pid_box);
                    },
                    pid_ptr,
                );
            }
        }
        kdebug!(LogSubsys::Syscall, "done (after if tid > 0 block)");
    });
    kdebug!(LogSubsys::Syscall, "returned from without_interrupts");
    code
}

// ═══════════════════════════════════════════════════════════════════════
// I/O handlers
// ═══════════════════════════════════════════════════════════════════════

pub(super) fn handler_write(regs: super::Registers) -> u64 {
    let fd = regs.rbx as u8;
    let ptr = regs.rcx as *const u8;
    let len = regs.rdx as usize;

    if !is_user_ptr_valid(regs.rcx, len as u64) || len > 4096 {
        return err_to_u64(SyscallError::Fault);
    }
    let slice = unsafe { core::slice::from_raw_parts(ptr, len) };

    let entry = current_handle_entry(fd);

    if entry.is_stdout() || entry.is_stderr() {
        if let Ok(s) = core::str::from_utf8(slice) {
            crate::console::print_str(s);
        }
        len as u64
    } else if entry.is_pipe_write() {
        match crate::object::pipe::PIPE_MANAGER.write(entry.native_id().unwrap_or(0) as u8, slice) {
            Ok(n) => n as u64,
            Err(_) => err_to_u64(SyscallError::Pipe),
        }
    } else if entry.obj_type() == Some(crate::object::ObType::Filesystem) {
        let drive_idx = entry.drive().unwrap_or(0) as usize;
        let inode_num = entry.native_id().unwrap_or(0) as u32;
        let handle_offset = entry.offset;
        let result = crate::globals::with_vfs(|vfs| {
            vfs.write(drive_idx, inode_num, handle_offset, slice)
        });
        match result {
            Ok(bytes_written) => {
                crate::hal::without_interrupts(|| {
                    let s = crate::scheduler::current_scheduler();
                    let mut lock = s.lock();
                    if let Some(ep) = lock.current_eprocess_mut() {
                        ep.handle_table[fd as usize].offset += bytes_written as u64;
                    }
                });
                bytes_written as u64
            }
            Err(_) => err_to_u64(SyscallError::Io),
        }
    } else {
        err_to_u64(SyscallError::BadF)
    }
}

pub(super) fn handler_yield(_regs: super::Registers) -> u64 {
    crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let mut lock = s.lock();
        let tid = lock.current_tid;
        if tid > 0 {
            if let Some(k) = lock.current_kthread_mut() {
                if k.state == ThreadState::Running {
                    k.state = ThreadState::Ready;
                }
                let idx = (k.priority as usize).min(
                    crate::scheduler::PRIORITY_COUNT as usize - 1);
                k.time_slice_remaining = crate::scheduler::TIME_SLICES[idx];
            }
        }
    });
    crate::net::network_poll_all();
    set_need_resched();
    0
}

pub(super) fn handler_read(regs: super::Registers) -> u64 {
    let fd = regs.rbx as u8;
    let buf_ptr = regs.rcx as *mut u8;
    let count = regs.rdx as usize;

    if !is_user_ptr_valid(regs.rcx, count as u64) || count > 4096 {
        return err_to_u64(SyscallError::Fault);
    }

    let entry = current_handle_entry(fd);

    if entry.is_stdin() {
        let vt = crate::scheduler::current_vt_num();
        let mut bytes_read = 0usize;
        while bytes_read < count {
            match crate::input::pop_byte_from_vt(vt as usize) {
                Some(byte) => {
                    unsafe { buf_ptr.add(bytes_read).write(byte); }
                    bytes_read += 1;
                    if byte == b'\r' || byte == b'\n' {
                        break;
                    }
                }
                None => {
                    if bytes_read > 0 {
                        break;
                    }
                    loop {
                        if let Some(b) = crate::input::pop_byte_from_vt(vt as usize) {
                            unsafe { buf_ptr.add(bytes_read).write(b); }
                            bytes_read += 1;
                            break;
                        }
                        crate::eventbus::EVENT_BUS.dispatch_pending();
                        if let Some(b) = crate::input::pop_byte_from_vt(vt as usize) {
                            unsafe { buf_ptr.add(bytes_read).write(b); }
                            bytes_read += 1;
                            break;
                        }
                        unsafe { core::arch::asm!("sti; hlt; cli", options(nomem, nostack)); }
                    }
                }
            }
        }
        bytes_read as u64
    } else if entry.is_pipe_read() {
        let pipe_id = entry.native_id().unwrap_or(0) as u8;
        let mut temp_buf = alloc::vec![0u8; count];
        match crate::object::pipe::PIPE_MANAGER.read(pipe_id, &mut temp_buf) {
            Ok(0) => {
                0
            }
            Ok(n) => {
                unsafe {
                    core::ptr::copy_nonoverlapping(temp_buf.as_ptr(), buf_ptr, n);
                }
                n as u64
            }
            Err(()) => {
                crate::object::pipe::block_current_for_pipe(pipe_id);
                err_to_u64(SyscallError::Again)
            }
        }
    } else {
        err_to_u64(SyscallError::BadF)
    }
}

pub(super) fn handler_dup2(regs: super::Registers) -> u64 {
    let old_fd = regs.rbx as u8;
    let new_fd = regs.rcx as u8;

    let src_entry = current_handle_entry(old_fd);
    if !src_entry.is_open() {
        return err_to_u64(SyscallError::BadF);
    }

    let dst_entry = current_handle_entry(new_fd);
    if dst_entry.is_pipe_read() {
        crate::object::pipe::PIPE_MANAGER.dec_read_ref(dst_entry.native_id().unwrap_or(0) as u8);
    } else if dst_entry.is_pipe_write() {
        crate::object::pipe::PIPE_MANAGER.dec_write_ref(dst_entry.native_id().unwrap_or(0) as u8);
    } else if dst_entry.has_ob_object() {
        let _ = crate::object::ob_close_object(dst_entry.object_id);
    }

    if src_entry.is_pipe_read() {
        crate::object::pipe::PIPE_MANAGER.inc_read_ref(src_entry.native_id().unwrap_or(0) as u8);
    } else if src_entry.is_pipe_write() {
        crate::object::pipe::PIPE_MANAGER.inc_write_ref(src_entry.native_id().unwrap_or(0) as u8);
    }

    set_current_handle(new_fd, src_entry);
    new_fd as u64
}

pub(super) fn handler_waitpid(regs: super::Registers) -> u64 {
    let wait_pid = regs.rbx as u32;

    if wait_pid == 0xFFFFFFFF {
        let child_pid = crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler();
            let scheduler = s.lock();
            let my_pid = scheduler.current_pid();
            for ep in scheduler.eprocesses.iter().flatten() {
                if ep.parent_pid == my_pid && ep.thread_count == 0 {
                    return Some(ep.pid);
                }
            }
            None
        });

        if let Some(pid) = child_pid {
            crate::scheduler::cleanup_terminated_process(pid);
            return pid as u64;
        }
        crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler();
            let mut lock = s.lock();
            let tid = lock.current_tid;
            if tid > 0 {
                if let Some(k) = lock.current_kthread_mut() {
                    if k.state == ThreadState::Running {
                        k.state = ThreadState::Ready;
                    }
                }
            }
        });
        set_need_resched();
        0
    } else {
        let is_terminated = crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler();
            let scheduler = s.lock();
            if let Some(ep) = scheduler.find_eprocess(wait_pid) {
                ep.thread_count == 0
            } else {
                true
            }
        });

        if is_terminated {
            crate::scheduler::cleanup_terminated_process(wait_pid);
            0
        } else {
            crate::kwait::kwait_block(crate::kwait::WaitReason::ChildExit { pid: wait_pid });
            err_to_u64(SyscallError::Again)
        }
    }
}

pub(super) fn handler_close(regs: super::Registers) -> u64 {
    let fd = regs.rbx as u8;
    let entry = current_handle_entry(fd);
    if entry.is_pipe_read() {
        crate::object::pipe::PIPE_MANAGER.dec_read_ref(entry.native_id().unwrap_or(0) as u8);
    } else if entry.is_pipe_write() {
        crate::object::pipe::PIPE_MANAGER.dec_write_ref(entry.native_id().unwrap_or(0) as u8);
    }
    if entry.object_id != 0 {
        let _ = crate::object::ob_close_object(entry.object_id);
    }
    set_current_handle(fd, crate::handle::HandleEntry::closed());
    0
}

// ═══════════════════════════════════════════════════════════════════════
// Memory handlers
// ═══════════════════════════════════════════════════════════════════════

pub(super) fn handler_brk(regs: super::Registers) -> u64 {
    let new_break = regs.rbx;
    let (heap_base, current_break) = crate::scheduler::current_process_heap_range();

    if heap_base == 0 {
        return err_to_u64(SyscallError::NoMem);
    }

    if new_break == 0 {
        return current_break;
    }

    let heap_limit = heap_base + crate::arch::x64::paging::PROCESS_HEAP_SIZE;

    if new_break < heap_base || new_break > heap_limit {
        return err_to_u64(SyscallError::Inval);
    }

    if new_break > current_break {
        let start_page = (current_break + 0xFFF) & !0xFFF;
        let end_page = new_break & !0xFFF;
        if end_page > start_page {
            let mut page = start_page;
            while page < end_page {
                match crate::arch::x64::paging::heap_alloc_page(page) {
                    Some(_) => {
                        unsafe { core::ptr::write_volatile(page as *mut u8, 0); }
                    }
                    None => {
                        crate::arch::x64::paging::heap_free_range(start_page, page);
                        crate::scheduler::set_current_heap_break(current_break);
                        return err_to_u64(SyscallError::NoMem);
                    }
                }
                page += crate::arch::x64::paging::PAGE_4K;
            }
        }
        unsafe {
            core::ptr::write_bytes(current_break as *mut u8, 0,
                (new_break - current_break) as usize);
        }
    } else if new_break < current_break {
        let shrink_start = new_break;
        let shrink_end = current_break;
        let start_page = (shrink_start + crate::arch::x64::paging::PAGE_4K - 1)
            & !(crate::arch::x64::paging::PAGE_4K - 1);
        let end_page = shrink_end & !(crate::arch::x64::paging::PAGE_4K - 1);
        let mut page = start_page;
        while page < end_page {
            crate::arch::x64::paging::heap_free_page(page);
            page += crate::arch::x64::paging::PAGE_4K;
        }
    }

    crate::scheduler::set_current_heap_break(new_break);
    new_break
}

pub(super) fn handler_mmap(regs: super::Registers) -> u64 {
    let _addr_hint = regs.rbx;
    let length = regs.rcx;
    let prot = regs.rdx as u16;
    let flags = regs.r8 as u16;
    let fd = regs.r9 as u8;

    if length == 0 || length > 0x100000 {
        return err_to_u64(SyscallError::Inval);
    }
    if prot & !3 != 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let is_anon = (flags & 1) != 0;

    if is_anon {
        let alloc_size = (length + 0xFFF) & !0xFFF;
        let region = crate::scheduler::MmapRegion {
            base: 0,
            len: alloc_size,
            prot,
            flags: 1,
            drive: 0,
            inode: 0,
            file_size: 0,
        };
        match crate::scheduler::add_current_mmap_region(region) {
            Some(base) => base,
            None => err_to_u64(SyscallError::NoMem),
        }
    } else {
        let (drive_idx, inode_num) = crate::hal::without_interrupts(|| {
            let s = scheduler::current_scheduler();
            let mut lock = s.lock();
            if let Some(ep) = lock.current_eprocess_mut() {
                let entry = ep.handle_table[fd as usize];
                if entry.obj_type() == Some(crate::object::ObType::Filesystem) {
                    return (entry.drive().unwrap_or(0) as usize, entry.native_id().unwrap_or(0) as u32);
                }
            }
            (usize::MAX, 0)
        });

        if drive_idx == usize::MAX {
            return err_to_u64(SyscallError::BadF);
        }

        let file_info = crate::globals::with_vfs(|vfs| {
            vfs.stat(drive_idx, inode_num)
        });
        let (file_size, file_mode) = match file_info {
            Ok(node) => (node.size, node.mode),
            Err(_) => return err_to_u64(SyscallError::NoEnt),
        };
        if (file_mode & crate::fs::vfs::MODE_FILE) == 0 {
            return err_to_u64(SyscallError::IsDir);
        }

        let alloc_size = (length + 0xFFF) & !0xFFF;
        let region = crate::scheduler::MmapRegion {
            base: 0,
            len: alloc_size,
            prot,
            flags: 0,
            drive: drive_idx as u8,
            inode: inode_num,
            file_size,
        };
        match crate::scheduler::add_current_mmap_region(region) {
            Some(base) => base,
            None => err_to_u64(SyscallError::NoMem),
        }
    }
}

pub(super) fn handler_munmap(regs: super::Registers) -> u64 {
    let addr = regs.rbx;
    let length = regs.rcx;

    if length == 0 || addr & 0xFFF != 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let region = crate::scheduler::remove_current_mmap_region(addr);
    match region {
        Some(r) => {
            crate::scheduler::free_current_mmap_pages(r.base, r.len);
            0
        }
        None => err_to_u64(SyscallError::Inval),
    }
}

pub(super) fn handler_loadlib(regs: super::Registers) -> u64 {
    let path_str = match copy_user_string(regs.rbx) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    if path_str.is_empty() {
        return err_to_u64(SyscallError::NoEnt);
    }

    match crate::nxl::nxl_load(&path_str) {
        Some(base) => {
            kinfo!(LogSubsys::Syscall, "sys_loadlib '{}' => 0x{:x}", path_str, base);
            base
        }
        None => {
            kerror!(LogSubsys::Syscall, "sys_loadlib FAILED '{}'", path_str);
            err_to_u64(SyscallError::NoEnt)
        }
    }
}

pub(super) fn handler_wait_alertable(_regs: super::Registers) -> u64 {
    if crate::apc::has_pending_user_apcs() {
        crate::apc::dispatch_one_user_apc();
        return crate::apc::APC_ALERTED;
    }
    crate::apc::block_current_alertable();
    if crate::apc::has_pending_user_apcs() {
        crate::apc::dispatch_one_user_apc();
        return crate::apc::APC_ALERTED;
    }
    0
}

pub(super) fn handler_sleep_ex(_regs: super::Registers) -> u64 {
    if crate::apc::has_pending_user_apcs() {
        crate::apc::dispatch_one_user_apc();
        return crate::apc::APC_ALERTED;
    }
    crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let mut lock = s.lock();
        let tid = lock.current_tid;
        if tid > 0 {
            if let Some(k) = lock.current_kthread_mut() {
                if k.state == crate::scheduler::ThreadState::Running {
                    k.state = crate::scheduler::ThreadState::Ready;
                }
                let idx = (k.priority as usize).min(
                    crate::scheduler::PRIORITY_COUNT as usize - 1);
                k.time_slice_remaining = crate::scheduler::TIME_SLICES[idx];
            }
        }
    });
    set_need_resched();
    if crate::apc::has_pending_user_apcs() {
        crate::apc::dispatch_one_user_apc();
        return crate::apc::APC_ALERTED;
    }
    0
}

pub(super) fn handler_set_exception_handler(regs: super::Registers) -> u64 {
    let handler_fn_addr = regs.rbx;

    if handler_fn_addr == 0 {
        let teb_base = crate::scheduler::current_teb_base();
        if teb_base == 0 {
            return (-1i64) as u64;
        }
        let teb = teb_base as *mut crate::exception::Teb;
        unsafe {
            (*teb).exception_list = None;
        }
        return 0;
    }

    if !is_user_ptr_valid(handler_fn_addr, 1) {
        return err_to_u64(SyscallError::Fault);
    }

    let handler_fn = unsafe {
        core::mem::transmute::<u64, extern "C" fn(u32, u64, u64) -> u32>(handler_fn_addr)
    };

    match crate::exception::set_thread_exception_handler(Some(handler_fn)) {
        0 => 0,
        _ => (-1i64) as u64,
    }
}

pub(super) fn handler_cursor_blink(regs: super::Registers) -> u64 {
    match regs.rbx {
        0 => { crate::console::set_cursor_blink(false); 0 }
        1 => { crate::console::set_cursor_blink(true); 0 }
        _ => err_to_u64(SyscallError::Inval),
    }
}

pub(super) fn handler_driver_unload(regs: super::Registers) -> u64 {
    let name_ptr = regs.rbx;
    let force = regs.rcx != 0;

    if name_ptr == 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let name = match copy_user_string(name_ptr) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    match crate::drivers::hotreload::unload_driver(&name, force) {
        Ok(_) => 0,
        Err(_) => err_to_u64(SyscallError::Io),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Poll handler
// ═══════════════════════════════════════════════════════════════════════

pub(super) fn handler_poll(regs: super::Registers) -> u64 {
    let fds_ptr = regs.rbx as *mut PollFd;
    let nfds = regs.rcx as usize;
    let _timeout = regs.rdx as i64;

    if fds_ptr.is_null() || nfds == 0 || nfds > 256 {
        return err_to_u64(SyscallError::Inval);
    }

    let mut fds = alloc::vec![PollFd { fd: 0, events: 0, revents: 0 }; nfds];
    for (i, fd_entry) in fds.iter_mut().enumerate().take(nfds) {
        unsafe {
            let src = fds_ptr.add(i);
            fd_entry.fd = core::ptr::read_volatile(&(*src).fd);
            fd_entry.events = core::ptr::read_volatile(&(*src).events);
        }
    }

    let mut ready_count: u64 = 0;
    for fd_entry in fds.iter_mut() {
        let fd = fd_entry.fd;
        if fd < 0 {
            fd_entry.revents = 0;
            continue;
        }
        let entry = current_handle_entry(fd as u8);
        if !entry.is_open() {
            fd_entry.revents = POLLERR;
            ready_count += 1;
            continue;
        }

        let mut rev: i16 = 0;
        if entry.is_stdin() {
            if fd_entry.events & POLLIN != 0 {
                rev |= POLLIN;
            }
        } else if entry.is_stdout() || entry.is_stderr() {
            if fd_entry.events & POLLOUT != 0 {
                rev |= POLLOUT;
            }
        } else if entry.is_pipe_read() {
            let pipe_id = entry.native_id().unwrap_or(0) as u8;
            let ready = crate::object::pipe::pipe_peek_read_ready(pipe_id).unwrap_or(false);
            if ready && fd_entry.events & POLLIN != 0 {
                rev |= POLLIN;
            }
            if crate::object::pipe::pipe_peek_write_closed(pipe_id).unwrap_or(false) {
                rev |= POLLHUP;
            }
        } else if entry.is_pipe_write() {
            if fd_entry.events & POLLOUT != 0 {
                rev |= POLLOUT;
            }
        } else if entry.obj_type() == Some(crate::object::ObType::Filesystem)
            || entry.obj_type() == Some(crate::object::ObType::Directory) {
            if fd_entry.events & POLLIN != 0 { rev |= POLLIN; }
            if fd_entry.events & POLLOUT != 0 { rev |= POLLOUT; }
        } else {
            rev |= POLLERR;
        }
        fd_entry.revents = rev;
        if rev != 0 {
            ready_count += 1;
        }
    }

    for (i, fd_entry) in fds.iter().enumerate().take(nfds) {
        unsafe {
            core::ptr::write_volatile(&mut (*fds_ptr.add(i)).revents, fd_entry.revents);
        }
    }

    ready_count
}

/// RAX 36: icmp_ping(ipv4_addr_be32) -> rtt_us or 0 on failure
/// Sends an ICMP echo request to the given IPv4 address (in big-endian u32)
/// and waits up to ~1 second for a reply. Returns round-trip time in microseconds,
/// or 0 if the ping failed (timeout, ARP failure, no NIC).
pub(super) fn handler_icmp_ping(regs: super::Registers) -> u64 {
    let ip_be = regs.rbx as u32;
    let dest_ip = Ipv4Addr::from_u32(ip_be);
    match crate::net::icmp::icmp_ping(dest_ip, 1_000_000) {
        Some(rtt_us) => rtt_us,
        None => 0,
    }
}
