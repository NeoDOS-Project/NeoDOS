// src/drivers/usb_hid/uhci.rs
//
// UHCI (Universal Host Controller Interface) driver.
// USB HID keyboard via polling.
// NOT CURRENTLY FUNCTIONAL — PIIX3 doesn't accept FLBASEADD writes.

#![allow(dead_code, unused_variables)]

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use core::ptr;

const PORTSC1_CCS: u16 = 1 << 0;
const PORTSC1_PES: u16 = 1 << 1;
const PORTSC1_PR:  u16 = 1 << 4;
const PORTSC1_PED: u16 = 1 << 7;

const USBCMD_RS:       u16 = 1 << 0;
const USBCMD_HCRESET:  u16 = 1 << 1;
const USBCMD_GRESET:   u16 = 1 << 2;
const USBSTS_HCH:      u16 = 1 << 5;



const TD_IOC:       u32 = 1 << 15;
const TD_ACTIVE:    u32 = 1 << 7;
const TD_MAX_ERRORS: u32 = 3;

const PID_SETUP: u32 = 0x2D;
const PID_IN:    u32 = 0x69;
const PID_OUT:   u32 = 0xE1;


const USB_DIR_OUT:       u8 = 0x00;
const USB_DIR_IN:       u8 = 0x80;
const USB_TYPE_STANDARD: u8 = 0x00;
const USB_TYPE_CLASS:   u8 = 0x20;
const USB_RECIP_DEVICE:    u8 = 0x00;
const USB_RECIP_INTERFACE: u8 = 0x01;

const USB_REQ_GET_DESCRIPTOR:   u8 = 0x06;
const USB_REQ_SET_ADDRESS:      u8 = 0x05;
const USB_REQ_SET_CONFIGURATION: u8 = 0x09;
const USB_REQ_GET_REPORT:      u8 = 0x01;

const USB_DT_DEVICE: u8 = 0x01;
const USB_DT_CONFIG: u8 = 0x02;

#[repr(C, packed)]
struct UsbSetupPacket {
    request_type: u8,
    request:      u8,
    value:        u16,
    index:        u16,
    length:       u16,
}

#[repr(C, align(16))]
#[derive(Copy, Clone)]
struct UhciTd {
    link:      u32,
    status:    u32,
    token:     u32,
    buffer:    u32,
}

#[repr(C, align(16))]
#[derive(Copy, Clone)]
struct UhciQh {
    horiz_link: u32,
    vert_link:  u32,
    _pad: [u32; 2],
}

impl UhciQh {
    const fn new() -> Self {
        UhciQh { horiz_link: 1, vert_link: 1, _pad: [0, 0] }
    }
}


#[repr(align(4096))]
struct UhciFrameList([u32; 1024]);
#[repr(align(16))]
struct UhciTdBuffer([UhciTd; 16]);
#[repr(align(16))]
struct UhciQhBuffer([UhciQh; 8]);

static mut TD_BUFFER:   UhciTdBuffer = UhciTdBuffer([UhciTd { link: 1, status: 0, token: 0, buffer: 0 }; 16]);
static mut DATA_BUFFER: [u8; 4096]   = [0; 4096];
static mut FRAME_LIST:  UhciFrameList = UhciFrameList([0; 1024]);
static mut QH_BUFFER:   UhciQhBuffer  = UhciQhBuffer([UhciQh::new(); 8]);



static DEVICE_ADDRESS: AtomicBool = AtomicBool::new(false);
static DEV_ADDR:     AtomicU8   = AtomicU8::new(0);
static HID_ENDPOINT: AtomicU8   = AtomicU8::new(0);

fn delay_us(us: u32) {
    for _ in 0..(us * 1000) {
        unsafe { core::arch::asm!("nop"); }
    }
}



fn delay_ms(ms: u32) {
    delay_us(ms * 1000);
}

fn port_in(base: u16, offset: u16) -> u16 {
    unsafe {
        let mut p = x86_64::instructions::port::Port::<u16>::new(base + offset);
        p.read()
    }
}

fn port_out(base: u16, offset: u16, val: u16) {
    unsafe {
        let mut p = x86_64::instructions::port::Port::<u16>::new(base + offset);
        p.write(val);
    }
}

fn port_out8(base: u16, offset: u16, val: u8) {
    unsafe {
        let mut p = x86_64::instructions::port::Port::<u8>::new(base + offset);
        p.write(val);
    }
}

fn port_out_dword(base: u16, offset: u16, val: u32) {
    unsafe {
        let mut p = x86_64::instructions::port::Port::<u32>::new(base + offset);
        p.write(val);
    }
}

fn write_flbaseadd(base: u16, addr: u32) {
    port_out_dword(base, 0x08, addr);
}


