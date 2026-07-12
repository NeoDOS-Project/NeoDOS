#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU8, Ordering, AtomicU32};

#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

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
    fn hst_inb(port: u16) -> u8;
    fn hst_outb(port: u16, val: u8);
    fn hst_inw(port: u16) -> u16;
    fn hst_outw(port: u16, val: u16);
    fn hst_inl(port: u16) -> u32;
    fn hst_outl(port: u16, val: u32);
    fn hst_log(level: u32, msg: *const u8, len: usize) -> i64;
    fn hst_register_block_device(
        name: *const u8, name_len: u32, device_id: u32,
        num_sectors: u64, sector_size: u32,
        read_fn: unsafe extern "C" fn(u32, u64, u8, *mut u8) -> i32,
        write_fn: unsafe extern "C" fn(u32, u64, u8, *const u8) -> i32,
    ) -> i32;
    fn hst_unregister_block_device(dev_idx: i32) -> i32;
}

// ── Logging helper ───────────────────────────────────────────────────
fn logln(msg: &str) {
    let mut buf = [0u8; 128];
    let p = buf.as_mut_ptr();
    unsafe {
        // Write "[VIO] " prefix using raw pointer writes
        *p = b'[';
        *p.add(1) = b'V';
        *p.add(2) = b'I';
        *p.add(3) = b'O';
        *p.add(4) = b']';
        *p.add(5) = b' ';
        let src = msg.as_bytes();
        let src_ptr = src.as_ptr();
        let n = if src.len() < 121 { src.len() } else { 121 };
        let mut i = 0u32;
        while (i as usize) < n {
            core::ptr::write(p.add(6 + i as usize), *src_ptr.add(i as usize));
            i += 1;
        }
        hst_log(0, buf.as_ptr(), (6 + n) as usize);
    }
}

// ── PCI identifiers ──────────────────────────────────────────────────
const VIRTIO_VENDOR: u16 = 0x1AF4;
const VIRTIO_BLK_LEGACY: u16 = 0x1001;
const VIRTIO_BLK_MODERN: u16 = 0x1042;

const MAX_DEVICES: usize = 4;

// ── Legacy VirtIO I/O register offsets ──────────────────────────────
const REG_HOST_FEATURES: u16 = 0x00;
const REG_GUEST_FEATURES: u16 = 0x04;
const REG_QUEUE_NUM: u16 = 0x0C;
const REG_QUEUE_SEL: u16 = 0x0E;
const REG_QUEUE_NOTIFY: u16 = 0x10;
const REG_STATUS: u16 = 0x12;

// ── Status bits ──────────────────────────────────────────────────────
const STA_ACK: u8 = 0x01;
const STA_DRIVER: u8 = 0x02;
const STA_DRIVER_OK: u8 = 0x04;
const STA_FEATURES_OK: u8 = 0x08;
const STA_FAILED: u8 = 0x80;

// Feature bits we accept
const ACCEPTED_FEATURES: u32 = (1 << 1) | (1 << 2) | (1 << 6) | (1 << 14);

// Block request types
const BLK_T_IN: u32 = 0;
const BLK_T_OUT: u32 = 1;

// ── Virtqueue layout (64 descriptors, legacy single-page) ────────────
const QS: u16 = 64;
const DESC_SIZE: usize = 16;
const DESCS_BYTES: usize = QS as usize * DESC_SIZE; // 1024
const AVAIL_OFF: usize = DESCS_BYTES; // 1024
const USED_OFF: usize = 2048; // align(1024+4+128, 2048) = 2048
const PAGE_SIZE: usize = 4096;

// ── Per-device state ─────────────────────────────────────────────────
struct VirtioDevice {
    io_base: u16,
    #[allow(dead_code)]
    num_sectors: u64,
    #[allow(dead_code)]
    block_size: u32,
    dev_idx: i32,       // kernel block device index
    queue_page: [u8; PAGE_SIZE],
    dma_page: [u8; PAGE_SIZE],
    last_used_idx: u16,
}

static DEV_COUNT: AtomicU32 = AtomicU32::new(0);

// Static storage for up to MAX_DEVICES
static mut DEVICES: [Option<VirtioDevice>; MAX_DEVICES] = [None, None, None, None];

