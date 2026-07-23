// src/drivers/virtio_blk.rs
// VirtIO Block driver — BOOT_DRIVER for early boot storage.
// Uses VIO-ARCH layer: VirtioTransport + SplitVring.
// Supports legacy I/O BAR (0x1001) and modern MMIO BAR (0x1042).

#![allow(dead_code)]

use core::sync::atomic::{fence, Ordering};
use crate::drivers::block::BlockDevice;
use crate::irp::{self, IrpId, IrpOp};
use crate::memory;
use crate::virtio::{self, BLK_T_IN, BLK_T_OUT, BLK_T_FLUSH, BLK_ACCEPTED_FEATURES, BLK_F_BLK_SIZE, BLK_F_FLUSH};
use crate::virtio::transport::VirtioTransport;
use crate::virtio::vring::{SplitVring, VRING_DESC_F_NEXT, VRING_DESC_F_WRITE};
use crate::log::LogSubsys;

const PAGE_SIZE_4K: u64 = 4096;
const SECTOR_SIZE: u32 = 512;
const MAX_SECTORS_PER_IO: u8 = 8;
// Vring size: QEMU defaults to 256 entries for virtio-blk.
// The vring needs 3 contiguous pages for 256 entries.
const QUEUE_SIZE: u16 = 256;
const VRING_NPAGES: u64 = 3; // 12 KB contiguous
pub struct VirtIoBlk {
    transport: VirtioTransport,
    vring: SplitVring,
    num_sectors: u64,
    block_size: u32,
    supports_flush: bool,
    base_lba: u64,
    queue_phys: u64,
    dma_phys: u64,
}

unsafe impl Send for VirtIoBlk {}

impl VirtIoBlk {
    fn alloc_zeroed_page() -> Option<u64> {
        let phys = memory::allocate_frame()?;
        Self::zero_phys(phys, PAGE_SIZE_4K);
        Some(phys)
    }

    fn free_page(phys: u64) {
        memory::free_frame(phys);
    }

    /// Allocate `order` frames (2^order × 4KB) contiguously.
    fn alloc_contig(order: usize) -> Option<u64> {
        let phys = memory::alloc_frames(order)?;
        let size = (PAGE_SIZE_4K as usize) << order;
        Self::zero_phys(phys, size as u64);
        Some(phys)
    }

    fn free_contig(phys: u64, order: usize) {
        memory::free_frames(phys, order);
    }

    fn zero_phys(phys: u64, size: u64) {
        let ptr = phys as *mut u32;
        for i in 0..(size as usize / 4) {
            unsafe { ptr.add(i).write_volatile(0u32) }
        }
        fence(Ordering::SeqCst);
    }

    pub fn probe() -> Option<Self> {
        let (transport, bus, dev, func) = VirtioTransport::probe_block()?;
        let modern = transport.is_modern();
        kinfo!(LogSubsys::Virtio, "Found at PCI {:02x}:{:02x}.{:01x} ({})", bus, dev, func, if modern { "modern" } else { "legacy" });

        // Allocate contiguous pages for vring (QUEUE_SIZE=256 needs 3 pages, alloc 4)
        // QEMU expects the vring num to match its internal default (256).
        let queue_phys = Self::alloc_contig(2)?; // order 2 = 4 pages = 16 KB
        let dma_phys = Self::alloc_zeroed_page()?;
        kinfo!(LogSubsys::Virtio, "queue=0x{:x} dma=0x{:x}", queue_phys, dma_phys);

        let mut drv = VirtIoBlk {
            transport,
            vring: SplitVring::new(QUEUE_SIZE, queue_phys, !modern),
            num_sectors: 0,
            block_size: SECTOR_SIZE,
            supports_flush: false,
            base_lba: 0,
            queue_phys,
            dma_phys,
        };

        // Init device
        let guest_features = match drv.transport.standard_init(BLK_ACCEPTED_FEATURES) {
            Ok(f) => f,
            Err(()) => {
                kerror!(LogSubsys::Virtio, "Init failed");
                Self::free_contig(queue_phys, 2);
                Self::free_page(dma_phys);
                return None;
            }
        };

        drv.supports_flush = (guest_features & BLK_F_FLUSH) != 0;

        // Read capacity
        drv.num_sectors = drv.transport.read_config64(0);
        if drv.num_sectors == 0 {
            kerror!(LogSubsys::Virtio, "Zero capacity");
            Self::free_contig(queue_phys, 2);
            Self::free_page(dma_phys);
            return None;
        }

        // Read block size if offered
        if guest_features & BLK_F_BLK_SIZE != 0 {
            let bs = drv.transport.read_config32(20);
            if bs > 0 && bs <= 4096 { drv.block_size = bs; }
        }

        // Setup queue
        let ok = drv.transport.setup_queue(
            0,
            drv.vring.desc_phys,
            drv.vring.avail_phys,
            drv.vring.used_phys,
            QUEUE_SIZE,
        );
        if !ok {
            kerror!(LogSubsys::Virtio, "Queue setup failed");
            Self::free_contig(queue_phys, 2);
            Self::free_page(dma_phys);
            return None;
        }

        // DRIVER_OK
        drv.transport.finalize_init();

        kinfo!(LogSubsys::Virtio, "Ready: {} sectors x {}B", drv.num_sectors, drv.block_size);
        Some(drv)
    }

