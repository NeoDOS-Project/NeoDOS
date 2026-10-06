//! Ob set — keyboard layout, repeat and LED configuration.

use crate::object::types::ObSetInfoClass;
use crate::syscall::{err_to_u64, SyscallError};
use alloc::string::ToString;

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObSetInfoClass::KeyboardLayout as u32
        || info_class == ObSetInfoClass::KeyboardSetLayout as u32
        || info_class == ObSetInfoClass::KeyboardSetRepeatDelay as u32
        || info_class == ObSetInfoClass::KeyboardSetRepeatRate as u32
        || info_class == ObSetInfoClass::KeyboardSetLeds as u32
        || info_class == ObSetInfoClass::KeyboardSetModifier as u32
}

/// Dispatch the `keyboard` info classes.
pub(super) fn dispatch(
    info_class: u32,
    _fd: u8,
    entry: crate::handle::HandleEntry,
    buf_ptr: u64,
    buf_size: usize,
) -> u64 {
    match info_class {
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
        _ => err_to_u64(SyscallError::Inval),
    }
}
