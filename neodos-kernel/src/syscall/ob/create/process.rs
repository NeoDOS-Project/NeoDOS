//! Ob create — process and thread objects.

use crate::syscall::{err_to_u64, SyscallError};
use crate::scheduler;
use crate::syscall::copy_handle_entry_for_child;

pub(super) fn handles(obj_type: crate::object::ObType) -> bool {
    obj_type == crate::object::ObType::Process
        || obj_type == crate::object::ObType::Thread
}

/// Dispatch the `process` object kinds.
pub(super) fn dispatch(
    obj_type: crate::object::ObType,
    path_str: &str,
    _fds_out: u64,
    attrs: u64,
) -> u64 {
    match obj_type {
        crate::object::ObType::Process => {
            crate::serial_println!("[OB] handler_ob_create Process: path='{}'", path_str);

            let stdin_fd = (attrs & 0xFF) as u8;
            let stdout_fd = ((attrs >> 8) & 0xFF) as u8;
            let stderr_fd = ((attrs >> 16) & 0xFF) as u8;

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

            // Shared kernel process-creation path (also used by the Service
            // Manager): VFS read + user slot + ELF load + spawn_usermode.
            let created = match crate::usermode::create_process_from_ob_path(
                &path_str, cwd_drive, &cwd_path, parent_pid, &path_str,
            ) {
                Ok(c) => c,
                Err(crate::usermode::CreateProcessError::NotFound) => return err_to_u64(SyscallError::NoEnt),
                Err(crate::usermode::CreateProcessError::InvalidElf) => return err_to_u64(SyscallError::Inval),
                Err(crate::usermode::CreateProcessError::NoMemory) => return err_to_u64(SyscallError::NoMem),
            };
            let child_pid = created.pid;
            crate::serial_println!("[OB] Process spawned: child_pid={} entry=0x{:x}", child_pid, created.entry);

            // ── Fix 1.2: per-process args storage ──
            // Atomically copy args from the shared 0x41F000 buffer into the child's
            // Eprocess.args. This eliminates the data-race where concurrent pipeline
            // spawns overwrite the shared buffer before the child has read it.
            // The legacy buffer is still written for backward-compat, but the child
            // now receives its args via the kernel-stored copy (ProcessArgs query).
            {
                const ARGS_ADDR: u64 = 0x41F000;
                let mut args_buf = [0u8; 256];
                let copy_ok = if ARGS_ADDR >= crate::arch::x64::paging::USER_BASE
                    && ARGS_ADDR.saturating_add(256) <= crate::arch::x64::paging::USER_LIMIT
                {
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            ARGS_ADDR as *const u8,
                            args_buf.as_mut_ptr(),
                            256,
                        );
                    }
                    true
                } else { false };
                if copy_ok {
                    crate::hal::without_interrupts(|| {
                        let s = crate::scheduler::current_scheduler();
                        let mut lock = s.lock();
                        if let Some(ep) = lock.find_eprocess_mut(child_pid) {
                            ep.args.copy_from_slice(&args_buf);
                        }
                    });
                }
            }

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
        _ => err_to_u64(SyscallError::Inval),
    }
}
