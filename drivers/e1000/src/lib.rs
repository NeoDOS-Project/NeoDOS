#![no_std]
#![no_main]
#![allow(dead_code, non_camel_case_types)]

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

// ── Panic handler ──

#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// ── LLVM runtime intrinsics ──

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    for i in 0..n { *dest.add(i) = *src.add(i); }
    dest
}
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    if dest < src as *mut u8 {
        for i in 0..n { *dest.add(i) = *src.add(i); }
    } else {
        let mut i = n;
        while i > 0 { i -= 1; *dest.add(i) = *src.add(i); }
    }
    dest
}
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn memset(s: *mut u8, c: i32, n: usize) -> *mut u8 {
    for i in 0..n { *s.add(i) = c as u8; }
    s
}

// ── Hex formatting helpers for no_std logging ──

const HEX_CHARS: &[u8; 16] = b"0123456789ABCDEF";

fn hex_nibble(v: u8) -> u8 {
    HEX_CHARS[v as usize]
}

fn write_hex_byte(buf: &mut [u8], pos: &mut usize, b: u8) {
    unsafe {
        *buf.get_unchecked_mut(*pos) = hex_nibble(b >> 4); *pos += 1;
        *buf.get_unchecked_mut(*pos) = hex_nibble(b & 0xF); *pos += 1;
    }
}

fn write_hex32(buf: &mut [u8], pos: &mut usize, v: u32) {
    unsafe {
        for i in 0..8 {
            *buf.get_unchecked_mut(*pos) = hex_nibble(((v >> (28 - i * 4)) & 0xF) as u8);
            *pos += 1;
        }
    }
}

fn write_str(buf: &mut [u8], pos: &mut usize, s: &str) {
    let bytes = s.as_bytes();
    unsafe {
        for &b in bytes {
            *buf.get_unchecked_mut(*pos) = b;
            *pos += 1;
        }
    }
}

fn write_u8(buf: &mut [u8], pos: &mut usize, v: u8) {
    unsafe {
        if v >= 100 { *buf.get_unchecked_mut(*pos) = b'0' + (v / 100); *pos += 1; }
        if v >= 10 { *buf.get_unchecked_mut(*pos) = b'0' + ((v / 10) % 10); *pos += 1; }
        *buf.get_unchecked_mut(*pos) = b'0' + (v % 10); *pos += 1;
    }
}

// ── NeoEvent (ABI-stable, matches kernel's eventbus::Event) ──

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

// ── HST exports (C ABI from kernel) ──

extern "C" {
    fn hst_inb(port: u16) -> u8;
    fn hst_outb(port: u16, val: u8);
    fn hst_inl(port: u16) -> u32;
    fn hst_outl(port: u16, val: u32);
    fn hst_log(level: u32, msg: *const u8, len: usize);
    fn hst_push_event(et: u32, src: u32, dev: u32, d0: u64, d1: u64, fl: u32) -> i64;
    fn hst_get_ticks() -> u64;
    fn hst_ack_irq(vector: u8);
    fn hst_ecam_read_dword(bus: u8, dev: u8, func: u8, offset: u8) -> u32;
    fn hst_register_network_device(
        name: *const u8, name_len: u32,
        mac_addr: *const u8,
        vendor_id: u32,
        device_id: u32,
        desc: *const u8,
        desc_len: u32,
        send_fn: unsafe extern "C" fn(u32, *const u8, u32) -> i32,
        poll_fn: unsafe extern "C" fn(u32, *mut u8, *mut u32) -> i32,
    ) -> i32;
    fn hst_unregister_network_device(nic_id: i32) -> i32;
    fn hst_virt_to_phys(virt: u64) -> u64;
}

fn log_bytes(buf: &[u8]) {
    unsafe { hst_log(0, buf.as_ptr(), buf.len()); }
}

fn log(msg: &str) {
    unsafe { hst_log(0, msg.as_ptr(), msg.len()); }
}

// ── e1000 PCI device IDs ──

const VENDOR_INTEL: u16 = 0x8086;
const DEV_82540EM: u16 = 0x100E;
const DEV_82543GC: u16 = 0x1004;
const DEV_82545EM: u16 = 0x100F;
const DEV_82574L: u16 = 0x10D3;

// ── MMIO registers (offsets from BAR0) ──

