#![no_std]
#![no_main]

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
    fn hst_outw(port: u16, val: u16);
    fn hst_outb(port: u16, val: u8);
    fn hst_log(level: u32, msg: *const u8, len: usize);
}

const PCI_ADDR: u16 = 0xCF8;
const PCI_DATA: u16 = 0xCFC;

// ACPI S5 (soft-off) sleep type
const SLP_TYP_S5: u16 = 5;
const SLP_EN: u16 = 1 << 13;

const EVENT_SHUTDOWN: u32 = 12;
const SOURCE_DRIVER: u32 = 1;

static INITIALIZED: AtomicU8 = AtomicU8::new(0);
static ACTIVE: AtomicU8 = AtomicU8::new(0);
static PM1A_PORT: AtomicU8 = AtomicU8::new(0);

fn pci_read_dword(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let addr = 0x8000_0000u32
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC);
    unsafe {
        hst_outl(PCI_ADDR, addr);
        hst_inl(PCI_DATA)
    }
}

fn pci_read_word(bus: u8, dev: u8, func: u8, offset: u8) -> u16 {
    let dword = pci_read_dword(bus, dev, func, offset);
    ((dword >> ((offset & 3) * 8)) & 0xFFFF) as u16
}

fn find_acpi_pm1a_port() -> Option<u16> {
    for bus in 0..=0 {
        for dev in 0..32 {
            for func in 0..8 {
                let vendor = pci_read_word(bus, dev, func, 0);
                if vendor == 0xFFFF || vendor == 0 {
                    if func == 0 { break; }
                    continue;
                }
                if vendor != 0x8086 {
                    continue;
                }
                let device_id = pci_read_word(bus, dev, func, 2);

                // PIIX4 ACPI: device 0x7113
                if device_id == 0x7113 {
                    let gpbase = pci_read_dword(bus, dev, func, 0x40);
                    if gpbase & 1 != 0 {
                        return Some(((gpbase & 0xFFF0) as u16) + 0x04);
                    }
                }

                // ICH9 LPC: device 0x2918 (ICH9) or 0x2916 (ICH9M)
                if device_id == 0x2918 || device_id == 0x2916 {
                    let abase = pci_read_dword(bus, dev, func, 0x40);
                    if abase & 1 != 0 {
                        return Some(((abase & 0xFFFE) as u16) + 0x04);
                    }
                }
            }
        }
    }
    None
}

fn acpi_poweroff() {
    // 1. Try ACPI S5 via PM1a if detected
    let port = PM1A_PORT.load(Ordering::Relaxed) as u16;
    if port != 0 {
        let slp_val = (SLP_TYP_S5 << 10) | SLP_EN;
        unsafe { hst_outw(port, slp_val) };
    }

    // 2. Fallback: QEMU/Bochs poweroff port (0x604 with 0x2000)
    unsafe { hst_outw(0x604, 0x2000) };

    // 3. Last resort: PS/2 keyboard reset (0x64, 0xFE)
    unsafe { hst_outb(0x64, 0xFE) };
}

#[no_mangle]
pub extern "C" fn driver_init() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) != 0 {
        return -1;
    }

    let port = match find_acpi_pm1a_port() {
        Some(p) => p,
        None => {
            let msg = b"acpi.nem: no ACPI controller found\r\n";
            unsafe { hst_log(1, msg.as_ptr(), msg.len()) };
            return -1;
        }
    };

    PM1A_PORT.store(port as u8, Ordering::Relaxed);
    INITIALIZED.store(1, Ordering::Release);

    let hex_digit = |v: u8| -> u8 { b"0123456789ABCDEF"[(v & 0xF) as usize] };
    let hex_chars = [
        hex_digit((port >> 12) as u8),
        hex_digit((port >> 8) as u8),
        hex_digit((port >> 4) as u8),
        hex_digit(port as u8),
    ];
    let msg = [
        b'a', b'c', b'p', b'i', b'.', b'n', b'e', b'm', b':', b' ',
        b'P', b'M', b'1', b'a', b' ', b'p', b'o', b'r', b't', b' ',
        b'0', b'x',
        hex_chars[0], hex_chars[1], hex_chars[2], hex_chars[3],
        b'\r', b'\n',
    ];
    unsafe { hst_log(2, msg.as_ptr(), msg.len()) };
    0
}

#[no_mangle]
pub extern "C" fn driver_activate() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) == 0 {
        return -1;
    }
    ACTIVE.store(1, Ordering::Release);
    let msg = b"acpi.nem: activated\r\n";
    unsafe { hst_log(2, msg.as_ptr(), msg.len()) };
    0
}

#[no_mangle]
pub extern "C" fn driver_on_event(event: *const NeoEvent) -> i32 {
    if ACTIVE.load(Ordering::Relaxed) == 0 || event.is_null() {
        return -1;
    }
    let ev = unsafe { &*event };
    if ev.event_type != EVENT_SHUTDOWN {
        return 1;
    }
    let msg = b"acpi.nem: shutdown requested, powering off via ACPI S5\r\n";
    unsafe { hst_log(2, msg.as_ptr(), msg.len()) };
    acpi_poweroff();
    0
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
