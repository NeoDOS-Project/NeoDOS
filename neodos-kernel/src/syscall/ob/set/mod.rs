//! Ob set — dispatcher facade (split by domain).
//!
//! `handler_ob_set_info` keeps the original validation prologue (including the
//! SetNicIp tracing) and routes each `ObSetInfoClass` to its owning domain.

use crate::log::LogSubsys;
use crate::object::types::ObSetInfoClass;
use crate::syscall::{current_handle_entry, err_to_u64, SyscallError};
use crate::syscall::util::is_user_ptr_valid;

mod process;
mod keyboard;
mod fs;
mod ipc;
mod net;
mod registry;
mod service;
mod power;
mod object;
mod time;

pub fn handler_ob_set_info(regs: crate::syscall::Registers) -> u64 {
    let fd = regs.rbx as u8;
    let info_class = regs.rcx as u32;
    let buf_ptr = regs.rdx;
    let buf_size = regs.r8 as usize;

    if info_class == ObSetInfoClass::SetNicIp as u32 {
        kdebug!(LogSubsys::Object, "ObSetInfo SetNicIp: fd={} class={} buf_ptr=0x{:x} buf_size={}", fd, info_class, buf_ptr, buf_size);
    }

    // Classes that carry no payload accept a null/empty buffer.
    if info_class != (ObSetInfoClass::FileDelete as u32)
        && info_class != (ObSetInfoClass::SetForegroundProcess as u32)
    {
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
        _ if process::handles(info_class) => process::dispatch(info_class, fd, entry, buf_ptr, buf_size),
        _ if keyboard::handles(info_class) => keyboard::dispatch(info_class, fd, entry, buf_ptr, buf_size),
        _ if fs::handles(info_class) => fs::dispatch(info_class, fd, entry, buf_ptr, buf_size),
        _ if ipc::handles(info_class) => ipc::dispatch(info_class, fd, entry, buf_ptr, buf_size),
        _ if net::handles(info_class) => net::dispatch(info_class, fd, entry, buf_ptr, buf_size),
        _ if registry::handles(info_class) => registry::dispatch(info_class, fd, entry, buf_ptr, buf_size),
        _ if service::handles(info_class) => service::dispatch(info_class, fd, entry, buf_ptr, buf_size),
        _ if power::handles(info_class) => power::dispatch(info_class, fd, entry, buf_ptr, buf_size),
        _ if object::handles(info_class) => object::dispatch(info_class, fd, entry, buf_ptr, buf_size),
        _ if time::handles(info_class) => time::dispatch(info_class, fd, entry, buf_ptr, buf_size),
        _ => err_to_u64(SyscallError::Inval),
    }
}

/// Validate the field ranges of an `ObSetInfoClass::DateTime` payload.
///
/// `year` is a two-digit Gregorian year (0–99, interpreted as 2000–2099), so
/// every year divisible by 4 in range is a leap year.
pub fn validate_datetime(second: u8, minute: u8, hour: u8, day: u8, month: u8, year: u8) -> bool {
    if month < 1 || month > 12 {
        return false;
    }
    if day < 1 || day > 31 {
        return false;
    }
    if hour > 23 || minute > 59 || second > 60 {
        return false;
    }
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year % 4 == 0 {
                29
            } else {
                28
            }
        }
        _ => return false,
    };
    day <= max_day
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

pub fn register_ob_set_tests() {
    use crate::{test_case, test_true};

    test_case!("ob_set_datetime_accepts_valid", {
        test_true!(validate_datetime(0, 0, 0, 1, 1, 0));
        test_true!(validate_datetime(59, 59, 23, 29, 2, 24)); // 2024-02-29
        test_true!(validate_datetime(60, 0, 12, 31, 12, 99)); // leap second
    });

    test_case!("ob_set_datetime_rejects_invalid", {
        test_true!(!validate_datetime(0, 0, 24, 1, 1, 0)); // hour
        test_true!(!validate_datetime(0, 60, 0, 1, 1, 0)); // minute
        test_true!(!validate_datetime(0, 0, 0, 0, 1, 0)); // day 0
        test_true!(!validate_datetime(0, 0, 0, 1, 0, 0)); // month 0
        test_true!(!validate_datetime(0, 0, 0, 1, 13, 0)); // month 13
        test_true!(!validate_datetime(0, 0, 0, 30, 2, 24)); // Feb 30
        test_true!(!validate_datetime(0, 0, 0, 29, 2, 23)); // 2023 not leap
        test_true!(!validate_datetime(0, 0, 0, 31, 4, 24)); // Apr 31
    });

    // #491: the RTC-write ACK guard lives in `time` (the module that already
    // owns the `rtc_bridge` dependency).
    time::register_tests();
}
// ═══════════════════════════════════════════════════════════════════════
// OB-014: ObEnum — RAX=64
// ═══════════════════════════════════════════════════════════════════════
