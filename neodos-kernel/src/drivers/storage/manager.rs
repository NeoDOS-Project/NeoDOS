use alloc::boxed::Box;
use crate::drivers::ata::BootAta;
use crate::drivers::boot_ahci::BootAhci;
use crate::drivers::nvme::NvmeDriver;
use crate::drivers::virtio_blk::VirtIoBlk;
use crate::log::LogSubsys;
/// Discover and register primary storage device for early boot (Phase 3).
///
/// Priority: NVMe > VirtIO > AHCI > ATA (PIO boot stub)
///
/// AHCI NEM driver loaded in Phase 3.85 registers additional block devices
/// at runtime idx ≥ 1, but the boot stub provides early-boot access.
pub fn init_storage() {
    let mut bdevs = crate::globals::BLOCK_DEVICES.lock();

    // 1. Probe NVMe (highest priority)
    let mut nvme_results = NvmeDriver::probe_all();
    let nvme = nvme_results[0].take();
    drop(nvme_results);

    if let Some(nvme) = nvme {
        kinfo!(LogSubsys::Nvme, "Using NVMe as primary block device");
        bdevs.register(Box::new(nvme));
        return;
    }

    // 2. Probe VirtIO Block (fast para QEMU/KVM)
    kdebug!(LogSubsys::Virtio, "calling VirtIoBlk::probe()...");
    if let Some(virtio) = VirtIoBlk::probe() {
        kinfo!(LogSubsys::Virtio, "Using VirtIO as primary block device");
        bdevs.register(Box::new(virtio));
        return;
    }
    kdebug!(LogSubsys::Virtio, "VirtIoBlk::probe() returned None");

    // 3. Probe AHCI boot stub (DMA, primary channel)
    if let Some(ahci) = BootAhci::probe() {
        kinfo!(LogSubsys::Ahci, "Using AHCI boot stub as primary block device");
        bdevs.register(Box::new(ahci));
        return;
    }

    // 4. ATA boot stub (PIO only, primary channel)
    //    Fallback for PIIX3/QEMU without AHCI.
    kinfo!(LogSubsys::Ata, "Using PIO boot stub as primary block device");
    bdevs.register(Box::new(BootAta::new()));
}
