#![no_std]
#![no_main]
#![allow(dead_code)]

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU8, AtomicU32, Ordering, fence};

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    for i in 0..n {
        *dest.add(i) = *src.add(i);
    }
    dest
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

// ── AHCI constants ──

const MAX_PORTS: usize = 2;
const MAX_CMD_SLOTS: usize = 32;
const MAX_PRD_ENTRIES: usize = 8;
const DMA_BUF_SIZE: usize = 4096;

const HBA_CAP: usize = 0x00;
const HBA_GHC: usize = 0x04;
const HBA_IS: usize = 0x08;
const HBA_PI: usize = 0x0C;
const HBA_VS: usize = 0x10;

const HBA_GHC_AE: u32 = 0x8000_0000;
const HBA_GHC_HR: u32 = 0x0000_0001;

const PORT_STRIDE: usize = 0x80;
const PORT_REG_BASE: usize = 0x100;

const PORT_CLB: usize = 0x00;
const PORT_CLBU: usize = 0x04;
const PORT_FB: usize = 0x08;
const PORT_FBU: usize = 0x0C;
const PORT_IS: usize = 0x10;
const PORT_IE: usize = 0x14;
const PORT_CMD: usize = 0x18;
const PORT_TFD: usize = 0x20;
const PORT_SIG: usize = 0x24;
const PORT_SSTS: usize = 0x28;
const PORT_SCTL: usize = 0x2C;
const PORT_SERR: usize = 0x30;
const PORT_CI: usize = 0x38;

const CMD_ST: u32 = 0x0001;
const CMD_FRE: u32 = 0x0010;
const CMD_POD: u32 = 0x0002;
const CMD_SUD: u32 = 0x0004;
const CMD_CR: u32 = 0x8000;
const CMD_FR: u32 = 0x4000;

const SATA_SIG_ATA: u32 = 0x0000_0101;
const SATA_SIG_ATAPI: u32 = 0xEB14_0101;
const SATA_DET_PRESENT: u32 = 0x03;
const SATA_IPM_ACTIVE: u32 = 0x01;

const TFD_BSY: u32 = 0x80;
const TFD_DRQ: u32 = 0x08;
const TFD_ERR: u32 = 0x01;

const CMD_ATA: u16 = 0x0000;
const CFLAG_C: u16 = 1 << 15;
const CFLAG_P: u16 = 1 << 6;
const CFLAG_A: u16 = 1 << 13;
const ATA_CMD_READ_DMA_EXT: u8 = 0x25;
const ATA_CMD_WRITE_DMA_EXT: u8 = 0x35;
const ATA_CMD_READ_FPDMA_QUEUED: u8 = 0x60;
const ATA_CMD_WRITE_FPDMA_QUEUED: u8 = 0x61;
const ATA_CMD_IDENTIFY_DEVICE: u8 = 0xEC;
const ATA_CMD_PACKET: u8 = 0xA0;
const ATAPI_FEAT_DMA: u8 = 0x01;

const ATAPI_SECTOR_SIZE: usize = 2048;

/// NCQ constants
const NCQ_SLOT_COUNT: usize = 32;
const NCQ_SLOT_BUF_SIZE: usize = DMA_BUF_SIZE;
const PORT_SACT: usize = 0x34;
const NCQ_SUPPORT_BIT: u16 = 1 << 8;

// ── AHCI data structures ──

#[derive(Copy, Clone)]
#[repr(C, packed)]
struct PrdtEntry {
    data_base: u32,
    data_base_hi: u32,
    reserved: u32,
    count: u32,
}

#[repr(C, packed)]
struct CmdTableInner {
    cfis: [u8; 64],
    acmd: [u8; 16],
    reserved: [u8; 48],
    prdt: [PrdtEntry; MAX_PRD_ENTRIES],
}

#[derive(Copy, Clone)]
#[repr(C, packed)]
struct CmdHeader {
    opts: u16,
    prdtl: u16,
    prdbc: u32,
    ctba: u32,
    ctba_hi: u32,
    reserved: [u32; 4],
}

#[repr(C, packed)]
struct FisRegH2D {
    fis_type: u8,
    pmport: u8,
    command: u8,
    features: u8,
    lba0: u8,
    lba1: u8,
    lba2: u8,
    device: u8,
    lba3: u8,
    lba4: u8,
    lba5: u8,
    features_exp: u8,
    sector_count: u8,
    sector_count_exp: u8,
    _res: u8,
    control: u8,
}

#[repr(C, align(1024))]
struct CmdList([CmdHeader; MAX_CMD_SLOTS]);

#[repr(C, align(256))]
struct RecvFis([u8; 256]);

#[repr(C, align(128))]
struct CmdTable(CmdTableInner);

const EMPTY_CMD_HEADER: CmdHeader = CmdHeader {
    opts: 0, prdtl: 0, prdbc: 0, ctba: 0, ctba_hi: 0, reserved: [0; 4],
};
const EMPTY_PRD: PrdtEntry = PrdtEntry {
    data_base: 0, data_base_hi: 0, reserved: 0, count: 0,
};
const EMPTY_CMD_TABLE: CmdTableInner = CmdTableInner {
    cfis: [0; 64], acmd: [0; 16], reserved: [0; 48],
    prdt: [EMPTY_PRD; MAX_PRD_ENTRIES],
};

static mut PORT_CMD_LIST: [CmdList; MAX_PORTS] = [
    CmdList([EMPTY_CMD_HEADER; MAX_CMD_SLOTS]),
    CmdList([EMPTY_CMD_HEADER; MAX_CMD_SLOTS]),
];
static mut PORT_RECV_FIS: [RecvFis; MAX_PORTS] = [
    RecvFis([0; 256]),
    RecvFis([0; 256]),
];
static mut PORT_CMD_TABLE: [CmdTable; MAX_PORTS] = [
    CmdTable(EMPTY_CMD_TABLE),
    CmdTable(EMPTY_CMD_TABLE),
];
static mut PORT_DMA_BUF: [[u8; DMA_BUF_SIZE]; MAX_PORTS] = [
    [0; DMA_BUF_SIZE],
    [0; DMA_BUF_SIZE],
];

