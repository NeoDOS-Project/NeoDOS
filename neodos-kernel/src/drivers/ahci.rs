use crate::serial_println;

const MAX_AHCI_PORTS: usize = 2;

const HBA_GHC: usize = 0x04;
const HBA_IS: usize = 0x0C;
const HBA_PI: usize = 0x0C;
const HBA_CAP: usize = 0x00;
const HBA_GHC_AE: u32 = 0x8000_0000;
const HBA_GHC_HR: u32 = 0x0000_0001;

const PORT_OFFSET: usize = 0x100;
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
const PORT_CI: usize = 0x38;
const PORT_SCR: usize = 0x28;

const CMD_ST: u32 = 0x0001;
const CMD_FRE: u32 = 0x0010;
const CMD_POD: u32 = 0x0002;
const CMD_SUD: u32 = 0x0004;
const CMD_START: u32 = CMD_ST | CMD_FRE | CMD_POD | CMD_SUD;

const SATA_SIG_ATA: u32 = 0x0000_0101;

const HBA_PORT_IPM_ACTIVE: u32 = 0x01;

const CFG_ATA: u16 = 0x0000;
const ATA_CMD_READ_DMA_EXT: u8 = 0x25;

const MAX_PRD_ENTRIES: usize = 8;

#[derive(Copy, Clone)]
#[repr(C, packed)]
struct AhciPrdtEntry {
    data_base: u32,
    data_base_hi: u32,
    reserved: u32,
    count: u32,
}

#[repr(C, packed)]
struct AhciCmdTable {
    cfis: [u8; 64],
    acmd: [u8; 16],
    reserved: [u8; 48],
    prdt: [AhciPrdtEntry; MAX_PRD_ENTRIES],
}

