//! Ob query — kernel CSPRNG reads (`\Device\Random`, #712/M8).

use crate::object::types::ObInfoClass;
use crate::syscall::{copy_to_user, err_to_u64, SyscallError};

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObInfoClass::Random as u32 || info_class == ObInfoClass::RandomCaps as u32
}

/// Reject queries aimed at anything other than `\Device\Random`.
fn is_random_device(entry: crate::handle::HandleEntry) -> bool {
    match crate::object::ob_lookup(entry.object_id) {
        Some(obj) => {
            obj.obj_type == crate::object::ObType::Device
                && obj.native_id == crate::random::DEVICE_NATIVE_ID
        }
        None => false,
    }
}

pub(super) fn dispatch(
    info_class: u32,
    _fd: u8,
    entry: crate::handle::HandleEntry,
    buf_ptr: u64,
    buf_size: usize,
) -> u64 {
    if !is_random_device(entry) {
        return err_to_u64(SyscallError::Inval);
    }

    // ── Random (48): fill the buffer, return the byte count ──
    if info_class == ObInfoClass::Random as u32 {
        if buf_size > crate::random::MAX_READ {
            return err_to_u64(SyscallError::Inval);
        }
        let mut chunk = [0u8; 256];
        let mut written = 0usize;
        while written < buf_size {
            let take = (buf_size - written).min(chunk.len());
            if crate::random::fill(&mut chunk[..take]).is_err() {
                return err_to_u64(SyscallError::Io);
            }
            if copy_to_user(buf_ptr + written as u64, &chunk[..take]).is_err() {
                return err_to_u64(SyscallError::Fault);
            }
            written += take;
        }
        return buf_size as u64;
    }

    // ── RandomCaps (49): 4-byte capability record ──
    #[repr(C)]
    struct SysRandomCaps {
        hw_rdrand: u8,
        hw_rdseed: u8,
        pool_ready: u8,
        _pad: u8,
    }
    let caps = crate::random::caps();
    let info = SysRandomCaps {
        hw_rdrand: caps.hw_rdrand,
        hw_rdseed: caps.hw_rdseed,
        pool_ready: caps.pool_ready,
        _pad: 0,
    };
    let sz = core::mem::size_of::<SysRandomCaps>();
    if buf_size < sz {
        return err_to_u64(SyscallError::Inval);
    }
    let bytes =
        unsafe { core::slice::from_raw_parts(&info as *const SysRandomCaps as *const u8, sz) };
    if copy_to_user(buf_ptr, bytes).is_err() {
        return err_to_u64(SyscallError::Fault);
    }
    sz as u64
}
