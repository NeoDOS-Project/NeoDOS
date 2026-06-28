//! Non-Ob syscall handlers — process, I/O, filesystem, memory, thread lifecycle.
//! All functions are `pub(super)` for SSDT registration in `mod.rs`.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use crate::serial_println;
use crate::scheduler::{self, ThreadState};
use super::{err_to_u64, SyscallError, is_user_ptr_valid, copy_user_string,
           current_handle_entry, set_current_handle, set_need_resched,
           copy_handle_entry_for_child, check_legacy_path_access,
           resolve_chdir_target, generate_info_content};

// ── ABI-stable directory entry struct ──

#[repr(C)]
struct DirEntryRaw {
    pub inode: u32,
    pub mode: u16,
    pub size: u32,
    pub name: [u8; 260],
}

// ── Fsck stats struct ──

#[repr(C)]
struct FsckStatsRaw {
    total_inodes: u32,
    used_inodes: u32,
    valid_inodes: u32,
    corrupted_inodes: u32,
    cross_linked_blocks: u32,
    orphan_inodes: u32,
    dangling_entries: u32,
    dir_errors: u32,
    superblock_errors: u32,
    repairs_applied: u32,
}

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

// ═══════════════════════════════════════════════════════════════════════
// Process lifecycle
// ═══════════════════════════════════════════════════════════════════════

pub(super) fn handler_spawn(regs: super::Registers) -> u64 {
    let path_str = match copy_user_string(regs.rbx) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };
    if path_str.is_empty() {
        return err_to_u64(SyscallError::NoEnt);
    }

    if let Err(e) = check_legacy_path_access(&path_str, crate::security::acl::ACCESS_EXECUTE) {
        return e;
    }

    let stdin_fd = regs.rcx as u8;
    let stdout_fd = regs.rdx as u8;
    let stderr_fd = regs.r8 as u8;
    serial_println!("[SPAWN] path='{}' stdin_fd={} stdout_fd={} stderr_fd={}",
        path_str, stdin_fd, stdout_fd, stderr_fd);

    const MAX_BIN: usize = 65536;
    static mut BIN_BUF: [u8; MAX_BIN] = [0u8; MAX_BIN];
    let bin_size = crate::globals::with_vfs(|vfs| {
        match vfs.resolve_path(&path_str) {
            Ok((drive_idx, node)) => {
                if (node.mode & crate::fs::vfs::MODE_FILE) == 0 { return 0; }
                unsafe {
                    match vfs.read(drive_idx, node.inode, 0, &mut BIN_BUF) {
                        Ok(n) => { if n > MAX_BIN { 0 } else { n } }
                        Err(_) => 0,
                    }
                }
            }
            Err(_) => 0,
        }
    });
    if bin_size < 4 {
        return err_to_u64(SyscallError::NoEnt);
    }

    let slot = match crate::arch::x64::paging::alloc_user_slot() {
        Some(s) => s,
        None => {
            return err_to_u64(SyscallError::NoMem);
        }
    };
    serial_println!("[SPAWN] allocated slot {} at code_base=0x{:x}",
        slot.slot_idx, slot.code_base);

    let data = unsafe { &BIN_BUF[..bin_size] };
    let result = match crate::elf::load_elf(data, None, slot.code_base) {
        Ok(r) => r,
        Err(_) => {
            crate::arch::x64::paging::free_user_slot(slot.slot_idx);
            return err_to_u64(SyscallError::Inval);
        }
    };
    let entry = result.entry;
    serial_println!("[SPAWN] ELF loaded: entry=0x{:x}, {} segments", entry, result.segments.len());

    let (parent_stdin_entry, parent_stdout_entry, parent_stderr_entry) = crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let lock = s.lock();
        let get_parent_entry = |fd: u8| -> Option<crate::handle::HandleEntry> {
            if let Some(ep) = lock.current_eprocess() {
                Some(ep.handle_table.get(fd))
            } else { None }
        };
        let sin = if stdin_fd != 0xFF { get_parent_entry(stdin_fd) } else { None };
        let sout = if stdout_fd != 0xFF { get_parent_entry(stdout_fd) } else { None };
        let serr = if stderr_fd != 0xFF { get_parent_entry(stderr_fd) } else { None };
        (sin, sout, serr)
    });

    if let Some(ref e) = parent_stdin_entry {
        if !e.is_open() {
            crate::arch::x64::paging::free_user_slot(slot.slot_idx);
            return err_to_u64(SyscallError::BadF);
        }
    }
    if let Some(ref e) = parent_stdout_entry {
        if !e.is_open() {
            crate::arch::x64::paging::free_user_slot(slot.slot_idx);
            return err_to_u64(SyscallError::BadF);
        }
    }
    if let Some(ref e) = parent_stderr_entry {
        if !e.is_open() {
            crate::arch::x64::paging::free_user_slot(slot.slot_idx);
            return err_to_u64(SyscallError::BadF);
        }
    }

    let (cwd_drive, cwd_path, parent_pid) = crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler().lock();
        let pid = s.current_pid();
        let cwd = if let Some(ep) = s.find_eprocess(pid) {
            (ep.cwd_drive, ep.cwd_path.clone())
        } else {
            (2u8, String::from("\\"))
        };
        (cwd.0, cwd.1, pid)
    });

    let child_pid = crate::usermode::spawn_usermode(
        entry, slot.stack_top, slot.slot_idx,
        cwd_drive, &cwd_path, parent_pid,
    );
    serial_println!("[SPAWN] child PID={}", child_pid);

    if stdin_fd != 0xFF || stdout_fd != 0xFF || stderr_fd != 0xFF {
        crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler();
            let mut lock = s.lock();
            if let Some(ep) = lock.find_eprocess_mut(child_pid) {
                if let Some(ref entry) = parent_stdin_entry {
                    let child_entry = copy_handle_entry_for_child(entry);
                    ep.handle_table.set(0, child_entry);
                }
                if let Some(ref entry) = parent_stdout_entry {
                    let child_entry = copy_handle_entry_for_child(entry);
                    ep.handle_table.set(1, child_entry);
                }
                if let Some(ref entry) = parent_stderr_entry {
                    let child_entry = copy_handle_entry_for_child(entry);
                    ep.handle_table.set(2, child_entry);
                }
            }
        });
    }

    let (child_kernel_top, child_tid, neoinit_kernel_top) = crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler().lock();
        let mut ct = 0u64;
        let mut c_tid = 0u32;
        let mut neo_top = 0u64;
        for th in s.kthreads.iter() {
            if let Some(k) = th {
                if k.pid == child_pid && k.tid > 0 {
                    ct = k.kernel_stack_top;
                    c_tid = k.tid;
                }
                if k.pid == parent_pid && k.tid > 0 {
                    neo_top = k.kernel_stack_top;
                }
            }
        }
        (ct, c_tid, neo_top)
    });

    crate::hal::without_interrupts(|| {
        let mut s = crate::scheduler::current_scheduler().lock();
        for th in s.kthreads.iter() {
            if let Some(k) = th {
                if k.tid == child_tid {
                    s.current_tid = k.tid;
                    break;
                }
            }
        }
        if let Some(k) = s.current_kthread_mut() {
            k.state = ThreadState::Running;
        }
    });

    crate::arch::x64::gdt::set_kernel_stack(child_kernel_top);
    crate::usermode::set_wait_pid(child_pid);

    serial_println!("[SPAWN] entering child at entry=0x{:x}, stack=0x{:x}", entry, slot.stack_top);
    crate::usermode::execute_usermode(entry, slot.stack_top);

    serial_println!("[SPAWN] child exited");
    crate::arch::x64::gdt::set_kernel_stack(neoinit_kernel_top);

    crate::hal::without_interrupts(|| {
        let mut s = crate::scheduler::current_scheduler().lock();
        for th in s.kthreads.iter() {
            if let Some(k) = th {
                if k.pid == parent_pid && k.tid > 0 {
                    s.current_tid = k.tid;
                    break;
                }
            }
        }
    });

    crate::scheduler::cleanup_terminated_process(child_pid);
    serial_println!("[SPAWN] done, returning child PID {}", child_pid);

    child_pid as u64
}