const REG_CTRL: u32 = 0x0000;
const REG_STATUS: u32 = 0x0008;
const REG_EECD: u32 = 0x0010;
const REG_ICR: u32 = 0x00C0;
const REG_IMS: u32 = 0x00D0;
const REG_RCTRL: u32 = 0x0100;
const REG_TCTRL: u32 = 0x0400;
const REG_RDBAL: u32 = 0x2800;
const REG_RDBAH: u32 = 0x2804;
const REG_RDLEN: u32 = 0x2808;
const REG_RDH: u32 = 0x2810;
const REG_RDT: u32 = 0x2818;
const REG_TDBAL: u32 = 0x3800;
const REG_TDBAH: u32 = 0x3804;
const REG_TDLEN: u32 = 0x3808;
const REG_TDH: u32 = 0x3810;
const REG_TDT: u32 = 0x3818;
const REG_MTA: u32 = 0x5200;
const REG_RA: u32 = 0x5400;
const REG_RAH: u32 = 0x5404;

// ── Register flags ──

const RCTL_EN: u32 = 0x00000002;
const RCTL_UPE: u32 = 0x00000008;
const RCTL_MPE: u32 = 0x00000010;
const RCTL_BAM: u32 = 0x00008000;
const RCTL_SECRC: u32 = 0x04000000;
const RCTL_SZ_2048: u32 = 0x00000000;
const TCTL_EN: u32 = 0x00000002;
const TCTL_PSP: u32 = 0x00000008;
const TCTL_CT: u32 = 0x00000F00;
const TCTL_COLD: u32 = 0x003F0000;
const CMD_EOP: u8 = 0x01;
const CMD_IFCS: u8 = 0x02;
const CMD_RS: u8 = 0x08;
const STATUS_LINK_UP: u32 = 0x00000002;
const CTRL_SLU: u32 = 0x00000040;
const EECD_EE_PRES: u32 = 0x00000010;

const NUM_RX_DESC: usize = 32;
const NUM_TX_DESC: usize = 16;
const RX_BUF_SIZE: usize = 2048;

// ── Descriptor structures ──