static INITIALIZED: AtomicU8 = AtomicU8::new(0);
static ACTIVE: AtomicU8 = AtomicU8::new(0);

// ── PCI config helpers (legacy PIO 0xCF8/0xCFC) ────────────────────
fn pci_read_word(bus: u8, dev: u8, func: u8, offset: u8) -> u16 {
    let addr = 0x8000_0000u32
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC);
    unsafe {
        hst_outl(0xCF8, addr);
        let dword = hst_inl(0xCFC);
        ((dword >> ((offset & 3) * 8)) & 0xFFFF) as u16
    }
}

fn pci_read_dword(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let addr = 0x8000_0000u32
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC);
    unsafe {
        hst_outl(0xCF8, addr);
        hst_inl(0xCFC)
    }
}

fn pci_write_word(bus: u8, dev: u8, func: u8, offset: u8, val: u16) {
    let addr = 0x8000_0000u32
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC);
    unsafe {
        hst_outl(0xCF8, addr);
        // Read dword, modify, write back
        let dword = hst_inl(0xCFC);
        let shift = (offset & 3) * 8;
        let mask = !(0xFFFFu32 << shift);
        let new = (dword & mask) | ((val as u32) << shift);
        hst_outl(0xCFC, new);
    }
}

// ── VirtIO legacy I/O helpers ────────────────────────────────────────
fn vio_read8(io: u16, reg: u16) -> u8 {
    unsafe { hst_inb(io + reg) }
}
fn vio_read16(io: u16, reg: u16) -> u16 {
    unsafe { hst_inw(io + reg) }
}
fn vio_read32(io: u16, reg: u16) -> u32 {
    unsafe { hst_inl(io + reg) }
}
fn vio_write8(io: u16, reg: u16, val: u8) {
    unsafe { hst_outb(io + reg, val) }
}
fn vio_write16(io: u16, reg: u16, val: u16) {
    unsafe { hst_outw(io + reg, val) }
}
fn vio_write32(io: u16, reg: u16, val: u32) {
    unsafe { hst_outl(io + reg, val) }
}

/// Probe for a VirtIO block device at the given PCI address.
/// Returns the I/O base address if found.
fn probe_device(bus: u8, dev: u8, func: u8) -> Option<u16> {
    let vendor = pci_read_word(bus, dev, func, 0);
    if vendor != VIRTIO_VENDOR { return None; }
    let did = pci_read_word(bus, dev, func, 2);
    if did != VIRTIO_BLK_LEGACY && did != VIRTIO_BLK_MODERN { return None; }

    // Enable PCI
    let cmd = pci_read_word(bus, dev, func, 0x04);
    pci_write_word(bus, dev, func, 0x04, cmd | 0x07);

    let bar0 = pci_read_dword(bus, dev, func, 0x10);
    if bar0 & 1 == 0 { return None; } // only legacy I/O BAR for now
    let io_base = (bar0 & 0xFFFC) as u16;
    if io_base == 0 { return None; }

    Some(io_base)
}

/// Initialize a VirtIO legacy device at the given I/O base.
/// Returns (num_sectors, block_size) if successful.
fn init_virtio(io: u16, sectors: &mut u64, block_size: &mut u32) -> bool {
    // Reset
    vio_write8(io, REG_STATUS, 0);
    // Wait for reset
    for _ in 0..1000 {
        if vio_read8(io, REG_STATUS) == 0 { break; }
    }

    // ACK | DRIVER
    vio_write8(io, REG_STATUS, STA_ACK);
    vio_write8(io, REG_STATUS, STA_ACK | STA_DRIVER);

    // Features
    let host_features = vio_read32(io, REG_HOST_FEATURES);
    let guest = host_features & ACCEPTED_FEATURES;
    vio_write32(io, REG_GUEST_FEATURES, guest);

    // FEATURES_OK
    vio_write8(io, REG_STATUS, STA_ACK | STA_DRIVER | STA_FEATURES_OK);
    if vio_read8(io, REG_STATUS) & STA_FAILED != 0 {
        return false;
    }

    // Read config: capacity (u64 at config offset 0)
    let cap_lo = vio_read32(io, 0x14);
    let cap_hi = vio_read32(io, 0x18);
    *sectors = (cap_hi as u64) << 32 | cap_lo as u64;
    *block_size = 512;

    // Setup queue
    vio_write16(io, REG_QUEUE_SEL, 0);
    let qsize = vio_read16(io, REG_QUEUE_NUM);
    if qsize == 0 { return false; }

    // DRIVER_OK
    vio_write8(io, REG_STATUS, STA_ACK | STA_DRIVER | STA_FEATURES_OK | STA_DRIVER_OK);

    true
}