pub(super) fn handler_poweroff(_regs: super::Registers) -> u64 {
    serial_println!("[POWEROFF] sys_poweroff called — shutting down");
    crate::globals::flush_cache_if_needed();
    let _ = crate::eventbus::EVENT_BUS.push_event(
        crate::eventbus::EVENT_SHUTDOWN,
        crate::eventbus::SOURCE_KERNEL,
        0, 0, 0, 0,
    );
    crate::eventbus::EVENT_BUS.dispatch_pending();
    crate::hal::poweroff();
}

pub(super) fn handler_exit(regs: super::Registers) -> u64 {
    let code = regs.rbx;
    crate::hal::without_interrupts(|| {
        serial_println!("[EXIT] enter code={}", code);
        let s = crate::scheduler::current_scheduler();
        let mut scheduler = s.lock();
        let tid = scheduler.current_tid;
        if tid > 0 {
            serial_println!("[EXIT] tid={} start", tid);
            if let Some(k) = scheduler.current_kthread_mut() {
                k.state = ThreadState::Terminated;
            }
            serial_println!("[EXIT] marked Terminated");
            let pid = scheduler.current_pid();
            serial_println!("[EXIT] pid={}", pid);
            if pid > 0 {
                serial_println!("[EXIT] getting eproc");
                let eproc = scheduler.current_eprocess_mut();
                serial_println!("[EXIT] got eproc: {:?}", eproc.is_some());
                if let Some(ep) = eproc {
                    ep.thread_count = ep.thread_count.saturating_sub(1);
                    ep.exit_code = code as i64;
                    serial_println!("[EXIT] thread_count={}", ep.thread_count);
                    if ep.thread_count == 0 {
                        serial_println!("[EXIT] freeing resources");
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
                    serial_println!("[EXIT] after resource freeing");
                }
            }
            serial_println!("[EXIT] wake_thread_joiner via KWait (OB-031)");
            let tj_magic = crate::kwait::WaitReason::ThreadJoin { tid }.encode_magic();
            for th in scheduler.kthreads.iter_mut() {
                if let Some(k) = th {
                    if k.waiting_for == Some(tj_magic) && matches!(k.state, ThreadState::Blocked { .. }) {
                        k.waiting_for = None;
                        k.state = ThreadState::Ready;
                        scheduler::Scheduler::enqueue_to_cpu_run_queue(k);
                        set_need_resched();
                    }
                }
            }
            serial_println!("[EXIT] checking: pid={} thread_count", pid);
            if pid > 0 {
                let ce_magic = crate::kwait::WaitReason::ChildExit { pid }.encode_magic();
                for th in scheduler.kthreads.iter_mut() {
                    if let Some(k) = th {
                        if k.waiting_for == Some(ce_magic) && matches!(k.state, ThreadState::Blocked { .. }) {
                            k.waiting_for = None;
                            k.state = ThreadState::Ready;
                            scheduler::Scheduler::enqueue_to_cpu_run_queue(k);
                            set_need_resched();
                        }
                    }
                }
            }
            if pid > 0 {
                let eproc = scheduler.current_eprocess();
                if eproc.map_or(true, |ep| ep.thread_count == 0) {
                    if pid == crate::usermode::current_wait_pid() {
                        crate::usermode::request_exit_to_kernel();
                    }
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
                eproc_ref.map_or(false, |ep| ep.thread_count == 0)
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
        serial_println!("[EXIT] done (after if tid > 0 block)");
    });
    serial_println!("[EXIT] returned from without_interrupts");
    code
}

// ═══════════════════════════════════════════════════════════════════════
// I/O handlers
// ═══════════════════════════════════════════════════════════════════════

pub(super) fn handler_write(regs: super::Registers) -> u64 {
    let fd = regs.rbx as u8;
    let ptr = regs.rcx as *const u8;
    let len = regs.rdx as usize;

    let entry = current_handle_entry(fd);

    if entry.is_stdout() || entry.is_stderr() {
        if !is_user_ptr_valid(regs.rcx, len as u64) || len > 4096 {
            return err_to_u64(SyscallError::Fault);
        }
        let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
        if let Ok(s) = core::str::from_utf8(slice) {
            crate::console::print_str(s);
        }
        len as u64
    } else if entry.is_pipe_write() {
        if !is_user_ptr_valid(regs.rcx, len as u64) || len > 4096 {
            return err_to_u64(SyscallError::Fault);
        }
        let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
        match crate::object::pipe::PIPE_MANAGER.write(entry.native_id().unwrap_or(0) as u8, slice) {
            Ok(n) => n as u64,
            Err(_) => err_to_u64(SyscallError::Pipe),
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
    set_need_resched();
    0
}

pub(super) fn handler_getpid(_regs: super::Registers) -> u64 {
    let pid = crate::hal::without_interrupts(|| {
        crate::scheduler::current_scheduler().lock().current_pid()
    });
    pid as u64
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
        let mut temp_buf = alloc::vec::Vec::with_capacity(count);
        temp_buf.resize(count, 0u8);
        loop {
            match crate::object::pipe::PIPE_MANAGER.read(pipe_id, &mut temp_buf) {
                Ok(0) => {
                    return 0;
                }
                Ok(n) => {
                    unsafe {
                        core::ptr::copy_nonoverlapping(temp_buf.as_ptr(), buf_ptr, n);
                    }
                    return n as u64;
                }
                Err(()) => {
                    crate::object::pipe::block_current_for_pipe(pipe_id);
                    return err_to_u64(SyscallError::Again);
                }
            }
        }
    } else {
        err_to_u64(SyscallError::BadF)
    }
}

pub(super) fn handler_pipe(regs: super::Registers) -> u64 {
    let fds_ptr = regs.rbx as *mut u64;
    if !is_user_ptr_valid(regs.rbx, 16) {
        return err_to_u64(SyscallError::Fault);
    }

    let pipe_id = match crate::object::pipe::PIPE_MANAGER.alloc() {
        Some(pid) => pid,
        None => return err_to_u64(SyscallError::NoMem),
    };

    let name = alloc::format!("PIPE{}", pipe_id);
    let ob_id = match crate::object::ob_create_object(
        crate::object::ObType::Pipe, &name, pipe_id as u64, 0, Some(&crate::object::pipe::PIPE_OPS),
    ) {
        Ok(id) => id,
        Err(_) => {
            crate::object::pipe::PIPE_MANAGER.free_pipe(pipe_id);
            return err_to_u64(SyscallError::NoMem);
        }
    };

    let handle_result = crate::hal::without_interrupts(|| -> Result<(u8, u8), ()> {
        let s = crate::scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(ep) = lock.current_eprocess_mut() {
            let read_entry = crate::handle::HandleEntry {
                object_id: ob_id,
                offset: 0,
            };
            let write_entry = crate::handle::HandleEntry {
                object_id: ob_id,
                offset: 1,
            };
            match crate::handle::alloc_two_handles(&mut ep.handle_table, read_entry, write_entry) {
                Some((r, w)) => Ok((r, w)),
                None => Err(()),
            }
        } else {
            Err(())
        }
    });

    let (rfd, wfd) = match handle_result {
        Ok(pair) => pair,
        Err(_) => {
            let _ = crate::object::ob_close_object(ob_id);
            return err_to_u64(SyscallError::NoMem);
        }
    };

    let _ = crate::object::ob_reference(ob_id);
    let _ = crate::object::ob_reference(ob_id);
    let _ = crate::object::ob_close_object(ob_id);

    crate::object::pipe::PIPE_MANAGER.inc_read_ref(pipe_id);
    crate::object::pipe::PIPE_MANAGER.inc_write_ref(pipe_id);

    unsafe {
        fds_ptr.write(rfd as u64);
        fds_ptr.add(1).write(wfd as u64);
    }
    0
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
            for ep in scheduler.eprocesses.iter() {
                if let Some(ep) = ep {
                    if ep.parent_pid == my_pid && ep.thread_count == 0 {
                        return Some(ep.pid);
                    }
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
        return 0;
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

// ═══════════════════════════════════════════════════════════════════════
// Filesystem handlers
// ═══════════════════════════════════════════════════════════════════════

pub(super) fn handler_open(regs: super::Registers) -> u64 {
    let path_ptr = regs.rbx as *const u8;
    let flags = regs.rcx;

    const O_CREAT: u64 = 1;

    if !is_user_ptr_valid(regs.rbx, 1) {
        return err_to_u64(SyscallError::Fault);
    }

    let mut path_bytes = [0u8; 256];
    let mut path_len = 0usize;

    unsafe {
        while path_len < 255 {
            let byte = path_ptr.add(path_len).read();
            if byte == 0 { break; }
            path_bytes[path_len] = byte;
            path_len += 1;
        }
    }

    if path_len == 0 {
        return err_to_u64(SyscallError::NoEnt);
    }

    let path = match core::str::from_utf8(&path_bytes[..path_len]) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Inval),
    };

    let try_ob_path: Option<alloc::string::String> = if path.starts_with('\\') && !path.contains(':') {
        Some(path.to_string())
    } else if path.contains(':') {
        let normalized = path.replace('/', "\\");
        Some(format!("\\Global\\FileSystem\\{}", normalized))
    } else {
        None
    };
    if let Some(ref ob_path) = try_ob_path {
        let token = crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler();
            let lock = s.lock();
            lock.current_eprocess()
                .map(|ep| ep.token)
                .unwrap_or(*crate::security::DEFAULT_ADMIN_TOKEN)
        });
        let desired_access = crate::security::acl::ACCESS_READ |
            crate::security::acl::ACCESS_WRITE;
        if let Ok(ob_id) = crate::object::ob_open_path(ob_path, &token, desired_access) {
            let entry = crate::handle::HandleEntry::ob_object(ob_id, desired_access);
            let fd = crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    crate::handle::alloc_handle(&mut ep.handle_table, entry)
                } else {
                    None
                }
            });
            match fd {
                Some(fd) => {
                    serial_println!("[OPEN-OB] fd={} for path='{}' ob_id={}",
                        fd, path, ob_id);
                    return fd as u64;
                }
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    return err_to_u64(SyscallError::NoMem);
                }
            }
        }
    }

    fn resolve_path_inner(vfs: &mut crate::fs::vfs::Vfs, path: &str) -> Result<(usize, crate::fs::vfs::VfsNode), crate::fs::vfs::VfsError> {
        let has_drive = path.contains(':');
        let starts_with_sep = path.starts_with('\\') || path.starts_with('/');
        if has_drive || starts_with_sep {
            vfs.resolve_path(path)
        } else {
            let (drive, cwd_path) = crate::scheduler::get_current_cwd();
            let drive_char = (b'A' + drive) as char;
            let abs = alloc::format!("{}:{}\\{}", drive_char, cwd_path, path);
            vfs.resolve_path(&abs)
        }
    }

    let (drive_idx, node) = match crate::globals::with_vfs(|vfs| resolve_path_inner(vfs, path)) {
        Ok(result) => result,
        Err(_) => {
            if (flags & O_CREAT) != 0 {
                match crate::globals::with_vfs(|vfs| vfs.create(path)) {
                    Ok(_) => {}
                    Err(_) => return err_to_u64(SyscallError::Io),
                }
                match crate::globals::with_vfs(|vfs| resolve_path_inner(vfs, path)) {
                    Ok((drv, created_node)) => {
                        let entry = if (created_node.mode & crate::fs::vfs::MODE_FILE) != 0 {
                            crate::handle::HandleEntry::file(drv as u8, created_node.inode)
                        } else {
                            return err_to_u64(SyscallError::Inval);
                        };
                        let fd = crate::hal::without_interrupts(|| {
                            let s = scheduler::current_scheduler();
                            let mut lock = s.lock();
                            if let Some(ep) = lock.current_eprocess_mut() {
                                crate::handle::alloc_handle(&mut ep.handle_table, entry)
                            } else {
                                None
                            }
                        });
                        return match fd {
                            Some(fd) => {
                                serial_println!("[OPEN-O_CREAT] fd={} for path={} inode={}",
                                    fd, path, created_node.inode);
                                fd as u64
                            }
                            None => err_to_u64(SyscallError::NoMem),
                        };
                    }
                    Err(_) => return err_to_u64(SyscallError::Io),
                }
            }
            return err_to_u64(SyscallError::NoEnt);
        }
    };

    let entry = if (node.mode & crate::fs::vfs::MODE_FILE) != 0 {
        crate::handle::HandleEntry::file(drive_idx as u8, node.inode)
    } else if (node.mode & crate::fs::vfs::MODE_DIR) != 0 {
        crate::handle::HandleEntry::dir(drive_idx as u8, node.inode)
    } else {
        return err_to_u64(SyscallError::Inval);
    };
    let fd = crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(ep) = lock.current_eprocess_mut() {
            crate::handle::alloc_handle(&mut ep.handle_table, entry)
        } else {
            None
        }
    });

    match fd {
        Some(fd) => {
            serial_println!("[OPEN] fd={} for path={} inode={} object_id={}",
                fd, path, node.inode, entry.object_id);
            fd as u64
        }
        None => err_to_u64(SyscallError::NoMem),
    }
}