// ── NCQ per-slot buffers (32 slots × 4 KB per port ──
// Flat byte arrays cast to proper types at access time to avoid
// compiler-inserted bounds checks that would require core panicking symbols.
const NCQ_CT_SIZE: usize = core::mem::size_of::<CmdTable>();
const NCQ_TOTAL_CTS: usize = MAX_PORTS * NCQ_SLOT_COUNT * NCQ_CT_SIZE;
const NCQ_TOTAL_DMA: usize = MAX_PORTS * NCQ_SLOT_COUNT * NCQ_SLOT_BUF_SIZE;
#[repr(C, align(128))]
struct NcqCtStorage([u8; NCQ_TOTAL_CTS]);
#[repr(C, align(64))]
struct NcqDmaStorage([u8; NCQ_TOTAL_DMA]);
static mut PORT_NCQ_RAW_CT: NcqCtStorage = NcqCtStorage([0; NCQ_TOTAL_CTS]);
static mut PORT_NCQ_RAW_DMA: NcqDmaStorage = NcqDmaStorage([0; NCQ_TOTAL_DMA]);

// ── Driver state ──

#[derive(Copy, Clone, PartialEq)]
enum DeviceType {
    Ata,
    Atapi,
}

struct AhciPortState {
    phys_port: u8,
    dev_type: DeviceType,
    present: u8,
    ncq_supported: u8,
    /// Bitmask of in-flight tags (0 = free, 1 = in use)
    tag_busy: u32,
}

static mut PORT_STATE: [AhciPortState; MAX_PORTS] = [
    AhciPortState { phys_port: 0, dev_type: DeviceType::Ata, present: 0, ncq_supported: 0, tag_busy: 0 },
    AhciPortState { phys_port: 0, dev_type: DeviceType::Ata, present: 0, ncq_supported: 0, tag_busy: 0 },
];

// HBA pointer and metadata (stored as u32 since AHCI BAR5 is < 4 GB)
static HBA_PTR: AtomicU32 = AtomicU32::new(0);
static PORT_COUNT: AtomicU8 = AtomicU8::new(0);

// ── PCI config access via HST I/O ports ──

const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

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

// ── MMIO helpers ──

fn hba_reg(hba: *mut u32, reg: usize) -> *mut u32 {
    unsafe { hba.add(reg / 4) }
}

fn port_reg(hba: *mut u32, port: usize, reg: usize) -> *mut u32 {
    unsafe { hba.add((PORT_REG_BASE + port * PORT_STRIDE + reg) / 4) }
}

fn mmio_read32(addr: *mut u32) -> u32 {
    unsafe { addr.read_volatile() }
}

fn mmio_write32(addr: *mut u32, val: u32) {
    unsafe { addr.write_volatile(val) }
}

// ── Logging helper ──

fn log_msg(msg: &[u8]) {
    unsafe { hst_log(2, msg.as_ptr(), msg.len()) }
}

fn log_hex(prefix: &[u8], val: u32) {
    let mut buf = [0u8; 48];
    let mut pos = 0usize;
    for &b in prefix { if pos < buf.len() { buf[pos] = b; pos += 1; } }
    let hex = |v: u8| -> u8 { if v < 10 { b'0' + v } else { b'A' + v - 10 } };
    pos += 8;
    let mut v = val;
    for i in 0..8 {
        let idx = pos - 1 - i;
        if idx < buf.len() {
            buf[idx] = hex((v & 0xF) as u8);
            v >>= 4;
        }
    }
    if pos < buf.len() - 2 {
        buf[pos] = b'\r'; buf[pos + 1] = b'\n';
        unsafe { hst_log(2, buf.as_ptr(), pos + 2) }
    }
}

fn log_str(s: &[u8]) {
    let mut buf = [0u8; 128];
    let mut pos = 0usize;
    for &b in s { if pos < buf.len() { buf[pos] = b; pos += 1; } }
    if pos < buf.len() - 2 {
        buf[pos] = b'\r'; buf[pos + 1] = b'\n';
        unsafe { hst_log(2, buf.as_ptr(), pos + 2) }
    }
}

// ── AHCI controller discovery ──

fn find_ahci_controller() -> Option<u32> {
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
                if class == 0x01 && subclass == 0x06 {
                    let bar5 = pci_config_read_dword(bus, dev, func, 0x24);
                    let bar5_addr = bar5 & 0xFFFF_FFF0;
                    log_str(b"ahci.nem: found AHCI controller");
                    log_hex(b"  BAR5=0x", bar5_addr);
                    let cmd = pci_config_read_word(bus, dev, func, 0x04);
                    pci_config_write_word(bus, dev, func, 0x04, cmd | 0x06);
                    log_str(b"ahci.nem: bus mastering enabled");
                    return Some(bar5_addr);
                }
            }
        }
    }
    None
}

// ── Port initialization ──

