//! Ob set — extracted from ob.rs
use alloc::string::{String, ToString};
use crate::scheduler::{self, ThreadState};
use crate::object::types::{ObInfoClass, ObSetInfoClass};
use crate::log::LogSubsys;
use crate::syscall::{current_handle_entry, copy_handle_entry_for_child, resolve_chdir_target, err_to_u64, ob_err_to_syscall, SyscallError};
use crate::syscall::util::{is_user_ptr_valid, copy_user_string};

pub fn handler_ob_set_info(regs: crate::syscall::Registers) -> u64 {
    let fd = regs.rbx as u8;
    let info_class = regs.rcx as u32;
    let buf_ptr = regs.rdx;
    let buf_size = regs.r8 as usize;

    if info_class == ObSetInfoClass::SetNicIp as u32 {
        kdebug!(LogSubsys::Object, "ObSetInfo SetNicIp: fd={} class={} buf_ptr=0x{:x} buf_size={}", fd, info_class, buf_ptr, buf_size);
    }

    if info_class != (ObSetInfoClass::FileDelete as u32) {
        if buf_ptr == 0 || buf_size == 0 {
            if info_class == ObSetInfoClass::SetNicIp as u32 {
                kdebug!(LogSubsys::Object, "ObSetInfo SetNicIp: REJECTED (null buf)");
            }
            return err_to_u64(SyscallError::Inval);
        }
        if !is_user_ptr_valid(buf_ptr, buf_size as u64) {
            if info_class == ObSetInfoClass::SetNicIp as u32 {
                kdebug!(LogSubsys::Object, "ObSetInfo SetNicIp: REJECTED (invalid user ptr)");
            }
            return err_to_u64(SyscallError::Fault);
        }
    }

    let entry = current_handle_entry(fd);
    if !entry.is_open() {
        if info_class == ObSetInfoClass::SetNicIp as u32 {
            kdebug!(LogSubsys::Object, "ObSetInfo SetNicIp: REJECTED (fd {} not open)", fd);
        }
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
                    for k in lock.kthreads.iter_mut().flatten() {
                        if k.pid == pid {
                            k.priority = priority as u8;
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
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            // buf = [rev: u8, ace_count: u8, (ace_type: u8, flags: u8, access_mask: u32 LE, sid_cnt: u8, sid_auth: [u8;6], sid_subs: u32×cnt)...]
            if buf_size < 2 {
                return err_to_u64(SyscallError::Inval);
            }
            let base = buf_ptr as *const u8;
            let _sd_rev = unsafe { core::ptr::read_volatile(base) };
            let ace_count = unsafe { core::ptr::read_volatile(base.add(1)) };
            let mut offset = 2usize;
            let mut acl = crate::security::acl::Acl::new();
            for _ in 0..ace_count {
                if offset + 7 > buf_size {
                    return err_to_u64(SyscallError::Inval);
                }
                let ace_type = unsafe { core::ptr::read_volatile(base.add(offset)) };
                let flags = unsafe { core::ptr::read_volatile(base.add(offset + 1)) };
                let access_mask = unsafe {
                    u32::from_le_bytes([
                        core::ptr::read_volatile(base.add(offset + 2)),
                        core::ptr::read_volatile(base.add(offset + 3)),
                        core::ptr::read_volatile(base.add(offset + 4)),
                        core::ptr::read_volatile(base.add(offset + 5)),
                    ])
                };
                let sid_cnt = unsafe { core::ptr::read_volatile(base.add(offset + 6)) } as usize;
                if sid_cnt > crate::security::sid::MAX_SUB_AUTHORITIES {
                    return err_to_u64(SyscallError::Inval);
                }
                offset += 7;
                if offset + 6 + sid_cnt * 4 > buf_size {
                    return err_to_u64(SyscallError::Inval);
                }
                let mut sid_auth = [0u8; 6];
                for j in 0..6 {
                    sid_auth[j] = unsafe { core::ptr::read_volatile(base.add(offset + j)) };
                }
                offset += 6;
                let mut sid_subs = [0u32; crate::security::sid::MAX_SUB_AUTHORITIES];
                for j in 0..sid_cnt {
                    sid_subs[j] = unsafe {
                        u32::from_le_bytes([
                            core::ptr::read_volatile(base.add(offset + j * 4)),
                            core::ptr::read_volatile(base.add(offset + j * 4 + 1)),
                            core::ptr::read_volatile(base.add(offset + j * 4 + 2)),
                            core::ptr::read_volatile(base.add(offset + j * 4 + 3)),
                        ])
                    };
                }
                offset += sid_cnt * 4;
                let sid = crate::security::sid::Sid::from_parts(1, &sid_auth, &sid_subs[..sid_cnt]);
                let ace = crate::security::acl::Ace { ace_type, flags, access_mask, sid };
                acl.insert_ace_canonical(ace);
            }
            let sd = crate::security::acl::SecurityDescriptor::new()
                .with_dacl(acl);
            match crate::object::ob_set_security(entry.object_id, sd) {
                Ok(()) => 0,
                Err(_) => err_to_u64(SyscallError::BadF),
            }
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
            if obj.obj_type == crate::object::ObType::KeyboardDevice {
                if buf_size < 1 { return err_to_u64(SyscallError::Inval); }
                let layout = unsafe { core::ptr::read_volatile(buf_ptr as *const u8) };
                let mut kbd = crate::kbd::KBD.lock();
                if (layout as usize) >= kbd.layouts.len() {
                    return err_to_u64(SyscallError::NoEnt);
                }
                kbd.state.active_layout_index = layout as u32;
                kbd.config.layout_name = kbd.layouts[layout as usize].name_str().to_string();
                let _ = crate::kbd::config::kbd_save_config(&kbd.config);
                let _ = crate::eventbus::EVENT_BUS.push_event(
                    crate::eventbus::EVENT_KEYB_LAYOUT,
                    crate::eventbus::SOURCE_KERNEL,
                    3, layout as u64, 0, 0,
                );
                return 0;
            }
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 9 {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 1 { return err_to_u64(SyscallError::Inval); }
            let layout = unsafe { core::ptr::read_volatile(buf_ptr as *const u8) };
            let mut kbd = crate::kbd::KBD.lock();
            if (layout as usize) >= kbd.layouts.len() {
                return err_to_u64(SyscallError::NoEnt);
            }
            kbd.state.active_layout_index = layout as u32;
            let _ = crate::eventbus::EVENT_BUS.push_event(
                crate::eventbus::EVENT_KEYB_LAYOUT,
                crate::eventbus::SOURCE_KERNEL,
                3, layout as u64, 0, 0,
            );
            0
        }
        _ if info_class == ObSetInfoClass::KeyboardSetLayout as u32 => {
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::KeyboardDevice {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 1 { return err_to_u64(SyscallError::Inval); }
            let name_len = buf_size.min(32);
            let mut name_buf = [0u8; 32];
            unsafe {
                core::ptr::copy_nonoverlapping(buf_ptr as *const u8, name_buf.as_mut_ptr(), name_len);
            }
            let name_end = name_buf.iter().position(|&b| b == 0).unwrap_or(name_len);
            let name = match core::str::from_utf8(&name_buf[..name_end]) {
                Ok(s) => s,
                Err(_) => return err_to_u64(SyscallError::Inval),
            };
            let mut kbd = crate::kbd::KBD.lock();
            match kbd.set_layout_by_name(name) {
                Ok(()) => 0,
                Err(()) => err_to_u64(SyscallError::NoEnt),
            }
        }
        _ if info_class == ObSetInfoClass::KeyboardSetRepeatDelay as u32 => {
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::KeyboardDevice {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 4 { return err_to_u64(SyscallError::Inval); }
            let delay = unsafe { core::ptr::read_volatile(buf_ptr as *const u32) };
            let mut kbd = crate::kbd::KBD.lock();
            match kbd.set_repeat_delay(delay) {
                Ok(()) => 0,
                Err(()) => err_to_u64(SyscallError::Inval),
            }
        }
        _ if info_class == ObSetInfoClass::KeyboardSetRepeatRate as u32 => {
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::KeyboardDevice {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 4 { return err_to_u64(SyscallError::Inval); }
            let rate = unsafe { core::ptr::read_volatile(buf_ptr as *const u32) };
            let mut kbd = crate::kbd::KBD.lock();
            match kbd.set_repeat_rate(rate) {
                Ok(()) => 0,
                Err(()) => err_to_u64(SyscallError::Inval),
            }
        }
        _ if info_class == ObSetInfoClass::KeyboardSetLeds as u32 => {
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::KeyboardDevice {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 1 { return err_to_u64(SyscallError::Inval); }
            let leds = unsafe { core::ptr::read_volatile(buf_ptr as *const u8) };
            let mut kbd = crate::kbd::KBD.lock();
            kbd.set_leds(leds);
            0
        }
        _ if info_class == ObSetInfoClass::KeyboardSetModifier as u32 => {
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::KeyboardDevice {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 1 { return err_to_u64(SyscallError::Inval); }
            if !crate::syscall::is_current_admin() {
                return err_to_u64(SyscallError::Perm);
            }
            let mods = unsafe { core::ptr::read_volatile(buf_ptr as *const u8) };
            let mut kbd = crate::kbd::KBD.lock();
            kbd.set_modifiers(mods);
            0
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
                    let _ = crate::object::namespace::ob_remove_object(obj_name);
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
            let mut temp_buf = alloc::vec![0u8; buf_size];
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
        _ if info_class == ObSetInfoClass::SocketConnect as u32 => {
            if buf_size < 6 { return err_to_u64(SyscallError::Inval); }
            if entry.object_id == 0 { return err_to_u64(SyscallError::BadF); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Socket {
                return err_to_u64(SyscallError::Inval);
            }
            let socket_id = obj.native_id as u32;
            let ip_bytes = unsafe { core::ptr::read_volatile(buf_ptr as *const [u8; 4]) };
            let port = unsafe { core::ptr::read_volatile((buf_ptr + 4) as *const u16) };
            let remote = crate::net::types::SocketAddrV4::new(
                crate::net::types::Ipv4Addr(ip_bytes),
                u16::from_be(port),
            );
            crate::net::socket::socket_set_remote(socket_id, remote);
            crate::net::socket::socket_set_connected(socket_id);
            kdebug!(LogSubsys::Object, "Connect OK sid={}", socket_id);
            0
        }
        _ if info_class == ObSetInfoClass::SocketBind as u32 => {
            if buf_size < 6 { return err_to_u64(SyscallError::Inval); }
            if entry.object_id == 0 { return err_to_u64(SyscallError::BadF); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Socket {
                return err_to_u64(SyscallError::Inval);
            }
            let socket_id = obj.native_id as u32;
            let ip_bytes = unsafe { core::ptr::read_volatile(buf_ptr as *const [u8; 4]) };
            let port = unsafe { core::ptr::read_volatile((buf_ptr + 4) as *const u16) };
            let local = crate::net::types::SocketAddrV4::new(
                crate::net::types::Ipv4Addr(ip_bytes),
                u16::from_be(port),
            );
            if crate::net::socket::socket_bind(socket_id, local) { 0 }
            else { err_to_u64(SyscallError::Inval) }
        }
        _ if info_class == ObSetInfoClass::SocketListen as u32 => {
            if entry.object_id == 0 { return err_to_u64(SyscallError::BadF); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Socket {
                return err_to_u64(SyscallError::Inval);
            }
            let socket_id = obj.native_id as u32;
            if crate::net::socket::socket_listen(socket_id) { 0 }
            else { err_to_u64(SyscallError::Inval) }
        }
        _ if info_class == ObSetInfoClass::SocketSend as u32 => {
            if entry.object_id == 0 { return err_to_u64(SyscallError::BadF); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Socket {
                return err_to_u64(SyscallError::Inval);
            }
            let socket_id = obj.native_id as u32;
            let mut temp = alloc::vec![0u8; buf_size];
            unsafe {
                core::ptr::copy_nonoverlapping(buf_ptr as *const u8, temp.as_mut_ptr(), buf_size);
            }
            match crate::net::socket::socket_send(socket_id, &temp) {
                Ok(n) => n as u64,
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        _ if info_class == ObSetInfoClass::SocketClose as u32 => {
            if entry.object_id == 0 { return err_to_u64(SyscallError::BadF); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Socket {
                return err_to_u64(SyscallError::Inval);
            }
            let socket_id = obj.native_id as u32;
            crate::net::socket::socket_close(socket_id);
            crate::net::socket::socket_free(socket_id);
            let _ = crate::object::namespace::ob_remove_object(obj.name_str());
            0
        }
        // ── RegistryCreateKey (23): create a subkey (name in buf) ──
        _ if info_class == ObSetInfoClass::RegistryCreateKey as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Key) {
                return err_to_u64(SyscallError::Inval);
            }
            let base = buf_ptr as *const u8;
            let name = {
                let mut s = alloc::string::String::new();
                for i in 0..buf_size {
                    let c = unsafe { core::ptr::read_volatile(base.add(i)) };
                    if c == 0 { break; }
                    s.push(c as char);
                }
                s
            };
            if name.is_empty() { return err_to_u64(SyscallError::Inval); }
            let native_id = match entry.native_id() {
                Some(id) => id,
                None => return err_to_u64(SyscallError::BadF),
            };
            match crate::cm::cm_create_key(native_id, &name) {
                Ok(_) => 0,
                Err(()) => err_to_u64(SyscallError::Exist),
            }
        }
        // ── RegistryDeleteKey (24): delete a subkey (name in buf) ──
        _ if info_class == ObSetInfoClass::RegistryDeleteKey as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Key) {
                return err_to_u64(SyscallError::Inval);
            }
            // If buf is empty, delete the key itself (like handler_cm_delete_key)
            if buf_size == 0 || unsafe { core::ptr::read_volatile(buf_ptr as *const u8) } == 0 {
                let native_id = match entry.native_id() {
                    Some(id) => id,
                    None => return err_to_u64(SyscallError::BadF),
                };
                match crate::cm::cm_delete_key(native_id) {
                    Ok(()) => {
                        let _ = crate::object::ob_destroy_object(entry.object_id);
                        0
                    }
                    Err(()) => err_to_u64(SyscallError::Inval),
                }
            } else {
                let base = buf_ptr as *const u8;
                let name = {
                    let mut s = alloc::string::String::new();
                    for i in 0..buf_size {
                        let c = unsafe { core::ptr::read_volatile(base.add(i)) };
                        if c == 0 { break; }
                        s.push(c as char);
                    }
                    s
                };
                let native_id = match entry.native_id() {
                    Some(id) => id,
                    None => return err_to_u64(SyscallError::BadF),
                };
                match crate::cm::cm_open_key(native_id, &name) {
                    Ok(subkey_native_id) => {
                        match crate::cm::cm_delete_key(subkey_native_id) {
                            Ok(()) => 0,
                            Err(()) => err_to_u64(SyscallError::Inval),
                        }
                    }
                    Err(()) => err_to_u64(SyscallError::NoEnt),
                }
            }
        }
        // ── RegistrySetValue (25): set a value on the key ──
        // buf = [name\0][value_type: u32 LE][data_len: u32 LE][data...]
        _ if info_class == ObSetInfoClass::RegistrySetValue as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Key) {
                return err_to_u64(SyscallError::Inval);
            }
            let base = buf_ptr as *const u8;
            let mut name_end = 0;
            while name_end < buf_size && buf_size - name_end >= 4 {
                let c = unsafe { core::ptr::read_volatile(base.add(name_end)) };
                if c == 0 { break; }
                name_end += 1;
            }
            if name_end == 0 || name_end >= buf_size - 8 {
                return err_to_u64(SyscallError::Inval);
            }
            let name_bytes = unsafe {
                core::slice::from_raw_parts(base, name_end)
            };
            let name = core::str::from_utf8(name_bytes).unwrap_or("");
            if name.is_empty() { return err_to_u64(SyscallError::Inval); }
            let payload_start = name_end + 1;
            if payload_start + 8 > buf_size {
                return err_to_u64(SyscallError::Inval);
            }
            let value_type = unsafe {
                u32::from_le_bytes([
                    core::ptr::read_volatile(base.add(payload_start)),
                    core::ptr::read_volatile(base.add(payload_start + 1)),
                    core::ptr::read_volatile(base.add(payload_start + 2)),
                    core::ptr::read_volatile(base.add(payload_start + 3)),
                ])
            };
            let data_len = unsafe {
                u32::from_le_bytes([
                    core::ptr::read_volatile(base.add(payload_start + 4)),
                    core::ptr::read_volatile(base.add(payload_start + 5)),
                    core::ptr::read_volatile(base.add(payload_start + 6)),
                    core::ptr::read_volatile(base.add(payload_start + 7)),
                ]) as usize
            };
            let data_start = payload_start + 8;
            if data_start + data_len > buf_size {
                return err_to_u64(SyscallError::Inval);
            }
            let data = unsafe {
                core::slice::from_raw_parts(base.add(data_start), data_len)
            };
            let native_id = match entry.native_id() {
                Some(id) => id,
                None => return err_to_u64(SyscallError::BadF),
            };
            match crate::cm::cm_set_value(native_id, name, value_type, data) {
                Ok(()) => 0,
                Err(()) => err_to_u64(SyscallError::NoMem),
            }
        }
        // ── RegistryDeleteValue (26): delete a value by name (name in buf) ──
        _ if info_class == ObSetInfoClass::RegistryDeleteValue as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Key) {
                return err_to_u64(SyscallError::Inval);
            }
            let base = buf_ptr as *const u8;
            let name = {
                let mut s = alloc::string::String::new();
                for i in 0..buf_size {
                    let c = unsafe { core::ptr::read_volatile(base.add(i)) };
                    if c == 0 { break; }
                    s.push(c as char);
                }
                s
            };
            if name.is_empty() { return err_to_u64(SyscallError::Inval); }
            let native_id = match entry.native_id() {
                Some(id) => id,
                None => return err_to_u64(SyscallError::BadF),
            };
            match crate::cm::cm_delete_value(native_id, &name) {
                Ok(()) => 0,
                Err(()) => err_to_u64(SyscallError::NoEnt),
            }
        }
        // ── SetNicIp (27): set NIC IP address and subnet mask ──
        _ if info_class == ObSetInfoClass::SetNicIp as u32 => {
            if buf_size < 8 { return err_to_u64(SyscallError::Inval); }
            let iface_idx = unsafe { core::ptr::read_volatile(buf_ptr as *const u32) };
            let ip_bytes = unsafe { core::ptr::read_volatile((buf_ptr + 4) as *const [u8; 4]) };
            let ip = crate::net::types::Ipv4Addr(ip_bytes);
            kdebug!(LogSubsys::Object, "SetNicIp: iface={} ip={}", iface_idx, ip);
            crate::net::nic::nic_set_ip(iface_idx, ip);
            if buf_size >= 12 {
                let mask_bytes = unsafe { core::ptr::read_volatile((buf_ptr + 8) as *const [u8; 4]) };
                let mask = crate::net::types::Ipv4Addr(mask_bytes);
                kdebug!(LogSubsys::Object, "SetNicMask: iface={} mask={}", iface_idx, mask);
                crate::net::nic::nic_set_mask(iface_idx, mask);
            }
            0
        }
        _ if info_class == ObSetInfoClass::ServiceStart as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Service {
                return err_to_u64(SyscallError::Inval);
            }
            let mut sm = crate::services::SERVICE_MANAGER.lock();
            let idx = match sm.find_by_obj_id(entry.object_id) {
                Some(i) => i,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            match sm.start_service(idx) {
                Ok(()) => 0,
                Err(_e) => err_to_u64(SyscallError::Busy),
            }
        }
        _ if info_class == ObSetInfoClass::ServiceStop as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Service {
                return err_to_u64(SyscallError::Inval);
            }
            let timeout_ms = if buf_size >= 4 {
                unsafe { core::ptr::read_volatile(buf_ptr as *const u32) }
            } else {
                0
            };
            let mut sm = crate::services::SERVICE_MANAGER.lock();
            let idx = match sm.find_by_obj_id(entry.object_id) {
                Some(i) => i,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            match sm.stop_service(idx, timeout_ms) {
                Ok(()) => 0,
                Err(_e) => err_to_u64(SyscallError::Busy),
            }
        }
        _ if info_class == ObSetInfoClass::ServiceRestart as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Service {
                return err_to_u64(SyscallError::Inval);
            }
            let timeout_ms = if buf_size >= 4 {
                unsafe { core::ptr::read_volatile(buf_ptr as *const u32) }
            } else {
                0
            };
            let mut sm = crate::services::SERVICE_MANAGER.lock();
            let idx = match sm.find_by_obj_id(entry.object_id) {
                Some(i) => i,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            match sm.restart_service(idx, timeout_ms) {
                Ok(()) => 0,
                Err(_e) => err_to_u64(SyscallError::Busy),
            }
        }
        _ if info_class == ObSetInfoClass::ServiceSetConfig as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Service {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 6 {
                return err_to_u64(SyscallError::Inval);
            }
            let start_type = unsafe { core::ptr::read_volatile(buf_ptr as *const u8) };
            let restart_policy = unsafe { core::ptr::read_volatile((buf_ptr + 1) as *const u8) };
            let max_failures = unsafe { core::ptr::read_volatile((buf_ptr + 2) as *const u32) };

            use crate::services::{ServiceStartType, ServiceRestartPolicy};
            let st = match start_type {
                0 => ServiceStartType::Boot,
                1 => ServiceStartType::System,
                2 => ServiceStartType::Auto,
                3 => ServiceStartType::Demand,
                4 => ServiceStartType::Disabled,
                _ => return err_to_u64(SyscallError::Inval),
            };
            let rp = match restart_policy {
                0 => ServiceRestartPolicy::Never,
                1 => ServiceRestartPolicy::OnCrash,
                2 => ServiceRestartPolicy::Always,
                _ => return err_to_u64(SyscallError::Inval),
            };
            let mut sm = crate::services::SERVICE_MANAGER.lock();
            let idx = match sm.find_by_obj_id(entry.object_id) {
                Some(i) => i,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            match sm.set_config(idx, st, rp, max_failures) {
                Ok(()) => 0,
                Err(_) => err_to_u64(SyscallError::Inval),
            }
        }
        _ if info_class == ObSetInfoClass::PowerShutdown as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::PowerManager {
                return err_to_u64(SyscallError::Inval);
            }
            crate::object::power::power_shutdown();
        }
        _ if info_class == ObSetInfoClass::PowerReboot as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::PowerManager {
                return err_to_u64(SyscallError::Inval);
            }
            crate::object::power::power_reboot();
        }
        _ if info_class == ObSetInfoClass::FsckRepair as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Filesystem) {
                return err_to_u64(SyscallError::Inval);
            }
            let drive_byte = entry.drive().unwrap_or(0xFF);
            if drive_byte == 0xFF {
                return err_to_u64(SyscallError::Inval);
            }
            let drive_char = (b'A' + drive_byte) as char;
            let repair = if buf_size >= 1 { unsafe { core::ptr::read_volatile::<u8>(buf_ptr as *const u8) != 0 } } else { false };
            crate::globals::with_vfs(|vfs| {
                let mut result = crate::fs::fsck::FsckStatsRaw {
                    total_blocks: 0, used_blocks: 0, free_blocks: 0,
                    total_nodes: 0, total_dirs: 0, total_files: 0,
                    errors: 0, warnings: 0, repaired: 0,
                };
                let drive_idx = match crate::fs::vfs::Vfs::drive_index(drive_char) {
                    Some(idx) => idx,
                    None => return,
                };
                if let Some(fs) = vfs.drives[drive_idx].as_mut() {
                    let _ = fs.fsck(repair, false, &mut result);
                }
            });
            0
        }
        _ if info_class == ObSetInfoClass::SetHostname as u32 => {
            if !crate::syscall::is_current_admin() {
                return err_to_u64(SyscallError::Perm);
            }
            if buf_size == 0 || buf_size > 64 {
                return err_to_u64(SyscallError::Inval);
            }
            let hostname_bytes = {
                let mut tmp = [0u8; 64];
                let copy_len = buf_size.min(63);
                unsafe {
                    core::ptr::copy_nonoverlapping(buf_ptr as *const u8, tmp.as_mut_ptr(), copy_len);
                }
                tmp[copy_len] = 0;
                let s = match core::str::from_utf8(&tmp[..copy_len]) {
                    Ok(s) => s.trim_end_matches('\0'),
                    Err(_) => return err_to_u64(SyscallError::Inval),
                };
                if s.is_empty() {
                    return err_to_u64(SyscallError::Inval);
                }
                let mut v = alloc::vec![0u8; s.len() + 1];
                v[..s.len()].copy_from_slice(s.as_bytes());
                v
            };
            let root_native = crate::cm::encode_cell(0, 0);
            let ctrl_native = match crate::cm::cm_open_key(root_native, "CurrentControlSet\\Control") {
                Ok(nid) => nid,
                Err(_) => return err_to_u64(SyscallError::NoEnt),
            };
            let key_native = match crate::cm::cm_open_key(ctrl_native, "ComputerName") {
                Ok(nid) => nid,
                Err(_) => match crate::cm::cm_create_key(ctrl_native, "ComputerName") {
                    Ok(nid) => nid,
                    Err(_) => return err_to_u64(SyscallError::Io),
                },
            };
            match crate::cm::cm_set_value(key_native, "ComputerName", 1, &hostname_bytes) {
                Ok(()) => {
                    let _ = crate::cm::cm_flush_key(key_native);
                    0
                }
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// OB-014: ObEnum — RAX=64
// ═══════════════════════════════════════════════════════════════════════

