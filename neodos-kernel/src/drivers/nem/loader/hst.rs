use crate::eventbus;
use crate::input;
use crate::hal;
use crate::drivers::caps::{CAP_IRQ, CAP_PORTIO, CAP_EVENT_BUS, CAP_INPUT, CAP_TIMING, CAP_LOG, CAP_MMIO, CAP_BLOCK_DEVICE, CAP_DMA};
use x86_64::structures::paging::PageTableFlags;
use crate::drivers::driver_runtime;
use crate::drivers::nem::driver::current_driver_id;
use crate::drivers::isolation;
use crate::log::LogSubsys;

pub type HstInb = unsafe extern "C" fn(u16) -> u8;
pub type HstOutb = unsafe extern "C" fn(u16, u8);
pub type HstInw = unsafe extern "C" fn(u16) -> u16;
pub type HstOutw = unsafe extern "C" fn(u16, u16);
pub type HstInl = unsafe extern "C" fn(u16) -> u32;
pub type HstOutl = unsafe extern "C" fn(u16, u32);
pub type HstPushEvent = unsafe extern "C" fn(u32, u32, u32, u64, u64, u32) -> i64;
pub type HstPushInput = unsafe extern "C" fn(u8);
pub type HstGetTicks = unsafe extern "C" fn() -> u64;
pub type HstAckIrq = unsafe extern "C" fn(u8);
pub type HstLog = unsafe extern "C" fn(u32, *const u8, usize);

#[repr(C)]
pub struct HalServiceTable {
    pub inb: HstInb,
    pub outb: HstOutb,
    pub inw: HstInw,
    pub outw: HstOutw,
    pub inl: HstInl,
    pub outl: HstOutl,
    pub push_event: HstPushEvent,
    pub push_input_byte: HstPushInput,
    pub get_ticks: HstGetTicks,
    pub ack_irq: HstAckIrq,
    pub log: HstLog,
}

/// Check that the current driver has the required capability.
/// Returns true if the capability is held or no driver context is set (kernel code).
fn check_cap(required: u64) -> bool {
    let id = current_driver_id();
    if id == 0 {
        return true; // kernel context — always allowed
    }
    driver_runtime::check_driver_cap(id, required).is_ok()
}

pub unsafe extern "C" fn hst_inb(port: u16) -> u8 {
    if !check_cap(CAP_PORTIO) { return 0; }
    hal::inb(port)
}
pub unsafe extern "C" fn hst_outb(port: u16, val: u8) {
    if !check_cap(CAP_PORTIO) { return; }
    hal::outb(port, val)
}
pub unsafe extern "C" fn hst_inw(port: u16) -> u16 {
    if !check_cap(CAP_PORTIO) { return 0; }
    hal::inw(port)
}
pub unsafe extern "C" fn hst_outw(port: u16, val: u16) {
    if !check_cap(CAP_PORTIO) { return; }
    hal::outw(port, val)
}
pub unsafe extern "C" fn hst_inl(port: u16) -> u32 {
    if !check_cap(CAP_PORTIO) { return 0; }
    hal::inl(port)
}
pub unsafe extern "C" fn hst_outl(port: u16, val: u32) {
    if !check_cap(CAP_PORTIO) { return; }
    hal::outl(port, val)
}
pub unsafe extern "C" fn hst_push_event(et: u32, src: u32, dev: u32, d0: u64, d1: u64, fl: u32) -> i64 {
    if !check_cap(CAP_EVENT_BUS) { return -1; }
    match eventbus::push_event(et, src, dev, d0, d1, fl) {
        Ok(id) => id as i64,
        Err(_) => -1,
    }
}
pub unsafe extern "C" fn hst_push_input_byte(byte: u8) {
    if !check_cap(CAP_INPUT) { return; }
    let _ = input::push_byte(byte);
    crate::syscall::wake_blocked_readers();
}
pub unsafe extern "C" fn hst_get_ticks() -> u64 {
    if !check_cap(CAP_TIMING) { return 0; }
    hal::get_ticks()
}
pub unsafe extern "C" fn hst_ack_irq(vec: u8) {
    if !check_cap(CAP_IRQ) { return; }
    hal::ack_irq(vec);
}
pub unsafe extern "C" fn hst_log(_level: u32, msg: *const u8, len: usize) {
    if !check_cap(CAP_LOG) { return; }
    // X4: Validate driver pointer before dereferencing
    let driver_id = current_driver_id();
    if driver_id != 0
        && isolation::validate_export_ptr(msg, len, false).is_err()
    {
        kerror!(LogSubsys::Isolation, "DENIED: hst_log with invalid pointer from driver {}", driver_id);
        return;
    }
    let s = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(msg, len)) };
    kdebug!(LogSubsys::Driver, "{}", s);
}

