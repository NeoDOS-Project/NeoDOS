#![no_std]
#![no_main]
#![allow(dead_code)]

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU8, Ordering};

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}

#[repr(C)]
pub struct NeoEvent {
    pub event_id: u64,
    pub event_type: u32,
    pub source: u32,
    pub timestamp: u64,
    pub device_id: u32,
    pub driver_target: u32,
    pub data0: u64,
    pub data1: u64,
    pub flags: u32,
}

extern "C" {
    fn hst_outl(port: u16, val: u32);
    fn hst_inl(port: u16) -> u32;
    fn hst_log(level: u32, msg: *const u8, len: usize);
    fn hst_push_event(et: u32, src: u32, dev: u32, d0: u64, d1: u64, fl: u32) -> i64;
}

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

const EVENT_PCI_READ_CONFIG: u32 = 0x1000;
const EVENT_PCI_WRITE_CONFIG: u32 = 0x1001;
const EVENT_PCI_READ_RESULT: u32 = 0x1002;
const EVENT_PCI_WRITE_DONE: u32 = 0x1003;
/// Kernel requests MSI configuration for a device.
/// data0[63:32] = vector, data0[31:0] = packed BDF (bus<<16|dev<<11|func<<8)
/// data1[7:0]   = cap_offset (from the PCI capability list)
const EVENT_MSI_CONFIGURE: u32 = 0x1010;
const EVENT_MSI_CONFIGURED: u32 = 0x1011;
const SOURCE_DRIVER: u32 = 1;

static INITIALIZED: AtomicU8 = AtomicU8::new(0);
static ACTIVE: AtomicU8 = AtomicU8::new(0);

fn pci_config_read_dword(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let addr = 0x8000_0000u32
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC);
    unsafe {
        hst_outl(CONFIG_ADDRESS, addr);
        hst_inl(CONFIG_DATA)
    }
}

fn pci_config_read_word(bus: u8, dev: u8, func: u8, offset: u8) -> u16 {
    let dword = pci_config_read_dword(bus, dev, func, offset);
    ((dword >> ((offset & 3) * 8)) & 0xFFFF) as u16
}

fn pci_config_write_dword(bus: u8, dev: u8, func: u8, offset: u8, value: u32) {
    let addr = 0x8000_0000u32
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC);
    unsafe {
        hst_outl(CONFIG_ADDRESS, addr);
        hst_outl(CONFIG_DATA, value);
    }
}

fn pci_config_write_word(bus: u8, dev: u8, func: u8, offset: u8, value: u16) {
    let aligned = offset & 0xFC;
    let dword = pci_config_read_dword(bus, dev, func, aligned);
    let shift = (offset & 3) * 8;
    let mask = !(0xFFFFu32 << shift);
    let new_dword = (dword & mask) | ((value as u32) << shift);
    pci_config_write_dword(bus, dev, func, aligned, new_dword);
}

fn hex_nibble(v: u8) -> u8 {
    if v < 10 { b'0' + v } else { b'A' + v - 10 }
}

