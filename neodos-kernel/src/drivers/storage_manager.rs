use alloc::boxed::Box;
use crate::drivers::ata::BootAta;
use crate::drivers::ahci::AhciDriver;
use crate::drivers::nvme::NvmeDriver;
use crate::serial_println;

/// Discover and register primary storage device.
///
/// Priority: NVMe > AHCI > ATA (PIO boot stub)
pub fn init_storage() {
    let mut bdevs = crate::globals::BLOCK_DEVICES.lock();

    // 1. Probe NVMe (highest priority)
    let mut nvme_results = NvmeDriver::probe_all();
    let nvme = nvme_results[0].take();
    drop(nvme_results);

    if let Some(nvme) = nvme {
        serial_println!("[NVMe] Using NVMe as primary block device");
        bdevs.register(Box::new(nvme));
        return;
    }

    // 2. Probe AHCI
    let mut ahci_results = AhciDriver::probe_all();
    let ahci = ahci_results[0].take();
    let ahci_port_count = ahci.as_ref().map(|a| a.port_count).unwrap_or(0);
    drop(ahci_results);

    if let Some(ahci) = ahci {
        if ahci_port_count > 0 {
            serial_println!("[AHCI] {} ports — using as primary block device", ahci_port_count);
            bdevs.register(Box::new(ahci));
            return;
        }
        serial_println!("[AHCI] found but no active ports; falling back to ATA");
    }

    // 3. ATA boot stub (PIO only, primary channel)
    serial_println!("[ATA] Using PIO boot stub as primary block device");
    bdevs.register(Box::new(BootAta::new()));
}