pub unsafe extern "C" fn hst_ecam_is_active() -> u64 {
    if hal::pci::ecam_is_active() { 1 } else { 0 }
}

pub unsafe extern "C" fn hst_ecam_read_dword(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    if !check_cap(CAP_MMIO) { return 0xFFFFFFFF; }
    if hal::pci::ecam_is_active() {
        hal::pci::ecam_read_config_dword(bus, dev, func, offset)
    } else {
        0xFFFFFFFF
    }
}

pub unsafe extern "C" fn hst_ecam_write_dword(bus: u8, dev: u8, func: u8, offset: u8, value: u32) {
    if !check_cap(CAP_MMIO) { return; }
    if hal::pci::ecam_is_active() {
        hal::pci::ecam_write_config_dword(bus, dev, func, offset, value);
    }
}

pub unsafe extern "C" fn hst_virt_to_phys(virt: u64) -> u64 {
    if !check_cap(CAP_DMA) { return 0; }
    match crate::hal::walk_ptes_4k(virt) {
        Some(pte) => {
            if pte.flags().contains(PageTableFlags::PRESENT) {
                let phys_base = pte.addr().as_u64();
                phys_base | (virt & 0xFFF)
            } else {
                0
            }
        }
        None => 0,
    }
}

pub unsafe extern "C" fn hst_register_block_device(
    name: *const u8,
    name_len: u32,
    device_id: u32,
    num_sectors: u64,
    sector_size: u32,
    read_fn: unsafe extern "C" fn(u32, u64, u8, *mut u8) -> i32,
    write_fn: unsafe extern "C" fn(u32, u64, u8, *const u8) -> i32,
) -> i32 {
    if !check_cap(CAP_BLOCK_DEVICE) { return -1; }
    // X4: Validate name pointer
    if current_driver_id() != 0
        && isolation::validate_export_ptr(name, name_len as usize, false).is_err()
    {
        kerror!(LogSubsys::Isolation, "DENIED: hst_register_block_device with invalid name from driver {}", current_driver_id());
        return -1;
    }
    let dev = crate::drivers::block::NemBlockDevice::new(
        device_id, num_sectors, sector_size, read_fn, write_fn,
    );
    let idx = crate::drivers::block::register_nem_block_device(dev);
    if idx >= 0 {
        let driver_id = current_driver_id();
        if driver_id != 0 {
            crate::drivers::hotreload::track_resource(
                driver_id,
                crate::drivers::hotreload::ResourceType::BlockDevice,
                idx as u32,
            );
        }
    }
    idx
}

pub unsafe extern "C" fn hst_unregister_block_device(dev_idx: i32) -> i32 {
    if !check_cap(CAP_BLOCK_DEVICE) { return -1; }
    if dev_idx < 0 { return -1; }
    let driver_id = current_driver_id();
    if driver_id != 0 {
        crate::drivers::hotreload::untrack_resource(
            driver_id,
            crate::drivers::hotreload::ResourceType::BlockDevice,
            dev_idx as u32,
        );
    }
    crate::drivers::block::unregister_nem_block_device(dev_idx as usize);
    0
}

pub fn build_hst() -> HalServiceTable {
    HalServiceTable {
        inb: hst_inb,
        outb: hst_outb,
        inw: hst_inw,
        outw: hst_outw,
        inl: hst_inl,
        outl: hst_outl,
        push_event: hst_push_event,
        push_input_byte: hst_push_input_byte,
        get_ticks: hst_get_ticks,
        ack_irq: hst_ack_irq,
        log: hst_log,
    }
}
