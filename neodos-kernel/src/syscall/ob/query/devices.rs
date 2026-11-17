//! Ob query — CPU, memory, drives and drivers.

use crate::object::types::ObInfoClass;
use crate::syscall::{err_to_u64, SyscallError, copy_to_user};
use crate::syscall::ob::types::{ObDeviceInfo, DriveInfoRaw, DriverInfoRaw};

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObInfoClass::CpuInfo as u32
        || info_class == ObInfoClass::Memory as u32
        || info_class == ObInfoClass::Drives as u32
        || info_class == ObInfoClass::Drivers as u32
        || info_class == ObInfoClass::Device as u32
}

/// Dispatch the `devices` info classes.
pub(super) fn dispatch(
    info_class: u32,
    _fd: u8,
    entry: crate::handle::HandleEntry,
    buf_ptr: u64,
    buf_size: usize,
) -> u64 {
    match info_class {
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
            let bytes = unsafe {
                core::slice::from_raw_parts(&info as *const crate::cpu::CpuInfoFull as *const u8, sz)
            };
            if copy_to_user(buf_ptr, bytes).is_err() {
                return err_to_u64(SyscallError::Fault);
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
            let bytes = unsafe {
                core::slice::from_raw_parts(&stats as *const crate::memory::MemoryStats as *const u8, copy_sz)
            };
            if copy_to_user(buf_ptr, bytes).is_err() {
                return err_to_u64(SyscallError::Fault);
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
            let mut kernel_out: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
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
                        let bytes = unsafe {
                            core::slice::from_raw_parts(&raw as *const DriveInfoRaw as *const u8, entry_size)
                        };
                        kernel_out.extend_from_slice(bytes);
                        count += 1;
                    }
                }
                (count * entry_size) as u64
            });
            if copy_to_user(buf_ptr, &kernel_out).is_err() {
                return err_to_u64(SyscallError::Fault);
            }
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
                        id: d.id, state: d.state as u8, category: d.category as u8,
                        driver_type: d.driver_type as u8, api_version: d.api_version,
                        abi_min: d.abi_min, abi_target: d.abi_target, abi_max: d.abi_max,
                        last_error: d.last_error, caps: d.caps, isolation_mode: d.isolation_mode,
                        events_received: d.events_received, tick_count: d.tick_count,
                        registered_at_tick: d.registered_at_tick, name: d.name,
                    };
                    let bytes = unsafe {
                        core::slice::from_raw_parts(&raw as *const DriverInfoRaw as *const u8, entry_size)
                    };
                    if copy_to_user(buf_ptr, bytes).is_err() {
                        return err_to_u64(SyscallError::Fault);
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
            let mut out = alloc::vec::Vec::new();
            out.resize(count * entry_size_bulk, 0u8);
            for (i, &id) in ids.iter().enumerate().take(count) {
                if let Some(d) = crate::drivers::driver_runtime::get_driver(id) {
                    let raw = DriverInfoRaw {
                        id: d.id, state: d.state as u8, category: d.category as u8,
                        driver_type: d.driver_type as u8, api_version: d.api_version,
                        abi_min: d.abi_min, abi_target: d.abi_target, abi_max: d.abi_max,
                        last_error: d.last_error, caps: d.caps, isolation_mode: d.isolation_mode,
                        events_received: d.events_received, tick_count: d.tick_count,
                        registered_at_tick: d.registered_at_tick, name: d.name,
                    };
                    let bytes = unsafe {
                        core::slice::from_raw_parts(&raw as *const DriverInfoRaw as *const u8, entry_size_bulk)
                    };
                    out[i * entry_size_bulk..(i + 1) * entry_size_bulk].copy_from_slice(bytes);
                }
            }
    drop(runtime);
    if copy_to_user(buf_ptr, &out).is_err() {
        return err_to_u64(SyscallError::Fault);
    }
    (count * entry_size_bulk) as u64
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
            let bytes = unsafe {
                core::slice::from_raw_parts(&di as *const ObDeviceInfo as *const u8, sz)
            };
            if copy_to_user(buf_ptr, bytes).is_err() {
                return err_to_u64(SyscallError::Fault);
            }
            sz as u64
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}
