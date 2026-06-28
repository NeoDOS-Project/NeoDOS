#![no_std]
#![no_main]
#![allow(dead_code)]
#![allow(static_mut_refs)]

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

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
    fn hst_inb(port: u16) -> u8;
    fn hst_outb(port: u16, val: u8);
    fn hst_inw(port: u16) -> u16;
    fn hst_outw(port: u16, val: u16);
    fn hst_inl(port: u16) -> u32;
    fn hst_outl(port: u16, val: u32);
    fn hst_log(level: u32, msg: *const u8, len: usize);
    fn hst_register_block_device(
        name: *const u8,
        name_len: u32,
        device_id: u32,
        num_sectors: u64,
        sector_size: u32,
        read_fn: unsafe extern "C" fn(u32, u64, u8, *mut u8) -> i32,
        write_fn: unsafe extern "C" fn(u32, u64, u8, *const u8) -> i32,
    ) -> i32;
}

static INITIALIZED: AtomicU8 = AtomicU8::new(0);
static ACTIVE: AtomicU8 = AtomicU8::new(0);

// IDE controller PCI info
static IDE_BMBA: AtomicU32 = AtomicU32::new(0);
static IDE_BUS: AtomicU8 = AtomicU8::new(0xFF);
static IDE_DEV: AtomicU8 = AtomicU8::new(0xFF);
static IDE_FUNC: AtomicU8 = AtomicU8::new(0xFF);

// ── ATA port constants ──

const ATA_PRIMARY_DATA: u16 = 0x1F0;
const ATA_PRIMARY_ERROR: u16 = 0x1F1;
const ATA_PRIMARY_SECTOR_COUNT: u16 = 0x1F2;
const ATA_PRIMARY_LBA_LOW: u16 = 0x1F3;
const ATA_PRIMARY_LBA_MID: u16 = 0x1F4;
const ATA_PRIMARY_LBA_HIGH: u16 = 0x1F5;
const ATA_PRIMARY_DRIVE_SEL: u16 = 0x1F6;
const ATA_PRIMARY_COMMAND: u16 = 0x1F7;
const ATA_PRIMARY_STATUS: u16 = 0x1F7;

const ATA_SECONDARY_DATA: u16 = 0x170;
const ATA_SECONDARY_ERROR: u16 = 0x171;
const ATA_SECONDARY_SECTOR_COUNT: u16 = 0x172;
const ATA_SECONDARY_LBA_LOW: u16 = 0x173;
const ATA_SECONDARY_LBA_MID: u16 = 0x174;
const ATA_SECONDARY_LBA_HIGH: u16 = 0x175;
const ATA_SECONDARY_DRIVE_SEL: u16 = 0x176;
const ATA_SECONDARY_COMMAND: u16 = 0x177;
const ATA_SECONDARY_STATUS: u16 = 0x177;

const ATA_CMD_READ_PIO: u8 = 0x20;
const ATA_CMD_READ_MULTIPLE: u8 = 0xC4;
const ATA_CMD_WRITE_MULTIPLE: u8 = 0xC5;
const ATA_CMD_READ_DMA: u8 = 0xC8;
const ATA_CMD_WRITE_DMA: u8 = 0xCA;

const BM_COMMAND: u16 = 0x0;
const BM_STATUS: u16 = 0x2;
const BM_PRDT_ADDRESS: u16 = 0x4;

const BM_CMD_START: u8 = 0x01;
const BM_CMD_WRITE: u8 = 0x08;
const BM_STAT_ACTIVE: u8 = 0x01;
const BM_STAT_ERROR: u8 = 0x02;
const BM_STAT_INTERRUPT: u8 = 0x04;

const ATA_DRIVE_SELECT_LBA_BASE: u8 = 0xE0;
const ATA_DRIVE_SELECT_MASTER: u8 = 0xE0;
const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

// ── Per-channel ATA state ──