pub fn init(base: u16) -> bool {
    crate::serial_println!("[UHCI] Init at I/O 0x{:04X}", base);

    let flbase = unsafe { ptr::addr_of!(FRAME_LIST.0) as u32 };
    let qh0_ptr = unsafe { ptr::addr_of!(QH_BUFFER.0[0]) as u32 };
    let qh1_ptr = unsafe { ptr::addr_of!(QH_BUFFER.0[1]) as u32 };

    unsafe {
        for i in 0..1024 { FRAME_LIST.0[i] = 1; }
        FRAME_LIST.0[0] = qh0_ptr | 0x0002;
        QH_BUFFER.0[0].horiz_link = qh1_ptr | 0x0002;

        QH_BUFFER.0[0].vert_link = 1;
        QH_BUFFER.0[1].horiz_link = 1;
        QH_BUFFER.0[1].vert_link = 1;
    }


    crate::serial_println!("[UHCI] FLBASE=0x{:08X} QH0=0x{:08X}", flbase, qh0_ptr);

    // 1. Stop
    port_out(base, 0, 0);
    delay_ms(10);

    // 2. Global Reset (GRESET)
    port_out(base, 0, USBCMD_GRESET);
    delay_ms(50);
    port_out(base, 0, 0);
    delay_ms(10);

    // 3. Host Controller Reset (HCRESET)
    port_out(base, 0, USBCMD_HCRESET);
    let mut _reset_ok = false;
    for _ in 0..100 {
        if (port_in(base, 0) & USBCMD_HCRESET) == 0 {
            _reset_ok = true;
            break;
        }
        delay_ms(10);
    }

    
    // Clear all status bits
    port_out(base, 0x02, 0x00FF);
    port_out(base, 0x04, 0); // Disable interrupts
    port_out(base, 0x06, 0); // FRNUM = 0

    let sts = port_in(base, 0x02);
    let frnum = port_in(base, 0x06);
    crate::serial_println!("[UHCI] Post-reset: STS=0x{:04X} FRNUM={}", sts, frnum);

    write_flbaseadd(base, flbase);
    delay_ms(10);
    port_out(base, 0x06, 0); // FRNUM = 0

    // 4. Start
    crate::serial_println!("[UHCI] Starting (RS only)...");
    port_out(base, 0x02, 0x00FF); // Clear status again
    port_out(base, 0, USBCMD_RS);
    
    let mut started = false;
    for _ in 0..100 {
        if (port_in(base, 0x02) & USBSTS_HCH) == 0 {
            started = true;
            break;
        }
        delay_ms(1);
    }

    if started {
        crate::serial_println!("[UHCI] Controller is RUNNING");
        return true;
    } else {
        let sts = port_in(base, 0x02);
        crate::serial_println!("[UHCI] FAILED TO START. STS=0x{:04X}", sts);
    }

    false
}



const TD_DATA_TOGGLE: u32 = 1 << 19;


