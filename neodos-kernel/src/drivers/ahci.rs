use crate::serial_println;
use core::sync::atomic::{fence, Ordering};

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
const ATA_CMD_PACKET: u8 = 0xA0;
const ATAPI_FEAT_DMA: u8 = 0x01;

pub const ATAPI_SECTOR_SIZE: usize = 2048;

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeviceType {
    Ata,
    Atapi,
}

pub struct AhciDriver {
    hba: *mut u32,
    ports: [u8; MAX_PORTS],
    port_types: [DeviceType; MAX_PORTS],
    pub port_count: usize,
    base_lba: u32,
    caps: u32,
}

unsafe impl Send for AhciDriver {}
unsafe impl Sync for AhciDriver {}


pub struct AhciInfo {
    pub bus: u8,
    pub device: u8,
    pub func: u8,
    pub bar5: u32,
}

fn port_reg(hba: *mut u32, port: usize, reg: usize) -> *mut u32 {
    unsafe { hba.add((PORT_REG_BASE + port * PORT_STRIDE + reg) / 4) }
}

fn hba_reg(hba: *mut u32, reg: usize) -> *mut u32 {
    unsafe { hba.add(reg / 4) }
}

fn find_controller() -> Option<AhciInfo> {
    for bus in 0..=0 {
        for dev in 0..32 {
            for func in 0..8 {
                let vendor = crate::drivers::pci::pci_config_read_word(bus, dev, func, 0);
                if vendor == 0xFFFF || vendor == 0 {
                    if func == 0 { break; }
                    continue;
                }
                let class_rev = crate::drivers::pci::pci_config_read_dword(bus, dev, func, 0x08);
                let class = ((class_rev >> 24) & 0xFF) as u8;
                let subclass = ((class_rev >> 16) & 0xFF) as u8;
                if class == 0x01 && subclass == 0x06 {
                    let bar5 = crate::drivers::pci::pci_config_read_dword(bus, dev, func, 0x24);
                    let bar5_addr = bar5 & 0xFFFF_FFF0;
                    serial_println!(
                        "[AHCI] Found at PCI {:02x}:{:02x}.{:x} BAR5=0x{:08x}",
                        bus, dev, func, bar5_addr
                    );
                    let cmd = crate::drivers::pci::pci_config_read_word(bus, dev, func, 0x04);
                    crate::drivers::pci::pci_config_write_word(bus, dev, func, 0x04, cmd | 0x06);
                    return Some(AhciInfo { bus, device: dev, func, bar5: bar5_addr });
                }
            }
        }
    }
    None
}