#[derive(Clone, Copy)]
struct AtaChannelState {
    data_port: u16,
    sector_count_port: u16,
    lba_low_port: u16,
    lba_mid_port: u16,
    lba_high_port: u16,
    drive_sel_port: u16,
    command_port: u16,
    status_port: u16,
    bmba: u16,
    present: u8,
    drive_type: u8,
    registered_dev_id: u32,
}

static mut CHANNELS: [AtaChannelState; 2] = [
    AtaChannelState {
        data_port: 0,
        sector_count_port: 0,
        lba_low_port: 0,
        lba_mid_port: 0,
        lba_high_port: 0,
        drive_sel_port: 0,
        command_port: 0,
        status_port: 0,
        bmba: 0,
        present: 0,
        drive_type: 0,
        registered_dev_id: 0,
    };
    2
];

// DMA buffers (4KB page-aligned)
#[repr(align(4096))]
struct DmaAligned([u8; 4096]);

#[repr(C, packed)]
struct PrdtEntry {
    data_buffer_phys: u32,
    count: u16,
    eot: u16,
}

static mut PRDT: DmaAligned = DmaAligned([0u8; 4096]);
static mut DMA_DATA: DmaAligned = DmaAligned([0u8; 4096]);

// ── PCI config access via HST ──

fn pci_config_read_dword(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let addr = 0x8000_0000u32
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC);
    unsafe {
        hst_outl(PCI_CONFIG_ADDRESS, addr);
        hst_inl(PCI_CONFIG_DATA)
    }
}

fn pci_config_read_word(bus: u8, dev: u8, func: u8, offset: u8) -> u16 {
    let dword = pci_config_read_dword(bus, dev, func, offset);
    ((dword >> ((offset & 3) * 8)) & 0xFFFF) as u16
}

fn pci_config_write_word(bus: u8, dev: u8, func: u8, offset: u8, value: u16) {
    let aligned = offset & 0xFC;
    let dword = pci_config_read_dword(bus, dev, func, aligned);
    let shift = (offset & 3) * 8;
    let mask = !(0xFFFFu32 << shift);
    let new_dword = (dword & mask) | ((value as u32) << shift);
    let addr = 0x8000_0000u32
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (aligned as u32 & 0xFC);
    unsafe {
        hst_outl(PCI_CONFIG_ADDRESS, addr);
        hst_outl(PCI_CONFIG_DATA, new_dword);
    }
}

fn pci_config_write_dword(bus: u8, dev: u8, func: u8, offset: u8, value: u32) {
    let addr = 0x8000_0000u32
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC);
    unsafe {
        hst_outl(PCI_CONFIG_ADDRESS, addr);
        hst_outl(PCI_CONFIG_DATA, value);
    }
}

// ── ATA helpers ──

fn wait_not_busy(status_port: u16) -> i32 {
    for _ in 0..1000000 {
        let status = unsafe { hst_inb(status_port) };
        if (status & 0x80) == 0 {
            return 0;
        }
    }
    -1
}

fn wait_data_ready(status_port: u16) -> i32 {
    for _ in 0..1000000 {
        let status = unsafe { hst_inb(status_port) };
        if (status & 0x08) != 0 {
            return 0;
        }
        if (status & 0x01) != 0 {
            return -1;
        }
    }
    -1
}

// ── Read callback for a registered block device ──