fn port_init(hba: *mut u32, port: usize) {
    if port >= MAX_PORTS { return; }
    unsafe {
        let p = port_reg(hba, port, 0);

        let cmd = mmio_read32(p.add(PORT_CMD / 4));
        if (cmd & CMD_ST) != 0 || (cmd & CMD_FRE) != 0 {
            mmio_write32(p.add(PORT_CMD / 4), cmd & !(CMD_ST | CMD_FRE));
            for _ in 0..10000 {
                let c = mmio_read32(p.add(PORT_CMD / 4));
                if (c & (CMD_CR | CMD_FR)) == 0 { break; }
            }
        }

        mmio_write32(p.add(PORT_IS / 4), 0xFFFFFFFF);

        let clb = core::ptr::addr_of!(PORT_CMD_LIST).add(port) as u32;
        let fb = core::ptr::addr_of!(PORT_RECV_FIS).add(port) as u32;
        mmio_write32(p.add(PORT_CLB / 4), clb);
        mmio_write32(p.add(PORT_CLBU / 4), 0);
        mmio_write32(p.add(PORT_FB / 4), fb);
        mmio_write32(p.add(PORT_FBU / 4), 0);

        mmio_write32(p.add(PORT_IE / 4), 0);
        mmio_write32(p.add(PORT_SERR / 4), 0xFFFFFFFF);

        mmio_write32(p.add(PORT_CMD / 4), CMD_ST | CMD_FRE | CMD_POD | CMD_SUD);

        for _ in 0..10000 {
            let c = mmio_read32(p.add(PORT_CMD / 4));
            if (c & CMD_CR) == 0 { break; }
        }
    }
}

fn port_reset(hba: *mut u32, port: usize) {
    unsafe {
        let p = port_reg(hba, port, 0);

        let cmd = mmio_read32(p.add(PORT_CMD / 4));
        mmio_write32(p.add(PORT_CMD / 4), cmd & !(CMD_ST | CMD_FRE));
        for _ in 0..10000 {
            let c = mmio_read32(p.add(PORT_CMD / 4));
            if (c & (CMD_CR | CMD_FR)) == 0 { break; }
        }

        mmio_write32(p.add(PORT_SCTL / 4), 0x0301);
        for _ in 0..1000 { core::hint::spin_loop(); }

        mmio_write32(p.add(PORT_SCTL / 4), 0x0300);
        for _ in 0..100000 { core::hint::spin_loop(); }

        let ssts = mmio_read32(p.add(PORT_SSTS / 4));
        if (ssts & 0x0F) == SATA_DET_PRESENT {
            port_init(hba, port);
        }
    }
}

// ── DMA helpers ──

fn build_read10_cdb(lba: u32, count: u8) -> [u8; 12] {
    [
        0x28, 0x00,
        (lba >> 24) as u8, (lba >> 16) as u8,
        (lba >> 8) as u8, lba as u8,
        0x00, 0x00, count, 0x00, 0x00, 0x00,
    ]
}

fn dma_xfer(hba: *mut u32, port: usize, pi: usize, lba: u64, count: u8, is_write: bool) -> i32 {
    let total = (count as usize) * 512;
    let prd_count = (total + DMA_BUF_SIZE - 1) / DMA_BUF_SIZE;
    let prd_used = prd_count.min(MAX_PRD_ENTRIES);

    unsafe {
        let p = port_reg(hba, port, 0);
        let table_ptr: *mut CmdTableInner = (core::ptr::addr_of_mut!(PORT_CMD_TABLE) as *mut CmdTable).add(pi).cast();
        let table = &mut *table_ptr;

        table.cfis = [0u8; 64];
        table.acmd = [0u8; 16];
        table.reserved = [0u8; 48];

        let fis = FisRegH2D {
            fis_type: 0x27,
            pmport: 0x80,
            command: if is_write { ATA_CMD_WRITE_DMA_EXT } else { ATA_CMD_READ_DMA_EXT },
            features: 0,
            lba0: lba as u8,
            lba1: (lba >> 8) as u8,
            lba2: (lba >> 16) as u8,
            device: 0x40,
            lba3: (lba >> 24) as u8,
            lba4: (lba >> 32) as u8,
            lba5: (lba >> 40) as u8,
            features_exp: 0,
            sector_count: count,
            sector_count_exp: 0,
            _res: 0,
            control: 0,
        };
        let fis_bytes = &fis as *const FisRegH2D as *const u8;
        for i in 0..64 { table.cfis[i] = fis_bytes.add(i).read(); }

        for e in table.prdt.iter_mut() {
            *e = EMPTY_PRD;
        }
        for i in 0..prd_used {
            let off = i * DMA_BUF_SIZE;
            let remain = total.saturating_sub(off);
            let chunk = remain.min(DMA_BUF_SIZE);
            let dma_buf_ptr = (core::ptr::addr_of!(PORT_DMA_BUF) as *const u8).add(pi * DMA_BUF_SIZE + off);
            table.prdt[i].data_base = dma_buf_ptr as u32;
            table.prdt[i].data_base_hi = 0;
            table.prdt[i].count = (chunk as u32 - 1) | 0x8000_0000;
        }

        fence(Ordering::Release);

        let ctba = core::ptr::addr_of!(PORT_CMD_TABLE).add(pi) as u32;
        let cl_ptr: *mut [CmdHeader; MAX_CMD_SLOTS] = &mut (*(core::ptr::addr_of_mut!(PORT_CMD_LIST) as *mut CmdList).add(pi)).0;
        let cl = &mut *cl_ptr;
        cl[0].opts = CMD_ATA | CFLAG_C | CFLAG_P;
        cl[0].prdtl = prd_used as u16;
        cl[0].prdbc = 0;
        cl[0].ctba = ctba;
        cl[0].ctba_hi = 0;
        cl[1..].fill(EMPTY_CMD_HEADER);

        mmio_write32(p.add(PORT_IE / 4), 0);
        mmio_write32(p.add(PORT_IS / 4), 0xFFFFFFFF);

        mmio_write32(p.add(PORT_CI / 4), 1);

        for _ in 0..1000000 {
            let ci = mmio_read32(p.add(PORT_CI / 4));
            if (ci & 1) == 0 { break; }
        }

        if (mmio_read32(p.add(PORT_CI / 4)) & 1) != 0 {
            log_hex(b"ahci: DMA timeout port=", port as u32);
            port_reset(hba, pi);
            return -1;
        }

        let tfd = mmio_read32(p.add(PORT_TFD / 4));
        if (tfd & TFD_ERR) != 0 || (tfd & TFD_BSY) != 0 {
            let serr = mmio_read32(p.add(PORT_SERR / 4));
            log_hex(b"ahci: DMA error TFD=0x", tfd);
            log_hex(b"  SERR=0x", serr);
            mmio_write32(p.add(PORT_SERR / 4), serr);
            port_reset(hba, pi);
            return -1;
        }

        fence(Ordering::Acquire);
    }
    0
}

