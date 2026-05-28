use alloc::boxed::Box;
use crate::drivers::ahci::AhciDriver;
use crate::drivers::ata::{AtaChannel, AtaDriver};
use crate::drivers::block::BlockDevice;
use crate::drivers::nvme::NvmeDriver;
use crate::serial_println;

struct IdeController {
    bus: u8,
    device: u8,
    func: u8,
    bus_master_base: u16,
}

fn find_ide_controller() -> Option<IdeController> {
    for bus in 0..=0 {
        for dev in 0..32 {
            for func in 0..8 {
                let vendor = crate::drivers::pci::pci_config_read_word(bus, dev, func, 0);
                if vendor == 0xFFFF || vendor == 0 {
                    if func == 0 {
                        break;
                    }
                    continue;
                }

                let class_rev = crate::drivers::pci::pci_config_read_dword(bus, dev, func, 0x08);
                let class = ((class_rev >> 24) & 0xFF) as u8;
                let subclass = ((class_rev >> 16) & 0xFF) as u8;

                if class == 0x01 && subclass == 0x01 {
                    let prog_if = ((class_rev >> 8) & 0xFF) as u8;
                    let bar4 = crate::drivers::pci::pci_config_read_dword(bus, dev, func, 0x20);

                    if (prog_if & 0x80) != 0 && (bar4 & 0x01) != 0 {
                        let bmba = (bar4 & 0xFFF0) as u16;
                        if bmba != 0 {
                            return Some(IdeController {
                                bus,
                                device: dev,
                                func,
                                bus_master_base: bmba,
                            });
                        }
                    }
                    break;
                }
            }
        }
    }
    None
}

fn enable_bus_master(ide: &IdeController) {
    let cmd = crate::drivers::pci::pci_config_read_word(ide.bus, ide.device, ide.func, 0x04);
    crate::drivers::pci::pci_config_write_word(ide.bus, ide.device, ide.func, 0x04, cmd | 0x04);
}

/// Discover and register all storage devices.
///
/// Priority: NVMe > AHCI > ATA (PIO/DMA)
pub fn init_storage() {
    let mut bdevs = crate::globals::BLOCK_DEVICES.lock();

    // 1. Probe NVMe (highest priority)
    let mut nvme_results = NvmeDriver::probe_all();
    let nvme = nvme_results[0].take();
    drop(nvme_results);

    if let Some(nvme) = nvme {
        serial_println!("[NVMe] Using NVMe as primary block device");
        bdevs.register(Box::new(nvme));
    }

    // 2. Create ATA drivers (fallback)
    let mut ata = AtaDriver::new(AtaChannel::Primary);
    let mut ata2 = AtaDriver::new(AtaChannel::Secondary);

    // 3. Scan PCI for IDE bus-master DMA
    if let Some(ide) = find_ide_controller() {
        enable_bus_master(&ide);
        ata.init_dma(ide.bus_master_base);
        ata2.init_dma(ide.bus_master_base + 8);
        crate::serial_println!("[+] ATA bus-master DMA enabled at BMBA 0x{:04X}", ide.bus_master_base);
    } else {
        crate::serial_println!("[!] No IDE bus-master controller found, using PIO");
    }

    // 4. Probe AHCI
    let mut ahci_results = AhciDriver::probe_all();
    let ahci = ahci_results[0].take();
    let ahci_port_count = ahci.as_ref().map(|a| a.port_count).unwrap_or(0);
    drop(ahci_results);

    // 5. Register primary device: AHCI > ATA
    let primary: Box<dyn BlockDevice> = if let Some(ahci) = ahci {
        if ahci_port_count > 0 {
            crate::serial_println!("[+] AHCI: {} ports — using as primary block device", ahci_port_count);
            Box::new(ahci)
        } else {
            crate::serial_println!("[-] AHCI controller found but no active ports; falling back to ATA");
            crate::serial_println!("[+] Using ATA (PIO/DMA) as primary block device");
            Box::new(ata)
        }
    } else {
        crate::serial_println!("[-] No AHCI controller found");
        crate::serial_println!("[+] Using ATA (PIO/DMA) as primary block device");
        Box::new(ata)
    };

    bdevs.register(primary);
}