unsafe fn log_dev(bus: u8, dev: u8, func: u8,
                   vendor: u16, device: u16,
                   class: u8, subclass: u8, prog_if: u8, rev: u8,
                   name: *const u8, name_len: usize)
{
    let h = hex_nibble;
    let prefix: &[u8] = &[
        b' ', b' ', b' ',
        h((bus >> 4) & 0xF), h(bus & 0xF),
        b':', h((dev >> 4) & 0xF), h(dev & 0xF),
        b'.', func + b'0',
        b' ', b'[',
        h(((vendor >> 12) & 0xF) as u8), h(((vendor >> 8) & 0xF) as u8),
        h(((vendor >> 4) & 0xF) as u8), h((vendor & 0xF) as u8),
        b':',
        h(((device >> 12) & 0xF) as u8), h(((device >> 8) & 0xF) as u8),
        h(((device >> 4) & 0xF) as u8), h((device & 0xF) as u8),
        b']',
        b' ', h((class >> 4) & 0xF), h(class & 0xF),
        b'.', h((subclass >> 4) & 0xF), h(subclass & 0xF),
        b'.', h((prog_if >> 4) & 0xF), h(prog_if & 0xF),
        b' ', b'(', b'r', b'e', b'v', b' ',
        h((rev >> 4) & 0xF), h(rev & 0xF),
        b')', b' ', b'-', b' ',
    ];
    let mut buf = [0u8; 64];
    let bp = buf.as_mut_ptr();
    let mut pos = 0usize;
    let pl = prefix.len();
    while pos < pl {
        unsafe { core::ptr::write(bp.add(pos), prefix.as_ptr().add(pos).read()) };
        pos += 1;
    }
    let mut i = 0usize;
    while i < name_len && pos < 62 {
        unsafe { core::ptr::write(bp.add(pos), name.add(i).read()) };
        pos += 1;
        i += 1;
    }
    unsafe {
        core::ptr::write(bp.add(pos), b'\r');
        pos += 1;
        core::ptr::write(bp.add(pos), b'\n');
        pos += 1;
        hst_log(2, bp, pos);
    }
}

fn find_capability(bus: u8, dev: u8, func: u8, cap_id: u8) -> Option<u8> {
    // Check Status.CapabilitiesList (bit 4 of offset 0x06)
    let status = pci_config_read_word(bus, dev, func, 0x06);
    if (status & (1 << 4)) == 0 { return None; }
    let mut ptr = (pci_config_read_word(bus, dev, func, 0x34) & 0xFF) as u8;
    let mut guard = 0u8;
    while ptr != 0 && guard < 48 {
        let header = pci_config_read_word(bus, dev, func, ptr);
        if (header & 0xFF) as u8 == cap_id { return Some(ptr); }
        ptr = ((header >> 8) & 0xFF) as u8;
        guard += 1;
    }
    None
}

fn configure_msi_registers(bus: u8, dev: u8, func: u8, cap: u8, vector: u8) {
    // Check 64-bit MSI capability flag (bit 7 of the control word at cap+2)
    let ctrl = pci_config_read_word(bus, dev, func, cap + 2);
    let is_64bit = (ctrl & (1 << 7)) != 0;

    // Message Address (CPU 0 local APIC)
    pci_config_write_dword(bus, dev, func, cap + 4, 0xFEE0_0000);
    if is_64bit {
        pci_config_write_dword(bus, dev, func, cap + 8, 0); // upper address = 0
    }

    // Message Data: fixed delivery, edge-triggered, vector in bits[7:0]
    let data_off = if is_64bit { cap + 12 } else { cap + 8 };
    pci_config_write_dword(bus, dev, func, data_off, (vector as u32) & 0xFF);

    // Enable MSI: clear MME[6:4], set Enable bit[0]
    let new_ctrl = (ctrl & !0x0070) | 0x0001;
    pci_config_write_word(bus, dev, func, cap + 2, new_ctrl);
}