unsafe extern "C" fn ata_read(device_id: u32, lba: u64, count: u8, buf: *mut u8) -> i32 {
    let idx = device_id as usize;
    if idx >= 2 { return -1; }
    let ch = if idx == 0 { &CHANNELS[0] } else { &CHANNELS[1] };
    if ch.present == 0 { return -1; }

    let cnt = count.clamp(1, 8);
    let abs_lba = lba as u32;
    // Use DMA if available, PIO fallback
    if ch.bmba != 0 {
        return ata_read_dma(idx, abs_lba, cnt, buf);
    }

    // PIO multiple read
    if wait_not_busy(ch.status_port) != 0 { return -1; }
    hst_outb(ch.drive_sel_port, ATA_DRIVE_SELECT_LBA_BASE | ((abs_lba >> 24) & 0x0F) as u8);
    hst_outb(ch.sector_count_port, cnt);
    hst_outb(ch.lba_low_port, abs_lba as u8);
    hst_outb(ch.lba_mid_port, (abs_lba >> 8) as u8);
    hst_outb(ch.lba_high_port, (abs_lba >> 16) as u8);
    hst_outb(ch.command_port, ATA_CMD_READ_MULTIPLE);

    for s in 0..cnt as usize {
        if wait_data_ready(ch.status_port) != 0 { return -1; }
        let off = s * 256;
        for i in 0..256 {
            let word = hst_inw(ch.data_port);
            let dst = buf.add(off * 2 + i * 2);
            *dst = word as u8;
            *dst.add(1) = (word >> 8) as u8;
        }
    }
    0
}

unsafe fn ata_read_dma(idx: usize, lba: u32, count: u8, buf: *mut u8) -> i32 {
    let ch = &*CHANNELS.as_ptr().add(idx);
    let bmba = ch.bmba;
    let total_bytes = (count as usize) * 512;

    let prdt_phys = &raw const PRDT as *const _ as u32;
    let data_phys = &raw const DMA_DATA as *const _ as u32;

    let prdt_base = &raw mut PRDT.0 as *mut u8 as *mut PrdtEntry;
    core::ptr::write(prdt_base, PrdtEntry {
        data_buffer_phys: data_phys,
        count: (total_bytes as u16).min(0xFFFE),
        eot: 0x8000,
    });
    // Zero out entries 1..512 using pointer arithmetic
    let count_entries = if total_bytes > 4096 { 8usize } else { 1usize };
    for i in 1..count_entries.min(512) {
        core::ptr::write(prdt_base.add(i), PrdtEntry {
            data_buffer_phys: 0,
            count: 0,
            eot: 0,
        });
    }

    core::sync::atomic::fence(core::sync::atomic::Ordering::Release);

    hst_outb(bmba + BM_STATUS, BM_STAT_INTERRUPT);
    hst_outb(bmba + BM_COMMAND, 0x00);
    hst_outl(bmba + BM_PRDT_ADDRESS, prdt_phys);

    if wait_not_busy(ch.status_port) != 0 { return -1; }
    hst_outb(ch.drive_sel_port, ATA_DRIVE_SELECT_LBA_BASE | ((lba >> 24) & 0x0F) as u8);
    hst_outb(ch.sector_count_port, count);
    hst_outb(ch.lba_low_port, lba as u8);
    hst_outb(ch.lba_mid_port, (lba >> 8) as u8);
    hst_outb(ch.lba_high_port, (lba >> 16) as u8);
    hst_outb(ch.command_port, ATA_CMD_READ_DMA);

    hst_outb(bmba + BM_COMMAND, BM_CMD_START);

    for _ in 0..2000000 {
        let st = hst_inb(bmba + BM_STATUS);
        if (st & BM_STAT_ACTIVE) == 0 {
            if (st & BM_STAT_ERROR) != 0 || (hst_inb(ch.status_port) & 0x01) != 0 {
                return -1;
            }
            let src = DMA_DATA.0.as_ptr();
            for i in 0..total_bytes {
                *buf.add(i) = *src.add(i);
            }
            return 0;
        }
    }
    -1
}

// ── Write callback for a registered block device ──

