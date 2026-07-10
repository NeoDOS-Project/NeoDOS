#![allow(dead_code)]

use crate::drivers::block::BlockDevice;
use crate::drivers::gpt::{self, MAX_NEODOS_PARTITIONS};

pub const MAX_PARTITIONS: usize = MAX_NEODOS_PARTITIONS;

/// Re-export GPT GUIDs for backward compatibility
pub use crate::drivers::gpt::{PART_TYPE_NEODOS, PART_TYPE_ESP};

#[derive(Clone, Copy, Debug)]
pub struct PartitionInfo {
    pub base_lba: u64,
    pub sector_count: u64,
    pub partition_type: [u8; 16],
}

impl PartitionInfo {
    pub fn new(base_lba: u64, sector_count: u64, partition_type: [u8; 16]) -> Self {
        PartitionInfo { base_lba, sector_count, partition_type }
    }
}

/// Parse GPT and return all partitions matching the given type GUID.
pub fn find_partitions_by_type(
    dev: &mut dyn BlockDevice,
    type_guid: &[u8; 16],
) -> [Option<PartitionInfo>; MAX_PARTITIONS] {
    let gpt_parts = gpt::parse_gpt_filter(|lba| gpt::read_sector_from_dev(dev, lba), type_guid);
    let mut result = [None; MAX_PARTITIONS];
    for (i, part) in gpt_parts.iter().enumerate() {
        if let Some(p) = part {
            result[i] = Some(PartitionInfo {
                base_lba: p.start_lba,
                sector_count: p.end_lba - p.start_lba,
                partition_type: *type_guid,
            });
        }
    }
    result
}