fn scan_bus(bus: u8, bus_list: *mut u8, bus_count: *mut u16) -> u16 {
    let mut dev_count: u16 = 0;
    let mut d = 0u8;
    while d < 32 {
        let vendor = pci_config_read_word(bus, d, 0, 0);
        if vendor == 0xFFFF || vendor == 0 {
            d += 1;
            continue;
        }
        let header_type = pci_config_read_word(bus, d, 0, 0x0E);
        let is_multi = (header_type & 0x80) != 0;
        let max_func = if is_multi { 8u8 } else { 1u8 };
        let mut func = 0u8;
        while func < max_func {
            let vendor = pci_config_read_word(bus, d, func, 0);
            if vendor == 0xFFFF || vendor == 0 {
                func += 1;
                continue;
            }
            let device_id = pci_config_read_word(bus, d, func, 2);
            let class_rev = pci_config_read_dword(bus, d, func, 0x08);
            let class = ((class_rev >> 24) & 0xFF) as u8;
            let subclass = ((class_rev >> 16) & 0xFF) as u8;
            let prog_if = ((class_rev >> 8) & 0xFF) as u8;
            let rev = (class_rev & 0xFF) as u8;
            let cn: &[u8] = match class {
                0x01 => b"Mass storage",
                0x02 => b"Network",
                0x03 => b"Display",
                0x04 => b"Multimedia",
                0x05 => b"Memory",
                0x06 => b"Bridge",
                0x07 => b"Comm",
                0x08 => b"System",
                0x09 => b"Input",
                0x0C => b"Serial bus",
                _ => b"Unknown",
            };
            unsafe {
                log_dev(bus, d, func,
                        vendor, device_id,
                        class, subclass, prog_if, rev,
                        cn.as_ptr(), cn.len());
            }
            dev_count += 1;
            // PCI-to-PCI bridge → enqueue secondary bus
            if class == 0x06 && subclass == 0x04 {
                let sec_bus = ((pci_config_read_dword(bus, d, func, 0x18) >> 8) & 0xFF) as u8;
                if sec_bus != 0 {
                    let count = unsafe { *bus_count as usize };
                    let mut already = false;
                    let mut i = 0usize;
                    while i < count {
                        if unsafe { *bus_list.add(i) } == sec_bus {
                            already = true;
                            break;
                        }
                        i += 1;
                    }
                    if !already && count < 256 {
                        unsafe {
                            *bus_list.add(count) = sec_bus;
                            *bus_count += 1;
                        }
                    }
                }
            }
            func += 1;
        }
        d += 1;
    }
    dev_count
}

#[no_mangle]
pub extern "C" fn driver_init() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) != 0 {
        return -1;
    }
    INITIALIZED.store(1, Ordering::Release);

    let msg = b"pci.nem: enumerating PCI buses\r\n";
    unsafe { hst_log(2, msg.as_ptr(), msg.len()) };

    let mut bus_arr = [0u8; 256];
    let bus_list = bus_arr.as_mut_ptr();
    unsafe { *bus_list = 0; }
    let mut bus_count: u16 = 1;
    let mut bus_idx: u16 = 0;
    let mut total_dev: u16 = 0;

    while (bus_idx as usize) < 256 && bus_idx < bus_count {
        let bus = unsafe { *bus_list.add(bus_idx as usize) };
        bus_idx += 1;
        total_dev += scan_bus(bus, bus_list, &mut bus_count as *mut u16);
    }

    // Format total_dev as decimal
    let mut dec = [0u8; 5];
    let dp = dec.as_mut_ptr();
    let mut n = total_dev;
    let mut len: usize = 0;
    loop {
        unsafe { *dp.add(len) = b'0' + (n % 10) as u8 };
        len += 1;
        n /= 10;
        if n == 0 { break; }
    }
    // reverse
    let mut i = 0usize;
    while i < len / 2 {
        let r = len - 1 - i;
        let t = unsafe { *dp.add(i) };
        unsafe {
            *dp.add(i) = *dp.add(r);
            *dp.add(r) = t;
        }
        i += 1;
    }
    let mut summary = [0u8; 48];
    let sp = summary.as_mut_ptr();
    let pref_bytes = b"pci.nem: " as *const u8;
    let suff_bytes = b" devices found\r\n" as *const u8;
    let mut pos: usize = 0;
    let mut k = 0usize;
    while k < 9 { unsafe { *sp.add(pos) = *pref_bytes.add(k) }; pos += 1; k += 1; }
    k = 0;
    while k < len { unsafe { *sp.add(pos) = *dp.add(k) }; pos += 1; k += 1; }
    k = 0;
    while k < 16 { unsafe { *sp.add(pos) = *suff_bytes.add(k) }; pos += 1; k += 1; }
    unsafe { hst_log(2, sp, pos) };

    0
}

