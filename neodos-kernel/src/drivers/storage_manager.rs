use alloc::boxed::Box;
use crate::drivers::ahci::AhciDriver;
use crate::drivers::ata::{AtaChannel, AtaDriver};
use crate::drivers::block::BlockDevice;
use crate::drivers::nvme::NvmeDriver;
use crate::drivers::pci;
use crate::serial_println;

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
    if let Some(ide) = pci::find_ide_controller() {
        pci::enable_bus_master(&ide);
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