fn ctrl_transfer(
    _base: u16,
    dev_addr: u8,
    ep: u8,
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    buf: &mut [u8],
) -> i32 {
    let td0_ptr = unsafe { ptr::addr_of!(TD_BUFFER.0[0]) as u32 };
    let td1_ptr = unsafe { ptr::addr_of!(TD_BUFFER.0[1]) as u32 };
    let td2_ptr = unsafe { ptr::addr_of!(TD_BUFFER.0[2]) as u32 };

    let data_ptr = unsafe { DATA_BUFFER.as_mut_ptr() as u32 };

    unsafe {
        let setup = UsbSetupPacket {
            request_type,
            request,
            value: value.to_le(),
            index: index.to_le(),
            length: (buf.len() as u16).to_le(),
        };
        ptr::write(DATA_BUFFER.as_mut_ptr() as *mut UsbSetupPacket, setup);

        // TD0: SETUP (Always DATA0)
        TD_BUFFER.0[0].buffer = data_ptr;
        TD_BUFFER.0[0].status = TD_ACTIVE | (TD_MAX_ERRORS << 26);
        TD_BUFFER.0[0].token = PID_SETUP
            | ((ep as u32) << 8)
            | ((dev_addr as u32) << 13)
            | ((8u32 - 1) << 21);

        // TD1: DATA (Always DATA1 for single-packet control transfers)
        TD_BUFFER.0[1].buffer = data_ptr + 64;
        TD_BUFFER.0[1].status = TD_ACTIVE | (TD_MAX_ERRORS << 26);
        TD_BUFFER.0[1].token = PID_IN
            | TD_DATA_TOGGLE
            | ((ep as u32) << 8)
            | ((dev_addr as u32) << 13)
            | (((buf.len().saturating_sub(1)) as u32) << 21);

        // TD2: STATUS (Always DATA1)
        TD_BUFFER.0[2].buffer = 0;
        TD_BUFFER.0[2].status = TD_IOC | TD_ACTIVE | (TD_MAX_ERRORS << 26);
        TD_BUFFER.0[2].token = PID_OUT
            | TD_DATA_TOGGLE
            | ((ep as u32) << 8)
            | ((dev_addr as u32) << 13)
            | (0u32 << 21);

        TD_BUFFER.0[0].link = td1_ptr | 0x04;
        TD_BUFFER.0[1].link = td2_ptr | 0x04;
        TD_BUFFER.0[2].link = 1;
        QH_BUFFER.0[0].vert_link = td0_ptr | 0x04;

    }

    let mut result = -1i32;
    for _ in 0..1_000_000 {
        let status = unsafe { TD_BUFFER.0[2].status };
        if status & TD_ACTIVE == 0 {
            let cc = (status >> 16) & 0x0F;
            if cc == 0 {
                let len_field = unsafe { TD_BUFFER.0[1].status };
                let actual_len = ((len_field) & 0x7FF).wrapping_add(1) as usize;
                // For UHCI, the length in status is (Actual Length - 1)
                
                if !buf.is_empty() {
                    let copy_len = actual_len.min(buf.len());
                    unsafe {
                        ptr::copy_nonoverlapping(
                            DATA_BUFFER.as_ptr().offset(64),
                            buf.as_mut_ptr(),
                            copy_len,
                        );
                    }
                    result = copy_len as i32;
                } else {
                    result = 0;
                }
                crate::serial_println!("[UHCI] Ctrl OK: {} bytes", result);
            } else {
                crate::serial_println!("[UHCI] Ctrl CC=0x{:X} status=0x{:08X}", cc, status);
            }
            break;
        }
        delay_us(2);
    }

    unsafe {
        TD_BUFFER.0[0].link = 1;
        TD_BUFFER.0[1].link = 1;
        TD_BUFFER.0[2].link = 1;
        QH_BUFFER.0[0].vert_link = 1;
    }


    result
}

fn interrupt_in_transfer(
    _base: u16,
    dev_addr: u8,
    ep: u8,
    buf: &mut [u8],
) -> i32 {
    let td0_ptr = unsafe { ptr::addr_of!(TD_BUFFER.0[4]) as u32 };
    let data_ptr = unsafe { DATA_BUFFER.as_mut_ptr() as u32 + 512 };

    unsafe {
        // Interrupt transfer uses toggling, but for simplicity we assume 
        // the device resets toggle on Set_Configuration or we just try both.
        // Actually, HID keyboards usually start with DATA0 after reset.
        static mut TOGGLE: bool = false;
        
        TD_BUFFER.0[4].buffer = data_ptr;
        TD_BUFFER.0[4].status = TD_IOC | TD_ACTIVE | (TD_MAX_ERRORS << 26);
        TD_BUFFER.0[4].token = PID_IN
            | (if TOGGLE { TD_DATA_TOGGLE } else { 0 })
            | (((ep & 0x0F) as u32) << 8)
            | ((dev_addr as u32) << 13)
            | (((buf.len().saturating_sub(1)) as u32) << 21);
        
        TD_BUFFER.0[4].link = 1;
        QH_BUFFER.0[1].vert_link = td0_ptr | 0x04;
        
        let mut result = -1i32;
        // Shorter timeout for polling
        for _ in 0..10_000 {
            let status = TD_BUFFER.0[4].status;
            if status & TD_ACTIVE == 0 {
                let cc = (status >> 16) & 0x0F;
                if cc == 0 {
                    let actual_len = (status & 0x7FF).wrapping_add(1) as usize;
                    let copy_len = actual_len.min(buf.len());
                    ptr::copy_nonoverlapping(data_ptr as *const u8, buf.as_mut_ptr(), copy_len);
                    result = copy_len as i32;
                    TOGGLE = !TOGGLE;
                }
                break;
            }
            delay_us(1);
        }
        
        QH_BUFFER.0[1].vert_link = 1;
        result
    }

}


fn get_dev_desc(base: u16, addr: u8, buf: &mut [u8]) -> i32 {
    ctrl_transfer(base, addr, 0,
        USB_DIR_IN | USB_TYPE_STANDARD | USB_RECIP_DEVICE,
        USB_REQ_GET_DESCRIPTOR,
        (USB_DT_DEVICE as u16) << 8, 0, buf)
}

fn get_cfg_desc(base: u16, addr: u8, buf: &mut [u8]) -> i32 {
    ctrl_transfer(base, addr, 0,
        USB_DIR_IN | USB_TYPE_STANDARD | USB_RECIP_DEVICE,
        USB_REQ_GET_DESCRIPTOR,
        (USB_DT_CONFIG as u16) << 8, 0, buf)
}