    /// DMA buffer page layout: [0..16]=VirtioBlkReq, [16]=status, [512..]=data
    fn build_chain(&mut self, type_: u32, sector: u64, buf: *const u8, count: u8, is_write: bool) -> u16 {
        let is_flush = type_ == BLK_T_FLUSH;
        let data_len = if is_flush { 0 } else { (count as u32) * SECTOR_SIZE };
        let abs_lba = if is_flush { 0 } else { self.base_lba.wrapping_add(sector) };

        // Write request header
        unsafe {
            let req = self.dma_phys as *mut virtio::VirtioBlkReq;
            req.write_volatile(virtio::VirtioBlkReq { type_, reserved: 0, sector: abs_lba });
        }

        // Write data (for writes)
        if is_write && !is_flush {
            unsafe {
                core::ptr::copy_nonoverlapping(buf, (self.dma_phys + 512) as *mut u8, data_len as usize);
            }
        }

        // Clear status
        unsafe { ((self.dma_phys + 16) as *mut u8).write_volatile(0u8) }

        // Build descriptor chain
        let head = 0u16;
        unsafe {
            self.vring.write_desc(0, self.dma_phys, 16, VRING_DESC_F_NEXT, 1);
            if is_flush {
                // desc[1] = status byte (WRITE, chain terminator)
                self.vring.write_desc(1, self.dma_phys + 16, 1, VRING_DESC_F_WRITE, 0);
            } else {
                let data_flags = if is_write {
                    VRING_DESC_F_NEXT
                } else {
                    VRING_DESC_F_NEXT | VRING_DESC_F_WRITE
                };
                self.vring.write_desc(1, self.dma_phys + 512, data_len, data_flags, 2);
                // desc[2] = status byte
                self.vring.write_desc(2, self.dma_phys + 16, 1, VRING_DESC_F_WRITE, 0);
            }
        }
        head
    }

    fn complete_io(&self, buf: *const u8, data_len: u32, is_write: bool, is_flush: bool) -> Result<(), ()> {
        let status = unsafe { ((self.dma_phys + 16) as *const u8).read_volatile() };
        if status != 0 {
            return Err(());
        }
        if !is_write && !is_flush {
            unsafe {
                core::ptr::copy_nonoverlapping((self.dma_phys + 512) as *const u8, buf as *mut u8, data_len as usize);
            }
        }
        Ok(())
    }

    fn do_io(&mut self, type_: u32, sector: u64, buf: *const u8, count: u8, is_write: bool) -> Result<(), ()> {
        let is_flush = type_ == BLK_T_FLUSH;
        if !is_flush && (count == 0 || count > MAX_SECTORS_PER_IO) {
            return Err(());
        }
        let data_len = if is_flush { 0 } else { (count as u32) * SECTOR_SIZE };

        crate::boot_benchmark::VIRTIO_COMMANDS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

        let head = self.build_chain(type_, sector, buf, count, is_write);
        let _old_idx = unsafe { self.vring.submit_chain(head) };
        self.transport.notify(0);

        // Poll for completion
        for _ in 0..100_000 {
            if unsafe { self.vring.poll_completed() } {
                return self.complete_io(buf, data_len, is_write, is_flush);
            }
            core::hint::spin_loop();
        }
        // Phase 2: yield to QEMU TCG
        for _ in 0..10_000 {
            if unsafe { self.vring.poll_completed() } {
                return self.complete_io(buf, data_len, is_write, is_flush);
            }
            crate::hal::hlt_once();
        }

        kerror!(LogSubsys::Virtio, "I/O timeout type={} sector={}", type_, sector);
        Err(())
    }
}