fn dma_packet(hba: *mut u32, port: usize, pi: usize, lba: u32, count: u8) -> i32 {
    let total = (count as usize) * ATAPI_SECTOR_SIZE;
    let prd_used = ((total + DMA_BUF_SIZE - 1) / DMA_BUF_SIZE).min(MAX_PRD_ENTRIES);

    unsafe {
        let p = port_reg(hba, port, 0);
        let table_ptr: *mut CmdTableInner = (core::ptr::addr_of_mut!(PORT_CMD_TABLE) as *mut CmdTable).add(pi).cast();
        let table = &mut *table_ptr;

        table.cfis = [0u8; 64];
        table.acmd = [0u8; 16];
        table.reserved = [0u8; 48];

        let fis = FisRegH2D {
            fis_type: 0x27,
            pmport: 0x80,
            command: ATA_CMD_PACKET,
            features: ATAPI_FEAT_DMA,
            lba0: 0, lba1: 0, lba2: 0,
            device: 0x00,
            lba3: 0, lba4: 0, lba5: 0,
            features_exp: 0,
            sector_count: count,
            sector_count_exp: 0,
            _res: 0,
            control: 0,
        };
        let fis_bytes = &fis as *const FisRegH2D as *const u8;
        for i in 0..64 { table.cfis[i] = fis_bytes.add(i).read(); }

        let cdb = build_read10_cdb(lba, count);
        for i in 0..12 { table.acmd[i] = cdb[i]; }

        for e in table.prdt.iter_mut() {
            *e = EMPTY_PRD;
        }
        for i in 0..prd_used {
            let off = i * DMA_BUF_SIZE;
            let remain = total.saturating_sub(off);
            let chunk = remain.min(DMA_BUF_SIZE);
            let dma_buf_ptr = (core::ptr::addr_of!(PORT_DMA_BUF) as *const u8).add(pi * DMA_BUF_SIZE + off);
            table.prdt[i].data_base = dma_buf_ptr as u32;
            table.prdt[i].data_base_hi = 0;
            table.prdt[i].count = (chunk as u32 - 1) | 0x8000_0000;
        }

        fence(Ordering::Release);

        let ctba = core::ptr::addr_of!(PORT_CMD_TABLE).add(pi) as u32;
        let cl_ptr: *mut [CmdHeader; MAX_CMD_SLOTS] = &mut (*(core::ptr::addr_of_mut!(PORT_CMD_LIST) as *mut CmdList).add(pi)).0;
        let cl = &mut *cl_ptr;
        cl[0].opts = CMD_ATA | CFLAG_C | CFLAG_P | CFLAG_A;
        cl[0].prdtl = prd_used as u16;
        cl[0].prdbc = 0;
        cl[0].ctba = ctba;
        cl[0].ctba_hi = 0;
        cl[1..].fill(EMPTY_CMD_HEADER);

        mmio_write32(p.add(PORT_IE / 4), 0);
        mmio_write32(p.add(PORT_IS / 4), 0xFFFFFFFF);

        mmio_write32(p.add(PORT_CI / 4), 1);

        for _ in 0..1000000 {
            let ci = mmio_read32(p.add(PORT_CI / 4));
            if (ci & 1) == 0 { break; }
        }

        if (mmio_read32(p.add(PORT_CI / 4)) & 1) != 0 {
            log_hex(b"ahci: PACKET timeout port=", port as u32);
            port_reset(hba, pi);
            return -1;
        }

        let tfd = mmio_read32(p.add(PORT_TFD / 4));
        if (tfd & TFD_ERR) != 0 || (tfd & TFD_BSY) != 0 {
            let serr = mmio_read32(p.add(PORT_SERR / 4));
            log_hex(b"ahci: PACKET error TFD=0x", tfd);
            log_hex(b"  SERR=0x", serr);
            mmio_write32(p.add(PORT_SERR / 4), serr);
            port_reset(hba, pi);
            return -1;
        }

        fence(Ordering::Acquire);
    }
    0
}

// ── NCQ helpers (all array access via unsafe ptr, no bounds checks) ──

/// Allocate a free NCQ tag. Returns None if all 32 tags busy.
fn ncq_tag_alloc(tag_busy: &mut u32) -> Option<u8> {
    for i in 0..NCQ_SLOT_COUNT {
        if (*tag_busy & (1u32 << i)) == 0 {
            *tag_busy |= 1u32 << i;
            return Some(i as u8);
        }
    }
    None
}

/// Free an NCQ tag.
fn ncq_tag_free(tag_busy: &mut u32, tag: u8) {
    *tag_busy &= !(1u32 << tag);
}

