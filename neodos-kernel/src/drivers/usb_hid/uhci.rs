// src/drivers/usb_hid/uhci.rs
//
// UHCI (Universal Host Controller Interface) driver.
// USB HID keyboard via polling.

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use core::ptr;

const PORTSC1_CCS: u16 = 1 << 0;
const PORTSC1_PES: u16 = 1 << 1;
const PORTSC1_PR:  u16 = 1 << 4;
const PORTSC1_PED: u16 = 1 << 7;

const USBCMD_RS:       u16 = 1 << 0;
const USBCMD_FLE:      u16 = 1 << 2;
const USBCMD_HCRESET:  u16 = 1 << 3;
const USBSTS_HCH:      u16 = 1 << 5;

const TD_IOC:       u32 = 1 << 15;
const TD_ACTIVE:    u32 = 1 << 7;
const TD_MAX_ERRORS: u32 = 3;

const PID_SETUP: u32 = 0x0D;
const PID_IN:    u32 = 0x10;
const PID_OUT:   u32 = 0x01;

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

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct UhciTd {
    link:      u32,
    status:    u32,
    token:     u32,
    buffer:    u32,
    _pad:      u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct UhciQh {
    horiz_link: u32,
    vert_link:  u32,
}

impl UhciQh {
    const fn new() -> Self {
        UhciQh { horiz_link: 1, vert_link: 1 }
    }
}

static mut TD_BUFFER:   [UhciTd; 16]  = [UhciTd { link: 1, status: 0, token: 0, buffer: 0, _pad: 0 }; 16];
static mut DATA_BUFFER: [u8; 4096]   = [0; 4096];
static mut FRAME_LIST:  [u32; 1024]  = [0; 1024];
static mut QH_BUFFER:   [UhciQh; 8]  = [UhciQh::new(); 8];

static DEVICE_ADDRESS: AtomicBool = AtomicBool::new(false);
static DEV_ADDR:     AtomicU8   = AtomicU8::new(0);
static HID_ENDPOINT: AtomicU8   = AtomicU8::new(0);

fn delay_us(us: u32) {
    for _ in 0..(us * 2) {
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

fn write_flbaseadd(base: u16, addr: u32) {
    unsafe {
        core::arch::asm!(
            "out dx, eax",
            in("dx") base + 0x14,
            in("eax") addr
        );
    }
}



pub fn init(base: u16) -> bool {
    crate::serial_println!("[UHCI] Init at I/O 0x{:04X}", base);

    let flbase = unsafe { (&FRAME_LIST as *const _ as u32) & 0xFFFFF000 };
    let qh0_ptr = unsafe { ptr::addr_of!(QH_BUFFER[0]) as u32 };
    let qh1_ptr = unsafe { ptr::addr_of!(QH_BUFFER[1]) as u32 };

    unsafe {
        for i in 0..1024 { FRAME_LIST[i] = 1; }
        FRAME_LIST[0] = qh0_ptr | 0x0002;
        QH_BUFFER[0].horiz_link = qh1_ptr | 0x0002;
        QH_BUFFER[0].vert_link = 1;
        QH_BUFFER[1].horiz_link = 1;
        QH_BUFFER[1].vert_link = 1;
    }
    crate::serial_println!("[UHCI] FLBASE=0x{:08X} QH0=0x{:08X}", flbase, qh0_ptr);

    let portsc = port_in(base, 0x10);
    crate::serial_println!("[UHCI] PORTSC=0x{:04X}", portsc);

    let cmd0 = port_in(base, 0);
    let sts0 = port_in(base, 0x04);
    let fl_lo0 = port_in(base, 0x14);
    let fl_hi0 = port_in(base, 0x16);
    crate::serial_println!("[UHCI] pre-init: CMD=0x{:04X} STS=0x{:04X} FLBASEADD=0x{:04X}:{:04X}",
        cmd0, sts0, fl_hi0, fl_lo0);

    port_out(base, 0, 0);
    delay_ms(10);

    port_out(base, 0, USBCMD_HCRESET);
    delay_ms(50);

    let cmd_r = port_in(base, 0);
    let sts_r = port_in(base, 0x04);
    crate::serial_println!("[UHCI] HCRESET CMD=0x{:04X} STS=0x{:04X}", cmd_r, sts_r);

    let fn_r = port_in(base, 0x0C);
    let fl_lo2 = port_in(base, 0x14);
    let fl_hi2 = port_in(base, 0x16);
    crate::serial_println!("[UHCI] post-reset: FRNUM={} FLBASEADD=0x{:04X}:{:04X}", fn_r, fl_hi2, fl_lo2);

    port_out(base, 0x04, 0xFF);
    delay_ms(5);
    port_out(base, 0x04, 0);
    port_out8(base, 0x0C, 0);
    port_out(base, 0x08, 0);

    let fl_lo3 = port_in(base, 0x14);
    let fl_hi3 = port_in(base, 0x16);
    crate::serial_println!("[UHCI] After clear: FLBASEADD=0x{:04X}:{:04X}", fl_hi3, fl_lo3);

    port_out8(base, 0x14, 0x00);
    delay_ms(2);
    port_out8(base, 0x15, 0x00);
    delay_ms(2);

    let fl_lo4 = port_in(base, 0x14);
    let fl_hi4 = port_in(base, 0x16);
    crate::serial_println!("[UHCI] After 16b zero: FLBASEADD=0x{:04X}:{:04X}", fl_hi4, fl_lo4);

    port_out8(base, 0x14, (flbase & 0xFF) as u8);
    delay_ms(2);
    port_out8(base, 0x15, ((flbase >> 8) & 0xFF) as u8);
    delay_ms(2);
    port_out8(base, 0x16, ((flbase >> 16) & 0xFF) as u8);
    delay_ms(2);
    port_out8(base, 0x17, ((flbase >> 24) & 0xFF) as u8);
    delay_ms(5);

    let fl_lo5 = port_in(base, 0x14);
    let fl_hi5 = port_in(base, 0x16);
    crate::serial_println!("[UHCI] After 32b FLBASEADD: FLBASEADD=0x{:04X}:{:04X}", fl_hi5, fl_lo5);

    port_out(base, 0, USBCMD_RS | USBCMD_FLE);
    delay_ms(50);

    let cmd2 = port_in(base, 0);
    let sts2 = port_in(base, 0x04);
    let fn2 = port_in(base, 0x0C);
    crate::serial_println!("[UHCI] After FLE+RS: CMD=0x{:04X} STS=0x{:04X} FRNUM={}", cmd2, sts2, fn2);

    let hch = (sts2 & USBSTS_HCH) != 0;
    let frnum_ok = fn2 != 64 && fn2 != 0;

    if !hch && frnum_ok {
        crate::serial_println!("[UHCI] Controller running!");
        return true;
    }

    crate::serial_println!("[UHCI] HCHalted={} FRNUM={}", hch, fn2);
    false
}

fn ctrl_transfer(
    base: u16,
    dev_addr: u8,
    ep: u8,
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    buf: &mut [u8],
) -> i32 {
    let td0_ptr = unsafe { ptr::addr_of!(TD_BUFFER[0]) as u32 };
    let td1_ptr = unsafe { ptr::addr_of!(TD_BUFFER[1]) as u32 };
    let td2_ptr = unsafe { ptr::addr_of!(TD_BUFFER[2]) as u32 };
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

        TD_BUFFER[0].buffer = data_ptr;
        TD_BUFFER[0].status = TD_IOC | TD_ACTIVE | (TD_MAX_ERRORS << 26);
        TD_BUFFER[0].token = PID_SETUP
            | ((ep as u32) << 8)
            | ((dev_addr as u32) << 13)
            | ((8u32 - 1) << 21);

        TD_BUFFER[1].buffer = data_ptr + 64;
        TD_BUFFER[1].status = TD_ACTIVE | (TD_MAX_ERRORS << 26);
        TD_BUFFER[1].token = PID_IN
            | ((ep as u32) << 8)
            | ((dev_addr as u32) << 13)
            | (((buf.len().saturating_sub(1)) as u32) << 21);

        TD_BUFFER[2].buffer = 0;
        TD_BUFFER[2].status = TD_IOC | TD_ACTIVE | (TD_MAX_ERRORS << 26);
        TD_BUFFER[2].token = PID_OUT
            | ((ep as u32) << 8)
            | ((dev_addr as u32) << 13)
            | (0u32 << 21);

        TD_BUFFER[0].link = td1_ptr | 0x04;
        TD_BUFFER[1].link = td2_ptr | 0x04;
        TD_BUFFER[2].link = 1;
        QH_BUFFER[0].vert_link = td0_ptr | 0x04;
    }

    let mut result = -1i32;
    for _ in 0..5_000_000 {
        let status = unsafe { TD_BUFFER[2].status };
        if status & TD_ACTIVE == 0 {
            let cc = status & 0x0F;
            if cc == 0 {
                let len_field = unsafe { TD_BUFFER[1].token };
                let actual_len = ((len_field >> 16) & 0x7FF) as usize;
                if actual_len > 0 && !buf.is_empty() {
                    let copy_len = actual_len.min(buf.len());
                    unsafe {
                        ptr::copy_nonoverlapping(
                            DATA_BUFFER.as_ptr().offset(64),
                            buf.as_mut_ptr(),
                            copy_len,
                        );
                    }
                }
                result = actual_len as i32;
                crate::serial_println!("[UHCI] Ctrl OK: {} bytes", result);
            } else {
                crate::serial_println!("[UHCI] Ctrl CC={}", cc);
            }
            break;
        }
        delay_us(5);
    }

    unsafe {
        TD_BUFFER[0].link = 1;
        TD_BUFFER[1].link = 1;
        TD_BUFFER[2].link = 1;
        QH_BUFFER[0].vert_link = 1;
    }

    if result < 0 {
        crate::serial_println!("[UHCI] Ctrl timeout");
    }
    result
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
    if get_report(base, addr, ep, &mut report) > 0 {
        super::hid::parse_hid_report(&report);
    }
}