pub(super) fn handler_readfile(regs: super::Registers) -> u64 {
    let fd = regs.rbx as u8;
    let buf_ptr = regs.rcx as *mut u8;
    let count = regs.rdx as usize;

    if !is_user_ptr_valid(regs.rcx, count as u64) || count > 4096 {
        return err_to_u64(SyscallError::Fault);
    }

    let mut is_info_obj = false;
    let mut info_type = 0u32;
    let mut info_offset = 0u64;
    let (drive_idx, inode_num, offset) = crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(ep) = lock.current_eprocess_mut() {
            let entry = ep.handle_table[fd as usize];
            if !entry.has_ob_object() { return (usize::MAX, 0, 0); }
            if let Some(obj) = crate::object::ob_lookup(entry.object_id) {
                if obj.obj_type == crate::object::ObType::Key {
                    is_info_obj = true;
                    info_type = obj.native_id as u32;
                    info_offset = entry.offset;
                    return (usize::MAX, 0, entry.offset);
                }
                if obj.obj_type != crate::object::ObType::Filesystem {
                    return (usize::MAX, 0, 0);
                }
                (obj.flags as usize, obj.native_id as u32, entry.offset)
            } else {
                (usize::MAX, 0, 0)
            }
        } else {
            (usize::MAX, 0, 0)
        }
    });

    if is_info_obj {
        let content = generate_info_content(info_type);
        if content.is_none() { return err_to_u64(SyscallError::Inval); }
        let content = content.unwrap();
        let content_len = content.len();
        let copy_len = core::cmp::min(count, content_len.saturating_sub(info_offset as usize));
        if copy_len > 0 {
            unsafe {
                core::ptr::copy_nonoverlapping(
                    content.as_ptr().add(info_offset as usize),
                    buf_ptr, copy_len,
                );
            }
        }
        crate::hal::without_interrupts(|| {
            let s = scheduler::current_scheduler();
            let mut lock = s.lock();
            if let Some(ep) = lock.current_eprocess_mut() {
                ep.handle_table[fd as usize].offset += copy_len as u64;
            }
        });
        return copy_len as u64;
    }

    if drive_idx == usize::MAX {
        return err_to_u64(SyscallError::BadF);
    }

    let mut temp_buf = Vec::with_capacity(count);
    temp_buf.resize(count, 0u8);

    let result = crate::globals::with_vfs(|vfs| {
        vfs.read(drive_idx, inode_num, offset, &mut temp_buf)
    });

    match result {
        Ok(bytes_read) => {
            unsafe {
                core::ptr::copy_nonoverlapping(temp_buf.as_ptr(), buf_ptr, bytes_read);
            }
            crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    ep.handle_table[fd as usize].offset += bytes_read as u64;
                }
            });
            bytes_read as u64
        }
        Err(_) => err_to_u64(SyscallError::Io),
    }
}

