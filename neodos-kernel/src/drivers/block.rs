#![allow(dead_code)]

use alloc::boxed::Box;
use crate::drivers::nvme::NvmeDriver;
use crate::irp::{self, IrpId, IrpOp, IrpStatus};
use core::sync::atomic::{AtomicU64, Ordering};

pub const MAX_BLOCK_DEVICES: usize = 8;

pub struct BlockDeviceManager {
    devices: [Option<Box<dyn BlockDevice>>; MAX_BLOCK_DEVICES],
    count: usize,
    refcounts: [u32; MAX_BLOCK_DEVICES],
}

impl BlockDeviceManager {
    pub fn new() -> Self {
        let devices: [Option<Box<dyn BlockDevice>>; MAX_BLOCK_DEVICES] = Default::default();
        BlockDeviceManager { devices, count: 0, refcounts: [0; MAX_BLOCK_DEVICES] }
    }

    /// Register a device. Scans for first free slot (stable indices).
    /// Returns the device index, which is stable until explicitly removed.
    pub fn register(&mut self, dev: Box<dyn BlockDevice>) -> Option<usize> {
        for i in 0..MAX_BLOCK_DEVICES {
            if self.devices[i].is_none() {
                self.devices[i] = Some(dev);
                self.count += 1;
                self.refcounts[i] = 0;
                return Some(i);
            }
        }
        None
    }

    pub fn get(&mut self, idx: usize) -> Option<&mut (dyn BlockDevice + '_)> {
        if let Some(Some(ref mut dev)) = self.devices.get_mut(idx) {
            return Some(&mut **dev);
        }
        None
    }

    pub fn swap(&mut self, idx: usize, dev: Box<dyn BlockDevice>) -> Option<Box<dyn BlockDevice>> {
        self.devices.get_mut(idx)?.replace(dev)
    }

    /// Acquire a device reference (increments refcount).
    pub fn acquire(&mut self, idx: usize) -> bool {
        if idx < MAX_BLOCK_DEVICES && self.devices[idx].is_some() {
            self.refcounts[idx] = self.refcounts[idx].saturating_add(1);
            true
        } else {
            false
        }
    }

    /// Release a device reference (decrements refcount).
    pub fn release(&mut self, idx: usize) {
        if idx < MAX_BLOCK_DEVICES {
            self.refcounts[idx] = self.refcounts[idx].saturating_sub(1);
        }
    }

    /// Get refcount for a device.
    pub fn refcount(&self, idx: usize) -> u32 {
        if idx < MAX_BLOCK_DEVICES { self.refcounts[idx] } else { 0 }
    }

    /// Remove a block device by index. Returns the removed device if found.
    /// Fails if refcount > 0.
    pub fn remove(&mut self, idx: usize) -> Option<Box<dyn BlockDevice>> {
        if idx < MAX_BLOCK_DEVICES && self.refcounts[idx] == 0 {
            let removed = self.devices[idx].take();
            if removed.is_some() {
                self.count = self.count.saturating_sub(1);
            }
            removed
        } else {
            None
        }
    }

    /// Force-remove a device regardless of refcount.
    pub fn force_remove(&mut self, idx: usize) -> Option<Box<dyn BlockDevice>> {
        if idx < MAX_BLOCK_DEVICES {
            self.refcounts[idx] = 0;
            let removed = self.devices[idx].take();
            if removed.is_some() {
                self.count = self.count.saturating_sub(1);
            }
            removed
        } else {
            None
        }
    }

    pub fn count(&self) -> usize {
        self.count
    }

    /// Find a registered device by name. Returns its index.
    pub fn find_by_name(&self, name: &str) -> Option<usize> {
        for i in 0..MAX_BLOCK_DEVICES {
            if let Some(ref dev) = self.devices[i] {
                if dev.device_name() == name {
                    return Some(i);
                }
            }
        }
        None
    }
}

/// Block device name maximum length.
pub const MAX_DEVICE_NAME_LEN: usize = 32;

/// Block device trait with IRP-based async I/O.
///
/// The primary interface is `submit_irp()` which enqueues an I/O request
/// for asynchronous processing (the device calls `irp::irp_complete()`
/// when done). The synchronous `read_blocks`/`write_blocks` methods are
/// retained for backward compatibility; each device driver still implements
/// them directly.
pub trait BlockDevice: Send {
    fn num_sectors(&self) -> Option<u64> { None }

