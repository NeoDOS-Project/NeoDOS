//! Ob (Object Manager) syscall handlers — ob_open, ob_create, ob_query_info,
//! ob_set_info, ob_enum, ob_wait, ob_destroy.
//! All functions are `pub(super)` for SSDT registration in `mod.rs`.

use alloc::string::ToString;
use alloc::vec::Vec;
use crate::scheduler;
use crate::object::types::{ObInfoClass, ObSetInfoClass};
use super::{err_to_u64, ob_err_to_syscall, SyscallError, is_user_ptr_valid, copy_user_string,
           current_handle_entry,
           copy_handle_entry_for_child, resolve_chdir_target, KEYBOARD_LAYOUT};

// ── Ob-specific ABI structs ──

#[repr(C)]
struct ObPipeFds {
    reader_fd: u64,
    writer_fd: u64,
}

#[repr(C)]
struct ObBasicInfo {
    obj_type: u32,
    refcount: u32,
    name: [u8; 32],
}

#[repr(C)]
struct ObFileInfo {
    size: u64,
    drive: u8,
    inode: u32,
    padding: [u8; 3],
}

#[repr(C)]
struct ObProcessInfo {
    pid: u32,
    parent_pid: u32,
    priority: u8,
    thread_count: u32,
    state: u8,
    padding: [u8; 2],
}

#[repr(C)]
struct ObPipeInfo {
    capacity: u32,
    read_refs: u32,
    write_refs: u32,
}

#[repr(C)]
struct ObThreadInfo {
    tid: u32,
    pid: u32,
    state: u8,
    priority: u8,
    padding: [u8; 2],
}

#[repr(C)]
struct ObDeviceInfo {
    device_id: u32,
    reserved: u32,
}

#[repr(C)]
struct SysDateTime {
    second: u8,
    minute: u8,
    hour: u8,
    day: u8,
    month: u8,
    year: u8,
    valid: u8,
}

#[repr(C)]
struct DriveInfoRaw {
    letter: u8,
    present: u8,
    fs_type: [u8; 16],
    label: [u8; 32],
    total_sectors: u64,
}

#[repr(C)]
struct DriverInfoRaw {
    id: u32,
    state: u8,
    category: u8,
    driver_type: u8,
    api_version: u16,
    abi_min: u16,
    abi_target: u16,
    abi_max: u16,
    last_error: u32,
    caps: u64,
    isolation_mode: u8,
    events_received: u64,
    tick_count: u64,
    registered_at_tick: u64,
    name: [u8; 8],
}

// ═══════════════════════════════════════════════════════════════════════
// OB-010: ObOpen — RAX=60
// ═══════════════════════════════════════════════════════════════════════