pub(super) fn handler_writefile(regs: super::Registers) -> u64 {
    let fd = regs.rbx as u8;
    let buf_ptr = regs.rcx as *const u8;
    let count = regs.rdx as usize;

    if !is_user_ptr_valid(regs.rcx, count as u64) || count > 4096 {
        return err_to_u64(SyscallError::Fault);
    }

    let (drive_idx, inode_num, offset) = crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(ep) = lock.current_eprocess_mut() {
            let entry = ep.handle_table[fd as usize];
            if !entry.has_ob_object() { return (usize::MAX, 0, 0); }
            if let Some(obj) = crate::object::ob_lookup(entry.object_id) {
                if obj.obj_type != crate::object::ObType::Filesystem {
                    return (usize::MAX, 0, 0);
                }
                (obj.flags as usize, obj.native_id as u32, entry.offset)
            } else {
                (usize::MAX, 0, 0)
            }
        } else {
            (usize::MAX, 0, 0)
        }
    });

    if drive_idx == usize::MAX {
        return err_to_u64(SyscallError::BadF);
    }

    let mut temp_buf = Vec::with_capacity(count);
    temp_buf.resize(count, 0u8);
    unsafe {
        core::ptr::copy_nonoverlapping(buf_ptr, temp_buf.as_mut_ptr(), count);
    }

    let result = crate::globals::with_vfs(|vfs| {
        vfs.write(drive_idx, inode_num, offset, &temp_buf)
    });

    match result {
        Ok(bytes_written) => {
            crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    ep.handle_table[fd as usize].offset += bytes_written as u64;
                }
            });
            bytes_written as u64
        }
        Err(_) => err_to_u64(SyscallError::Io),
    }
}