unsafe extern "C" fn ata_write(device_id: u32, lba: u64, count: u8, buf: *const u8) -> i32 {
    let idx = device_id as usize;
    if idx >= 2 { return -1; }
    let ch = if idx == 0 { &CHANNELS[0] } else { &CHANNELS[1] };
    if ch.present == 0 { return -1; }

    let cnt = count.clamp(1, 8);
    let abs_lba = lba as u32;

    // Use DMA if available, PIO fallback
    if ch.bmba != 0 {
        return ata_write_dma(idx, abs_lba, cnt, buf);
    }

    // PIO multiple write
    if wait_not_busy(ch.status_port) != 0 { return -1; }
    hst_outb(ch.sector_count_port, cnt);
    hst_outb(ch.lba_low_port, (abs_lba & 0xFF) as u8);
    hst_outb(ch.lba_mid_port, ((abs_lba >> 8) & 0xFF) as u8);
    hst_outb(ch.lba_high_port, ((abs_lba >> 16) & 0xFF) as u8);
    let drive_byte = ATA_DRIVE_SELECT_LBA_BASE | ((abs_lba >> 24) & 0x0F) as u8;
    hst_outb(ch.drive_sel_port, drive_byte);
    hst_outb(ch.command_port, ATA_CMD_WRITE_MULTIPLE);

    if wait_not_busy(ch.status_port) != 0 { return -1; }

    for s in 0..cnt as usize {
        let off = s * 256;
        for i in 0..256 {
            let src = buf.add(off * 2 + i * 2);
            let word = u16::from_le_bytes([*src, *src.add(1)]);
            hst_outw(ch.data_port, word);
        }
    }

    if wait_not_busy(ch.status_port) != 0 { return -1; }
    0
}

unsafe fn ata_write_dma(idx: usize, lba: u32, count: u8, buf: *const u8) -> i32 {
    let ch = &*CHANNELS.as_ptr().add(idx);
    let bmba = ch.bmba;
    let total_bytes = (count as usize) * 512;

    let prdt_phys = &raw const PRDT as *const _ as u32;
    let data_phys = &raw const DMA_DATA as *const _ as u32;

    let dst = DMA_DATA.0.as_mut_ptr();
    for i in 0..total_bytes {
        *dst.add(i) = *buf.add(i);
    }

    let prdt_base = &raw mut PRDT.0 as *mut u8 as *mut PrdtEntry;
    core::ptr::write(prdt_base, PrdtEntry {
        data_buffer_phys: data_phys,
        count: (total_bytes as u16).min(0xFFFE),
        eot: 0x8000,
    });
    let count_entries = if total_bytes > 4096 { 8usize } else { 1usize };
    for i in 1..count_entries.min(512) {
        core::ptr::write(prdt_base.add(i), PrdtEntry {
            data_buffer_phys: 0,
            count: 0,
            eot: 0,
        });
    }

    core::sync::atomic::fence(core::sync::atomic::Ordering::Release);

    hst_outb(bmba + BM_STATUS, BM_STAT_INTERRUPT);
    hst_outb(bmba + BM_COMMAND, 0x00);
    hst_outl(bmba + BM_PRDT_ADDRESS, prdt_phys);

    if wait_not_busy(ch.status_port) != 0 { return -1; }
    hst_outb(ch.drive_sel_port, ATA_DRIVE_SELECT_LBA_BASE | ((lba >> 24) & 0x0F) as u8);
    hst_outb(ch.sector_count_port, count);
    hst_outb(ch.lba_low_port, (lba & 0xFF) as u8);
    hst_outb(ch.lba_mid_port, ((lba >> 8) & 0xFF) as u8);
    hst_outb(ch.lba_high_port, ((lba >> 16) & 0xFF) as u8);
    hst_outb(ch.command_port, ATA_CMD_WRITE_DMA);

    hst_outb(bmba + BM_COMMAND, BM_CMD_START | BM_CMD_WRITE);

    for _ in 0..2000000 {
        let st = hst_inb(bmba + BM_STATUS);
        if (st & BM_STAT_ACTIVE) == 0 {
            if (st & BM_STAT_ERROR) != 0 {
                return -1;
            }
            return 0;
        }
    }
    -1
}

// ── Probe IDE controller via PCI ──