#[derive(Copy, Clone)]
#[repr(C, packed)]
struct AhciCmdHeader {
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
struct CmdList([AhciCmdHeader; 32]);

#[repr(C, align(256))]
struct RecvFis([u8; 256]);

#[repr(C, align(128))]
struct CmdTable(AhciCmdTable);

static mut CMD_LIST: CmdList = CmdList([AhciCmdHeader {
    opts: 0,
    prdtl: 0,
    prdbc: 0,
    ctba: 0,
    ctba_hi: 0,
    reserved: [0; 4],
}; 32]);
static mut RECV_FIS: RecvFis = RecvFis([0u8; 256]);
static mut CMD_TABLE: CmdTable = CmdTable(AhciCmdTable {
    cfis: [0u8; 64],
    acmd: [0u8; 16],
    reserved: [0u8; 48],
    prdt: [AhciPrdtEntry {
        data_base: 0,
        data_base_hi: 0,
        reserved: 0,
        count: 0,
    }; MAX_PRD_ENTRIES],
});
static mut DMA_BUF: [u8; 4096] = [0u8; 4096];

pub struct AhciDriver {
    hba: *mut u32,
    ports: [u8; MAX_AHCI_PORTS],
    pub port_count: usize,
    base_lba: u32,
}

impl AhciDriver {
    pub fn probe_all() -> [Option<Self>; MAX_AHCI_PORTS] {
        let mut result = [None, None];
        let info = match find_ahci_controller() {
            Some(i) => i,
            None => return result,
        };
        let hba = info.bar5 as *mut u32;
        unsafe {
            let ghc = hba.add(HBA_GHC / 4).read_volatile();
            hba.add(HBA_GHC / 4).write_volatile(ghc | HBA_GHC_AE);
            let pi = hba.add(HBA_PI / 4).read_volatile();
            let mut found = 0usize;
            for p in 0..32 {
                if found >= MAX_AHCI_PORTS {
                    break;
                }
                if (pi & (1 << p)) == 0 {
                    continue;
                }
                let port_base = hba.add((PORT_OFFSET + p * 0x80) / 4);
                let ssts = port_base.add(PORT_SSTS / 4).read_volatile();
                let ipm = (ssts >> 8) & 0x0F;
                let det = ssts & 0x0F;
                if det != 0x03 || ipm != HBA_PORT_IPM_ACTIVE {
                    continue;
                }
                let cmd_val = port_base.add(PORT_CMD / 4).read_volatile();
                if (cmd_val & CMD_ST) != 0 {
                    port_base.add(PORT_CMD / 4).write_volatile(cmd_val & !CMD_ST);
                    for _ in 0..100000 {
                        if (port_base.add(PORT_CMD / 4).read_volatile() & CMD_ST) == 0 {
                            break;
                        }
                    }
                }
                let sig = port_base.add(PORT_SIG / 4).read_volatile();
                if sig != SATA_SIG_ATA {
                    continue;
                }
                cmd_list_init(hba, p as usize);
                serial_println!("[AHCI] Port {}: ATA device detected", p);
                if sig != SATA_SIG_ATA {
                    continue;
                }
                cmd_list_init(hba, p as usize);
                serial_println!("[AHCI] Port {}: ATA device detected", p);
                let mut ports = [0u8; MAX_AHCI_PORTS];
                ports[found] = p as u8;
                if found == 0 {
                    let driver = AhciDriver { hba, ports, port_count: 1, base_lba: 0 };
                    result[0] = Some(driver);
                } else {
                    result[found - 1].as_mut().unwrap().ports[found] = p as u8;
                    result[found - 1].as_mut().unwrap().port_count = found + 1;
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

    pub fn set_base_lba(&mut self, lba: u32) {
        self.base_lba = lba;
    }

    pub fn base_lba(&self) -> u32 {
        self.base_lba
    }

    pub fn read_sector(&mut self, lba: u32) -> Result<[u8; 512], ()> {
        let abs_lba = self.base_lba.wrapping_add(lba) as u64;
        self.ata_dma(self.ports[0], abs_lba, 1)
    }

    pub fn read_sector_master(&mut self, lba: u32) -> Result<[u8; 512], ()> {
        self.ata_dma(self.ports[0], lba as u64, 1)
    }

    pub fn read_sectors(&mut self, lba: u32, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        let abs_lba = self.base_lba.wrapping_add(lba) as u64;
        let data = self.ata_dma(self.ports[0], abs_lba, count.max(1).min(8))?;
        let n = (count as usize).min(8) * 512;
        buf[..n].copy_from_slice(&data[..n]);
        Ok(())
    }

    pub fn read_sector_secondary(&mut self, lba: u32) -> Result<[u8; 512], ()> {
        if self.port_count < 2 {
            return Err(());
        }
        let abs_lba = self.base_lba.wrapping_add(lba) as u64;
        self.ata_dma(self.ports[1], abs_lba, 1)
    }

    pub fn read_sector_secondary_master(&mut self, lba: u32) -> Result<[u8; 512], ()> {
        if self.port_count < 2 {
            return Err(());
        }
        self.ata_dma(self.ports[1], lba as u64, 1)
    }

    pub fn read_secondary_sectors(&mut self, lba: u32, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        if self.port_count < 2 {
            return Err(());
        }
        let abs_lba = self.base_lba.wrapping_add(lba) as u64;
        let data = self.ata_dma(self.ports[1], abs_lba, count.max(1).min(8))?;
        let n = (count as usize).min(8) * 512;
        buf[..n].copy_from_slice(&data[..n]);
        Ok(())
    }

    pub fn write_sector(&mut self, _lba: u32, _data: &[u8; 512]) -> Result<(), ()> {
        Err(())
    }

    pub fn write_sectors(&mut self, _lba: u32, _count: u8, _data: &[u8]) -> Result<(), ()> {
        Err(())
    }

    fn ata_dma(&mut self, port: u8, lba: u64, count: u8) -> Result<[u8; 512], ()> {
        unsafe {
            let port_base = self.hba.add((PORT_OFFSET + port as usize * 0x80) / 4);

            port_base.add(PORT_IE / 4).write_volatile(0);
            port_base.add(PORT_IS / 4).write_volatile(0xFFFFFFFF);

            let mut fis = FisRegH2D {
                fis_type: 0x27,
                pmport: 0x80,
                command: ATA_CMD_READ_DMA_EXT,
                features: 0,
                lba0: (lba & 0xFF) as u8,
                lba1: ((lba >> 8) & 0xFF) as u8,
                lba2: ((lba >> 16) & 0xFF) as u8,
                device: 0x40,
                lba3: ((lba >> 24) & 0xFF) as u8,
                lba4: ((lba >> 32) & 0xFF) as u8,
                lba5: ((lba >> 40) & 0xFF) as u8,
                features_exp: 0,
                sector_count: count,
                sector_count_exp: 0,
                _res: 0,
                control: 0,
            };

            let table = &mut CMD_TABLE.0;
            table.cfis[..64].copy_from_slice(core::mem::transmute::<&FisRegH2D, &[u8; 64]>(&fis));
            for b in table.acmd.iter_mut() { *b = 0; }
            for b in table.reserved.iter_mut() { *b = 0; }

            let total_bytes = (count as usize) * 512;
            let dma_phys = &DMA_BUF as *const _ as u32;
            table.prdt[0].data_base = dma_phys;
            table.prdt[0].data_base_hi = 0;
            table.prdt[0].count = ((total_bytes as u32) - 1) | 0x8000_0000;
            for i in 1..MAX_PRD_ENTRIES {
                table.prdt[i].data_base = 0;
                table.prdt[i].data_base_hi = 0;
                table.prdt[i].count = 0;
            }

            core::sync::atomic::fence(core::sync::atomic::Ordering::Release);

            let ctba = &CMD_TABLE as *const _ as u32;
            let header = &mut CMD_LIST.0[0];
            header.opts = CFG_ATA | (1 << 15);
            header.prdtl = 1;
            header.ctba = ctba;
            header.ctba_hi = 0;

            let clba = &CMD_LIST as *const _ as u32;
            let clba_hi = 0u32;
            let fb = &RECV_FIS as *const _ as u32;
            let fb_hi = 0u32;

            port_base.add(PORT_CLB / 4).write_volatile(clba);
            port_base.add(PORT_CLBU / 4).write_volatile(clba_hi);
            port_base.add(PORT_FB / 4).write_volatile(fb);
            port_base.add(PORT_FBU / 4).write_volatile(fb_hi);
            port_base.add(PORT_CMD / 4).write_volatile(CMD_START);

            for _ in 0..1000 {
                let tfd = port_base.add(PORT_TFD / 4).read_volatile();
                if (tfd & 0x85) == 0x00 {
                    break;
                }
            }

            port_base.add(PORT_CI / 4).write_volatile(1);

            for _ in 0..1000000 {
                let ci = port_base.add(PORT_CI / 4).read_volatile();
                if (ci & 1) == 0 {
                    break;
                }
            }

            port_base.add(PORT_CMD / 4).write_volatile(0);
            port_base.add(PORT_IE / 4).write_volatile(0);

            let tfd = port_base.add(PORT_TFD / 4).read_volatile();
            if (tfd & 0x01) != 0 {
                return Err(());
            }

            let mut buf = [0u8; 512];
            buf.copy_from_slice(&DMA_BUF[..512]);
            core::sync::atomic::fence(core::sync::atomic::Ordering::Acquire);
            Ok(buf)
        }
    }
}

fn cmd_list_init(hba: *mut u32, port: usize) {
    unsafe {
        let base = hba.add((PORT_OFFSET + port * 0x80) / 4);
        let clb = &CMD_LIST as *const _ as u32;
        let fb = &RECV_FIS as *const _ as u32;
        base.add(PORT_CLB / 4).write_volatile(clb);
        base.add(PORT_CLBU / 4).write_volatile(0);
        base.add(PORT_FB / 4).write_volatile(fb);
        base.add(PORT_FBU / 4).write_volatile(0);
    }
}

pub struct AhciInfo {
    pub bus: u8,
    pub device: u8,
    pub func: u8,
    pub bar5: u32,
}

fn find_ahci_controller() -> Option<AhciInfo> {
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
                    serial_println!("[AHCI] Found at PCI {:02x}:{:02x}.{:x} BAR5=0x{:08x}", bus, dev, func, bar5_addr);
                    let cmd = crate::drivers::pci::pci_config_read_word(bus, dev, func, 0x04);
                    crate::drivers::pci::pci_config_write_word(bus, dev, func, 0x04, cmd | 0x06);
                    return Some(AhciInfo { bus, device: dev, func, bar5: bar5_addr });
                }
            }
        }
    }
    None
}