pub(super) fn handler_close(regs: super::Registers) -> u64 {
    let fd = regs.rbx as u8;
    let entry = current_handle_entry(fd);
    if entry.object_id != 0 {
        let _ = crate::object::ob_close_object(entry.object_id);
    }
    set_current_handle(fd, crate::handle::HandleEntry::closed());
    0
}

pub(super) fn handler_readdir(regs: super::Registers) -> u64 {
    let fd = regs.rbx as u8;
    let buf_ptr = regs.rcx as *mut u8;

    if !is_user_ptr_valid(regs.rcx, core::mem::size_of::<DirEntryRaw>() as u64) {
        return err_to_u64(SyscallError::Fault);
    }

    let (drive_idx, dir_inode, current_index) = crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(ep) = lock.current_eprocess_mut() {
            let entry = ep.handle_table[fd as usize];
            if entry.obj_type() == Some(crate::object::ObType::Directory) {
                (entry.drive().unwrap_or(0) as usize, entry.native_id().unwrap_or(0) as u32, entry.offset)
            } else {
                return (usize::MAX, 0, 0);
            }
        } else {
            (usize::MAX, 0, 0)
        }
    });

    if drive_idx == usize::MAX {
        return err_to_u64(SyscallError::BadF);
    }

    let vfs_entry = crate::globals::with_vfs(|vfs| {
        vfs.readdir(drive_idx, dir_inode, current_index as usize)
    });

    match vfs_entry {
        Ok(Some(entry)) => {
            let raw = DirEntryRaw {
                inode: entry.node.inode,
                mode: entry.node.mode,
                size: entry.node.size,
                name: {
                    let mut n = [0u8; 260];
                    let name_bytes = entry.name.as_bytes();
                    let copy_len = name_bytes.len().min(259);
                    n[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
                    n
                },
            };
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &raw as *const DirEntryRaw as *const u8,
                    buf_ptr,
                    core::mem::size_of::<DirEntryRaw>(),
                );
            }
            crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    ep.handle_table[fd as usize].offset = current_index + 1;
                }
            });
            1
        }
        Ok(None) => 0,
        Err(_) => err_to_u64(SyscallError::Io),
    }
}