fn set_addr(base: u16, addr: u8, new: u8) -> i32 {
    ctrl_transfer(base, addr, 0,
        USB_DIR_OUT | USB_TYPE_STANDARD | USB_RECIP_DEVICE,
        USB_REQ_SET_ADDRESS, new as u16, 0, &mut [])
}

fn set_cfg(base: u16, addr: u8, cfg: u8) -> i32 {
    ctrl_transfer(base, addr, 0,
        USB_DIR_OUT | USB_TYPE_STANDARD | USB_RECIP_DEVICE,
        USB_REQ_SET_CONFIGURATION, cfg as u16, 0, &mut [])
}

fn get_report(base: u16, addr: u8, ep: u8, buf: &mut [u8]) -> i32 {
    ctrl_transfer(base, addr, ep,
        USB_DIR_IN | USB_TYPE_CLASS | USB_RECIP_INTERFACE,
        USB_REQ_GET_REPORT, 0x0100, 0, buf)
}

fn find_hid_ep(buf: &[u8]) -> Option<(u8, u8)> {
    if buf.len() < 9 { return None; }
    let total = ((buf[2] as usize) | ((buf[3] as usize) << 8)).min(buf.len());
    let mut pos = 0;
    while pos + 4 <= total {
        let len = buf[pos] as usize;
        let kind = buf[pos + 1];
        if len == 0 || pos + len > total { break; }
        if kind == 0x05 && len >= 7 {
            let addr = buf[pos + 2];
            let attr = buf[pos + 3];
            if (addr & 0x80) != 0 && (attr & 0x03) == 0x03 {
                return Some((addr, buf[pos + 6]));
            }
        }
        pos += len;
    }
    None
}

fn enumerate(base: u16) -> bool {
    crate::serial_println!("[UHCI] Enumerating...");

    let mut dev_buf = [0u8; 8];
    if get_dev_desc(base, 0, &mut dev_buf) < 0 {
        crate::serial_println!("[UHCI] GetDevDesc failed");
        return false;
    }
    crate::serial_println!("[UHCI] Class=0x{:02X} max_pkt={}", dev_buf[4], dev_buf[7]);

    if set_addr(base, 0, 1) < 0 {
        crate::serial_println!("[UHCI] SetAddress failed");
        return false;
    }
    DEV_ADDR.store(1, Ordering::Relaxed);
    delay_ms(2);

    let mut cfg_buf = [0u8; 256];
    if get_cfg_desc(base, 1, &mut cfg_buf) < 0 {
        crate::serial_println!("[UHCI] GetCfgDesc failed");
        return false;
    }

    if let Some((ep, _)) = find_hid_ep(&cfg_buf) {
        HID_ENDPOINT.store(ep, Ordering::Relaxed);
        if set_cfg(base, 1, 1) >= 0 {
            crate::serial_println!("[UHCI] Ready! ep=0x{:02X}", ep);
            return true;
        }
    }
    false
}

pub fn detect_keyboard(base: u16) -> bool {
    let portsc = port_in(base, 0x10);
    crate::serial_println!("[UHCI] PORTSC=0x{:04X} CCS={} PES={}",
        portsc, (portsc & PORTSC1_CCS) != 0, (portsc & PORTSC1_PES) != 0);

    if portsc & PORTSC1_CCS == 0 {
        crate::serial_println!("[UHCI] No device");
        return false;
    }

    let mut ok = false;
    for _ in 0..3 {
        let cur = port_in(base, 0x10);
        port_out(base, 0x10, cur | PORTSC1_PED | PORTSC1_PR);
        delay_ms(20);
        port_out(base, 0x10, cur | PORTSC1_PED);
        delay_ms(10);

        if port_in(base, 0x10) & PORTSC1_PES != 0 {
            ok = true;
            break;
        }
        delay_ms(20);
    }

    if !ok {
        port_out(base, 0x10, port_in(base, 0x10) | PORTSC1_PED);
        delay_ms(10);
        ok = true;
    }

    if !ok {
        crate::serial_println!("[UHCI] Port enable failed");
        return false;
    }

    if enumerate(base) {
        DEVICE_ADDRESS.store(true, Ordering::Relaxed);
        true
    } else {
        false
    }
}

pub fn has_keyboard() -> bool {
    DEVICE_ADDRESS.load(Ordering::Relaxed)
}

pub fn poll_keyboard(base: u16) {
    if !DEVICE_ADDRESS.load(Ordering::Relaxed) { return; }
    let ep = HID_ENDPOINT.load(Ordering::Relaxed);
    if ep == 0 { return; }
    let addr = DEV_ADDR.load(Ordering::Relaxed);
    let mut report = [0u8; 8];
    if interrupt_in_transfer(base, addr, ep, &mut report) > 0 {
        super::hid::parse_hid_report(&report);
    }
}

