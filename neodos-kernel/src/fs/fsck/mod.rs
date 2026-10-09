//! FSCK framework — trait-based filesystem integrity checking.
//!
//! Defines the [`FsckTrait`] contract plus the ABI-stable [`FsckStatsRaw`]
//! returned to user mode. Each filesystem implements it next to its format:
//! [`ne2`] for NeoFS v2 and [`fat32`] for FAT32. The VFS `FileSystem::fsck`
//! method dispatches here, and `ObInfoClass::FsckStatus` /
//! `ObSetInfoClass::FsckRepair` expose it to Ring 3 through `fsck.nxe`.

pub mod fat32;
pub mod ne2;

/// ABI-stable statistics copied back to user mode.
///
/// Layout must stay in sync with `libneodos::syscall::FsckStats`.
#[repr(C)]
pub struct FsckStatsRaw {
    pub total_blocks: u64,
    pub used_blocks: u64,
    pub free_blocks: u64,
    pub total_nodes: u64,
    pub total_dirs: u64,
    pub total_files: u64,
    pub errors: u32,
    pub warnings: u32,
    pub repaired: u32,
}

/// In-kernel FSCK result before ABI conversion.
#[derive(Default, Clone)]
pub struct FsckStats {
    pub total_blocks: u64,
    pub used_blocks: u64,
    pub free_blocks: u64,
    pub total_nodes: u64,
    pub total_dirs: u64,
    pub total_files: u64,
    pub errors: u32,
    pub warnings: u32,
    /// Number of issues actually fixed by a repair pass (0 for check-only).
    pub repaired: u32,
}

impl FsckStats {
    pub fn to_raw(&self) -> FsckStatsRaw {
        FsckStatsRaw {
            total_blocks: self.total_blocks,
            used_blocks: self.used_blocks,
            free_blocks: self.free_blocks,
            total_nodes: self.total_nodes,
            total_dirs: self.total_dirs,
            total_files: self.total_files,
            errors: self.errors,
            warnings: self.warnings,
            repaired: self.repaired,
        }
    }
}

/// Filesystem-agnostic integrity checker.
///
/// `check` must never write to the volume; `repair` performs checks and fixes
/// the subset of issues that can be repaired safely, reporting how many were
/// fixed in [`FsckStats::repaired`].
pub trait FsckTrait {
    /// Run a read-only integrity check.
    fn check(&self) -> FsckStats;
    /// Run an integrity check and repair what can be repaired.
    fn repair(&self) -> FsckStats;
}

/// Register all filesystem checker tests.
pub fn register_fsck_tests() {
    ne2::register_tests();
    fat32::register_tests();
}

// ── Test support ────────────────────────────────────────────────────
//
// Tests are registered at runtime (no `#[cfg(test)]`), so this backing
// device lives here unconditionally and is shared by both checkers.

pub(crate) struct TestBlockDevice {
    pub sectors: alloc::vec::Vec<[u8; 512]>,
}

impl crate::drivers::block::BlockDevice for TestBlockDevice {
    fn submit_irp(&mut self, _irp_id: crate::irp::IrpId) -> Result<(), ()> {
        Ok(())
    }

    fn read_blocks(&mut self, lba: u64, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        let start = lba as usize;
        let end = start + count as usize;
        if end > self.sectors.len() {
            return Err(());
        }
        let mut off = 0;
        for i in start..end {
            let len = core::cmp::min(512, buf.len() - off);
            buf[off..off + len].copy_from_slice(&self.sectors[i][..len]);
            off += len;
        }
        Ok(())
    }

    fn write_blocks(&mut self, lba: u64, count: u8, buf: &[u8]) -> Result<(), ()> {
        let start = lba as usize;
        let end = start + count as usize;
        if end > self.sectors.len() {
            return Err(());
        }
        let mut off = 0;
        for i in start..end {
            let len = core::cmp::min(512, buf.len() - off);
            self.sectors[i][..len].copy_from_slice(&buf[off..off + len]);
            off += len;
        }
        Ok(())
    }

    fn set_base_lba(&mut self, _lba: u64) {}
    fn base_lba(&self) -> u64 {
        0
    }
}

/// Register an in-memory device and return its index.
pub(crate) fn register_test_device(sectors: alloc::vec::Vec<[u8; 512]>) -> usize {
    let dev = TestBlockDevice { sectors };
    let id = crate::globals::with_block_devices(|bdevs| {
        bdevs.register(alloc::boxed::Box::new(dev)).unwrap()
    });
    // Device indices are reused after `force_remove`; drop any pages the
    // previous device at this index left behind.
    crate::globals::with_page_cache(|pc| {
        pc.invalidate_device(id as u64);
    });
    id
}