pub(super) fn handler_ob_open(regs: super::Registers) -> u64 {
    let path_ptr = regs.rbx;
    let desired_access = regs.rcx as u32;

    if path_ptr == 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let path_str = match copy_user_string(path_ptr) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    if path_str.is_empty() {
        return err_to_u64(SyscallError::NoEnt);
    }

    let path = path_str;

    let token = crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let lock = s.lock();
        lock.current_eprocess()
            .map(|ep| ep.token)
            .unwrap_or(*crate::security::DEFAULT_ADMIN_TOKEN)
    });

    let ob_id = match crate::object::ob_open_path(&path, &token, desired_access) {
        Ok(id) => id,
        Err(e) => return err_to_u64(ob_err_to_syscall(e)),
    };

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
            fd as u64
        }
        None => {
            let _ = crate::object::ob_close_object(ob_id);
            err_to_u64(SyscallError::NoMem)
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// OB-011: ObCreate — RAX=61
// ═══════════════════════════════════════════════════════════════════════

pub(super) fn handler_ob_create(regs: super::Registers) -> u64 {
    // Serialize: ensure all previous memory ops are visible (prevents QEMU TCG TB coalescing)
    // On real hw, lfence+rdtsc serves as a serialization point.
    // On QEMU TCG, rdtsc forces a translation-block exit that allows pending timer
    // interrupts to be delivered BEFORE we enter the long-running Process creation path.
    let _tsc = unsafe { crate::hal::raw::raw_read_tsc() };
    let path_ptr = regs.rbx;
    let obj_type_val = regs.rcx as u32;
    let fds_out = regs.rdx;
    let attrs = regs.r8;

    if path_ptr == 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let path_str = match copy_user_string(path_ptr) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    if path_str.is_empty() || !path_str.starts_with('\\') {
        return err_to_u64(SyscallError::Inval);
    }

    let obj_type = match obj_type_val {
        1 => crate::object::ObType::Process,
        2 => crate::object::ObType::Driver,
        4 => crate::object::ObType::Pipe,
        11 => crate::object::ObType::Directory,
        13 => crate::object::ObType::Event,
        14 => crate::object::ObType::Semaphore,
        15 => crate::object::ObType::Timer,
        16 => crate::object::ObType::Thread,
        17 => crate::object::ObType::Section,
        _ => return err_to_u64(SyscallError::Inval),
    };

    match obj_type {
        crate::object::ObType::Pipe => {
            if fds_out == 0 || !is_user_ptr_valid(fds_out, 16) {
                return err_to_u64(SyscallError::Fault);
            }
            let ob_id = match crate::object::ob_create_object_path(
                &path_str, obj_type, 0, None,
            ) {
                Ok(id) => id,
                Err(e) => return err_to_u64(ob_err_to_syscall(e)),
            };
            let obj = crate::object::ob_lookup(ob_id).unwrap();
            let pipe_id = obj.native_id as u8;
            let read_entry = crate::handle::HandleEntry::pipe_read(pipe_id);
            let write_entry = crate::handle::HandleEntry::pipe_write(pipe_id);
            let (rfd, wfd) = crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    match crate::handle::alloc_two_handles(&mut ep.handle_table, read_entry, write_entry) {
                        Some((r, w)) => {
                            crate::object::pipe::PIPE_MANAGER.inc_read_ref(pipe_id);
                            crate::object::pipe::PIPE_MANAGER.inc_write_ref(pipe_id);
                            (r as u64, w as u64)
                        }
                        None => (0u64, 0u64)
                    }
                } else {
                    (0u64, 0u64)
                }
            });
            if rfd == 0 {
                let _ = crate::object::ob_close_object(ob_id);
                return err_to_u64(SyscallError::NoMem);
            }
            unsafe {
                (fds_out as *mut u64).write(rfd);
                (fds_out as *mut u64).add(1).write(wfd);
            }
            rfd
        }
        crate::object::ObType::Directory => {
            if path_str.starts_with("\\Global\\FileSystem\\") {
                let vfs_path = &path_str["\\Global\\FileSystem\\".len()..];
                if !vfs_path.is_empty() {
                    match crate::globals::with_vfs(|vfs| vfs.mkdir(vfs_path)) {
                        Ok(_) => {},
                        Err(_) => return err_to_u64(SyscallError::Io),
                    }
                }
            }
            let ob_id = match crate::object::ob_create_object(
                obj_type, &path_str, 0, 0, None,
            ) {
                Ok(id) => id,
                Err(_) => return err_to_u64(SyscallError::NoMem),
            };
            {
                let _ = crate::object::namespace::ob_create_directory_tree(&path_str);
            }
            let _ = crate::object::namespace::ob_insert_object(&path_str, ob_id);
            let entry = crate::handle::HandleEntry::ob_object(ob_id, 0);
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
                    fd as u64
                }
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    err_to_u64(SyscallError::NoMem)
                }
            }
        }
        crate::object::ObType::Process => {
            let stdin_fd = (attrs & 0xFF) as u8;
            let stdout_fd = ((attrs >> 8) & 0xFF) as u8;
            let stderr_fd = ((attrs >> 16) & 0xFF) as u8;

            const MAX_BIN: usize = 65536;
            let bin_data = {
                let mut buf = alloc::vec::Vec::with_capacity(MAX_BIN);
                buf.resize(MAX_BIN, 0u8);
                let vfs_path = if path_str.starts_with("\\Global\\FileSystem\\") {
                    &path_str["\\Global\\FileSystem\\".len()..]
                } else {
                    &path_str
                };
                let bin_size = crate::globals::with_vfs(|vfs| {
                    match vfs.resolve_path(vfs_path) {
                        Ok((drive_idx, node)) => {
                            if (node.mode & crate::fs::vfs::MODE_FILE) == 0 { return 0; }
                            match vfs.read(drive_idx, node.inode, 0, &mut buf) {
                                Ok(n) => { if n > MAX_BIN { 0 } else { n } }
                                Err(_) => 0,
                            }
                        }
                        Err(_) => 0,
                    }
                });
                if bin_size < 4 {
                    return err_to_u64(SyscallError::NoEnt);
                }
                buf.truncate(bin_size);
                buf
            };

            let slot = match crate::arch::x64::paging::alloc_user_slot() {
                Some(s) => s,
                None => return err_to_u64(SyscallError::NoMem),
            };

            let result = match crate::elf::load_elf(&bin_data, None, slot.code_base) {
                Ok(r) => r,
                Err(_) => {
                    crate::arch::x64::paging::free_user_slot(slot.slot_idx);
                    return err_to_u64(SyscallError::Inval);
                }
            };

            let (cwd_drive, cwd_path, parent_pid) = crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler().lock();
                let pid = s.current_pid();
                let cwd = if let Some(ep) = s.find_eprocess(pid) {
                    (ep.cwd_drive, ep.cwd_path.clone())
                } else {
                    (2u8, alloc::string::String::from("\\"))
                };
                (cwd.0, cwd.1, pid)
            });

            let child_pid = crate::usermode::spawn_usermode(
                result.entry, slot.stack_top, slot.slot_idx,
                cwd_drive, &cwd_path, parent_pid,
            );

            if stdin_fd != 0xFF || stdout_fd != 0xFF || stderr_fd != 0xFF {
                let (parent_stdin_entry, parent_stdout_entry, parent_stderr_entry) = crate::hal::without_interrupts(|| {
                    let s = scheduler::current_scheduler();
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
                crate::hal::without_interrupts(|| {
                    let s = scheduler::current_scheduler();
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

            let ob_id = crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler().lock();
                if let Some(ep) = s.find_eprocess(child_pid) {
                    ep.ob_id
                } else {
                    None
                }
            });
            let actual_ob_id = match ob_id {
                Some(id) => id,
                None => return err_to_u64(SyscallError::Io),
            };

            if let Err(_) = crate::object::ob_open_object(actual_ob_id, 0) {
                return err_to_u64(SyscallError::Io);
            }

            let entry = crate::handle::HandleEntry::ob_object(actual_ob_id, 0);
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
                Some(fd) => fd as u64,
                None => {
                    let _ = crate::object::ob_close_object(actual_ob_id);
                    err_to_u64(SyscallError::NoMem)
                }
            }
        }
        crate::object::ObType::Driver => {
            let driver_path = if path_str.starts_with("\\Global\\FileSystem\\") {
                &path_str["\\Global\\FileSystem\\".len()..]
            } else {
                &path_str
            };
            match crate::drivers::nem::load_nem_driver(driver_path) {
                Ok(driver_id) => {
                    let driver_name = alloc::format!("driver/{}", driver_id);
                    let ob_id = match crate::object::ob_create_object(
                        crate::object::ObType::Driver, &driver_name,
                        driver_id as u64, 0, None,
                    ) {
                        Ok(id) => id,
                        Err(_) => return err_to_u64(SyscallError::Io),
                    };
                    let ns_path = alloc::format!("\\Driver\\{}", driver_id);
                    let _ = crate::object::namespace::ob_insert_object(&ns_path, ob_id);

                    let entry = crate::handle::HandleEntry::ob_object(ob_id, 0);
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
                        Some(fd) => fd as u64,
                        None => {
                            let _ = crate::object::ob_close_object(ob_id);
                            err_to_u64(SyscallError::NoMem)
                        }
                    }
                }
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        crate::object::ObType::Event => {
            let ob_id = match crate::object::ob_create_object_path(
                &path_str, obj_type, 0, None,
            ) {
                Ok(id) => id,
                Err(e) => return err_to_u64(ob_err_to_syscall(e)),
            };
            let entry = crate::handle::HandleEntry::ob_object(ob_id, 0);
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
                    fd as u64
                }
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    err_to_u64(SyscallError::NoMem)
                }
            }
        }
        crate::object::ObType::Thread => {
            let entry = attrs;
            let tid = crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                let pid = lock.current_pid();
                if pid == 0 {
                    return None;
                }
                let stack = if let Some(ep) = lock.find_eprocess(pid) {
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
                };
                if stack == 0 {
                    return None;
                }
                lock.add_thread_to_process(pid, entry, stack)
            });
            let tid = match tid {
                Some(id) => id,
                None => return err_to_u64(SyscallError::NoMem),
            };
            let ns_path = alloc::format!("\\Ob\\Thread\\{}", tid);
            let ob_id = match crate::object::ob_create_object(
                crate::object::ObType::Thread, &ns_path,
                tid as u64, 0, None,
            ) {
                Ok(id) => id,
                Err(_) => return err_to_u64(SyscallError::NoMem),
            };
            let _ = crate::object::namespace::ob_insert_object(&ns_path, ob_id);
            let entry = crate::handle::HandleEntry::ob_object(ob_id, 0);
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
                Some(fd) => fd as u64,
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    err_to_u64(SyscallError::NoMem)
                }
            }
        }
        crate::object::ObType::Semaphore => {
            let initial_count = (attrs & 0xFFFF) as i32;
            let max_count = ((attrs >> 16) & 0xFFFF) as i32;
            let ob_id = match crate::object::ob_create_object_path(
                &path_str, obj_type, attrs as u32,
                Some(&crate::object::semaphore::SEMAPHORE_OPS),
            ) {
                Ok(id) => id,
                Err(e) => return err_to_u64(ob_err_to_syscall(e)),
            };
            let sem_id = match crate::object::semaphore::alloc_semaphore(ob_id, initial_count, max_count) {
                Some(id) => id,
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    return err_to_u64(SyscallError::Inval);
                }
            };
            {
                let mut table = crate::object::OB_TABLE.lock();
                if let Some(obj) = table.lookup_mut(ob_id) {
                    obj.native_id = sem_id as u64;
                }
            }
            let entry = crate::handle::HandleEntry::ob_object(ob_id, 0);
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
                Some(fd) => fd as u64,
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    err_to_u64(SyscallError::NoMem)
                }
            }
        }
        crate::object::ObType::Timer => {
            let period_ms = (attrs & 0x7FFFFFFF) as u64;
            let periodic = (attrs >> 31) & 1 != 0;
            if period_ms == 0 || period_ms > 3600000 {
                return err_to_u64(SyscallError::Inval);
            }
            let ob_id = match crate::object::ob_create_object_path(
                &path_str, obj_type, 0,
                Some(&crate::object::timer::TIMER_OPS),
            ) {
                Ok(id) => id,
                Err(e) => return err_to_u64(ob_err_to_syscall(e)),
            };
            let timer_id = match crate::object::timer::alloc_timer(ob_id, period_ms, periodic) {
                Some(id) => id,
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    return err_to_u64(SyscallError::NoMem);
                }
            };
            {
                let mut table = crate::object::OB_TABLE.lock();
                if let Some(obj) = table.lookup_mut(ob_id) {
                    obj.native_id = timer_id as u64;
                }
            }
            let entry = crate::handle::HandleEntry::ob_object(ob_id, 0);
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
                Some(fd) => fd as u64,
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    err_to_u64(SyscallError::NoMem)
                }
            }
        }
        crate::object::ObType::Section => {
            let size = (attrs & 0xFFFF_FFFF) as u64;
            let prot = ((attrs >> 32) & 0xFF) as u32;
            if size == 0 || size > 0x100000 || prot == 0 || prot > 3 {
                return err_to_u64(SyscallError::Inval);
            }
            let ob_id = match crate::object::ob_create_object_path(
                &path_str, obj_type, 0,
                Some(&crate::object::section::SECTION_OPS),
            ) {
                Ok(id) => id,
                Err(e) => return err_to_u64(ob_err_to_syscall(e)),
            };
            let section_id = match crate::object::section::alloc_section(ob_id, size, prot) {
                Some(id) => id,
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    return err_to_u64(SyscallError::NoMem);
                }
            };
            {
                let mut table = crate::object::OB_TABLE.lock();
                if let Some(obj) = table.lookup_mut(ob_id) {
                    obj.native_id = section_id as u64;
                }
            }
            let entry = crate::handle::HandleEntry::ob_object(ob_id, 0);
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
                Some(fd) => fd as u64,
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    err_to_u64(SyscallError::NoMem)
                }
            }
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// OB-012: ObQueryInfo — RAX=62
// ═══════════════════════════════════════════════════════════════════════