pub(super) fn handler_mkdir(regs: super::Registers) -> u64 {
    let path_str = match copy_user_string(regs.rbx) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };
    if path_str.is_empty() {
        return err_to_u64(SyscallError::NoEnt);
    }

    if let Err(e) = check_legacy_path_access(&path_str, crate::security::acl::ACCESS_WRITE) {
        return e;
    }

    match crate::globals::with_vfs(|vfs| vfs.mkdir(&path_str)) {
        Ok(_) => 0,
        Err(_) => err_to_u64(SyscallError::Io),
    }
}

pub(super) fn handler_unlink(regs: super::Registers) -> u64 {
    let path_str = match copy_user_string(regs.rbx) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };
    if path_str.is_empty() {
        return err_to_u64(SyscallError::NoEnt);
    }

    if let Err(e) = check_legacy_path_access(&path_str, crate::security::acl::ACCESS_DELETE) {
        return e;
    }

    match crate::globals::with_vfs(|vfs| vfs.remove_file(&path_str)) {
        Ok(_) => 0,
        Err(_) => err_to_u64(SyscallError::Io),
    }
}

pub(super) fn handler_rmdir(regs: super::Registers) -> u64 {
    let path_str = match copy_user_string(regs.rbx) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };
    if path_str.is_empty() {
        return err_to_u64(SyscallError::NoEnt);
    }

    if let Err(e) = check_legacy_path_access(&path_str, crate::security::acl::ACCESS_DELETE) {
        return e;
    }

    match crate::globals::with_vfs(|vfs| vfs.remove_dir(&path_str)) {
        Ok(_) => 0,
        Err(_) => err_to_u64(SyscallError::Io),
    }
}

pub(super) fn handler_rename(regs: super::Registers) -> u64 {
    let old_path = match copy_user_string(regs.rbx) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };
    let new_path = match copy_user_string(regs.rcx) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };
    if old_path.is_empty() || new_path.is_empty() {
        return err_to_u64(SyscallError::NoEnt);
    }

    if let Err(e) = check_legacy_path_access(&old_path,
        crate::security::acl::ACCESS_WRITE | crate::security::acl::ACCESS_DELETE) {
        return e;
    }

    let new_leaf = match new_path.rfind(|c| c == '\\' || c == '/') {
        Some(idx) => &new_path[idx + 1..],
        None => &new_path,
    };
    if new_leaf.is_empty() {
        return err_to_u64(SyscallError::Inval);
    }

    match crate::globals::with_vfs(|vfs| vfs.rename(&old_path, new_leaf)) {
        Ok(_) => 0,
        Err(_) => err_to_u64(SyscallError::Io),
    }
}

pub(super) fn handler_chdir(regs: super::Registers) -> u64 {
    let path_str = match copy_user_string(regs.rbx) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };
    match resolve_chdir_target(path_str) {
        Ok((new_drive, new_cwd_path)) => {
            crate::scheduler::set_current_cwd(new_drive, &new_cwd_path);
            0
        }
        Err(err) => err_to_u64(err),
    }
}

