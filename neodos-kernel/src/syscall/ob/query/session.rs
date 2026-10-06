//! Ob query — basic/name/pipe/version/keyboard/service classes.

use crate::object::types::ObInfoClass;
use crate::syscall::{err_to_u64, SyscallError};
use crate::syscall::ob::types::{ObBasicInfo, ObPipeInfo};

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObInfoClass::Basic as u32
        || info_class == ObInfoClass::Name as u32
        || info_class == ObInfoClass::Pipe as u32
        || info_class == ObInfoClass::Version as u32
        || info_class == ObInfoClass::KeyboardLayout as u32
        || info_class == ObInfoClass::KeyboardInfo as u32
        || info_class == ObInfoClass::KeyboardCaps as u32
        || info_class == ObInfoClass::KeyboardLayouts as u32
        || info_class == ObInfoClass::ServiceState as u32
        || info_class == ObInfoClass::ServiceConfig as u32
        || info_class == ObInfoClass::ServiceStatus as u32
}

/// Dispatch the `session` info classes.
pub(super) fn dispatch(
    info_class: u32,
    _fd: u8,
    entry: crate::handle::HandleEntry,
    buf_ptr: u64,
    buf_size: usize,
) -> u64 {
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
                read_refs,
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
            let copy_len = ver.len().min(buf_size);
            unsafe {
                core::ptr::copy_nonoverlapping(ver.as_ptr(), buf_ptr as *mut u8, copy_len);
            }
            ver.len() as u64
        }
        _ if info_class == ObInfoClass::KeyboardLayout as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type == crate::object::ObType::KeyboardDevice {
                if buf_size < 1 { return err_to_u64(SyscallError::Inval); }
                let kbd = crate::kbd::KBD.lock();
                unsafe { core::ptr::write_volatile(buf_ptr as *mut u8, kbd.state.active_layout_index as u8); }
                return 1u64;
            }
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 9 {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 1 { return err_to_u64(SyscallError::Inval); }
            let kbd = crate::kbd::KBD.lock();
            unsafe { core::ptr::write_volatile(buf_ptr as *mut u8, kbd.state.active_layout_index as u8); }
            1u64
        }
        _ if info_class == ObInfoClass::KeyboardInfo as u32 => {
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::KeyboardDevice {
                return err_to_u64(SyscallError::Inval);
            }
            let sz = core::mem::size_of::<crate::kbd::KbdState>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            let kbd = crate::kbd::KBD.lock();
            let state = crate::kbd::KbdState {
                modifiers: kbd.state.modifiers,
                leds: kbd.state.leds,
                active_layout_index: kbd.state.active_layout_index,
            };
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &state as *const crate::kbd::KbdState as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::KeyboardCaps as u32 => {
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::KeyboardDevice {
                return err_to_u64(SyscallError::Inval);
            }
            let sz = core::mem::size_of::<crate::kbd::KbdCaps>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            let kbd = crate::kbd::KBD.lock();
            let caps = crate::kbd::KbdCaps {
                max_layouts: 64,
                supports_repeat_config: true,
                supports_led_control: true,
                supports_hotkeys: true,
                num_layouts: kbd.layouts.len() as u32,
                _pad: [0u8; 3],
            };
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &caps as *const crate::kbd::KbdCaps as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::KeyboardLayouts as u32 => {
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::KeyboardDevice {
                return err_to_u64(SyscallError::Inval);
            }
            let kbd = crate::kbd::KBD.lock();
            let entry_sz = core::mem::size_of::<crate::kbd::KbdLayoutInfo>();
            let max_entries = buf_size / entry_sz;
            let count = kbd.layouts.len().min(max_entries);
            for i in 0..count {
                let info = kbd.layouts[i].to_info(i as u32);
                let offset = i * entry_sz;
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &info as *const crate::kbd::KbdLayoutInfo as *const u8,
                        (buf_ptr + offset as u64) as *mut u8, entry_sz,
                    );
                }
            }
            (count * entry_sz) as u64
        }
        _ if info_class == ObInfoClass::ServiceState as u32 => {
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
            let sm = crate::services::SERVICE_MANAGER.lock();
            let idx = match sm.find_by_obj_id(entry.object_id) {
                Some(i) => i,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            let svc = &sm.services[idx];
            let state_bytes = [svc.state as u8];
            let pid_bytes = svc.pid.to_le_bytes();
            let tick_bytes = svc.start_tick.to_le_bytes();
            let out: [u8; 13] = [
                state_bytes[0],
                pid_bytes[0], pid_bytes[1], pid_bytes[2], pid_bytes[3],
                tick_bytes[0], tick_bytes[1], tick_bytes[2], tick_bytes[3],
                tick_bytes[4], tick_bytes[5], tick_bytes[6], tick_bytes[7],
            ];
            let sz = out.len();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(out.as_ptr(), buf_ptr as *mut u8, sz);
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::ServiceConfig as u32 => {
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
            let sm = crate::services::SERVICE_MANAGER.lock();
            let idx = match sm.find_by_obj_id(entry.object_id) {
                Some(i) => i,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            let svc = &sm.services[idx];
            let st = svc.start_type as u8;
            let rp = svc.restart_policy as u8;
            let mf = svc.max_failures.to_le_bytes();
            let mut display = [0u8; 128];
            let dn_bytes = svc.display_name.as_bytes();
            let dn_len = dn_bytes.len().min(127);
            display[..dn_len].copy_from_slice(&dn_bytes[..dn_len]);
            let mut binpath = [0u8; 256];
            let bp_bytes = svc.binary_path.as_bytes();
            let bp_len = bp_bytes.len().min(255);
            binpath[..bp_len].copy_from_slice(&bp_bytes[..bp_len]);
            let mut out = alloc::vec::Vec::with_capacity(394);
            out.push(st);
            out.push(rp);
            out.extend_from_slice(&mf);
            out.extend_from_slice(&display);
            out.extend_from_slice(&binpath);
            let sz = out.len();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(out.as_ptr(), buf_ptr as *mut u8, sz);
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::ServiceStatus as u32 => {
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
            let sm = crate::services::SERVICE_MANAGER.lock();
            let idx = match sm.find_by_obj_id(entry.object_id) {
                Some(i) => i,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            let svc = &sm.services[idx];
            let state = [svc.state as u8];
            let pid = svc.pid.to_le_bytes();
            let ecnt = svc.exit_count.to_le_bytes();
            let lec = svc.last_exit_code.to_le_bytes();
            let fc = svc.failure_count.to_le_bytes();
            let tick = svc.start_tick.to_le_bytes();
            let mut out = [0u8; 29];
            out[0] = state[0];
            out[1..5].copy_from_slice(&pid);
            out[5..9].copy_from_slice(&ecnt);
            out[9..17].copy_from_slice(&lec);
            out[17..21].copy_from_slice(&fc);
            out[21..29].copy_from_slice(&tick);
            let sz = out.len();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(out.as_ptr(), buf_ptr as *mut u8, sz);
            }
            sz as u64
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}