    fn device_name(&self) -> &str { "" }

    fn sector_size(&self) -> u32 { 512 }

    /// Submit an I/O Request Packet to this device.
    /// The IRP must have been allocated from the global pool.
    /// The device processes the IRP (possibly asynchronously) and calls
    /// `irp::irp_complete()` when done.
    fn submit_irp(&mut self, irp_id: IrpId) -> Result<(), ()>;

    /// Poll the status of a previously submitted IRP.
    /// Returns the current status without blocking.
    fn poll_irp(&mut self, irp_id: IrpId) -> IrpStatus {
        irp::irp_get_status(irp_id)
    }

    /// Synchronous read of `count` sectors starting at `lba`.
    fn read_blocks(&mut self, lba: u64, count: u8, buf: &mut [u8]) -> Result<(), ()>;

    /// Synchronous write of `count` sectors starting at `lba`.
    fn write_blocks(&mut self, lba: u64, count: u8, buf: &[u8]) -> Result<(), ()>;

    fn flush(&mut self) -> Result<(), ()> { Ok(()) }

    fn set_base_lba(&mut self, lba: u64);

    fn base_lba(&self) -> u64;

    fn read_sector(&mut self, lba: u64) -> Result<[u8; 512], ()> {
        let mut buf = [0u8; 512];
        self.read_blocks(lba, 1, &mut buf)?;
        Ok(buf)
    }

    fn write_sector(&mut self, lba: u64, data: &[u8; 512]) -> Result<(), ()> {
        self.write_blocks(lba, 1, data)
    }
}

// ── RamDisk: backed by a memory buffer loaded by the bootloader ──────

static RAM_DISK_BASE: AtomicU64 = AtomicU64::new(0);
static RAM_DISK_SIZE: AtomicU64 = AtomicU64::new(0);

pub fn set_ram_disk(base: u64, size: u64) {
    RAM_DISK_BASE.store(base, Ordering::Relaxed);
    RAM_DISK_SIZE.store(size, Ordering::Relaxed);
}

fn ram_disk_buf() -> Option<&'static [u8]> {
    let base = RAM_DISK_BASE.load(Ordering::Relaxed);
    let size = RAM_DISK_SIZE.load(Ordering::Relaxed) as usize;
    if base != 0 && size >= 512 {
        unsafe { Some(core::slice::from_raw_parts(base as *const u8, size)) }
    } else {
        None
    }
}

pub struct RamDisk;

impl RamDisk {
    pub fn available() -> bool {
        ram_disk_buf().is_some()
    }
}

impl BlockDevice for RamDisk {
    fn submit_irp(&mut self, irp_id: IrpId) -> Result<(), ()> {
        let params = irp::irp_get_params(irp_id).ok_or(())?;
        match params.op {
            IrpOp::Read => {
                let ram = ram_disk_buf().ok_or(())?;
                let offset = (params.lba as usize) * 512;
                let len = (params.count as usize) * 512;
                if offset + len <= ram.len() && params.buf_len >= len {
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            ram.as_ptr().add(offset),
                            params.buf,
                            len,
                        );
                    }
                    irp::irp_complete_result(irp_id, Ok(()));
                } else {
                    irp::irp_complete_result(irp_id, Err(()));
                }
            }
            IrpOp::Write => {
                irp::irp_complete_result(irp_id, Err(()));
            }
            _ => {
                irp::irp_complete_result(irp_id, Ok(()));
            }
        }
        Ok(())
    }

    fn read_blocks(&mut self, lba: u64, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        let ram = ram_disk_buf().ok_or(())?;
        let offset = (lba as usize) * 512;
        let len = (count as usize) * 512;
        if offset + len <= ram.len() && buf.len() >= len {
            buf[..len].copy_from_slice(&ram[offset..offset + len]);
            Ok(())
        } else {
            Err(())
        }
    }

    fn write_blocks(&mut self, _lba: u64, _count: u8, _buf: &[u8]) -> Result<(), ()> {
        Err(())
    }

    fn set_base_lba(&mut self, _lba: u64) {}
    fn base_lba(&self) -> u64 { 0 }
}