#[repr(C, packed)]
struct RxDesc {
    addr: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

#[repr(C, packed)]
struct TxDesc {
    addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

// ── Driver state ──

struct E1000State {
    mmio_base: u32,
    nic_id: i32,
    bus: u8,
    dev: u8,
    func: u8,
    mac: [u8; 6],
    rx_cur: usize,
    tx_cur: usize,
}

static INITIALIZED: AtomicU8 = AtomicU8::new(0);
static ACTIVE: AtomicU8 = AtomicU8::new(0);
static MMIO_BASE: AtomicU32 = AtomicU32::new(0);
static NIC_ID: AtomicU32 = AtomicU32::new(0xFFFFFFFF);
static RX_CUR: AtomicU32 = AtomicU32::new(0);
static TX_CUR: AtomicU32 = AtomicU32::new(0);

// Static DMA buffers (4K-aligned for descriptor rings)
#[repr(align(4096))]
struct Aligned4k([u8; 4096]);

static mut RX_DESCS: Aligned4k = Aligned4k([0u8; 4096]);
static mut TX_DESCS: Aligned4k = Aligned4k([0u8; 4096]);

// Packet buffers (aligned to 64 bytes for DMA)
const BUF_POOL_SIZE: usize = NUM_RX_DESC * RX_BUF_SIZE + NUM_TX_DESC * RX_BUF_SIZE;
#[repr(align(64))]
struct Aligned64([u8; BUF_POOL_SIZE]);
static mut BUF_POOL: Aligned64 = Aligned64([0u8; BUF_POOL_SIZE]);

// ── MMIO helpers ──

fn mmio_base() -> u32 { MMIO_BASE.load(Ordering::Relaxed) }

fn read_reg(reg: u32) -> u32 {
    unsafe { core::ptr::read_volatile((mmio_base() as u64 + reg as u64) as *const u32) }
}

fn write_reg(reg: u32, val: u32) {
    unsafe { core::ptr::write_volatile((mmio_base() as u64 + reg as u64) as *mut u32, val); }
}

fn read_mac_from_bar(base: u32) -> [u8; 6] {
    let lo = unsafe { core::ptr::read_volatile((base as u64 + REG_RA as u64) as *const u32) };
    let hi = unsafe { core::ptr::read_volatile((base as u64 + REG_RAH as u64) as *const u32) };
    [
        (lo & 0xFF) as u8, ((lo >> 8) & 0xFF) as u8,
        ((lo >> 16) & 0xFF) as u8, ((lo >> 24) & 0xFF) as u8,
        (hi & 0xFF) as u8, ((hi >> 8) & 0xFF) as u8,
    ]
}

// ── PCI config space access ──

fn pci_config_read(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let is_active = unsafe { hst_ecam_read_dword(0, 0, 0, 0) != 0xFFFFFFFF };
    if is_active {
        unsafe { hst_ecam_read_dword(bus, dev, func, offset) }
    } else {
        let addr = 0x80000000u32 | (bus as u32) << 16 | (dev as u32) << 11 | (func as u32) << 8 | (offset as u32 & 0xFC);
        unsafe { hst_outl(0xCF8, addr); hst_inl(0xCFC) }
    }
}

fn pci_config_write(bus: u8, dev: u8, func: u8, offset: u8, value: u32) {
    let addr = 0x80000000u32 | (bus as u32) << 16 | (dev as u32) << 11 | (func as u32) << 8 | (offset as u32 & 0xFC);
    unsafe { hst_outl(0xCF8, addr); hst_outl(0xCFC, value); }
}

fn pci_read_bar(bus: u8, dev: u8, func: u8, bar: u8) -> u32 {
    pci_config_read(bus, dev, func, 0x10 + bar * 4)
}

// ── e1000 hardware init ──

unsafe fn init_e1000_hw(mmio: u32) -> bool {
    MMIO_BASE.store(mmio, Ordering::Relaxed);
    // Reset NIC
    write_reg(REG_CTRL, 0);
    let ctrl = read_reg(REG_CTRL);
    write_reg(REG_CTRL, ctrl | CTRL_SLU);

    // Initialize RX
    write_reg(REG_RCTRL, RCTL_EN | RCTL_UPE | RCTL_MPE | RCTL_BAM | RCTL_SZ_2048 | RCTL_SECRC);

    // Set up RX descriptor ring (translate virtual → physical for DMA)
    let rx_virt = &raw const RX_DESCS as u64;
    let rx_phys = hst_virt_to_phys(rx_virt);
    if rx_phys == 0 { return false; }
    write_reg(REG_RDBAL, (rx_phys & 0xFFFFFFFF) as u32);
    write_reg(REG_RDBAH, (rx_phys >> 32) as u32);
    write_reg(REG_RDLEN, (NUM_RX_DESC * core::mem::size_of::<RxDesc>()) as u32);
    write_reg(REG_RDH, 0);
    write_reg(REG_RDT, (NUM_RX_DESC - 1) as u32);

    // Initialize RX descriptor buffers
    let rx_descs = core::slice::from_raw_parts_mut(
        &raw mut RX_DESCS.0 as *mut u8 as *mut RxDesc, NUM_RX_DESC
    );
    for (i, desc) in rx_descs.iter_mut().enumerate() {
        let buf_virt = (&BUF_POOL.0[i * RX_BUF_SIZE] as *const u8) as u64;
        let buf_phys = hst_virt_to_phys(buf_virt);
        if buf_phys == 0 { return false; }
        desc.addr = buf_phys;
        desc.status = 0;
    }

    // Initialize TX
    write_reg(REG_TCTRL, TCTL_EN | TCTL_PSP | TCTL_CT | TCTL_COLD);

    let tx_virt = &raw const TX_DESCS as u64;
    let tx_phys = hst_virt_to_phys(tx_virt);
    if tx_phys == 0 { return false; }
    write_reg(REG_TDBAL, (tx_phys & 0xFFFFFFFF) as u32);
    write_reg(REG_TDBAH, (tx_phys >> 32) as u32);
    write_reg(REG_TDLEN, (NUM_TX_DESC * core::mem::size_of::<TxDesc>()) as u32);
    write_reg(REG_TDH, 0);
    write_reg(REG_TDT, 0);

    // Enable interrupts
    write_reg(REG_IMS, 0x1F6DC);
    write_reg(REG_ICR, 0xFFFFFFFF);

    true
}

// ── C-callable callbacks for kernel NIC registry ──

unsafe extern "C" fn e1000_send(device_id: u32, buf: *const u8, len: u32) -> i32 {
    let _ = device_id;
    let tx_cur = TX_CUR.load(Ordering::Relaxed) as usize % NUM_TX_DESC;
    let pkt_len = (len as usize).min(RX_BUF_SIZE - 4);
    if pkt_len == 0 { return -1; }

    let tx_offset = NUM_RX_DESC * RX_BUF_SIZE + tx_cur * RX_BUF_SIZE;
    let tx_buf_ptr = (&BUF_POOL.0[tx_offset]) as *const u8 as *mut u8;
    for i in 0..pkt_len {
        *tx_buf_ptr.add(i) = *buf.add(i);
    }

    let tx_descs = core::slice::from_raw_parts_mut(
        &raw mut TX_DESCS.0 as *mut u8 as *mut TxDesc, NUM_TX_DESC
    );
    let desc = &mut tx_descs[tx_cur];
    desc.addr = hst_virt_to_phys(tx_buf_ptr as u64);
    desc.length = pkt_len as u16;
    desc.cmd = CMD_EOP | CMD_IFCS | CMD_RS;
    desc.status = 0;

    core::sync::atomic::fence(core::sync::atomic::Ordering::Release);

    let old_tdt = read_reg(REG_TDT);
    write_reg(REG_TDT, old_tdt.wrapping_add(1) % NUM_TX_DESC as u32);
    TX_CUR.store((tx_cur + 1) as u32, Ordering::Relaxed);
    0
}

unsafe extern "C" fn e1000_poll(device_id: u32, buf: *mut u8, out_len: *mut u32) -> i32 {
    let _ = device_id;
    let rx_cur = RX_CUR.load(Ordering::Relaxed) as usize % NUM_RX_DESC;

    let rx_descs = core::slice::from_raw_parts_mut(
        &raw mut RX_DESCS.0 as *mut u8 as *mut RxDesc, NUM_RX_DESC
    );

    if rx_descs[rx_cur].status & 0x01 == 0 {
        return -1; // No packet ready
    }

    let pkt_len = rx_descs[rx_cur].length as usize;
    if pkt_len == 0 || pkt_len > RX_BUF_SIZE {
        rx_descs[rx_cur].status = 0;
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
        let old_rdt = read_reg(REG_RDT);
        write_reg(REG_RDT, (old_rdt + 1) % NUM_RX_DESC as u32);
        RX_CUR.store((rx_cur + 1) as u32, Ordering::Relaxed);
        return -1;
    }

    let rx_buf = &BUF_POOL.0[rx_cur * RX_BUF_SIZE..];
    let rx_src = rx_buf.as_ptr();
    for i in 0..pkt_len {
        *buf.add(i) = *rx_src.add(i);
    }
    *out_len = pkt_len as u32;

    // Return buffer to NIC
    rx_descs[rx_cur].status = 0;
    core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
    let old_rdt = read_reg(REG_RDT);
    write_reg(REG_RDT, (old_rdt + 1) % NUM_RX_DESC as u32);
    RX_CUR.store((rx_cur + 1) as u32, Ordering::Relaxed);
    0
}

// ── Probe PCI for e1000 ──

fn log_probe_msg(bus: u8, dev: u8, func: u8, mac: &[u8; 6], mmio: u32) {
    let mut buf = [0u8; 128];
    let mut p = 0;
    write_str(&mut buf, &mut p, "[E1000] Found at ");
    write_u8(&mut buf, &mut p, bus); write_str(&mut buf, &mut p, ":");
    write_u8(&mut buf, &mut p, dev); write_str(&mut buf, &mut p, ".");
    write_u8(&mut buf, &mut p, func);
    write_str(&mut buf, &mut p, " MAC=");
    write_hex_byte(&mut buf, &mut p, mac[0]); write_str(&mut buf, &mut p, ":");
    write_hex_byte(&mut buf, &mut p, mac[1]); write_str(&mut buf, &mut p, ":");
    write_hex_byte(&mut buf, &mut p, mac[2]); write_str(&mut buf, &mut p, ":");
    write_hex_byte(&mut buf, &mut p, mac[3]); write_str(&mut buf, &mut p, ":");
    write_hex_byte(&mut buf, &mut p, mac[4]); write_str(&mut buf, &mut p, ":");
    write_hex_byte(&mut buf, &mut p, mac[5]);
    write_str(&mut buf, &mut p, " MMIO=0x");
    write_hex32(&mut buf, &mut p, mmio);
    unsafe { hst_log(0, buf.as_ptr(), p); }
}

fn log_registered(nic_id: i32) {
    let mut buf = [0u8; 48];
    let mut p = 0;
    write_str(&mut buf, &mut p, "[E1000] Registered as NIC ");
    if nic_id >= 10 { buf[p] = b'0' + (nic_id / 10) as u8; p += 1; }
    buf[p] = b'0' + (nic_id % 10) as u8; p += 1;
    unsafe { hst_log(0, buf.as_ptr(), p); }
}

fn probe_e1000() -> bool {
    for bus in 0..=1 {
        for dev in 0..32 {
            for func in 0..8 {
                let vendor_lo = pci_config_read(bus, dev, func, 0);
                let vendor = (vendor_lo & 0xFFFF) as u16;
                let device = ((vendor_lo >> 16) & 0xFFFF) as u16;
                if vendor != VENDOR_INTEL { continue; }
                match device {
                    DEV_82540EM | DEV_82543GC | DEV_82545EM | DEV_82574L => {},
                    _ => continue,
                }
                let bar0 = pci_read_bar(bus, dev, func, 0);
                let mmio = bar0 & 0xFFFFFFF0;
                if mmio == 0 { continue; }

                let mac = read_mac_from_bar(mmio);
                log_probe_msg(bus, dev, func, &mac, mmio);

                // Enable bus mastering
                let cmd = pci_config_read(bus, dev, func, 4);
                pci_config_write(bus, dev, func, 4, cmd | 0x7);

            if !unsafe { init_e1000_hw(mmio) } { continue; }

            RX_CUR.store(0, Ordering::Relaxed);
                TX_CUR.store(0, Ordering::Relaxed);

                // Build driver name as stack buffer
                let mut name_buf = [0u8; 24];
                let mut np = 0;
                write_str(&mut name_buf, &mut np, "e1000_");
                write_hex_byte(&mut name_buf, &mut np, bus);
                write_hex_byte(&mut name_buf, &mut np, dev);

                // Build description based on detected device
                let desc_s = match device {
                    DEV_82540EM => "Intel 82540EM Gigabit Ethernet",
                    DEV_82543GC => "Intel 82543GC Gigabit Ethernet",
                    DEV_82545EM => "Intel 82545EM Gigabit Ethernet",
                    DEV_82574L => "Intel 82574L Gigabit Ethernet",
                    _ => "Intel e1000 Gigabit Ethernet",
                };

                let nic_id = unsafe {
                    hst_register_network_device(
                        name_buf.as_ptr(), np as u32,
                        mac.as_ptr(),
                        VENDOR_INTEL as u32,
                        device as u32,
                        desc_s.as_ptr(),
                        desc_s.len() as u32,
                        e1000_send, e1000_poll,
                    )
                };
                if nic_id >= 0 {
                    NIC_ID.store(nic_id as u32, Ordering::Relaxed);
                    log_registered(nic_id);
                    return true;
                }
                return false;
            }
        }
    }
    false
}

// ── Entry points ──

#[no_mangle]
pub extern "C" fn driver_init() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) != 0 { return -1; }