impl AhciDriver {
    pub fn probe_all() -> [Option<Self>; MAX_PORTS] {
        let mut result = [None, None];
        let info = match find_controller() {
            Some(i) => i,
            None => return result,
        };
        let hba = info.bar5 as *mut u32;

        unsafe {
            let ghc = hba_reg(hba, HBA_GHC).read_volatile();
            hba_reg(hba, HBA_GHC).write_volatile(ghc | HBA_GHC_AE);

            let caps = hba_reg(hba, HBA_CAP).read_volatile();
            let pi = hba_reg(hba, HBA_PI).read_volatile();

            let s64a = (caps >> 31) & 1;
            let n_ports = (caps & 0x1F) as usize;
            serial_println!(
                "[AHCI] CAP=0x{:08x} PI=0x{:08x} ports={} 64bit={}",
                caps, pi, n_ports, s64a
            );

            let mut found = 0usize;
            for p in 0..32.min(n_ports + 1) {
                if found >= MAX_PORTS { break; }
                if (pi & (1 << p)) == 0 { continue; }

                let sig = port_reg(hba, p, PORT_SIG).read_volatile();
                let ssts = port_reg(hba, p, PORT_SSTS).read_volatile();
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
                    serial_println!("[AHCI] Port {}: unknown sig=0x{:08x}, skipping", p, sig);
                    continue;
                };

                serial_println!(
                    "[AHCI] Port {}: {:?} detected (SSTS=0x{:08x})",
                    p, dev_type, ssts
                );

                let mut ports = [0u8; MAX_PORTS];
                ports[found] = p as u8;
                let mut port_types = [DeviceType::Ata; MAX_PORTS];
                port_types[found] = dev_type;

                if found == 0 {
                    let mut driver = AhciDriver {
                        hba, ports, port_types, port_count: 1, base_lba: 0, caps,
                    };
                    driver.port_init(0);
                    result[0] = Some(driver);
                } else {
                    let driver = result[0].as_mut().unwrap();
                    driver.ports[found] = p as u8;
                    driver.port_types[found] = dev_type;
                    driver.port_count = found + 1;
                    driver.port_init(found);
                }
                found += 1;
            }
        }
        result
    }

    pub fn probe() -> Option<Self> {
        let mut all = Self::probe_all();
        all[0].take()
    }

    fn port_init(&mut self, idx: usize) {
        unsafe {
            let port = self.ports[idx] as usize;
            let p = port_reg(self.hba, port, 0);

            let cmd = p.add(PORT_CMD / 4).read_volatile();
            if (cmd & CMD_ST) != 0 || (cmd & CMD_FRE) != 0 {
                p.add(PORT_CMD / 4).write_volatile(cmd & !(CMD_ST | CMD_FRE));
                for _ in 0..10000 {
                    let c = p.add(PORT_CMD / 4).read_volatile();
                    if (c & (CMD_CR | CMD_FR)) == 0 { break; }
                }
            }

            p.add(PORT_IS / 4).write_volatile(0xFFFFFFFF);

            let clb = &PORT_CMD_LIST[idx] as *const _ as u32;
            let fb = &PORT_RECV_FIS[idx] as *const _ as u32;
            p.add(PORT_CLB / 4).write_volatile(clb);
            p.add(PORT_CLBU / 4).write_volatile(0);
            p.add(PORT_FB / 4).write_volatile(fb);
            p.add(PORT_FBU / 4).write_volatile(0);

            p.add(PORT_IE / 4).write_volatile(0);
            p.add(PORT_SERR / 4).write_volatile(0xFFFFFFFF);

            p.add(PORT_CMD / 4).write_volatile(CMD_ST | CMD_FRE | CMD_POD | CMD_SUD);

            for _ in 0..10000 {
                let c = p.add(PORT_CMD / 4).read_volatile();
                if (c & CMD_CR) == 0 { break; }
            }
        }
    }

    fn port_reset(&mut self, idx: usize) {
        unsafe {
            let port = self.ports[idx] as usize;
            let p = port_reg(self.hba, port, 0);

            let cmd = p.add(PORT_CMD / 4).read_volatile();
            p.add(PORT_CMD / 4).write_volatile(cmd & !(CMD_ST | CMD_FRE));
            for _ in 0..10000 {
                let c = p.add(PORT_CMD / 4).read_volatile();
                if (c & (CMD_CR | CMD_FR)) == 0 { break; }
            }

            p.add(PORT_SCTL / 4).write_volatile(0x0301);
            for _ in 0..1000 { core::hint::spin_loop(); }

            p.add(PORT_SCTL / 4).write_volatile(0x0300);
            for _ in 0..100000 { core::hint::spin_loop(); }

            let ssts = p.add(PORT_SSTS / 4).read_volatile();
            if (ssts & 0x0F) == SATA_DET_PRESENT {
                self.port_init(idx);
            }
        }
    }

    pub fn set_base_lba(&mut self, lba: u32) {
        self.base_lba = lba;
    }

    pub fn base_lba(&self) -> u32 {
        self.base_lba
    }

    pub fn read_sector(&mut self, lba: u32) -> Result<[u8; 512], ()> {
        let abs_lba = self.base_lba.wrapping_add(lba) as u64;
        self.dma_xfer(self.ports[0] as usize, abs_lba, 1, false)?;
        let mut buf = [0u8; 512];
        unsafe { buf.copy_from_slice(&PORT_DMA_BUF[0][..512]); }
        Ok(buf)
    }

    pub fn read_sector_master(&mut self, lba: u32) -> Result<[u8; 512], ()> {
        self.dma_xfer(self.ports[0] as usize, lba as u64, 1, false)?;
        let mut buf = [0u8; 512];
        unsafe { buf.copy_from_slice(&PORT_DMA_BUF[0][..512]); }
        Ok(buf)
    }

    pub fn read_sectors(&mut self, lba: u32, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        let abs_lba = self.base_lba.wrapping_add(lba) as u64;
        let cnt = count.max(1).min(8);
        let n = (cnt as usize) * 512;
        self.dma_xfer(self.ports[0] as usize, abs_lba, cnt, false)?;
        unsafe { buf[..n].copy_from_slice(&PORT_DMA_BUF[0][..n]); }
        Ok(())
    }

    pub fn read_sector_secondary(&mut self, lba: u32) -> Result<[u8; 512], ()> {
        if self.port_count < 2 { return Err(()); }
        let abs_lba = self.base_lba.wrapping_add(lba) as u64;
        self.dma_xfer(self.ports[1] as usize, abs_lba, 1, false)?;
        let mut buf = [0u8; 512];
        unsafe { buf.copy_from_slice(&PORT_DMA_BUF[1][..512]); }
        Ok(buf)
    }

    pub fn read_sector_secondary_master(&mut self, lba: u32) -> Result<[u8; 512], ()> {
        if self.port_count < 2 { return Err(()); }
        self.dma_xfer(self.ports[1] as usize, lba as u64, 1, false)?;
        let mut buf = [0u8; 512];
        unsafe { buf.copy_from_slice(&PORT_DMA_BUF[1][..512]); }
        Ok(buf)
    }

    pub fn read_secondary_sectors(&mut self, lba: u32, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        if self.port_count < 2 { return Err(()); }
        let abs_lba = self.base_lba.wrapping_add(lba) as u64;
        let cnt = count.max(1).min(8);
        let n = (cnt as usize) * 512;
        self.dma_xfer(self.ports[1] as usize, abs_lba, cnt, false)?;
        unsafe { buf[..n].copy_from_slice(&PORT_DMA_BUF[1][..n]); }
        Ok(())
    }

    pub fn write_sector(&mut self, lba: u32, data: &[u8; 512]) -> Result<(), ()> {
        let abs_lba = self.base_lba.wrapping_add(lba) as u64;
        unsafe { PORT_DMA_BUF[0][..512].copy_from_slice(data); }
        self.dma_xfer(self.ports[0] as usize, abs_lba, 1, true)
    }

    pub fn write_sectors(&mut self, lba: u32, count: u8, data: &[u8]) -> Result<(), ()> {
        let abs_lba = self.base_lba.wrapping_add(lba) as u64;
        let cnt = count.max(1).min(8);
        let n = (cnt as usize) * 512;
        if data.len() < n { return Err(()); }
        unsafe { PORT_DMA_BUF[0][..n].copy_from_slice(&data[..n]); }
        self.dma_xfer(self.ports[0] as usize, abs_lba, cnt, true)
    }

    pub fn port_type(&self, idx: usize) -> Option<DeviceType> {
        if idx >= self.port_count { return None; }
        Some(self.port_types[idx])
    }

    pub fn read_atapi_sector(&mut self, lba: u32) -> Result<[u8; ATAPI_SECTOR_SIZE], ()> {
        if self.port_count < 1 || self.port_types[0] != DeviceType::Atapi {
            return Err(());
        }
        let count = (DMA_BUF_SIZE / ATAPI_SECTOR_SIZE).min(1) as u8;
        self.dma_packet(self.ports[0] as usize, lba, count, false)?;
        let mut buf = [0u8; ATAPI_SECTOR_SIZE];
        unsafe { buf.copy_from_slice(&PORT_DMA_BUF[0][..ATAPI_SECTOR_SIZE]); }
        Ok(buf)
    }

    pub fn read_atapi_sectors(&mut self, lba: u32, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        if self.port_count < 1 || self.port_types[0] != DeviceType::Atapi {
            return Err(());
        }
        let cnt = count.max(1).min((DMA_BUF_SIZE / ATAPI_SECTOR_SIZE) as u8);
        let n = (cnt as usize) * ATAPI_SECTOR_SIZE;
        if buf.len() < n { return Err(()); }
        self.dma_packet(self.ports[0] as usize, lba, cnt, false)?;
        unsafe { buf[..n].copy_from_slice(&PORT_DMA_BUF[0][..n]); }
        Ok(())
    }

    pub fn read_atapi_sector_secondary(&mut self, lba: u32) -> Result<[u8; ATAPI_SECTOR_SIZE], ()> {
        if self.port_count < 2 || self.port_types[1] != DeviceType::Atapi {
            return Err(());
        }
        let count = (DMA_BUF_SIZE / ATAPI_SECTOR_SIZE).min(1) as u8;
        self.dma_packet(self.ports[1] as usize, lba, count, false)?;
        let mut buf = [0u8; ATAPI_SECTOR_SIZE];
        unsafe { buf.copy_from_slice(&PORT_DMA_BUF[1][..ATAPI_SECTOR_SIZE]); }
        Ok(buf)
    }

    pub fn read_atapi_sectors_secondary(&mut self, lba: u32, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        if self.port_count < 2 || self.port_types[1] != DeviceType::Atapi {
            return Err(());
        }
        let cnt = count.max(1).min((DMA_BUF_SIZE / ATAPI_SECTOR_SIZE) as u8);
        let n = (cnt as usize) * ATAPI_SECTOR_SIZE;
        if buf.len() < n { return Err(()); }
        self.dma_packet(self.ports[1] as usize, lba, cnt, false)?;
        unsafe { buf[..n].copy_from_slice(&PORT_DMA_BUF[1][..n]); }
        Ok(())
    }

    fn build_read10_cdb(lba: u32, count: u8) -> [u8; 12] {
        [
            0x28,
            0x00,
            (lba >> 24) as u8,
            (lba >> 16) as u8,
            (lba >> 8) as u8,
            lba as u8,
            0x00,
            0x00,
            count,
            0x00,
            0x00,
            0x00,
        ]
    }

    fn dma_packet(&mut self, port: usize, lba: u32, count: u8, _is_write: bool) -> Result<(), ()> {
        let pi = self.ports.iter().position(|&p| p as usize == port).ok_or(())?;
        let total = (count as usize) * ATAPI_SECTOR_SIZE;
        let prd_used = ((total + DMA_BUF_SIZE - 1) / DMA_BUF_SIZE).min(MAX_PRD_ENTRIES);

        unsafe {
            let p = port_reg(self.hba, port, 0);
            let table = &mut PORT_CMD_TABLE[pi].0;

            table.cfis = [0u8; 64];
            table.acmd = [0u8; 16];
            table.reserved = [0u8; 48];

            let fis = FisRegH2D {
                fis_type: 0x27,
                pmport: 0x80,
                command: ATA_CMD_PACKET,
                features: ATAPI_FEAT_DMA,
                lba0: 0,
                lba1: 0,
                lba2: 0,
                device: 0x00,
                lba3: 0,
                lba4: 0,
                lba5: 0,
                features_exp: 0,
                sector_count: count,
                sector_count_exp: 0,
                _res: 0,
                control: 0,
            };
            let fis_bytes = &fis as *const FisRegH2D as *const u8;
            for i in 0..64 { table.cfis[i] = fis_bytes.add(i).read(); }

            let cdb = Self::build_read10_cdb(lba, count);
            for i in 0..12 { table.acmd[i] = cdb[i]; }

            for e in table.prdt.iter_mut() {
                *e = EMPTY_PRD;
            }
            for i in 0..prd_used {
                let off = i * DMA_BUF_SIZE;
                let remain = total.saturating_sub(off);
                let chunk = remain.min(DMA_BUF_SIZE);
                let dma_phys = (&PORT_DMA_BUF[pi][off] as *const u8) as u32;
                table.prdt[i].data_base = dma_phys;
                table.prdt[i].data_base_hi = 0;
                table.prdt[i].count = (chunk as u32 - 1) | 0x8000_0000;
            }

            fence(Ordering::Release);

            let ctba = &PORT_CMD_TABLE[pi] as *const _ as u32;
            let cl = &mut PORT_CMD_LIST[pi].0;
            cl[0].opts = CMD_ATA | CFLAG_C | CFLAG_P | CFLAG_A;
            cl[0].prdtl = prd_used as u16;
            cl[0].prdbc = 0;
            cl[0].ctba = ctba;
            cl[0].ctba_hi = 0;
            cl[1..].fill(EMPTY_CMD_HEADER);

            p.add(PORT_IE / 4).write_volatile(0);
            p.add(PORT_IS / 4).write_volatile(0xFFFFFFFF);

            p.add(PORT_CI / 4).write_volatile(1);

            for _ in 0..1000000 {
                let ci = p.add(PORT_CI / 4).read_volatile();
                if (ci & 1) == 0 { break; }
            }

            p.add(PORT_IE / 4).write_volatile(0);

            if (p.add(PORT_CI / 4).read_volatile() & 1) != 0 {
                serial_println!("[AHCI] PACKET timeout port={} lba={}", port, lba);
                self.port_reset(pi);
                return Err(());
            }

            let tfd = p.add(PORT_TFD / 4).read_volatile();
            if (tfd & TFD_ERR) != 0 || (tfd & TFD_BSY) != 0 {
                let serr = p.add(PORT_SERR / 4).read_volatile();
                serial_println!(
                    "[AHCI] PACKET error port={} lba={} TFD=0x{:02x} SERR=0x{:08x}",
                    port, lba, tfd, serr
                );
                p.add(PORT_SERR / 4).write_volatile(serr);
                self.port_reset(pi);
                return Err(());
            }

            fence(Ordering::Acquire);
            Ok(())
        }
    }

    fn dma_xfer(&mut self, port: usize, lba: u64, count: u8, is_write: bool) -> Result<(), ()> {
        let pi = self.ports.iter().position(|&p| p as usize == port).ok_or(())?;
        let total = (count as usize) * 512;
        let prd_count = (total + DMA_BUF_SIZE - 1) / DMA_BUF_SIZE;
        let prd_used = prd_count.min(MAX_PRD_ENTRIES);

        unsafe {
            let p = port_reg(self.hba, port, 0);
            let table = &mut PORT_CMD_TABLE[pi].0;

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
                let dma_phys = (&PORT_DMA_BUF[pi][off] as *const u8) as u32;
                table.prdt[i].data_base = dma_phys;
                table.prdt[i].data_base_hi = 0;
                table.prdt[i].count = (chunk as u32 - 1) | 0x8000_0000;
            }

            fence(Ordering::Release);

            let ctba = &PORT_CMD_TABLE[pi] as *const _ as u32;
            let cl = &mut PORT_CMD_LIST[pi].0;
            cl[0].opts = CMD_ATA | CFLAG_C | CFLAG_P;
            cl[0].prdtl = prd_used as u16;
            cl[0].prdbc = 0;
            cl[0].ctba = ctba;
            cl[0].ctba_hi = 0;
            cl[1..].fill(EMPTY_CMD_HEADER);

            p.add(PORT_IE / 4).write_volatile(0);
            p.add(PORT_IS / 4).write_volatile(0xFFFFFFFF);

            p.add(PORT_CI / 4).write_volatile(1);

            for _ in 0..1000000 {
                let ci = p.add(PORT_CI / 4).read_volatile();
                if (ci & 1) == 0 { break; }
            }

            p.add(PORT_IE / 4).write_volatile(0);

            if (p.add(PORT_CI / 4).read_volatile() & 1) != 0 {
                serial_println!("[AHCI] DMA timeout port={} lba={}", port, lba);
                self.port_reset(pi);
                return Err(());
            }

            let tfd = p.add(PORT_TFD / 4).read_volatile();
            if (tfd & TFD_ERR) != 0 || (tfd & TFD_BSY) != 0 {
                let serr = p.add(PORT_SERR / 4).read_volatile();
                serial_println!(
                    "[AHCI] DMA error port={} lba={} TFD=0x{:02x} SERR=0x{:08x}",
                    port, lba, tfd, serr
                );
                p.add(PORT_SERR / 4).write_volatile(serr);
                self.port_reset(pi);
                return Err(());
            }

            fence(Ordering::Acquire);
            Ok(())
        }
    }
}