// ── Direct BlockDevice implementations ──────────────────────────────

impl BlockDevice for crate::drivers::ata::BootAta {
    fn submit_irp(&mut self, irp_id: IrpId) -> Result<(), ()> {
        let params = crate::irp::irp_get_params(irp_id).ok_or(())?;
        match params.op {
            IrpOp::Read => {
                let buf = unsafe { core::slice::from_raw_parts_mut(params.buf, params.buf_len) };
                let result = self.read_blocks(params.lba, params.count, buf);
                crate::irp::irp_complete_result(irp_id, result);
            }
            IrpOp::Write => {
                let buf = unsafe { core::slice::from_raw_parts(params.buf as *const u8, params.buf_len) };
                let result = self.write_blocks(params.lba, params.count, buf);
                crate::irp::irp_complete_result(irp_id, result);
            }
            _ => crate::irp::irp_complete_result(irp_id, Ok(())),
        }
        Ok(())
    }

    fn read_blocks(&mut self, lba: u64, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        self.read_blocks(lba, count, buf)
    }

    fn write_blocks(&mut self, lba: u64, count: u8, buf: &[u8]) -> Result<(), ()> {
        self.write_blocks(lba, count, buf)
    }

    fn set_base_lba(&mut self, lba: u64) {
        self.set_base_lba(lba);
    }

    fn base_lba(&self) -> u64 {
        self.base_lba()
    }

    fn read_sector(&mut self, lba: u64) -> Result<[u8; 512], ()> {
        self.read_sector(lba)
    }

    fn write_sector(&mut self, lba: u64, data: &[u8; 512]) -> Result<(), ()> {
        self.write_sector(lba, data)
    }
}

impl BlockDevice for NvmeDriver {
    fn submit_irp(&mut self, irp_id: IrpId) -> Result<(), ()> {
        let params = crate::irp::irp_get_params(irp_id).ok_or(())?;
        match params.op {
            IrpOp::Read => {
                let buf = unsafe { core::slice::from_raw_parts_mut(params.buf, params.buf_len) };
                let result = self.read_sectors(params.lba, params.count, buf);
                crate::irp::irp_complete_result(irp_id, result.map_err(|_| ()));
            }
            IrpOp::Write => {
                let buf = unsafe { core::slice::from_raw_parts(params.buf as *const u8, params.buf_len) };
                let result = self.write_sectors(params.lba, params.count, buf);
                crate::irp::irp_complete_result(irp_id, result.map_err(|_| ()));
            }
            _ => crate::irp::irp_complete_result(irp_id, Ok(())),
        }
        Ok(())
    }

    fn read_blocks(&mut self, lba: u64, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        self.read_sectors(lba, count, buf)
    }

    fn write_blocks(&mut self, lba: u64, count: u8, buf: &[u8]) -> Result<(), ()> {
        self.write_sectors(lba, count, buf)
    }

    fn set_base_lba(&mut self, lba: u64) {
        NvmeDriver::set_base_lba(self, lba as u32);
    }

    fn base_lba(&self) -> u64 {
        NvmeDriver::base_lba(self) as u64
    }

    fn read_sector(&mut self, lba: u64) -> Result<[u8; 512], ()> {
        NvmeDriver::read_sector(self, lba as u32)
    }

    fn write_sector(&mut self, lba: u64, data: &[u8; 512]) -> Result<(), ()> {
        NvmeDriver::write_sector(self, lba as u32, data)
    }
}

// ── NEM block device registry ──
// Allows standalone NEM drivers to register block devices with the kernel.
// These are stored separately from the built-in BlockDeviceManager.

const MAX_NEM_BLOCK_DEVICES: usize = 4;

type NemBlockReadFn = unsafe extern "C" fn(u32, u64, u8, *mut u8) -> i32;
type NemBlockWriteFn = unsafe extern "C" fn(u32, u64, u8, *const u8) -> i32;

pub struct NemBlockDevice {
    pub device_id: u32,
    pub num_sectors: u64,
    pub sector_size: u32,
    pub read_fn: NemBlockReadFn,
    pub write_fn: NemBlockWriteFn,
    pub base_lba: u64,
}