/// Write a descriptor entry to the device's queue page.
fn write_desc(queue: &mut [u8; PAGE_SIZE], idx: u16, addr: u64, len: u32, flags: u16, next: u16) {
    let base = idx as usize * 16;
    let p = unsafe { queue.as_mut_ptr().add(base) } as *mut u64;
    unsafe {
        *p = addr;
        *p.add(1) = (len as u64) | ((flags as u64) << 32) | ((next as u64) << 48);
    }
}

/// Submit a block I/O request to a VirtIO device (synchronous poll).
fn vio_request(dev: &mut VirtioDevice, type_: u32, sector: u64, buf: *const u8, count: u8, is_write: bool) -> i32 {
    let io = dev.io_base;
    let dma = &mut dev.dma_page;
    let queue = &mut dev.queue_page;
    let data_len = (count as u32) * 512;

    // Build request header at dma+0
    unsafe {
        let req = dma.as_mut_ptr() as *mut u32;
        *req = type_;
        *req.add(1) = 0;
        *(req.add(2) as *mut u64) = sector;
    }
    // Write data for writes
    if is_write {
        unsafe {
            core::ptr::copy_nonoverlapping(buf, dma.as_mut_ptr().add(512), data_len as usize);
        }
    }
    // Status byte
    unsafe { *(dma.as_mut_ptr().add(16)) = 0; }

    // Build descriptor chain (3 entries: header, data, status)
    write_desc(queue, 0, dma.as_ptr() as u64, 16, 1, 1); // NEXT
    let data_flags = if is_write { 1u16 } else { 3u16 }; // NEXT or NEXT|WRITE
    write_desc(queue, 1, dma.as_ptr() as u64 + 512, data_len, data_flags, 2);
    write_desc(queue, 2, dma.as_ptr() as u64 + 16, 1, 2, 0); // WRITE (chain end)

    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

    // Submit to available ring
    let avail_idx_addr = unsafe { queue.as_mut_ptr().add(AVAIL_OFF + 2) } as *mut u16;
    let avail_ring_addr = unsafe { queue.as_mut_ptr().add(AVAIL_OFF + 4) } as *mut u16;
    let old_idx = unsafe { *avail_idx_addr };
    let slot = (old_idx as usize) % (QS as usize);
    unsafe { *avail_ring_addr.add(slot) = 0; }
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    unsafe { *avail_idx_addr = old_idx.wrapping_add(1); }
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

    // Doorbell
    vio_write16(io, REG_QUEUE_NOTIFY, 0);
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

    // Poll for completion
    let used_idx_addr = unsafe { queue.as_ptr().add(USED_OFF + 2) } as *const u16;
    for _ in 0..200_000_000 {
        let new = unsafe { *used_idx_addr };
        if new != dev.last_used_idx {
            dev.last_used_idx = dev.last_used_idx.wrapping_add(1);
            let status = unsafe { *dma.as_ptr().add(16) };
            if !is_write && status == 0 {
                unsafe {
                    core::ptr::copy_nonoverlapping(dma.as_ptr().add(512), buf as *mut u8, data_len as usize);
                }
            }
            return if status == 0 { 0 } else { -1 };
        }
        core::hint::spin_loop();
    }
    -1 // timeout
}

// ── Block device callbacks ───────────────────────────────────────────
unsafe extern "C" fn vio_read(device_id: u32, lba: u64, count: u8, buf: *mut u8) -> i32 {
    if (device_id as usize) >= MAX_DEVICES { return -1; }
    let dev = match get_device(device_id as usize) {
        Some(d) => d,
        None => return -1,
    };
    vio_request(dev, BLK_T_IN, lba, buf as *const u8, count.min(8), false)
}