fn find_ide_controller() -> i32 {
    for bus in 0..=0 {
        for dev in 0..32 {
            for func in 0..8 {
                let vendor = pci_config_read_word(bus, dev, func, 0);
                if vendor == 0xFFFF || vendor == 0 {
                    if func == 0 { break; }
                    continue;
                }

                let class_rev = pci_config_read_dword(bus, dev, func, 0x08);
                let class = ((class_rev >> 24) & 0xFF) as u8;
                let subclass = ((class_rev >> 16) & 0xFF) as u8;

                if class == 0x01 && subclass == 0x01 {
                    let prog_if = ((class_rev >> 8) & 0xFF) as u8;
                    let bar4 = pci_config_read_dword(bus, dev, func, 0x20);

                    if (prog_if & 0x80) != 0 && (bar4 & 0x01) != 0 {
                        let bmba = (bar4 & 0xFFF0) as u16;
                        if bmba != 0 {
                            // Enable bus-mastering
                            let cmd = pci_config_read_word(bus, dev, func, 0x04);
                            pci_config_write_word(bus, dev, func, 0x04, cmd | 0x04);

                            IDE_BMBA.store(bmba as u32, Ordering::Relaxed);
                            IDE_BUS.store(bus, Ordering::Relaxed);
                            IDE_DEV.store(dev, Ordering::Relaxed);
                            IDE_FUNC.store(func, Ordering::Relaxed);
                            return 0;
                        }
                    }
                    break;
                }
            }
        }
    }
    -1
}

// ── Init ATA channel ──

fn init_channel(ch: &mut AtaChannelState) -> i32 {
    // Probe for device on this channel
    let status = unsafe { hst_inb(ch.status_port) };
    if status == 0xFF {
        // No device present
        ch.present = 0;
        return -1;
    }
    ch.present = 1;
    0
}

// ── NEM driver entry points ──