/// Execute a single NCQ FPDMA queued command (read or write).
/// Returns 0 on success, -1 on error.
unsafe fn ncq_setup_fis(table: &mut CmdTableInner, lba: u64, count: u8, is_write: bool, tag: u8) {
    table.cfis = [0u8; 64];
    table.acmd = [0u8; 16];
    table.reserved = [0u8; 48];
    table.cfis[0] = 0x27;
    table.cfis[1] = 0x80;
    table.cfis[2] = if is_write { ATA_CMD_WRITE_FPDMA_QUEUED } else { ATA_CMD_READ_FPDMA_QUEUED };
    table.cfis[4] = lba as u8;
    table.cfis[5] = (lba >> 8) as u8;
    table.cfis[6] = (lba >> 16) as u8;
    table.cfis[7] = 0x40;
    table.cfis[8] = (lba >> 24) as u8;
    table.cfis[9] = (lba >> 32) as u8;
    table.cfis[10] = (lba >> 40) as u8;
    table.cfis[12] = (tag & 0x1F) << 3;
    table.cfis[13] = 0;
    table.cfis[3] = (tag >> 3) & 0x03;
}

unsafe fn ncq_setup_prdt(table: &mut CmdTableInner, total: usize, slot_buf: *mut u8) -> u16 {
    for e in table.prdt.iter_mut() { *e = EMPTY_PRD; }
    let nprd = ((total + DMA_BUF_SIZE - 1) / DMA_BUF_SIZE).min(MAX_PRD_ENTRIES);
    let prdt_ptr = table.prdt.as_mut_ptr();
    for p in 0..nprd {
        let off = p * DMA_BUF_SIZE;
        let remain = total - off;
        let chunk = if remain > DMA_BUF_SIZE { DMA_BUF_SIZE } else { remain };
        (*prdt_ptr.add(p)).data_base = slot_buf.add(off) as u32;
        (*prdt_ptr.add(p)).data_base_hi = 0;
        (*prdt_ptr.add(p)).count = (chunk as u32 - 1) | 0x8000_0000;
    }
    nprd as u16
}

unsafe fn ncq_setup_header(cl: &mut [CmdHeader; MAX_CMD_SLOTS], tag: u8, nprd: u16, table_addr: u32) {
    let h = &mut *cl.as_mut_ptr().add(tag as usize);
    h.opts = CMD_ATA | CFLAG_C | CFLAG_P;
    h.prdtl = nprd;
    h.prdbc = 0;
    h.ctba = table_addr;
    h.ctba_hi = 0;
}

fn ncq_dma_xfer(hba: *mut u32, port: usize, pi: usize, lba: u64, count: u8, is_write: bool, tag: u8) -> i32 {
    let total = (count as usize) * 512;
    unsafe {
        let p = port_reg(hba, port, 0);

        let ct_base = core::ptr::addr_of!(PORT_NCQ_RAW_CT) as *const u8 as *mut CmdTable;
        let table: &mut CmdTableInner = &mut (*(ct_base.add(pi * NCQ_SLOT_COUNT + tag as usize))).0;
        let table_addr = table as *mut CmdTableInner as u32;

        ncq_setup_fis(table, lba, count, is_write, tag);

        let slot_buf = (core::ptr::addr_of!(PORT_NCQ_RAW_DMA) as *const u8 as *mut u8)
            .add((pi * NCQ_SLOT_COUNT + tag as usize) * NCQ_SLOT_BUF_SIZE);
        let nprd = ncq_setup_prdt(table, total, slot_buf);

        fence(Ordering::Release);

        let sact_addr = port_reg(hba, port, PORT_SACT);
        mmio_write32(sact_addr, 1u32 << tag);

        let cl_ptr: *mut [CmdHeader; MAX_CMD_SLOTS] =
            &mut (*(core::ptr::addr_of_mut!(PORT_CMD_LIST) as *mut CmdList).add(pi)).0;
        ncq_setup_header(&mut *cl_ptr, tag, nprd, table_addr);

        fence(Ordering::SeqCst);
        mmio_write32(p.add(PORT_CI / 4), 1u32 << tag);

        for _ in 0..1_000_000 {
            let ci = mmio_read32(p.add(PORT_CI / 4));
            if (ci & (1u32 << tag)) == 0 { break; }
            core::hint::spin_loop();
        }

        if (mmio_read32(p.add(PORT_CI / 4)) & (1u32 << tag)) != 0 {
            log_hex(b"ahci: NCQ timeout tag=", tag as u32);
            port_reset(hba, pi);
            return -1;
        }

        let tfd = mmio_read32(p.add(PORT_TFD / 4));
        if (tfd & TFD_ERR) != 0 || (tfd & TFD_BSY) != 0 {
            let serr = mmio_read32(p.add(PORT_SERR / 4));
            log_hex(b"ahci: NCQ error TFD=0x", tfd);
            log_hex(b"  SERR=0x", serr);
            if serr != 0 { mmio_write32(p.add(PORT_SERR / 4), serr); }
            port_reset(hba, pi);
            return -1;
        }

        fence(Ordering::Acquire);
    }
    0
}

