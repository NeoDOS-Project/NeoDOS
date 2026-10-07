//! FSCK framework — trait-based filesystem integrity checking.
//!
//! The FS-agnostic interface ([`FsckTrait`], [`FsckStats`], [`FsckStatsRaw`])
//! lives in the driver layer so both `fs` and `drivers` can reference it
//! without crossing subsystem boundaries; it is re-exported here for callers
//! under `fs`.
//!
//! Implementations:
//! - [`ne2`] — NeoFS v2 (NE2), B-tree walker + freelist reconstruction.
//! - `drivers::fsck_fat32` — FAT32 chain / cross-link / orphan analysis.

pub mod ne2;

pub use crate::drivers::fsck::{FsckStatsRaw, FsckTrait};

/// Register NeoFS FSCK tests. FAT32 tests are registered from
/// `drivers::fsck_fat32` because `fs` may not depend on `drivers/fat32`.
pub fn register_fsck_tests() {
    ne2::register_tests();
}