pub(super) fn handler_ob_query_info(regs: super::Registers) -> u64 {
    let fd = regs.rbx as u8;
    let info_class = regs.rcx as u32;
    let buf_ptr = regs.rdx;
    let buf_size = regs.r8 as usize;

    if buf_ptr == 0 || buf_size == 0 {
        return err_to_u64(SyscallError::Inval);
    }
    if !is_user_ptr_valid(buf_ptr, buf_size as u64) {
        return err_to_u64(SyscallError::Fault);
    }

    let entry = current_handle_entry(fd);
    if !entry.is_open() {
        return err_to_u64(SyscallError::BadF);
    }

    match info_class {
        _ if info_class == ObInfoClass::Basic as u32 => {
            if entry.object_id == 0 {
                let basic = ObBasicInfo {
                    obj_type: entry.obj_type().map(|t| t as u32).unwrap_or(0),
                    refcount: 1,
                    name: {
                        let mut n = [0u8; 32];
                        let src: &[u8] = if entry.is_stdin() {
                            b"STDIN"
                        } else if entry.is_stdout() {
                            b"STDOUT"
                        } else if entry.is_stderr() {
                            b"STDERR"
                        } else {
                            b"HANDLE"
                        };
                        let len = src.len().min(31);
                        n[..len].copy_from_slice(&src[..len]);
                        n
                    },
                };
                let sz = core::mem::size_of::<ObBasicInfo>();
                if buf_size < sz { return err_to_u64(SyscallError::Inval); }
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &basic as *const ObBasicInfo as *const u8,
                        buf_ptr as *mut u8, sz,
                    );
                }
                return sz as u64;
            }
            if let Some(obj) = crate::object::ob_lookup(entry.object_id) {
                let mut name = [0u8; 32];
                let src = obj.name;
                let len = src.iter().position(|&b| b == 0).unwrap_or(32).min(31);
                name[..len].copy_from_slice(&src[..len]);
                let basic = ObBasicInfo {
                    obj_type: obj.obj_type as u32,
                    refcount: obj.refcount,
                    name,
                };
                let sz = core::mem::size_of::<ObBasicInfo>();
                if buf_size < sz { return err_to_u64(SyscallError::Inval); }
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &basic as *const ObBasicInfo as *const u8,
                        buf_ptr as *mut u8, sz,
                    );
                }
                sz as u64
            } else {
                err_to_u64(SyscallError::BadF)
            }
        }
        _ if info_class == ObInfoClass::Name as u32 => {
            if entry.object_id == 0 {
                return 0u64;
            }
            if let Some(obj) = crate::object::ob_lookup(entry.object_id) {
                let name_str = obj.name_str();
                let bytes = name_str.as_bytes();
                let copy_len = bytes.len().min(buf_size - 1).min(255);
                unsafe {
                    core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr as *mut u8, copy_len);
                    (buf_ptr as *mut u8).add(copy_len).write(0u8);
                }
                copy_len as u64
            } else {
                err_to_u64(SyscallError::BadF)
            }
        }
        _ if info_class == ObInfoClass::File as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Filesystem) {
                return err_to_u64(SyscallError::Inval);
            }
            let drive = entry.drive().unwrap_or(0);
            let inode = entry.native_id().unwrap_or(0) as u32;
            let size = crate::globals::with_vfs(|vfs| {
                vfs.stat(drive as usize, inode).map(|n| n.size).unwrap_or(0)
            });
            let fi = ObFileInfo {
                size: size as u64,
                drive,
                inode,
                padding: [0u8; 3],
            };
            let sz = core::mem::size_of::<ObFileInfo>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &fi as *const ObFileInfo as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::Process as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Process {
                return err_to_u64(SyscallError::Inval);
            }
            let pid = obj.native_id as u32;
            let pi = crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler();
                let lock = s.lock();
                if let Some(ep) = lock.find_eprocess(pid) {
                    ObProcessInfo {
                        pid,
                        parent_pid: ep.parent_pid,
                        priority: {
                            let mut prio = 2u8;
                            for th in lock.kthreads.iter() {
                                if let Some(k) = th {
                                    if k.pid == pid {
                                        prio = k.priority as u8;
                                        break;
                                    }
                                }
                            }
                            prio
                        },
                        thread_count: ep.thread_count as u32,
                        state: if ep.thread_count == 0 { 1u8 } else { 0u8 },
                        padding: [0u8; 2],
                    }
                } else {
                    ObProcessInfo {
                        pid, parent_pid: 0, priority: 0,
                        thread_count: 0, state: 0, padding: [0u8; 2],
                    }
                }
            });
            let sz = core::mem::size_of::<ObProcessInfo>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &pi as *const ObProcessInfo as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::Thread as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Process {
                return err_to_u64(SyscallError::Inval);
            }
            let pid = obj.native_id as u32;
            let ti = crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler();
                let lock = s.lock();
                let mut found = ObThreadInfo {
                    tid: 0, pid, state: 0, priority: 0, padding: [0u8; 2],
                };
                for kt in lock.kthreads.iter() {
                    if let Some(k) = kt {
                        if k.pid == pid {
                            found.tid = k.tid;
                            found.state = k.state.to_u8();
                            found.priority = k.priority;
                            break;
                        }
                    }
                }
                found
            });
            let sz = core::mem::size_of::<ObThreadInfo>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &ti as *const ObThreadInfo as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::Pipe as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Pipe) {
                return err_to_u64(SyscallError::Inval);
            }
            let pipe_id = entry.native_id().unwrap_or(0) as u8;
            let capacity = crate::object::pipe::PIPE_BUF_SIZE;
            let read_refs = crate::object::pipe::pipe_peek_read_ready(pipe_id)
                .map(|_| 1u32).unwrap_or(0);
            let info = ObPipeInfo {
                capacity: capacity as u32,
                read_refs: read_refs,
                write_refs: 0,
            };
            let sz = core::mem::size_of::<ObPipeInfo>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &info as *const ObPipeInfo as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::Device as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            let di = ObDeviceInfo {
                device_id: obj.native_id as u32,
                reserved: 0,
            };
            let sz = core::mem::size_of::<ObDeviceInfo>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &di as *const ObDeviceInfo as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::CpuInfo as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 3 {
                return err_to_u64(SyscallError::Inval);
            }
            let sz = core::mem::size_of::<crate::cpu::CpuInfoFull>();
            if buf_size < (sz as usize) { return err_to_u64(SyscallError::Inval); }
            let info = crate::cpu::get_cpu_info_full();
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &info as *const crate::cpu::CpuInfoFull as *const u8,
                    buf_ptr as *mut u8, sz as usize,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::Version as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 4 {
                return err_to_u64(SyscallError::Inval);
            }
            let ver = crate::KERNEL_VERSION.as_bytes();
            let copy_len = ver.len().min(buf_size as usize);
            unsafe {
                core::ptr::copy_nonoverlapping(ver.as_ptr(), buf_ptr as *mut u8, copy_len);
            }
            ver.len() as u64
        }
        _ if info_class == ObInfoClass::DateTime as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 5 {
                return err_to_u64(SyscallError::Inval);
            }
            let sz = core::mem::size_of::<SysDateTime>() as usize;
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            let dt = crate::drivers::rtc_bridge::request_datetime();
            let sysdt = match dt {
                Some(d) => SysDateTime {
                    second: d.second,
                    minute: d.minute,
                    hour: d.hour,
                    day: d.day,
                    month: d.month,
                    year: d.year,
                    valid: 1,
                },
                None => SysDateTime {
                    second: 0, minute: 0, hour: 0,
                    day: 0, month: 0, year: 0, valid: 0,
                },
            };
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &sysdt as *const SysDateTime as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::Memory as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 1 {
                return err_to_u64(SyscallError::Inval);
            }
            const OLD_SZ: usize = 48;
            let full_sz = core::mem::size_of::<crate::memory::MemoryStats>();
            if buf_size < OLD_SZ { return err_to_u64(SyscallError::Inval); }
            let copy_sz = core::cmp::min(buf_size, full_sz);
            let stats = crate::memory::stats();
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &stats as *const crate::memory::MemoryStats as *const u8,
                    buf_ptr as *mut u8, copy_sz,
                );
            }
            copy_sz as u64
        }
        _ if info_class == ObInfoClass::Drives as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 6 {
                return err_to_u64(SyscallError::Inval);
            }
            let entry_size = core::mem::size_of::<DriveInfoRaw>();
            let max_entries = buf_size / entry_size;
            if max_entries == 0 { return 0u64; }
            let written = crate::globals::with_vfs(|vfs| {
                let mut count = 0usize;
                for i in 0..26 {
                    if count >= max_entries { break; }
                    if vfs.drives[i].is_some() {
                        let letter = (b'A' + i as u8) as char;
                        let label = vfs.volume_label(letter).unwrap_or_default();
                        let (fs_type_str, total_sectors) = {
                            let fs = vfs.drives[i].as_ref().unwrap();
                            (fs.fs_type(), fs.total_sectors())
                        };
                        let mut fs_type_bytes = [0u8; 16];
                        let fst = fs_type_str.as_bytes();
                        let copy_len = fst.len().min(15);
                        fs_type_bytes[..copy_len].copy_from_slice(&fst[..copy_len]);
                        let mut label_bytes = [0u8; 32];
                        let lbl = label.as_bytes();
                        let lbl_len = lbl.len().min(31);
                        label_bytes[..lbl_len].copy_from_slice(&lbl[..lbl_len]);
                        let raw = DriveInfoRaw {
                            letter: i as u8 + b'A',
                            present: 1,
                            fs_type: fs_type_bytes,
                            label: label_bytes,
                            total_sectors,
                        };
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                &raw as *const DriveInfoRaw as *const u8,
                                (buf_ptr as *mut u8).add(count * entry_size),
                                entry_size,
                            );
                        }
                        count += 1;
                    }
                }
                (count * entry_size) as u64
            });
            written
        }
        _ if info_class == ObInfoClass::Drivers as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };

            if obj.obj_type == crate::object::ObType::Driver {
                let driver_id = obj.native_id as u32;
                let entry_size = core::mem::size_of::<DriverInfoRaw>();
                if buf_size < entry_size { return 0u64; }
                if let Some(d) = crate::drivers::driver_runtime::get_driver(driver_id) {
                    let raw = DriverInfoRaw {
                        id: d.id as u32, state: d.state as u8, category: d.category as u8,
                        driver_type: d.driver_type as u8, api_version: d.api_version,
                        abi_min: d.abi_min, abi_target: d.abi_target, abi_max: d.abi_max,
                        last_error: d.last_error, caps: d.caps, isolation_mode: d.isolation_mode,
                        events_received: d.events_received, tick_count: d.tick_count,
                        registered_at_tick: d.registered_at_tick, name: d.name,
                    };
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            &raw as *const DriverInfoRaw as *const u8,
                            buf_ptr as *mut u8,
                            entry_size,
                        );
                    }
                    return entry_size as u64;
                }
                return err_to_u64(SyscallError::NoEnt);
            }

            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 7 {
                return err_to_u64(SyscallError::Inval);
            }
            let entry_size_bulk = core::mem::size_of::<DriverInfoRaw>();
            let max_entries = buf_size / entry_size_bulk;
            if max_entries == 0 { return 0u64; }
            let runtime = crate::drivers::driver_runtime::DRIVER_RUNTIME.lock();
            let ids = runtime.driver_ids();
            let count = ids.len().min(max_entries);
            for i in 0..count {
                if let Some(d) = crate::drivers::driver_runtime::get_driver(ids[i]) {
                    let raw = DriverInfoRaw {
                        id: d.id as u32, state: d.state as u8, category: d.category as u8,
                        driver_type: d.driver_type as u8, api_version: d.api_version,
                        abi_min: d.abi_min, abi_target: d.abi_target, abi_max: d.abi_max,
                        last_error: d.last_error, caps: d.caps, isolation_mode: d.isolation_mode,
                        events_received: d.events_received, tick_count: d.tick_count,
                        registered_at_tick: d.registered_at_tick, name: d.name,
                    };
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            &raw as *const DriverInfoRaw as *const u8,
                            (buf_ptr as *mut u8).add(i * entry_size_bulk),
                            entry_size_bulk,
                        );
                    }
                }
            }
            drop(runtime);
            (count * entry_size_bulk) as u64
        }
        _ if info_class == ObInfoClass::Cwd as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 8 {
                return err_to_u64(SyscallError::Inval);
            }
            let (drive, path) = crate::scheduler::get_current_cwd();
            let full = alloc::format!("{}:{}", (b'A' + drive) as char, path);
            let bytes = full.as_bytes();
            let copy_len = bytes.len().min(buf_size.saturating_sub(1));
            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr as *mut u8, copy_len);
                (buf_ptr as *mut u8).add(copy_len).write(0);
            }
            copy_len as u64
        }
        _ if info_class == ObInfoClass::KeyboardLayout as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 9 {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 1 { return err_to_u64(SyscallError::Inval); }
            let layout = KEYBOARD_LAYOUT.load(core::sync::atomic::Ordering::Relaxed);
            unsafe { core::ptr::write_volatile(buf_ptr as *mut u8, layout); }
            1u64
        }
        _ if info_class == ObInfoClass::ReadContent as u32 => {
            let (drive_idx, inode_num, handle_offset) = crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    let e = ep.handle_table[fd as usize];
                    if e.has_ob_object() {
                        if let Some(obj) = crate::object::ob_lookup(e.object_id) {
                            if obj.obj_type == crate::object::ObType::Filesystem {
                                return (obj.flags as usize, obj.native_id as u32, e.offset);
                            }
                        }
                    }
                    if let Some(ot) = e.obj_type() {
                        if ot == crate::object::ObType::Filesystem {
                            return (e.drive().unwrap_or(0) as usize, e.native_id().unwrap_or(0) as u32, e.offset);
                        }
                    }
                }
                (usize::MAX, 0, 0)
            });
            if drive_idx == usize::MAX {
                return err_to_u64(SyscallError::Inval);
            }
            let mut temp_buf = Vec::with_capacity(buf_size);
            temp_buf.resize(buf_size, 0u8);
            let result = crate::globals::with_vfs(|vfs| {
                vfs.read(drive_idx, inode_num, handle_offset, &mut temp_buf)
            });
            match result {
                Ok(bytes_read) => {
                    unsafe {
                        core::ptr::copy_nonoverlapping(temp_buf.as_ptr(), buf_ptr as *mut u8, bytes_read);
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
        _ if info_class == ObInfoClass::VolumeLabel as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Filesystem) {
                return err_to_u64(SyscallError::Inval);
            }
            let drive_byte = entry.drive().unwrap_or(0xFF);
            if drive_byte == 0xFF {
                return err_to_u64(SyscallError::Inval);
            }
            let drive_char = (b'A' + drive_byte) as char;
            let result = crate::globals::with_vfs(|vfs| {
                vfs.volume_label(drive_char)
            });
            match result {
                Ok(label) => {
                    let bytes = label.as_bytes();
                    let copy_len = bytes.len().min(buf_size.saturating_sub(1));
                    unsafe {
                        core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr as *mut u8, copy_len);
                        (buf_ptr as *mut u8).add(copy_len).write(0);
                    }
                    copy_len as u64
                }
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// OB-013: ObSetInfo — RAX=63
// ═══════════════════════════════════════════════════════════════════════

pub(super) fn handler_ob_set_info(regs: super::Registers) -> u64 {
    let fd = regs.rbx as u8;
    let info_class = regs.rcx as u32;
    let buf_ptr = regs.rdx;
    let buf_size = regs.r8 as usize;

    if info_class != (ObSetInfoClass::FileDelete as u32) {
        if buf_ptr == 0 || buf_size == 0 {
            return err_to_u64(SyscallError::Inval);
        }
        if !is_user_ptr_valid(buf_ptr, buf_size as u64) {
            return err_to_u64(SyscallError::Fault);
        }
    }

    let entry = current_handle_entry(fd);
    if !entry.is_open() {
        return err_to_u64(SyscallError::BadF);
    }

    match info_class {
        _ if info_class == ObSetInfoClass::ProcessPriority as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Process {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 4 {
                return err_to_u64(SyscallError::Inval);
            }
            let priority = unsafe { core::ptr::read_volatile(buf_ptr as *const u32) };
            if priority > 3 {
                return err_to_u64(SyscallError::Inval);
            }
            let pid = obj.native_id as u32;
            crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler();
                let mut lock = s.lock();
                lock.set_process_priority(pid, priority as u8);
            });
            0
        }
        _ if info_class == ObSetInfoClass::ThreadPriority as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Process && obj.obj_type != crate::object::ObType::Thread {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 4 {
                return err_to_u64(SyscallError::Inval);
            }
            let priority = unsafe { core::ptr::read_volatile(buf_ptr as *const u32) };
            if priority > 3 {
                return err_to_u64(SyscallError::Inval);
            }
            crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler();
                let mut lock = s.lock();
                if obj.obj_type == crate::object::ObType::Process {
                    let pid = obj.native_id as u32;
                    for kt in lock.kthreads.iter_mut() {
                        if let Some(k) = kt {
                            if k.pid == pid {
                                k.priority = priority as u8;
                            }
                        }
                    }
                } else {
                    let tid = obj.native_id as u32;
                    if let Some(k) = lock.find_kthread_mut(tid) {
                        k.priority = priority as u8;
                    }
                }
            });
            0
        }
        _ if info_class == ObSetInfoClass::ObjectName as u32 => {
            let name = match copy_user_string(buf_ptr) {
                Ok(s) => s,
                Err(_) => return err_to_u64(SyscallError::Fault),
            };
            if name.len() > 31 || name.is_empty() {
                return err_to_u64(SyscallError::Inval);
            }
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            match crate::object::ob_set_object_name(entry.object_id, &name) {
                Ok(_) => 0,
                Err(_) => err_to_u64(SyscallError::BadF),
            }
        }
        _ if info_class == ObSetInfoClass::Security as u32 => {
            return err_to_u64(SyscallError::NoSys);
        }
        _ if info_class == ObSetInfoClass::ProcessTerminate as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Process {
                return err_to_u64(SyscallError::Inval);
            }
            let pid = obj.native_id as u32;
            if pid == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler();
                let mut lock = s.lock();
                if lock.kill_pid(pid) {
                    lock.wake_waiters(pid);
                    0
                } else {
                    err_to_u64(SyscallError::NoEnt)
                }
            })
        }
        _ if info_class == ObSetInfoClass::KeyboardLayout as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 9 {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 1 { return err_to_u64(SyscallError::Inval); }
            let layout = unsafe { core::ptr::read_volatile(buf_ptr as *const u8) };
            if layout > 1 { return err_to_u64(SyscallError::Inval); }
            KEYBOARD_LAYOUT.store(layout, core::sync::atomic::Ordering::Relaxed);
            match crate::eventbus::EVENT_BUS.push_event(
                crate::eventbus::EVENT_KEYB_LAYOUT,
                crate::eventbus::SOURCE_KERNEL,
                3, layout as u64, 0, 0,
            ) {
                Ok(_) => 0,
                Err(_) => err_to_u64(SyscallError::Again),
            }
        }
        _ if info_class == ObSetInfoClass::VfsRename as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            let obj_name = obj.name_str();
            if !obj_name.starts_with("\\Global\\FileSystem\\") {
                return err_to_u64(SyscallError::Inval);
            }
            let old_vfs_path = &obj_name["\\Global\\FileSystem\\".len()..];
            if old_vfs_path.is_empty() {
                return err_to_u64(SyscallError::Inval);
            }
            let new_path = {
                let mut tmp = [0u8; 256];
                let copy_len = buf_size.min(255);
                unsafe {
                    core::ptr::copy_nonoverlapping(buf_ptr as *const u8, tmp.as_mut_ptr(), copy_len);
                }
                match core::str::from_utf8(&tmp[..copy_len]) {
                    Ok(s) => s.to_string(),
                    Err(_) => return err_to_u64(SyscallError::Inval),
                }
            };
            if new_path.is_empty() {
                return err_to_u64(SyscallError::Inval);
            }
            match crate::globals::with_vfs(|vfs| vfs.rename(old_vfs_path, &new_path)) {
                Ok(_) => {
                    let _ = crate::object::namespace::ob_remove_object(&obj_name);
                    let new_ob_name = alloc::format!("\\Global\\FileSystem\\{}", new_path);
                    let _ = crate::object::ob_set_object_name(entry.object_id, &new_ob_name);
                    {
                        let _ = crate::object::namespace::ob_create_directory_tree(&new_ob_name);
                    }
                    let _ = crate::object::namespace::ob_insert_object(&new_ob_name, entry.object_id);
                    0
                }
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        _ if info_class == ObSetInfoClass::WriteContent as u32 => {
            let (drive_idx, inode_num, handle_offset) = crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    let e = ep.handle_table[fd as usize];
                    if e.has_ob_object() {
                        if let Some(obj) = crate::object::ob_lookup(e.object_id) {
                            if obj.obj_type == crate::object::ObType::Filesystem {
                                return (obj.flags as usize, obj.native_id as u32, e.offset);
                            }
                        }
                    }
                    if let Some(ot) = e.obj_type() {
                        if ot == crate::object::ObType::Filesystem {
                            return (e.drive().unwrap_or(0) as usize, e.native_id().unwrap_or(0) as u32, e.offset);
                        }
                    }
                }
                (usize::MAX, 0, 0)
            });
            if drive_idx == usize::MAX {
                return err_to_u64(SyscallError::Inval);
            }
            let mut temp_buf = Vec::with_capacity(buf_size);
            temp_buf.resize(buf_size, 0u8);
            unsafe {
                core::ptr::copy_nonoverlapping(buf_ptr as *const u8, temp_buf.as_mut_ptr(), buf_size);
            }
            let result = crate::globals::with_vfs(|vfs| {
                vfs.write(drive_idx, inode_num, handle_offset, &temp_buf)
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
        _ if info_class == ObSetInfoClass::SetCwd as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 8 {
                return err_to_u64(SyscallError::Inval);
            }
            let path_str = match copy_user_string(buf_ptr) {
                Ok(s) => s,
                Err(_) => return err_to_u64(SyscallError::Fault),
            };
            if path_str.is_empty() {
                return err_to_u64(SyscallError::Inval);
            }
            match resolve_chdir_target(path_str) {
                Ok((new_drive, new_cwd_path)) => {
                    crate::scheduler::set_current_cwd(new_drive, &new_cwd_path);
                    0
                }
                Err(_) => err_to_u64(SyscallError::NoEnt),
            }
        }
        _ if info_class == ObSetInfoClass::SetVolumeLabel as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Filesystem) {
                return err_to_u64(SyscallError::Inval);
            }
            let drive_byte = entry.drive().unwrap_or(0xFF);
            if drive_byte == 0xFF {
                return err_to_u64(SyscallError::Inval);
            }
            let drive_char = (b'A' + drive_byte) as char;
            let label = match copy_user_string(buf_ptr) {
                Ok(s) => s,
                Err(_) => return err_to_u64(SyscallError::Fault),
            };
            if label.len() > 31 || label.is_empty() {
                return err_to_u64(SyscallError::Inval);
            }
            match crate::globals::with_vfs(|vfs| vfs.set_volume_label(drive_char, &label)) {
                Ok(_) => 0,
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        _ if info_class == ObSetInfoClass::SetProcessVt as u32 => {
            if entry.object_id == 0 { return err_to_u64(SyscallError::Inval); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 11 {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 1 { return err_to_u64(SyscallError::Inval); }
            let new_vt = unsafe { core::ptr::read_volatile(buf_ptr as *const u8) };
            if new_vt >= crate::input::vt::VT_COUNT as u8 { return err_to_u64(SyscallError::Inval); }
            crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() { ep.vt_num = new_vt; }
            });
            0
        }
        _ if info_class == ObSetInfoClass::TimerStart as u32 => {
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Timer {
                return err_to_u64(SyscallError::Inval);
            }
            let timer_id = obj.native_id as u32;
            if crate::object::timer::start_timer(timer_id) { 0 }
            else { err_to_u64(SyscallError::Inval) }
        }
        _ if info_class == ObSetInfoClass::TimerCancel as u32 => {
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Timer {
                return err_to_u64(SyscallError::Inval);
            }
            let timer_id = obj.native_id as u32;
            if crate::object::timer::cancel_timer(timer_id) { 0 }
            else { err_to_u64(SyscallError::Inval) }
        }
        _ if info_class == ObSetInfoClass::SemaphoreRelease as u32 => {
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Semaphore {
                return err_to_u64(SyscallError::Inval);
            }
            let sem_id = obj.native_id as u32;
            let release_count = if buf_size >= 4 {
                (unsafe { core::ptr::read_volatile(buf_ptr as *const u32) }) as i32
            } else {
                1
            };
            if crate::object::semaphore::release_semaphore(sem_id, release_count) { 0 }
            else { err_to_u64(SyscallError::Inval) }
        }
        _ if info_class == ObSetInfoClass::SectionMapView as u32 => {
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Section {
                return err_to_u64(SyscallError::Inval);
            }
            let section_id = obj.native_id as u32;
            match crate::object::section::map_view(section_id) {
                Some(base) => {
                    if buf_size >= 8 {
                        unsafe { core::ptr::write_volatile(buf_ptr as *mut u64, base); }
                    }
                    base
                }
                None => err_to_u64(SyscallError::NoMem),
            }
        }
        _ if info_class == ObSetInfoClass::SectionUnmapView as u32 => {
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Section {
                return err_to_u64(SyscallError::Inval);
            }
            let section_id = obj.native_id as u32;
            let base = if buf_size >= 8 {
                unsafe { core::ptr::read_volatile(buf_ptr as *const u64) }
            } else {
                return err_to_u64(SyscallError::Inval);
            };
            if crate::object::section::unmap_view(section_id, base) { 0 }
            else { err_to_u64(SyscallError::Inval) }
        }
        _ if info_class == ObSetInfoClass::FileCreate as u32 => {
            if buf_size < 3 { return err_to_u64(SyscallError::Inval); }
            let path_str = match copy_user_string(buf_ptr) {
                Ok(s) => s,
                Err(_) => return err_to_u64(SyscallError::Fault),
            };
            if !path_str.contains(':') { return err_to_u64(SyscallError::Inval); }
            let node = match crate::globals::with_vfs(|vfs| vfs.create(&path_str)) {
                Ok(n) => n,
                Err(_) => return err_to_u64(SyscallError::Io),
            };
            let drive_idx = {
                let drive_letter = path_str.as_bytes()[0].to_ascii_uppercase();
                (drive_letter - b'A') as usize
            };
            let inode = node.inode;
            let ob_name = alloc::format!("\\Global\\FileSystem\\{}", path_str);
            let ob_id = match crate::object::ob_create_object(
                crate::object::ObType::Filesystem, &ob_name,
                inode as u64, drive_idx as u32, None,
            ) {
                Ok(id) => id,
                Err(_) => return err_to_u64(SyscallError::NoMem),
            };
            {
                let _ = crate::object::namespace::ob_create_directory_tree(&ob_name);
            }
            let _ = crate::object::namespace::ob_insert_object(&ob_name, ob_id);
            let entry = crate::handle::HandleEntry::ob_object(ob_id, 0);
            let fd = crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    crate::handle::alloc_handle(&mut ep.handle_table, entry)
                } else { None }
            });
            match fd {
                Some(fd_val) => {
                    if buf_size >= 1 {
                        unsafe { core::ptr::write_volatile(buf_ptr as *mut u8, fd_val); }
                    }
                    fd_val as u64
                }
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    err_to_u64(SyscallError::NoMem)
                }
            }
        }
        _ if info_class == ObSetInfoClass::FileDelete as u32 => {
            if entry.object_id == 0 { return err_to_u64(SyscallError::BadF); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Filesystem {
                return err_to_u64(SyscallError::Inval);
            }
            let obj_name = obj.name_str();
            if !obj_name.starts_with("\\Global\\FileSystem\\") {
                return err_to_u64(SyscallError::Inval);
            }
            let vfs_path = &obj_name["\\Global\\FileSystem\\".len()..];
            if vfs_path.is_empty() {
                return err_to_u64(SyscallError::Inval);
            }
            let _ = crate::globals::with_vfs(|vfs| vfs.remove_file(vfs_path));
            let _ = crate::object::namespace::ob_remove_object(obj_name);
            crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    ep.handle_table[fd as usize].close();
                }
            });
            0
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// OB-014: ObEnum — RAX=64
// ═══════════════════════════════════════════════════════════════════════