fn ncq_batch_xfer(hba: *mut u32, port: usize, pi: usize,
    cmds: &[(u64, u8, bool)], tags: &[u8], results: &mut [i32])
{
    let n = cmds.len().min(NCQ_SLOT_COUNT);
    if n == 0 { return; }
    unsafe {
        let p = port_reg(hba, port, 0);

        let mut sact_mask: u32 = 0;
        for i in 0..n {
            sact_mask |= 1u32 << *tags.get_unchecked(i);
        }
        mmio_write32(p.add(PORT_SACT / 4), sact_mask);
        fence(Ordering::Release);

        let mut ci_mask: u32 = 0;
        let ct_base = core::ptr::addr_of!(PORT_NCQ_RAW_CT) as *const u8 as *mut CmdTable;

        for i in 0..n {
            let &(lba, count, is_write) = cmds.get_unchecked(i);
            let tag = *tags.get_unchecked(i);

            let table: &mut CmdTableInner = &mut (*(ct_base.add(pi * NCQ_SLOT_COUNT + tag as usize))).0;
            let table_addr = table as *mut CmdTableInner as u32;

            ncq_setup_fis(table, lba, count, is_write, tag);

            let total = (count as usize) * 512;
            let slot_buf = (core::ptr::addr_of!(PORT_NCQ_RAW_DMA) as *const u8 as *mut u8)
                .add((pi * NCQ_SLOT_COUNT + tag as usize) * NCQ_SLOT_BUF_SIZE);
            let nprd = ncq_setup_prdt(table, total, slot_buf);

            let cl_ptr: *mut [CmdHeader; MAX_CMD_SLOTS] =
                &mut (*(core::ptr::addr_of_mut!(PORT_CMD_LIST) as *mut CmdList).add(pi)).0;
            ncq_setup_header(&mut *cl_ptr, tag, nprd, table_addr);

            ci_mask |= 1u32 << tag;
        }

        fence(Ordering::SeqCst);
        mmio_write32(p.add(PORT_CI / 4), ci_mask);

        for _ in 0..10_000_000 {
            let ci = mmio_read32(p.add(PORT_CI / 4));
            if (ci & ci_mask) == 0 { break; }
            core::hint::spin_loop();
        }

        for i in 0..n {
            let tag = *tags.get_unchecked(i);
            let ci = mmio_read32(p.add(PORT_CI / 4));
            if (ci & (1u32 << tag)) != 0 {
                *results.get_unchecked_mut(i) = -1;
                continue;
            }
            let tfd = mmio_read32(p.add(PORT_TFD / 4));
            if (tfd & TFD_ERR) != 0 {
                let serr = mmio_read32(p.add(PORT_SERR / 4));
                log_hex(b"ahci: NCQ batch err tag=", tag as u32);
                log_hex(b"  serr=0x", serr);
                if serr != 0 { mmio_write32(p.add(PORT_SERR / 4), serr); }
                *results.get_unchecked_mut(i) = -1;
                continue;
            }
            *results.get_unchecked_mut(i) = 0;
        }

        fence(Ordering::Acquire);
    }
}

/// Send IDENTIFY DEVICE to detect NCQ support (word 76 bit 8).
fn identify_ncq_supported(hba: *mut u32, port: usize, pi: usize) -> bool {
    unsafe {
        let p = port_reg(hba, port, 0);
        let table_ptr: *mut CmdTableInner =
            (core::ptr::addr_of_mut!(PORT_CMD_TABLE) as *mut CmdTable).add(pi).cast();
        let table = &mut *table_ptr;
        let dbuf = (core::ptr::addr_of!(PORT_DMA_BUF) as *mut u8).add(pi * DMA_BUF_SIZE);

        table.cfis = [0u8; 64];
        table.cfis[0] = 0x27;
        table.cfis[1] = 0x80;
        table.cfis[2] = ATA_CMD_IDENTIFY_DEVICE;
        table.cfis[7] = 0x40;

        for e in table.prdt.iter_mut() { *e = EMPTY_PRD; }
        table.prdt[0].data_base = dbuf as u32;
        table.prdt[0].data_base_hi = 0;
        table.prdt[0].count = (512 - 1) | 0x8000_0000;

        fence(Ordering::Release);

        let ctba = core::ptr::addr_of!(PORT_CMD_TABLE).add(pi) as u32;
        let cl_ptr: *mut [CmdHeader; MAX_CMD_SLOTS] =
            &mut (*(core::ptr::addr_of_mut!(PORT_CMD_LIST) as *mut CmdList).add(pi)).0;
        let cl = &mut *cl_ptr;
        cl[0].opts = CMD_ATA | CFLAG_C | CFLAG_P;
        cl[0].prdtl = 1;
        cl[0].prdbc = 0;
        cl[0].ctba = ctba;
        cl[0].ctba_hi = 0;
        cl[1..].fill(EMPTY_CMD_HEADER);

        mmio_write32(p.add(PORT_IE / 4), 0);
        mmio_write32(p.add(PORT_IS / 4), 0xFFFFFFFF);
        mmio_write32(p.add(PORT_CI / 4), 1);

        for _ in 0..10_000_000 {
            let ci = mmio_read32(p.add(PORT_CI / 4));
            if (ci & 1) == 0 { break; }
        }

        let tfd = mmio_read32(p.add(PORT_TFD / 4));
        if (tfd & (TFD_BSY | TFD_DRQ | TFD_ERR)) != 0 {
            return false;
        }

        let word76 = (dbuf as *const u16).add(76).read_volatile();
        (word76 & NCQ_SUPPORT_BIT) != 0
    }
}

// ── Block device callbacks ──