#[no_mangle]
pub extern "C" fn driver_init() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) != 0 {
        return -1;
    }
    INITIALIZED.store(1, Ordering::Release);

    let msg = b"ata.nem: initializing\r\n";
    unsafe { hst_log(2, msg.as_ptr(), msg.len()) };

    // Initialize channel states with port addresses
    unsafe {
        CHANNELS[0] = AtaChannelState {
            data_port: ATA_PRIMARY_DATA,
            sector_count_port: ATA_PRIMARY_SECTOR_COUNT,
            lba_low_port: ATA_PRIMARY_LBA_LOW,
            lba_mid_port: ATA_PRIMARY_LBA_MID,
            lba_high_port: ATA_PRIMARY_LBA_HIGH,
            drive_sel_port: ATA_PRIMARY_DRIVE_SEL,
            command_port: ATA_PRIMARY_COMMAND,
            status_port: ATA_PRIMARY_STATUS,
            bmba: 0,
            present: 0,
            drive_type: 0,
            registered_dev_id: 0,
        };
        CHANNELS[1] = AtaChannelState {
            data_port: ATA_SECONDARY_DATA,
            sector_count_port: ATA_SECONDARY_SECTOR_COUNT,
            lba_low_port: ATA_SECONDARY_LBA_LOW,
            lba_mid_port: ATA_SECONDARY_LBA_MID,
            lba_high_port: ATA_SECONDARY_LBA_HIGH,
            drive_sel_port: ATA_SECONDARY_DRIVE_SEL,
            command_port: ATA_SECONDARY_COMMAND,
            status_port: ATA_SECONDARY_STATUS,
            bmba: 0,
            present: 0,
            drive_type: 0,
            registered_dev_id: 0,
        };
    }

    // Find IDE controller and set up DMA
    let has_bmba = find_ide_controller() == 0;
    if has_bmba {
        let bmba = IDE_BMBA.load(Ordering::Relaxed) as u16;
        unsafe {
            CHANNELS[0].bmba = bmba;
            CHANNELS[1].bmba = bmba + 8;
        }
        let mut log_buf = [0u8; 48];
        let prefix = b"ata.nem: DMA at 0x";
        unsafe {
            let mut pos = 0usize;
            for &b in prefix { *log_buf.get_unchecked_mut(pos) = b; pos += 1; }
            let h = |v: u8| -> u8 { if v < 10 { b'0' + v } else { b'A' + v - 10 } };
            *log_buf.get_unchecked_mut(pos) = h(((bmba >> 12) & 0xF) as u8); pos += 1;
            *log_buf.get_unchecked_mut(pos) = h(((bmba >> 8) & 0xF) as u8); pos += 1;
            *log_buf.get_unchecked_mut(pos) = h(((bmba >> 4) & 0xF) as u8); pos += 1;
            *log_buf.get_unchecked_mut(pos) = h((bmba & 0xF) as u8); pos += 1;
            *log_buf.get_unchecked_mut(pos) = b'\r'; pos += 1;
            *log_buf.get_unchecked_mut(pos) = b'\n'; pos += 1;
            hst_log(2, log_buf.as_ptr(), pos);
        }
    } else {
        let msg = b"ata.nem: no DMA, using PIO\r\n";
        unsafe { hst_log(2, msg.as_ptr(), msg.len()) };
    }

    // Probe channels
    for i in 0..2 {
        let ch = unsafe { &mut *CHANNELS.as_mut_ptr().add(i) };
        if init_channel(ch) == 0 {
            // Register block device
            let name: [u8; 8] = if i == 0 {
                *b"ATA0    "
            } else {
                *b"ATA1    "
            };
            let reg = unsafe {
                hst_register_block_device(
                    name.as_ptr(),
                    4,
                    i as u32,
                    0x0FFFFFFF,  // ~137 GB max
                    512,
                    ata_read,
                    ata_write,
                )
            };
            if reg >= 0 {
                ch.registered_dev_id = i as u32;
                let mut log_buf = [0u8; 48];
                let prefix = if i == 0 { b"ata.nem: ATA0 reg=" } else { b"ata.nem: ATA1 reg=" };
                unsafe {
                    let mut pos = 0usize;
                    for &b in prefix { *log_buf.get_unchecked_mut(pos) = b; pos += 1; }
                    *log_buf.get_unchecked_mut(pos) = b'0' + (reg as u8); pos += 1;
                    *log_buf.get_unchecked_mut(pos) = b'\r'; pos += 1;
                    *log_buf.get_unchecked_mut(pos) = b'\n'; pos += 1;
                    hst_log(2, log_buf.as_ptr(), pos);
                }
            } else {
                let msg = if i == 0 { b"ata.nem: ATA0 reg FAIL\r\n" } else { b"ata.nem: ATA1 reg FAIL\r\n" };
                unsafe { hst_log(2, msg.as_ptr(), msg.len()) };
            }
        } else {
            let msg = if i == 0 { b"ata.nem: ATA0 not present\r\n" } else { b"ata.nem: ATA1 not present\r\n" };
            unsafe { hst_log(2, msg.as_ptr(), msg.len()) };
        }
    }

    let ok = b"ata.nem: init done\r\n";
    unsafe { hst_log(2, ok.as_ptr(), ok.len()) };
    0
}

#[no_mangle]
pub extern "C" fn driver_activate() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) == 0 {
        return -1;
    }
    ACTIVE.store(1, Ordering::Release);
    let msg = b"ata.nem: activated\r\n";
    unsafe { hst_log(2, msg.as_ptr(), msg.len()) };
    0
}

#[no_mangle]
pub extern "C" fn driver_on_event(event: *const NeoEvent) -> i32 {
    if ACTIVE.load(Ordering::Relaxed) == 0 || event.is_null() {
        return -1;
    }
    // ATA does not need to handle events currently
    0
}

#[no_mangle]
pub extern "C" fn driver_fini() {
    ACTIVE.store(0, Ordering::Release);
    INITIALIZED.store(0, Ordering::Release);
    let msg = b"ata.nem: shutdown\r\n";
    unsafe { hst_log(2, msg.as_ptr(), msg.len()) };
}

#[no_mangle]
pub extern "C" fn driver_is_active() -> i32 {
    if ACTIVE.load(Ordering::Relaxed) != 0 { 1 } else { 0 }
}
