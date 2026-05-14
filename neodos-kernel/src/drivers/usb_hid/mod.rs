// src/drivers/usb_hid/mod.rs
//
// USB HID driver for keyboard support (polling mode).
// NOT CURRENTLY FUNCTIONAL — PIIX3 doesn't accept FLBASEADD writes.

#![allow(dead_code)]

pub mod uhci;
pub mod hid;

use spin::Mutex;
use lazy_static::lazy_static;

lazy_static! {
    pub static ref USB_HC_BASE: Mutex<Option<u16>> = Mutex::new(None);
}

lazy_static! {
    static ref USB_INPUT_BUFFER: Mutex<UsbInputBuffer> = Mutex::new(UsbInputBuffer::new());
}

pub const USB_BUFFER_SIZE: usize = 256;

pub struct UsbInputBuffer {
    buffer: [u8; USB_BUFFER_SIZE],
    head: usize,
    tail: usize,
}

#[allow(dead_code)]
impl UsbInputBuffer {
    pub const fn new() -> Self {
        UsbInputBuffer {
            buffer: [0; USB_BUFFER_SIZE],
            head: 0,
            tail: 0,
        }
    }

    pub fn push(&mut self, byte: u8) -> bool {
        let next = (self.tail + 1) % USB_BUFFER_SIZE;
        if next == self.head {
            return false;
        }
        self.buffer[self.tail] = byte;
        self.tail = next;
        true
    }

    pub fn pop(&mut self) -> Option<u8> {
        if self.head == self.tail {
            return None;
        }
        let byte = self.buffer[self.head];
        self.head = (self.head + 1) % USB_BUFFER_SIZE;
        Some(byte)
    }

    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }
}

pub fn usb_push_byte(byte: u8) {
    USB_INPUT_BUFFER.lock().push(byte);
}

fn delay_ms(ms: u32) {
    for _ in 0..(ms * 1000 * 1000) {
        unsafe { core::arch::asm!("nop"); }
    }
}

pub fn has_usb_keyboard() -> bool {

    uhci::has_keyboard()
}

fn find_usb_io_base(config_addr: u16, config_data: u16, bus: u8, dev: u8, func: u8) -> Option<u16> {
    // Try all BARs (0-5)
    for (i, bar_offset) in [0x10u8, 0x14, 0x18, 0x1C, 0x20, 0x24].iter().enumerate() {
        let bar = pci_read_dword(config_addr, config_data, bus, dev, func, *bar_offset);
        crate::serial_println!("[USB]   BAR{} = 0x{:08X}", i, bar);
        if bar != 0 && bar != 0xFFFFFFFF && (bar & 1) != 0 {
            let io_base = (bar & 0xFFFE) as u16;
            if io_base >= 0xC060 && io_base <= 0xC06F {
                continue;
            }

            // Read PCI config
            let cmd = pci_read_word(config_addr, config_data, bus, dev, func, 0x04);
            let intr_line = pci_read_byte(config_addr, config_data, bus, dev, func, 0x3C);
            let intr_pin = pci_read_byte(config_addr, config_data, bus, dev, func, 0x3D);
            let rev = pci_read_byte(config_addr, config_data, bus, dev, func, 0x08);
            let new_cmd = cmd | 0x05;
            pci_write_word(config_addr, config_data, bus, dev, func, 0x04, new_cmd);
            let cmd_after = pci_read_word(config_addr, config_data, bus, dev, func, 0x04);
            crate::serial_println!("[USB]   PCI cmd 0x{:04X}->0x{:04X} intr={} pin={} rev=0x{:02X}",
                cmd, cmd_after, intr_line, intr_pin, rev);

            return Some(io_base);
        }
    }
    None
}

/// Initialize USB keyboard: scan PCI, init UHCI, enumerate device
pub fn init_usb_keyboard() -> bool {
    crate::serial_println!("[USB] Scanning for USB host controllers...");

    let controller = scan_pci_for_usb();

    if let Some(ctrl) = controller {
        if let Some(io_base) = ctrl.io_base {
            crate::serial_println!("[USB] UHCI at I/O 0x{:04X}", io_base);
            if uhci::init(io_base) {
                if uhci::detect_keyboard(io_base) {
                    crate::serial_println!("[USB] USB keyboard detected!");
                    *USB_HC_BASE.lock() = Some(io_base as u16);
                    return true;
                }
            }
        } else if let Some(mmio) = ctrl.mmio_base {
            crate::serial_println!("[USB] USB controller at MMIO 0x{:08X} (prog_if=0x{:02X}) - requires MMIO support",
                mmio, ctrl.prog_if);
        }
    }

    crate::serial_println!("[USB] No USB keyboard detected");
    false
}

/// Poll the USB keyboard for new keypresses
pub fn poll_usb_keyboard() {
    let base_lock = USB_HC_BASE.lock();
    if let Some(io_base) = *base_lock {
        uhci::poll_keyboard(io_base);
    }
}