unsafe extern "C" fn ahci_read(device_id: u32, lba: u64, count: u8, buf: *mut u8) -> i32 {
    let pi = device_id as usize;
    if pi >= MAX_PORTS { return -1; }
    let ps = &*(core::ptr::addr_of!(PORT_STATE) as *const AhciPortState).add(pi);
    if ps.present == 0 { return -1; }
    let hba = HBA_PTR.load(Ordering::Relaxed) as *mut u32;
    if hba.is_null() { return -1; }
    let phys_port = ps.phys_port as usize;

    let cnt = if count < 1 { 1 } else if count > 8 { 8 } else { count };

    if ps.dev_type == DeviceType::Atapi {
        let total = (cnt as usize) * ATAPI_SECTOR_SIZE;
        if dma_packet(hba, phys_port, pi, lba as u32, cnt) != 0 {
            return -1;
        }
        let src = (core::ptr::addr_of!(PORT_DMA_BUF) as *const u8).add(pi * DMA_BUF_SIZE);
        core::ptr::copy_nonoverlapping(src, buf, total);
        0
    } else if ps.ncq_supported != 0 {
        // NCQ path: allocate tag, issue FPDMA QUEUED READ
        let ps_mut = &mut *(core::ptr::addr_of_mut!(PORT_STATE) as *mut AhciPortState).add(pi);
        let tag = match ncq_tag_alloc(&mut ps_mut.tag_busy) {
            Some(t) => t,
            None => {
                // No free tag, fall back to legacy
                let total = (cnt as usize) * 512;
                if dma_xfer(hba, phys_port, pi, lba, cnt, false) != 0 {
                    return -1;
                }
                let src = (core::ptr::addr_of!(PORT_DMA_BUF) as *const u8).add(pi * DMA_BUF_SIZE);
                core::ptr::copy_nonoverlapping(src, buf, total);
                return 0;
            }
        };
        let rc = ncq_dma_xfer(hba, phys_port, pi, lba, cnt, false, tag);
        ncq_tag_free(&mut ps_mut.tag_busy, tag);
        if rc != 0 {
            return -1;
        }
        let slot_buf = (core::ptr::addr_of!(PORT_NCQ_RAW_DMA) as *const u8)
            .add((pi * NCQ_SLOT_COUNT + tag as usize) * NCQ_SLOT_BUF_SIZE);
        let total = (cnt as usize) * 512;
        core::ptr::copy_nonoverlapping(slot_buf, buf, total);
        0
    } else {
        let total = (cnt as usize) * 512;
        if dma_xfer(hba, phys_port, pi, lba, cnt, false) != 0 {
            return -1;
        }
        let src = (core::ptr::addr_of!(PORT_DMA_BUF) as *const u8).add(pi * DMA_BUF_SIZE);
        core::ptr::copy_nonoverlapping(src, buf, total);
        0
    }
}

unsafe extern "C" fn ahci_write(device_id: u32, lba: u64, count: u8, buf: *const u8) -> i32 {
    let pi = device_id as usize;
    if pi >= MAX_PORTS { return -1; }
    let ps = &*(core::ptr::addr_of!(PORT_STATE) as *const AhciPortState).add(pi);
    if ps.present == 0 { return -1; }
    let hba = HBA_PTR.load(Ordering::Relaxed) as *mut u32;
    if hba.is_null() { return -1; }
    let phys_port = ps.phys_port as usize;

    let cnt = if count < 1 { 1 } else if count > 8 { 8 } else { count };
    let total = (cnt as usize) * 512;

    if ps.ncq_supported != 0 {
        // NCQ path: pre-load buffer, issue FPDMA QUEUED WRITE
        let ps_mut = &mut *(core::ptr::addr_of_mut!(PORT_STATE) as *mut AhciPortState).add(pi);
        let tag = match ncq_tag_alloc(&mut ps_mut.tag_busy) {
            Some(t) => t,
            None => {
                // Fall back to legacy
                let dst = (core::ptr::addr_of_mut!(PORT_DMA_BUF) as *mut u8).add(pi * DMA_BUF_SIZE);
                core::ptr::copy_nonoverlapping(buf, dst, total);
                return if dma_xfer(hba, phys_port, pi, lba, cnt, true) != 0 { -1 } else { 0 };
            }
        };
        let slot_buf = (core::ptr::addr_of_mut!(PORT_NCQ_RAW_DMA) as *mut u8 as *mut u8)
            .add((pi * NCQ_SLOT_COUNT + tag as usize) * NCQ_SLOT_BUF_SIZE);
        core::ptr::copy_nonoverlapping(buf, slot_buf, total);
        let rc = ncq_dma_xfer(hba, phys_port, pi, lba, cnt, true, tag);
        ncq_tag_free(&mut ps_mut.tag_busy, tag);
        if rc != 0 { -1 } else { 0 }
    } else {
        let dst = (core::ptr::addr_of_mut!(PORT_DMA_BUF) as *mut u8).add(pi * DMA_BUF_SIZE);
        core::ptr::copy_nonoverlapping(buf, dst, total);
        if dma_xfer(hba, phys_port, pi, lba, cnt, true) != 0 {
            return -1;
        }
        0
    }
}

/// Batch NCQ read: issue up to `count` concurrent FPDMA QUEUED READ commands.
/// Returns number of successfully completed reads.
#[no_mangle]
pub unsafe extern "C" fn ahci_ncq_batch_read(
    device_id: u32, lba_base: u64, count: u32, sector_count: u8, bufs: *mut *mut u8,
) -> i32 {
    let pi = device_id as usize;
    if pi >= MAX_PORTS || count == 0 || count > NCQ_SLOT_COUNT as u32 { return -1; }
    let ps = &*(core::ptr::addr_of!(PORT_STATE) as *const AhciPortState).add(pi);
    if ps.present == 0 || ps.ncq_supported == 0 { return -1; }
    let hba = HBA_PTR.load(Ordering::Relaxed) as *mut u32;
    if hba.is_null() { return -1; }
    let phys_port = ps.phys_port as usize;

    let n = count as usize;
    let ps_mut = &mut *(core::ptr::addr_of_mut!(PORT_STATE) as *mut AhciPortState).add(pi);

    // Allocate tags (raw ptr arithmetic, no bounds checks)
    let mut tags = [0u8; NCQ_SLOT_COUNT];
    let mut tag_count = 0usize;
    for _ in 0..n {
        if let Some(tag) = ncq_tag_alloc(&mut ps_mut.tag_busy) {
            *tags.as_mut_ptr().add(tag_count) = tag;
            tag_count += 1;
        }
    }
    if tag_count == 0 { return 0; }

    // Build command list
    let mut cmds = [(0u64, 0u8, false); NCQ_SLOT_COUNT];
    let cmds_ptr = cmds.as_mut_ptr();
    let tags_ptr = tags.as_ptr();
    for i in 0..tag_count {
        *cmds_ptr.add(i) = (lba_base.wrapping_add(i as u64), sector_count, false);
    }

    // Execute batch
    let mut results = [-1i32; NCQ_SLOT_COUNT];
    ncq_batch_xfer(hba, phys_port, pi,
        core::slice::from_raw_parts(cmds_ptr, tag_count),
        core::slice::from_raw_parts(tags_ptr, tag_count),
        core::slice::from_raw_parts_mut(results.as_mut_ptr(), tag_count));

    // Copy back data and free tags (raw ptr arithmetic, no bounds checks)
    let results_ptr = results.as_ptr();
    let mut ok_count = 0i32;
    for i in 0..tag_count {
        let tag = *tags_ptr.add(i);
        let result = *results_ptr.add(i);
        if result == 0 {
            let slot_buf = (core::ptr::addr_of!(PORT_NCQ_RAW_DMA) as *const u8)
                .add((pi * NCQ_SLOT_COUNT + tag as usize) * NCQ_SLOT_BUF_SIZE);
            let total = (sector_count as usize) * 512;
            let user_buf = *bufs.add(i);
            core::ptr::copy_nonoverlapping(slot_buf, user_buf, total);
            ok_count += 1;
        }
        ncq_tag_free(&mut ps_mut.tag_busy, tag);
    }
    ok_count
}