unsafe extern "C" fn vio_write(device_id: u32, lba: u64, count: u8, buf: *const u8) -> i32 {
    if (device_id as usize) >= MAX_DEVICES { return -1; }
    let dev = match get_device(device_id as usize) {
        Some(d) => d,
        None => return -1,
    };
    vio_request(dev, BLK_T_OUT, lba, buf, count.min(8), true)
}

fn get_device(idx: usize) -> Option<&'static mut VirtioDevice> {
    if idx >= MAX_DEVICES { return None; }
    unsafe {
        let p = &raw mut DEVICES as *mut Option<VirtioDevice>;
        match *p.add(idx) {
            Some(ref mut d) => Some(d),
            None => None,
        }
    }
}

// ── NEM entry points ─────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn driver_init() -> i32 {
    logln("virtio-blk init");

    let mut found = 0u32;

    for bus in 0..=0 {
        for dev in 0..32 {
            for func in 0..8 {
                if found >= MAX_DEVICES as u32 { break; }

                let io = match probe_device(bus as u8, dev as u8, func as u8) {
                    Some(io) => io,
                    None => { if func == 0 { break; } continue; }
                };

                let mut sectors = 0u64;
                let mut block_size = 0u32;
                if !init_virtio(io, &mut sectors, &mut block_size) {
                    continue;
                }

                // Register block device
                let mut name_bytes = *b"VIRTIO0    \0";
                unsafe { *(name_bytes.as_mut_ptr().add(6)) = b'0' + found as u8; }
                let dev_idx = unsafe {
                    hst_register_block_device(
                        name_bytes.as_ptr(), 8, found,
                        sectors, block_size,
                        vio_read, vio_write,
                    )
                };
                if dev_idx < 0 { continue; }

                let vdev = VirtioDevice {
                    io_base: io,
                    num_sectors: sectors,
                    block_size,
                    dev_idx,
                    queue_page: [0u8; PAGE_SIZE],
                    dma_page: [0u8; PAGE_SIZE],
                    last_used_idx: 0,
                };

                unsafe {
                    let p = &raw const DEVICES as *const _ as *mut Option<VirtioDevice>;
                    core::ptr::write_volatile(p.add(found as usize), Some(vdev));
                }
                found += 1;

                // Debug message via raw output (no formatting)
                logln("registered device");
            }
            if found >= MAX_DEVICES as u32 { break; }
        }
        if found >= MAX_DEVICES as u32 { break; }
    }

    if found == 0 { return -1; }

    DEV_COUNT.store(found, Ordering::Relaxed);
    INITIALIZED.store(1, Ordering::Release);
    0
}

#[no_mangle]
pub extern "C" fn driver_activate() -> i32 {
    ACTIVE.store(1, Ordering::Release);
    0
}

#[no_mangle]
pub extern "C" fn driver_on_event(_event: *const NeoEvent) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn driver_fini() {
    ACTIVE.store(0, Ordering::Release);
    // Unregister devices
    let count = DEV_COUNT.load(Ordering::Relaxed);
    for i in 0..count {
        let ptr = &raw mut DEVICES as *mut Option<VirtioDevice>;
        unsafe {
            if let Some(ref mut dev) = *ptr.add(i as usize) {
                let _ = hst_unregister_block_device(dev.dev_idx);
                *ptr.add(i as usize) = None;
            }
        }
    }
    DEV_COUNT.store(0, Ordering::Relaxed);
    INITIALIZED.store(0, Ordering::Release);
}

#[no_mangle]
pub extern "C" fn driver_is_active() -> i32 {
    if ACTIVE.load(Ordering::Acquire) != 0 { 1 } else { 0 }
}

// ── compiler-rt builtins ─────────────────────────────────────────────
#[no_mangle]
pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    for i in 0..n { *dest.add(i) = *src.add(i); }
    dest
}

#[no_mangle]
pub unsafe extern "C" fn memset(s: *mut u8, c: i32, n: usize) -> *mut u8 {
    let v = c as u8;
    for i in 0..n { *s.add(i) = v; }
    s
}