// Scan PCI bus for USB controllers (class 0x0C:0x03)
fn scan_pci_for_usb() -> Option<UsbController> {
    let config_addr = 0xCF8u16;
    let config_data = 0xCFCu16;

    for bus in 0..=0 {
        for dev in 0..32 {
            for func in 0..8 {
                let vendor = pci_read_word(config_addr, config_data, bus, dev, func, 0);
                if vendor == 0xFFFF || vendor == 0 {
                    if func == 0 { break; }
                    continue;
                }

                let device = pci_read_word(config_addr, config_data, bus, dev, func, 2);
                let class_rev = pci_read_dword(config_addr, config_data, bus, dev, func, 0x08);
                let class = ((class_rev >> 24) & 0xFF) as u8;
                let subclass = ((class_rev >> 16) & 0xFF) as u8;
                let prog_if = ((class_rev >> 8) & 0xFF) as u8;

                crate::serial_println!("[PCI] bus={} dev={} func={} {:04X}:{:04X} class=0x{:02X}:0x{:02X}:{:02X}",
                    bus, dev, func, vendor, device, class, subclass, prog_if);

                if class == 0x0C && subclass == 0x03 {
                    for bar_offset in [0x10u8, 0x14, 0x18, 0x1C, 0x20, 0x24] {
                        let bar = pci_read_dword(config_addr, config_data, bus, dev, func, bar_offset);
                        if bar == 0 || bar == 0xFFFFFFFF { continue; }

                        let is_io = (bar & 1) != 0;
                        let io_base = (bar & 0xFFFE) as u16;
                        let mmio_base = (bar & !0xF) as u64;

                        // Enable PCI command bits
                        let cmd = pci_read_word(config_addr, config_data, bus, dev, func, 0x04);
                        let new_cmd = cmd | 0x07;
                        if new_cmd != cmd {
                            pci_write_word(config_addr, config_data, bus, dev, func, 0x04, new_cmd);
                        }

                        let intr_line = pci_read_byte(config_addr, config_data, bus, dev, func, 0x3C);

                        // Log all BARs for the USB device
                        crate::serial_println!("[USB]   {:04X}:{:04X} BAR{} = 0x{:08X} (io={}, base=0x{:04X}/0x{:08X})",
                            vendor, device, bar_offset >> 2, bar, is_io, io_base, mmio_base);

                        if is_io && io_base != 0 {
                            if io_base < 0x10 {
                                crate::serial_println!("[USB]   {:04X}:{:04X} BAR{} -> I/O 0x{:04X} SKIPPED (invalid)",
                                    vendor, device, bar_offset >> 2, io_base);
                                continue;
                            }
                            crate::serial_println!("[USB]   {:04X}:{:04X} BAR{} -> I/O 0x{:04X} (intr={}, prog_if=0x{:02X})",
                                vendor, device, bar_offset >> 2, io_base, intr_line, prog_if);
                            
                            // Disable Legacy Support (USBLEGSUP at 0xC0)
                            // Bit 15 is A20/BIOS ownership. MUST BE CLEARED.
                            // Bit 13 is HC OS Owned. SET IT.
                            pci_write_word(config_addr, config_data, bus, dev, func, 0xC0, 0x2000);
                            delay_ms(10);
                            let legsup = pci_read_word(config_addr, config_data, bus, dev, func, 0xC0);
                            crate::serial_println!("[USB]   Legacy Support (0xC0) = 0x{:04X}", legsup);


                            return Some(UsbController {
                                io_base: Some(io_base),
                                mmio_base: None,
                                prog_if,
                            });

                        }
                        if !is_io && mmio_base != 0 && mmio_base < 0xFFFF_FFFF {
                            crate::serial_println!("[USB]   {:04X}:{:04X} BAR{} -> MMIO 0x{:08X} (intr={}, prog_if=0x{:02X})",
                                vendor, device, bar_offset >> 2, mmio_base, intr_line, prog_if);
                            return Some(UsbController {
                                io_base: None,
                                mmio_base: Some(mmio_base as u32),
                                prog_if,
                            });
                        }
                    }
                }

                if func == 0 && class != 0x0C {
                    break;
                }
            }
        }
    }

    crate::serial_println!("[PCI] No USB controller found!");
    None
}

struct UsbController {
    io_base: Option<u16>,
    mmio_base: Option<u32>,
    prog_if: u8,
}

fn pci_read_dword(addr_port: u16, data_port: u16, bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let address = 0x8000_0000u32
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((offset as u32) & 0xFC);

    unsafe {
        let mut addr = x86_64::instructions::port::Port::<u32>::new(addr_port);
        let mut data = x86_64::instructions::port::Port::<u32>::new(data_port);
        addr.write(address);
        data.read()
    }
}

fn pci_read_byte(addr_port: u16, data_port: u16, bus: u8, dev: u8, func: u8, offset: u8) -> u8 {
    let aligned = offset & 0xFC;
    let dword = pci_read_dword(addr_port, data_port, bus, dev, func, aligned);
    ((dword >> ((offset & 3) * 8)) & 0xFF) as u8
}

fn pci_read_word(addr_port: u16, data_port: u16, bus: u8, dev: u8, func: u8, offset: u8) -> u16 {
    let aligned = offset & 0xFC;
    let dword = pci_read_dword(addr_port, data_port, bus, dev, func, aligned);
    ((dword >> ((offset & 3) * 8)) & 0xFFFF) as u16
}

fn pci_write_word(addr_port: u16, data_port: u16, bus: u8, dev: u8, func: u8, offset: u8, value: u16) {
    let aligned = offset & 0xFC;
    let dword = pci_read_dword(addr_port, data_port, bus, dev, func, aligned);
    let shift = ((offset & 3) * 8) as u32;
    let mask = !(0xFFFFu32 << shift);
    let new_dword = (dword & mask) | ((value as u32) << shift);

    let address = 0x8000_0000u32
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((aligned as u32) & 0xFC);

    unsafe {
        let mut addr = x86_64::instructions::port::Port::<u32>::new(addr_port);
        let mut data = x86_64::instructions::port::Port::<u32>::new(data_port);
        addr.write(address);
        data.write(new_dword);
    }
}