// ── NEM driver entry points ──

#[no_mangle]
pub extern "C" fn driver_init() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) != 0 {
        return -1;
    }
    INITIALIZED.store(1, Ordering::Release);

    log_str(b"ahci.nem: initializing");

    let bar5 = match find_ahci_controller() {
        Some(a) => a,
        None => {
            log_str(b"ahci.nem: no AHCI controller found");
            return -1;
        }
    };

    HBA_PTR.store(bar5, Ordering::Relaxed);
    let hba = bar5 as *mut u32;

    // Enable AHCI
    let ghc = mmio_read32(hba_reg(hba, HBA_GHC));
    mmio_write32(hba_reg(hba, HBA_GHC), ghc | HBA_GHC_AE);

    let caps = mmio_read32(hba_reg(hba, HBA_CAP));
    let pi = mmio_read32(hba_reg(hba, HBA_PI));
    let _s64a = (caps >> 31) & 1;

    log_hex(b"ahci.nem: CAP=0x", caps);
    log_hex(b"ahci.nem: PI=0x", pi);

    let n_ports = (caps & 0x1F) as usize;
    let mut found = 0usize;

    for p in 0..32.min(n_ports + 1) {
        if found >= MAX_PORTS { break; }
        if (pi & (1 << p)) == 0 { continue; }

        let sig = mmio_read32(port_reg(hba, p, PORT_SIG));
        let ssts = mmio_read32(port_reg(hba, p, PORT_SSTS));
        let det = ssts & 0x0F;
        let ipm = (ssts >> 8) & 0x0F;

        if det != SATA_DET_PRESENT || ipm != SATA_IPM_ACTIVE {
            continue;
        }

        let dev_type = if sig == SATA_SIG_ATA {
            DeviceType::Ata
        } else if sig == SATA_SIG_ATAPI {
            DeviceType::Atapi
        } else {
            log_hex(b"ahci.nem: unknown sig=0x", sig);
            continue;
        };

        port_init(hba, p);

        // Detect NCQ support for ATA devices (needs port initialized first)
        let ncq_ok = if dev_type == DeviceType::Ata {
            identify_ncq_supported(hba, p, found)
        } else {
            false
        };

        log_str(if ncq_ok { b"ahci.nem: NCQ supported" } else { b"ahci.nem: no NCQ" });

        unsafe {
            let ps = &mut *(core::ptr::addr_of_mut!(PORT_STATE) as *mut AhciPortState).add(found);
            *ps = AhciPortState {
                phys_port: p as u8,
                dev_type,
                present: 1,
                ncq_supported: if ncq_ok { 1 } else { 0 },
                tag_busy: 0,
            };
        }

        found += 1;
    }

    PORT_COUNT.store(found as u8, Ordering::Relaxed);

    if found == 0 {
        log_str(b"ahci.nem: no active ports");
        return -1;
    }

    // Register block devices for each port
    for i in 0..found {
        let name: [u8; 8] = if i == 0 {
            *b"AHCI0   "
        } else {
            *b"AHCI1   "
        };
        let is_atapi = unsafe {
            let ps = &*(core::ptr::addr_of!(PORT_STATE) as *const AhciPortState).add(i);
            ps.dev_type == DeviceType::Atapi
        };
        let sector_size = if is_atapi { ATAPI_SECTOR_SIZE as u32 } else { 512 };
        let reg = unsafe {
            hst_register_block_device(
                name.as_ptr(),
                5,
                i as u32,
                0x0FFFFFFF,
                sector_size,
                ahci_read,
                ahci_write,
            )
        };
        if reg >= 0 {
            log_hex(b"ahci.nem: reg success idx=", reg as u32);
        } else {
            log_str(b"ahci.nem: reg FAILED");
        }
    }

    log_str(b"ahci.nem: init done");
    0
}

#[no_mangle]
pub extern "C" fn driver_activate() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) == 0 {
        return -1;
    }
    ACTIVE.store(1, Ordering::Release);
    log_str(b"ahci.nem: activated");
    0
}

#[no_mangle]
pub extern "C" fn driver_on_event(_event: *const NeoEvent) -> i32 {
    if ACTIVE.load(Ordering::Relaxed) == 0 {
        return -1;
    }
    0
}

#[no_mangle]
pub extern "C" fn driver_fini() {
    ACTIVE.store(0, Ordering::Release);
    INITIALIZED.store(0, Ordering::Release);
    log_str(b"ahci.nem: shutdown");
}

#[no_mangle]
pub extern "C" fn driver_is_active() -> i32 {
    if ACTIVE.load(Ordering::Relaxed) != 0 { 1 } else { 0 }
}