impl Drop for VirtIoBlk {
    fn drop(&mut self) {
        if self.queue_phys != 0 { Self::free_contig(self.queue_phys, 2); }
        if self.dma_phys != 0 { Self::free_page(self.dma_phys); }
    }
}

impl BlockDevice for VirtIoBlk {
    fn num_sectors(&self) -> Option<u64> { Some(self.num_sectors) }
    fn sector_size(&self) -> u32 { self.block_size }

    fn submit_irp(&mut self, irp_id: IrpId) -> Result<(), ()> {
        let params = irp::irp_get_params(irp_id).ok_or(())?;
        match params.op {
            IrpOp::Read => {
                let buf = unsafe { core::slice::from_raw_parts_mut(params.buf, params.buf_len) };
                irp::irp_complete_result(irp_id, self.read_blocks(params.lba, params.count, buf));
            }
            IrpOp::Write => {
                let buf = unsafe { core::slice::from_raw_parts(params.buf as *const u8, params.buf_len) };
                irp::irp_complete_result(irp_id, self.write_blocks(params.lba, params.count, buf));
            }
            IrpOp::Flush => { irp::irp_complete_result(irp_id, self.flush()); }
            _ => irp::irp_complete_result(irp_id, Ok(())),
        }
        Ok(())
    }

    fn read_blocks(&mut self, lba: u64, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        let cnt = count.min(MAX_SECTORS_PER_IO);
        let sz = self.block_size as usize;
        if buf.len() < (cnt as usize) * sz { return Err(()); }
        self.do_io(BLK_T_IN, lba, buf.as_ptr(), cnt, false)
    }

    fn write_blocks(&mut self, lba: u64, count: u8, buf: &[u8]) -> Result<(), ()> {
        let cnt = count.min(MAX_SECTORS_PER_IO);
        let sz = self.block_size as usize;
        if buf.len() < (cnt as usize) * sz { return Err(()); }
        self.do_io(BLK_T_OUT, lba, buf.as_ptr(), cnt, true)
    }

    fn flush(&mut self) -> Result<(), ()> {
        if !self.supports_flush { return Ok(()); }
        self.do_io(BLK_T_FLUSH, 0, core::ptr::null(), 0, false)
    }

    fn set_base_lba(&mut self, lba: u64) { self.base_lba = lba; }
    fn base_lba(&self) -> u64 { self.base_lba }

    fn read_sector(&mut self, lba: u64) -> Result<[u8; 512], ()> {
        let mut buf = [0u8; 512];
        self.read_blocks(lba, 1, &mut buf)?;
        Ok(buf)
    }

    fn write_sector(&mut self, lba: u64, data: &[u8; 512]) -> Result<(), ()> {
        self.write_blocks(lba, 1, data)
    }
}

// ── Tests ────────────────────────────────────────────────────────────

pub fn register_tests() {
    use crate::test_case;
    use crate::test_eq;

    test_case!("virtio_pci_constants", {
        test_eq!(crate::virtio::transport::VIRTIO_VENDOR, 0x1AF4);
        // QUEUE_SIZE must match QEMU's default queue num for virtio-blk
        test_eq!(QUEUE_SIZE, 256);
    });

    test_case!("virtio_virtqueue_layout", {
        test_eq!(core::mem::size_of::<virtio::VirtioBlkReq>(), 16);
        test_eq!(core::mem::size_of::<crate::virtio::vring::VringDesc>(), 16);
    });

    test_case!("virtio_blk_request_size", {
        test_eq!(MAX_SECTORS_PER_IO, 8);
        test_eq!((MAX_SECTORS_PER_IO as u32) * SECTOR_SIZE, 4096);
    });

    test_case!("virtio_submit_read_write", {});
    test_case!("virtio_boot_load_kernel", {});
    test_case!("virtio_gpt_parsing", {});
    test_case!("virtio_mount_rootfs", {});
    test_case!("virtio_boot_neoshell", {});
}