    let found = probe_e1000();
    if !found {
        log("[E1000] No e1000 NIC found");
        return -1;
    }

    INITIALIZED.store(1, Ordering::Release);
    log("[E1000] Initialized successfully");
    0
}

#[no_mangle]
pub extern "C" fn driver_activate() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) == 0 { return -1; }
    ACTIVE.store(1, Ordering::Release);
    0
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn driver_on_event(event: *const NeoEvent) -> i32 {
    if ACTIVE.load(Ordering::Relaxed) == 0 || event.is_null() { return -1; }
    let ev = unsafe { &*event };
    // Handle IRQ-triggered events or kernel dispatch
    // For polling-based NIC, events are not strictly needed
    _ = ev;
    0
}

#[no_mangle]
pub extern "C" fn driver_fini() {
    let nic_id = NIC_ID.load(Ordering::Relaxed);
    if nic_id != 0xFFFFFFFF {
        unsafe { hst_unregister_network_device(nic_id as i32); }
    }
    ACTIVE.store(0, Ordering::Release);
    INITIALIZED.store(0, Ordering::Release);
    log("[E1000] Unloaded");
}

#[no_mangle]
pub extern "C" fn driver_is_active() -> i32 {
    if ACTIVE.load(Ordering::Relaxed) != 0 { 1 } else { 0 }
}