// Safety: NemBlockDevice only contains function pointers and plain data; Send is safe.
unsafe impl Send for NemBlockDevice {}

impl NemBlockDevice {
    pub fn new(
        device_id: u32,
        num_sectors: u64,
        sector_size: u32,
        read_fn: NemBlockReadFn,
        write_fn: NemBlockWriteFn,
    ) -> Self {
        NemBlockDevice {
            device_id,
            num_sectors,
            sector_size,
            read_fn,
            write_fn,
            base_lba: 0,
        }
    }
}

impl BlockDevice for NemBlockDevice {
    fn num_sectors(&self) -> Option<u64> {
        Some(self.num_sectors)
    }

    fn sector_size(&self) -> u32 {
        self.sector_size
    }

    fn submit_irp(&mut self, irp_id: IrpId) -> Result<(), ()> {
        let params = crate::irp::irp_get_params(irp_id).ok_or(())?;
        match params.op {
            IrpOp::Read => {
                let abs_lba = self.base_lba.wrapping_add(params.lba);
                let rc = unsafe { (self.read_fn)(self.device_id, abs_lba, params.count, params.buf) };
                crate::irp::irp_complete_result(irp_id, if rc == 0 { Ok(()) } else { Err(()) });
            }
            IrpOp::Write => {
                let abs_lba = self.base_lba.wrapping_add(params.lba);
                let rc = unsafe { (self.write_fn)(self.device_id, abs_lba, params.count, params.buf as *const u8) };
                crate::irp::irp_complete_result(irp_id, if rc == 0 { Ok(()) } else { Err(()) });
            }
            _ => crate::irp::irp_complete_result(irp_id, Ok(())),
        }
        Ok(())
    }

    fn read_blocks(&mut self, lba: u64, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        let abs_lba = self.base_lba.wrapping_add(lba);
        let rc = unsafe { (self.read_fn)(self.device_id, abs_lba, count, buf.as_mut_ptr()) };
        if rc == 0 { Ok(()) } else { Err(()) }
    }

    fn write_blocks(&mut self, lba: u64, count: u8, buf: &[u8]) -> Result<(), ()> {
        let abs_lba = self.base_lba.wrapping_add(lba);
        let rc = unsafe { (self.write_fn)(self.device_id, abs_lba, count, buf.as_ptr()) };
        if rc == 0 { Ok(()) } else { Err(()) }
    }

    fn set_base_lba(&mut self, lba: u64) {
        self.base_lba = lba;
    }

    fn base_lba(&self) -> u64 {
        self.base_lba
    }

    fn read_sector(&mut self, lba: u64) -> Result<[u8; 512], ()> {
        let mut buf = [0u8; 512];
        self.read_blocks(lba, 1, &mut buf)?;
        Ok(buf)
    }

    fn write_sector(&mut self, lba: u64, data: &[u8; 512]) -> Result<(), ()> {
        self.write_blocks(lba, 1, data)
    }
}

/// Register a NemBlockDevice by adding it to the global BlockDeviceManager.
pub fn register_nem_block_device(dev: NemBlockDevice) -> i32 {
    let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
    match bdevs.register(alloc::boxed::Box::new(dev)) {
        Some(idx) => {
            crate::serial_println!("[BLK] NEM block device registered at idx={}", idx);
            idx as i32
        }
        None => -1,
    }
}

/// Unregister a NemBlockDevice by its index.
/// Only succeeds if the device's refcount is 0 (no active IoStacks).
pub fn unregister_nem_block_device(idx: usize) -> bool {
    let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
    if bdevs.refcount(idx) > 0 {
        crate::serial_println!("[BLK] Cannot unregister device at idx={}: refcount={}", idx, bdevs.refcount(idx));
        return false;
    }
    if bdevs.remove(idx).is_some() {
        crate::serial_println!("[BLK] NEM block device unregistered at idx={}", idx);
        true
    } else {
        false
    }
}

/// Force-unregister a NemBlockDevice, bypassing refcount check.
/// Used by hot-unload with /F flag.
pub fn force_unregister_nem_block_device(idx: usize) -> bool {
    let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
    if bdevs.force_remove(idx).is_some() {
        crate::serial_println!("[BLK] NEM block device FORCE unregistered at idx={}", idx);
        true
    } else {
        false
    }
}