pub(super) fn handler_ob_enum(regs: super::Registers) -> u64 {
    let dir_fd = regs.rbx as u8;
    let buf_ptr = regs.rcx;
    let max_entries = regs.rdx as usize;

    if buf_ptr == 0 || max_entries == 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let entry_size = core::mem::size_of::<crate::object::ObEnumEntry>() as u64;
    if !is_user_ptr_valid(buf_ptr, entry_size.saturating_mul(max_entries as u64)) {
        return err_to_u64(SyscallError::Fault);
    }

    let entry = current_handle_entry(dir_fd);
    if !entry.is_open() {
        return err_to_u64(SyscallError::BadF);
    }

    let use_vfs = if entry.object_id != 0 {
        matches!(entry.obj_type(), Some(crate::object::ObType::Filesystem) | Some(crate::object::ObType::Directory))
            && crate::object::ob_lookup(entry.object_id).map_or(false, |obj| {
                let s = obj.name_str();
                s.starts_with("\\Global\\FileSystem\\") || s.starts_with("dir/") || s.starts_with("file/")
            })
    } else {
        false
    };
    if use_vfs {
        let drv = entry.drive().unwrap_or(0);
        let nid = entry.native_id().unwrap_or(0);
        let drive_idx = drv as usize;
        let dir_inode = nid as u32;
        let mut entries = alloc::vec::Vec::new();
        let result: Result<(), ()> = crate::globals::with_vfs(|vfs| {
            let mut idx = 0usize;
            loop {
                match vfs.readdir(drive_idx, dir_inode, idx) {
                    Ok(Some(vfs_entry)) => {
                        let name_bytes = vfs_entry.name.as_bytes();
                        let mut name_arr = [0u8; 32];
                        let len = name_bytes.len().min(31);
                        name_arr[..len].copy_from_slice(&name_bytes[..len]);
                        let obj_type = if (vfs_entry.node.mode & crate::fs::vfs::MODE_DIR) != 0 {
                            crate::object::ObType::Directory
                        } else {
                            crate::object::ObType::Filesystem
                        };
                        entries.push(crate::object::ObEnumEntry {
                            id: vfs_entry.node.inode as u64,
                            obj_type: obj_type as u32,
                            name: name_arr,
                            mode: vfs_entry.node.mode,
                            _pad: [0u8; 2],
                            size: vfs_entry.node.size,
                        });
                        idx += 1;
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
            Ok(())
        });
        return match result {
            Ok(()) => {
                let count = core::cmp::min(max_entries, entries.len());
                for i in 0..count {
                    let raw = &entries[i];
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            raw as *const crate::object::ObEnumEntry as *const u8,
                            (buf_ptr as *mut u8).add(i * core::mem::size_of::<crate::object::ObEnumEntry>()),
                            core::mem::size_of::<crate::object::ObEnumEntry>(),
                        );
                    }
                }
                count as u64
            }
            Err(_) => err_to_u64(SyscallError::Inval),
        };
    }

    let path = if entry.object_id != 0 {
        crate::object::namespace::ob_find_path_by_id(entry.object_id)
    } else {
        None
    };
    let dir_path = match path {
        Some(p) => p,
        None => return err_to_u64(SyscallError::Inval),
    };
    let ob_entries = match crate::object::ob_enum_directory(&dir_path) {
        Ok(e) => e,
        Err(_) => return err_to_u64(SyscallError::Inval),
    };
    let count = core::cmp::min(max_entries, ob_entries.len());
    for i in 0..count {
        let raw = &ob_entries[i];
        unsafe {
            core::ptr::copy_nonoverlapping(
                raw as *const crate::object::ObEnumEntry as *const u8,
                (buf_ptr as *mut u8).add(i * core::mem::size_of::<crate::object::ObEnumEntry>()),
                core::mem::size_of::<crate::object::ObEnumEntry>(),
            );
        }
    }
    count as u64
}

// ═══════════════════════════════════════════════════════════════════════
// OB-020: ObWait — RAX=65
// ═══════════════════════════════════════════════════════════════════════

pub(super) fn handler_ob_wait(regs: super::Registers) -> u64 {
    let handle_count = regs.rbx as usize;
    let handles_ptr = regs.rcx;
    let wait_type = regs.rdx as u32;
    let _timeout_ms = regs.r8 as u64;

    if handle_count == 0 || handles_ptr == 0 {
        return err_to_u64(SyscallError::Inval);
    }
    if !is_user_ptr_valid(handles_ptr, (handle_count as u64) * 8) {
        return err_to_u64(SyscallError::Fault);
    }
    if handle_count > 1 {
        return err_to_u64(SyscallError::NoSys);
    }
    if wait_type > 1 {
        return err_to_u64(SyscallError::Inval);
    }

    let fd = unsafe { (handles_ptr as *const u64).read() } as u8;
    let entry = current_handle_entry(fd);

    if entry.object_id == 0 {
        return err_to_u64(SyscallError::BadF);
    }

    let obj = match crate::object::ob_lookup(entry.object_id) {
        Some(o) => o,
        None => return err_to_u64(SyscallError::BadF),
    };

    let reason = match obj.obj_type {
        crate::object::ObType::Process => {
            let pid = obj.native_id as u32;
            crate::kwait::WaitReason::ChildExit { pid }
        }
        crate::object::ObType::Pipe => {
            let pipe_id = obj.native_id as u8;
            if let Some(true) = crate::object::pipe::pipe_peek_read_ready(pipe_id) {
                return 0;
            }
            crate::kwait::WaitReason::PipeRead { pipe_id: pipe_id as u16 }
        }
        crate::object::ObType::Event => {
            let event_type = obj.native_id as u32;
            crate::kwait::WaitReason::Event { event_type }
        }
        crate::object::ObType::Timer => {
            let timer_id = obj.native_id as u32;
            crate::kwait::WaitReason::Timer { timer_id }
        }
        crate::object::ObType::Semaphore => {
            let sem_id = obj.native_id as u32;
            if crate::object::semaphore::try_wait_semaphore(sem_id) {
                return 0;
            }
            crate::kwait::WaitReason::Semaphore { sem_id }
        }
        crate::object::ObType::Thread => {
            let tid = obj.native_id as u32;
            crate::kwait::WaitReason::ThreadJoin { tid }
        }
        _ => return err_to_u64(SyscallError::NoSys),
    };

    crate::kwait::kwait_block(reason);
    0
}

// ═══════════════════════════════════════════════════════════════════════
// OB-066: ObDestroy — RAX=66
// ═══════════════════════════════════════════════════════════════════════

pub(super) fn handler_ob_destroy(regs: super::Registers) -> u64 {
    let fd = regs.rbx as u8;

    let (object_id, obj_type, name) = crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(ep) = lock.current_eprocess_mut() {
            let entry = ep.handle_table[fd as usize];
            if !entry.is_open() {
                return (0, crate::object::ObType::Unknown, alloc::string::String::new());
            }
            let oid = entry.object_id;
            let ot = entry.obj_type().unwrap_or(crate::object::ObType::Unknown);
            if oid == 0 {
                return (0, crate::object::ObType::Unknown, alloc::string::String::new());
            }
            let obj_name = match crate::object::ob_lookup(oid) {
                Some(o) => o.name_str().to_string(),
                None => alloc::string::String::new(),
            };
            (oid, ot, obj_name)
        } else {
            (0, crate::object::ObType::Unknown, alloc::string::String::new())
        }
    });

    if object_id == 0 {
        return err_to_u64(SyscallError::BadF);
    }

    if obj_type == crate::object::ObType::Directory && name.starts_with("\\Global\\FileSystem\\") {
        let vfs_path = &name["\\Global\\FileSystem\\".len()..];
        if !vfs_path.is_empty() {
            let _ = crate::globals::with_vfs(|vfs| vfs.remove_dir(vfs_path));
        }
    } else if obj_type == crate::object::ObType::Driver {
        let driver_name = if name.ends_with('\0') {
            &name[..name.len() - 1]
        } else {
            &name
        };
        let _ = crate::drivers::hotreload::unload_driver(driver_name, false);
    }

    if name.starts_with("\\Global\\FileSystem\\") {
        let _ = crate::object::namespace::ob_remove_object(&name);
    }

    crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(ep) = lock.current_eprocess_mut() {
            ep.handle_table[fd as usize].close();
        }
    });
    0
}