pub(super) fn handler_chdir_parent(regs: super::Registers) -> u64 {
    let path_str = match copy_user_string(regs.rbx) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    let (parent_pid, target_drive, target_path) = match resolve_chdir_target(path_str) {
        Ok((drive, path)) => {
            let parent_pid = crate::hal::without_interrupts(|| {
                let scheduler = crate::scheduler::current_scheduler().lock();
                let current_pid = scheduler.current_pid();
                scheduler.find_eprocess(current_pid).map(|ep| ep.parent_pid).unwrap_or(0)
            });
            if parent_pid == 0 {
                return err_to_u64(SyscallError::NoEnt);
            }
            (parent_pid, drive, path)
        }
        Err(err) => return err_to_u64(err),
    };

    if crate::scheduler::set_cwd_for_pid(parent_pid, target_drive, &target_path) {
        0
    } else {
        err_to_u64(SyscallError::NoEnt)
    }
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
            serial_println!("[SYS] sys_loadlib '{}' => 0x{:x}", path_str, base);
            base
        }
        None => {
            serial_println!("[SYS] sys_loadlib FAILED '{}'", path_str);
            err_to_u64(SyscallError::NoEnt)
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Thread lifecycle handlers
// ═══════════════════════════════════════════════════════════════════════

pub(super) fn handler_thread_create(regs: super::Registers) -> u64 {
    let entry = regs.rbx;
    let user_stack = regs.rcx;

    if entry == 0 || entry >= crate::arch::x64::paging::USER_LIMIT {
        return err_to_u64(SyscallError::Inval);
    }

    let result = crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        let pid = lock.current_pid();
        if pid == 0 {
            return Err(SyscallError::Inval);
        }

        let stack = if user_stack != 0 {
            user_stack
        } else {
            if let Some(ep) = lock.find_eprocess(pid) {
                if let Some(slot_idx) = ep.user_slot {
                    let slot_size = 0x20000u64;
                    let max_bin = 0x10000u64;
                    let user_stack_size = 0x10000u64;
                    let stack_top = crate::arch::x64::paging::USER_BASE
                        + slot_idx as u64 * slot_size
                        + max_bin + user_stack_size;
                    stack_top - 0x1000
                } else {
                    0
                }
            } else {
                0
            }
        };

        if stack == 0 {
            return Err(SyscallError::NoMem);
        }

        match lock.add_thread_to_process(pid, entry, stack) {
            Some(tid) => {
                serial_println!("[SYS] thread_create: PID {} TID {} entry=0x{:x} stack=0x{:x}",
                    pid, tid, entry, stack);
                Ok(tid)
            }
            None => Err(SyscallError::NoMem),
        }
    });

    match result {
        Ok(tid) => tid as u64,
        Err(e) => err_to_u64(e),
    }
}

pub(super) fn handler_thread_join(regs: super::Registers) -> u64 {
    let target_tid = regs.rbx as u32;

    let is_done = crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let lock = s.lock();
        if let Some(k) = lock.find_kthread(target_tid) {
            k.state == ThreadState::Terminated
        } else {
            true
        }
    });

    if !is_done {
        crate::kwait::kwait_block(crate::kwait::WaitReason::ThreadJoin { tid: target_tid });
        return err_to_u64(SyscallError::Again);
    }

    crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        lock.recycle_thread(target_tid);
    });

    0
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

// ═══════════════════════════════════════════════════════════════════════
// Volume / Fsck handlers
// ═══════════════════════════════════════════════════════════════════════

pub(super) fn handler_get_volume_label(regs: super::Registers) -> u64 {
    let drive_char = (regs.rbx & 0xFF) as u8 as char;
    let buf_ptr = regs.rcx as *mut u8;
    let buf_size = regs.rdx as usize;

    if buf_ptr.is_null() || buf_size == 0 {
        return err_to_u64(SyscallError::Inval);
    }
    if !is_user_ptr_valid(regs.rcx, buf_size as u64) {
        return err_to_u64(SyscallError::Fault);
    }

    let result = crate::globals::with_vfs(|vfs| {
        vfs.volume_label(drive_char.to_ascii_uppercase())
    });

    match result {
        Ok(label) => {
            let bytes = label.as_bytes();
            let copy_len = core::cmp::min(bytes.len(), buf_size.saturating_sub(1));
            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr, copy_len);
                buf_ptr.add(copy_len).write(0);
            }
            copy_len as u64
        }
        Err(_) => {
            unsafe { buf_ptr.write(0); }
            0
        }
    }
}

pub(super) fn handler_set_volume_label(regs: super::Registers) -> u64 {
    let drive_char = (regs.rbx & 0xFF) as u8 as char;
    let label_ptr = regs.rcx;

    if label_ptr == 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let label = match copy_user_string(label_ptr) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    if label.len() > 11 {
        return err_to_u64(SyscallError::Inval);
    }

    match crate::globals::with_vfs(|vfs| {
        vfs.set_volume_label(drive_char.to_ascii_uppercase(), &label)
    }) {
        Ok(()) => {
            crate::globals::NEED_CACHE_FLUSH.store(true, core::sync::atomic::Ordering::Relaxed);
            0
        }
        Err(_) => err_to_u64(SyscallError::Io),
    }
}