#[no_mangle]
pub extern "C" fn driver_activate() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) == 0 {
        return -1;
    }
    ACTIVE.store(1, Ordering::Release);
    let msg = b"pci.nem: activated\r\n";
    unsafe { hst_log(2, msg.as_ptr(), msg.len()) };
    0
}

#[no_mangle]
pub extern "C" fn driver_on_event(event: *const NeoEvent) -> i32 {
    if ACTIVE.load(Ordering::Relaxed) == 0 || event.is_null() {
        return -1;
    }
    let ev = unsafe { &*event };

    match ev.event_type {
        EVENT_PCI_READ_CONFIG => {
            let packed = ev.data0 as u32;
            let bus = ((packed >> 16) & 0x1F) as u8;
            let dev = ((packed >> 11) & 0x1F) as u8;
            let func = ((packed >> 8) & 0x07) as u8;
            let offset = (packed & 0xFF) as u8;
            let value = pci_config_read_dword(bus, dev, func, offset);
            let _ = unsafe {
                hst_push_event(
                    EVENT_PCI_READ_RESULT, SOURCE_DRIVER, 0,
                    packed as u64, value as u64, 0,
                )
            };
            0
        }
        EVENT_PCI_WRITE_CONFIG => {
            let packed = (ev.data0 & 0xFFFF_FFFF) as u32;
            let value = ev.data1 as u32;
            let bus = ((packed >> 16) & 0x1F) as u8;
            let dev = ((packed >> 11) & 0x1F) as u8;
            let func = ((packed >> 8) & 0x07) as u8;
            let offset = (packed & 0xFF) as u8;
            pci_config_write_dword(bus, dev, func, offset, value);
            let _ = unsafe {
                hst_push_event(
                    EVENT_PCI_WRITE_DONE, SOURCE_DRIVER, 0,
                    packed as u64, 0, 0,
                )
            };
            0
        }
        EVENT_MSI_CONFIGURE => {
            // data0: [63:32] = vector, [31:0] = packed BDF (bus<<16|dev<<11|func<<8)
            // data1: [7:0]   = cap_offset (0 = auto-discover via find_capability)
            let packed  = (ev.data0 & 0xFFFF_FFFF) as u32;
            let vector  = ((ev.data0 >> 32) & 0xFF) as u8;
            let bus     = ((packed >> 16) & 0xFF) as u8;
            let dev     = ((packed >> 11) & 0x1F) as u8;
            let func    = ((packed >>  8) & 0x07) as u8;
            let cap_hint = (ev.data1 & 0xFF) as u8;

            // Use provided cap_offset or auto-locate (MSI capability ID = 0x05)
            let cap = if cap_hint != 0 {
                Some(cap_hint)
            } else {
                find_capability(bus, dev, func, 0x05)
            };

            match cap {
                Some(cap_off) => {
                    configure_msi_registers(bus, dev, func, cap_off, vector);
                    let msg = b"pci.nem: MSI configured\r\n";
                    unsafe { hst_log(2, msg.as_ptr(), msg.len()) };
                    let _ = unsafe {
                        hst_push_event(EVENT_MSI_CONFIGURED, SOURCE_DRIVER, 0, packed as u64, 0, 0)
                    };
                    0
                }
                None => {
                    let msg = b"pci.nem: MSI cap not found\r\n";
                    unsafe { hst_log(1, msg.as_ptr(), msg.len()) };
                    -1
                }
            }
        }
        _ => 1,
    }
}

#[no_mangle]
pub extern "C" fn driver_fini() {
    ACTIVE.store(0, Ordering::Release);
    INITIALIZED.store(0, Ordering::Release);
}

#[no_mangle]
pub extern "C" fn driver_is_active() -> i32 {
    if ACTIVE.load(Ordering::Relaxed) != 0 { 1 } else { 0 }
}
