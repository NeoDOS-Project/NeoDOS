//! Ob create — extracted from ob.rs
use alloc::string::{String, ToString};
use crate::scheduler::{self, ThreadState};
use crate::object::types::{ObInfoClass, ObSetInfoClass};
use crate::log::LogSubsys;
use crate::syscall::{current_handle_entry, copy_handle_entry_for_child, resolve_chdir_target, err_to_u64, ob_err_to_syscall, SyscallError};
use crate::syscall::util::{is_user_ptr_valid, copy_user_string};

pub fn handler_ob_create(regs: crate::syscall::Registers) -> u64 {
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
         18 => crate::object::ObType::Socket,
         20 => crate::object::ObType::Service,
        _ => return err_to_u64(SyscallError::Inval),
    };

    match obj_type {
        crate::object::ObType::Pipe => {
            if fds_out == 0 || !is_user_ptr_valid(fds_out, 16) {
                return err_to_u64(SyscallError::Fault);
            }
            let ob_id = match crate::object::ob_create_object_path(
                &path_str, obj_type, 0, Some(&crate::object::pipe::PIPE_OPS),
            ) {
                Ok(id) => id,
                Err(e) => return err_to_u64(ob_err_to_syscall(e)),
            };
            let obj = crate::object::ob_lookup(ob_id).unwrap();
            let pipe_id = obj.native_id as u8;
            let read_entry = crate::handle::HandleEntry {
                object_id: ob_id,
                offset: 0,
            };
            let write_entry = crate::handle::HandleEntry {
                object_id: ob_id,
                offset: 1,
            };
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
            crate::object::ob_reference(ob_id).ok();
            crate::object::ob_reference(ob_id).ok();
            let _ = crate::object::ob_close_object(ob_id);
            unsafe {
                (fds_out as *mut u64).write(rfd);
                (fds_out as *mut u64).add(1).write(wfd);
            }
            rfd
        }
        crate::object::ObType::Directory => {
            // VFS-3.3: VFS paths under \Global\FileSystem\ delegate to
            // vfs.mkdir and return a VFS-backed fd (no Ob namespace entry).
            if let Some(vfs_path) = path_str.strip_prefix("\\Global\\FileSystem\\") {
                if !vfs_path.is_empty() {
                    match crate::globals::with_vfs(|vfs| vfs.mkdir(vfs_path)) {
                        Ok(_) => {},
                        Err(_) => return err_to_u64(SyscallError::Io),
                    }
                }
                let token = crate::hal::without_interrupts(|| {
                    let s = scheduler::current_scheduler();
                    let lock = s.lock();
                    lock.current_eprocess()
                        .map(|ep| ep.token.clone())
                        .unwrap_or(crate::security::DEFAULT_ADMIN_TOKEN.clone())
                });
                return match crate::object::ob_open_path(&path_str, &token, 0) {
                    Ok(ob_id) => {
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
                };
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
            crate::serial_println!("[OB] handler_ob_create Process: path='{}'", path_str);

            let stdin_fd = (attrs & 0xFF) as u8;
            let stdout_fd = ((attrs >> 8) & 0xFF) as u8;
            let stderr_fd = ((attrs >> 16) & 0xFF) as u8;

            const MAX_BIN: usize = 65536;
            let bin_data = {
                let mut buf = alloc::vec![0u8; MAX_BIN];
                let vfs_path = path_str.strip_prefix("\\Global\\FileSystem\\").unwrap_or(&path_str);
                let bin_size = crate::globals::with_vfs(|vfs| {
                    match vfs.resolve_path(vfs_path) {
                        Ok((drive_idx, node)) => {
                            if (node.mode & crate::fs::vfs::MODE_FILE) == 0 { return 0; }
                            match vfs.read(drive_idx, node.inode, 0, &mut buf) {
                                Ok(n) => { if n > MAX_BIN { 0 } else { n } }
                                Err(e) => { crate::serial_println!("[OB] Process: vfs.read failed inode={}: {:?}", node.inode, e); 0 }
                            }
                        }
                        Err(e) => { crate::serial_println!("[OB] Process: resolve_path failed for '{}': {:?}", vfs_path, e); 0 }
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

            let child_pid = match crate::usermode::spawn_usermode(
                result.entry, slot.stack_top, slot.slot_idx,
                cwd_drive, &cwd_path, parent_pid,
            ) {
                Ok(pid) => {
                    crate::serial_println!("[OB] Process spawned: child_pid={} entry=0x{:x}", pid, result.entry);
                    pid
                }
                Err(_) => {
                    crate::arch::x64::paging::free_user_slot(slot.slot_idx);
                    return err_to_u64(SyscallError::NoMem);
                }
            };

            if stdin_fd != 0xFF || stdout_fd != 0xFF || stderr_fd != 0xFF {
                let (parent_stdin_entry, parent_stdout_entry, parent_stderr_entry) = crate::hal::without_interrupts(|| {
                    let s = scheduler::current_scheduler();
                    let lock = s.lock();
                    let get_parent_entry = |fd: u8| -> Option<crate::handle::HandleEntry> {
                        lock.current_eprocess().map(|ep| ep.handle_table.get(fd))
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
            {
                let obj = crate::object::ob_lookup(actual_ob_id);
                crate::serial_println!("[OB_CREATE] child_pid={} ob_id={} obj_type={:?} native_id={}", child_pid, actual_ob_id, obj.map(|o| o.obj_type), obj.map(|o| o.native_id).unwrap_or(9999));
            }

            if crate::object::ob_open_object(actual_ob_id, 0).is_err() {
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
                Some(fd) => {
                    crate::serial_println!("[OB_CREATE] fd={} for child_pid={} ob_id={}", fd, child_pid, actual_ob_id);
                    fd as u64
                },
                None => {
                    let _ = crate::object::ob_close_object(actual_ob_id);
                    err_to_u64(SyscallError::NoMem)
                }
            }
        }
        crate::object::ObType::Driver => {
            let driver_path = path_str.strip_prefix("\\Global\\FileSystem\\").unwrap_or(&path_str);
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
                    ob_id as u64
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
            let period_ms = attrs & 0x7FFFFFFF;
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
            let size = attrs & 0xFFFF_FFFF;
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
        crate::object::ObType::Socket => {
            if !crate::net::net_is_initialized() {
                return err_to_u64(SyscallError::NoSys);
            }
            let socket_type_val = attrs & 0xFF;
            let sock_type = match socket_type_val {
                1 => crate::net::types::SocketType::Tcp,
                2 => crate::net::types::SocketType::Udp,
                3 => crate::net::types::SocketType::Raw,
                _ => return err_to_u64(SyscallError::Inval),
            };
            let port = ((attrs >> 8) & 0xFFFF) as u16;

            let socket_id = match crate::net::socket::socket_alloc(sock_type) {
                Some(id) => id,
                None => return err_to_u64(SyscallError::NoMem),
            };

            // Assign default NIC if available (no NIC_REGISTRY lock ordering concern
            // since we don't hold SOCKET_MANAGER lock here).
            crate::net::socket::socket_assign_default_nic(socket_id);

            if sock_type == crate::net::types::SocketType::Tcp {
                if let Some(tcp_id) = crate::net::tcp::tcp_alloc_connection() {
                    crate::net::socket::socket_set_tcp_conn(socket_id, tcp_id);
                    crate::net::tcp::tcp_bind(tcp_id, crate::net::types::SocketAddrV4::new(
                        crate::net::types::Ipv4Addr::unspecified(), port,
                    ));
                }
            }

            let ob_id = match crate::object::ob_create_object_path(
                &path_str, obj_type, socket_id, None,
            ) {
                Ok(id) => id,
                Err(e) => {
                    crate::net::socket::socket_free(socket_id);
                    return err_to_u64(ob_err_to_syscall(e));
                }
            };

            // Store socket_id in entry's offset for direct retrieval by socket ops
            let entry = crate::handle::HandleEntry::ob_object(ob_id, socket_id);
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
                    crate::net::socket::socket_free(socket_id);
                    err_to_u64(SyscallError::NoMem)
                }
            }
        }
        crate::object::ObType::Service => {
            // Creating a Service object from user-mode requires admin privileges
            // Delegate to ServiceManager::register for the given path.
            let ob_id = match crate::object::ob_create_object_path(
                &path_str, obj_type, 0, None,
            ) {
                Ok(id) => id,
                Err(_) => return err_to_u64(SyscallError::NoMem),
            };
            let _ = crate::object::namespace::ob_create_directory_tree(&path_str);
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

