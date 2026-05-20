use alloc::boxed::Box;
use crate::drivers::ahci::AhciDriver;
use crate::drivers::ata::{AtaChannel, AtaDriver};
use crate::drivers::block::BlockDevice;
use crate::drivers::pci;

/// Discover and register all storage devices.
///
/// 1. Creates ATA drivers (Primary + Secondary)
/// 2. Scans PCI for IDE bus-master DMA
/// 3. Probes AHCI
/// 4. Registers the best available device at index 0 in BlockDeviceManager
///
/// Priority: AHCI > ATA (PIO/DMA)
pub fn init_storage() {
    let mut bdevs = crate::globals::BLOCK_DEVICES.lock();

    // 1. Create ATA drivers
    let mut ata = AtaDriver::new(AtaChannel::Primary);
    let mut ata2 = AtaDriver::new(AtaChannel::Secondary);

    // 2. Scan PCI for IDE bus-master DMA
    if let Some(ide) = pci::find_ide_controller() {
        pci::enable_bus_master(&ide);
        ata.init_dma(ide.bus_master_base);
        ata2.init_dma(ide.bus_master_base + 8);
        crate::serial_println!("[+] ATA bus-master DMA enabled at BMBA 0x{:04X}", ide.bus_master_base);
    } else {
        crate::serial_println!("[!] No IDE bus-master controller found, using PIO");
    }

    // 3. Probe AHCI
    let mut ahci_results = AhciDriver::probe_all();
    let ahci = ahci_results[0].take();
    let ahci_port_count = ahci.as_ref().map(|a| a.port_count).unwrap_or(0);
    drop(ahci_results);

    // 4. Register primary device: AHCI > ATA
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
