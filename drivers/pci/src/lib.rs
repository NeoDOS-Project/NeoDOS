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
        b'0', b'0',
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
    buf[..prefix.len()].copy_from_slice(prefix);
    core::ptr::copy_nonoverlapping(name, buf[prefix.len()..].as_mut_ptr(), name_len);
    let total = prefix.len() + name_len;
    buf[total] = b'\r';
    buf[total + 1] = b'\n';
    hst_log(2, buf.as_ptr(), total + 2);
}

#[no_mangle]
pub extern "C" fn driver_init() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) != 0 {
        return -1;
    }
    INITIALIZED.store(1, Ordering::Release);

    let msg = b"pci.nem: scanning PCI bus 0\r\n";
    unsafe { hst_log(2, msg.as_ptr(), msg.len()) };

    let mut dev_count: u8 = 0;
    for bus in 0..=0 {
        for dev in 0..32 {
            for func in 0..8 {
                let vendor = pci_config_read_word(bus, dev, func, 0);
                if vendor == 0xFFFF || vendor == 0 {
                    if func == 0 {
                        break;
                    }
                    continue;
                }

                let device_id = pci_config_read_word(bus, dev, func, 2);
                let class_rev = pci_config_read_dword(bus, dev, func, 0x08);
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
                    log_dev(bus as u8, dev as u8, func as u8,
                            vendor, device_id,
                            class, subclass, prog_if, rev,
                            cn.as_ptr(), cn.len());
                }

                dev_count += 1;
            }
        }
    }

    let dev_tens = dev_count / 10 + b'0';
    let dev_ones = dev_count % 10 + b'0';
    let summary = [
        b'p', b'c', b'i', b'.', b'n', b'e', b'm', b':', b' ',
        dev_tens, dev_ones,
        b' ', b'd', b'e', b'v', b'i', b'c', b'e', b's', b' ',
        b'f', b'o', b'u', b'n', b'd', b'\r', b'\n',
    ];
    unsafe { hst_log(2, summary.as_ptr(), summary.len()) };

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