pub(super) fn handler_fsck(regs: super::Registers) -> u64 {
    let buf_ptr = regs.rbx;
    let _drive_char = (regs.rcx & 0xFF) as u8 as char;
    let repair_flag = regs.rdx != 0;

    if buf_ptr == 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let stats_size = core::mem::size_of::<FsckStatsRaw>() as u64;
    if !is_user_ptr_valid(buf_ptr, stats_size) {
        return err_to_u64(SyscallError::Fault);
    }

    let mode = if repair_flag {
        crate::fs::fsck::FsckMode::Repair
    } else {
        crate::fs::fsck::FsckMode::CheckOnly
    };

    let result = core::cell::UnsafeCell::new(FsckStatsRaw {
        total_inodes: 0, used_inodes: 0, valid_inodes: 0,
        corrupted_inodes: 0, cross_linked_blocks: 0,
        orphan_inodes: 0, dangling_entries: 0, dir_errors: 0,
        superblock_errors: 0, repairs_applied: 0,
    });

    let res = crate::hal::without_interrupts(|| {
        let mut cache_lock = crate::globals::BLOCK_CACHE.lock();
        let cache = match cache_lock.as_mut() {
            Some(c) => c,
            None => return err_to_u64(SyscallError::Io),
        };
        let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
        let dev = match bdevs_lock.get(0) {
            Some(d) => d,
            None => return err_to_u64(SyscallError::NoDev),
        };
        let partition_base = crate::globals::PRIMARY_PARTITION_BASE.load(Ordering::Relaxed) as u32;

        let stats = crate::fs::fsck::run(cache, dev, mode, partition_base);

        let raw = unsafe { &mut *result.get() };
        raw.total_inodes = stats.total_inodes;
        raw.used_inodes = stats.used_inodes;
        raw.valid_inodes = stats.valid_inodes;
        raw.corrupted_inodes = stats.corrupted_inodes;
        raw.cross_linked_blocks = stats.cross_linked_blocks;
        raw.orphan_inodes = stats.orphan_inodes;
        raw.dangling_entries = stats.dangling_entries;
        raw.dir_errors = stats.dir_errors;
        raw.superblock_errors = stats.superblock_errors;
        raw.repairs_applied = stats.repairs_applied;

        if stats.repairs_applied > 0 {
            crate::globals::NEED_CACHE_FLUSH.store(true, Ordering::Relaxed);
        }

        0u64
    });

    if res != 0 {
        return res;
    }

    let raw = unsafe { &*result.get() };
    unsafe {
        core::ptr::copy_nonoverlapping(
            raw as *const FsckStatsRaw as *const u8,
            buf_ptr as *mut u8,
            core::mem::size_of::<FsckStatsRaw>(),
        );
    }

    0
}

// ═══════════════════════════════════════════════════════════════════════
// Driver lifecycle handlers
// ═══════════════════════════════════════════════════════════════════════

pub(super) fn handler_driver_load(regs: super::Registers) -> u64 {
    let path_ptr = regs.rbx;

    if path_ptr == 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let path = match copy_user_string(path_ptr) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    match crate::drivers::nem::load_nem_driver(&path) {
        Ok(id) => id as u64,
        Err(_) => err_to_u64(SyscallError::Io),
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
    for i in 0..nfds {
        unsafe {
            let src = fds_ptr.add(i);
            fds[i].fd = core::ptr::read_volatile(&(*src).fd);
            fds[i].events = core::ptr::read_volatile(&(*src).events);
        }
    }

    let mut ready_count: u64 = 0;
    for i in 0..nfds {
        let fd = fds[i].fd;
        if fd < 0 {
            fds[i].revents = 0;
            continue;
        }
        let entry = current_handle_entry(fd as u8);
        if !entry.is_open() {
            fds[i].revents = POLLERR;
            ready_count += 1;
            continue;
        }

        let mut rev: i16 = 0;
        if entry.is_stdin() {
            if fds[i].events & POLLIN != 0 {
                rev |= POLLIN;
            }
        } else if entry.is_stdout() || entry.is_stderr() {
            if fds[i].events & POLLOUT != 0 {
                rev |= POLLOUT;
            }
        } else if entry.is_pipe_read() {
            let pipe_id = entry.native_id().unwrap_or(0) as u8;
            let ready = crate::object::pipe::pipe_peek_read_ready(pipe_id).unwrap_or(false);
            if ready && fds[i].events & POLLIN != 0 {
                rev |= POLLIN;
            }
            if crate::object::pipe::pipe_peek_write_closed(pipe_id).unwrap_or(false) {
                rev |= POLLHUP;
            }
        } else if entry.is_pipe_write() {
            if fds[i].events & POLLOUT != 0 {
                rev |= POLLOUT;
            }
        } else if entry.obj_type() == Some(crate::object::ObType::Filesystem)
            || entry.obj_type() == Some(crate::object::ObType::Directory) {
            if fds[i].events & POLLIN != 0 { rev |= POLLIN; }
            if fds[i].events & POLLOUT != 0 { rev |= POLLOUT; }
        } else {
            rev |= POLLERR;
        }
        fds[i].revents = rev;
        if rev != 0 {
            ready_count += 1;
        }
    }

    for i in 0..nfds {
        unsafe {
            core::ptr::write_volatile(&mut (*fds_ptr.add(i)).revents, fds[i].revents);
        }
    }

    ready_count
}
