#![allow(dead_code)]

use crate::drivers::block::BlockDevice;

pub const MAX_PARTITIONS: usize = 4;

/// GUID for NeoDOS filesystem partition
pub const PART_TYPE_NEODOS: [u8; 16] = [
    0xA2, 0xA0, 0xD0, 0xEB, 0xE5, 0xB9, 0x33, 0x44,
    0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26, 0x99, 0xC7,
];

/// GUID for EFI System Partition (ESP / FAT32)
pub const PART_TYPE_ESP: [u8; 16] = [
    0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11,
    0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B,
];

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

fn read_u64_le(buf: &[u8], offset: usize) -> Option<u64> {
    let arr: [u8; 8] = buf[offset..offset + 8].try_into().ok()?;
    Some(u64::from_le_bytes(arr))
}

fn read_u32_le(buf: &[u8], offset: usize) -> Option<u32> {
    let arr: [u8; 4] = buf[offset..offset + 4].try_into().ok()?;
    Some(u32::from_le_bytes(arr))
}

fn read_sector_from_dev(dev: &mut dyn BlockDevice, lba: u32) -> Result<[u8; 512], ()> {
    let saved_base = dev.base_lba();
    dev.set_base_lba(0);
    let result = dev.read_sector(lba as u64);
    dev.set_base_lba(saved_base);
    result
}

/// Parse GPT and return all partitions matching the given type GUID.
pub fn find_partitions_by_type(
    dev: &mut dyn BlockDevice,
    type_guid: &[u8; 16],
) -> [Option<PartitionInfo>; MAX_PARTITIONS] {
    let mut result = [None; MAX_PARTITIONS];
    let gpt_sig: [u8; 8] = *b"EFI PART";

    let gpt_header = match read_sector_from_dev(dev, 1) {
        Ok(h) => h,
        Err(_) => return result,
    };
    if &gpt_header[0..8] != gpt_sig {
        return result;
    }

    let part_entry_lba = match read_u64_le(&gpt_header, 72) {
        Some(lba) => lba,
        None => return result,
    };
    let num_entries = match read_u32_le(&gpt_header, 80) {
        Some(n) => n.min(128),
        None => return result,
    };
    let entry_size = match read_u32_le(&gpt_header, 84) {
        Some(s) => s.max(128) as u64,
        None => return result,
    };

    let entries_per_sector = 512 / entry_size;
    let mut found = 0usize;

    for i in 0..num_entries {
        if found >= MAX_PARTITIONS {
            break;
        }
        let sector_idx = i as u64 / entries_per_sector;
        let offset_in_sector = (i as u64 % entries_per_sector) * entry_size;
        let lba = part_entry_lba + sector_idx;

        let sector = match read_sector_from_dev(dev, lba as u32) {
            Ok(s) => s,
            Err(_) => return result,
        };
        let entry_offset = offset_in_sector as usize;

        if &sector[entry_offset..entry_offset + 16] == type_guid {
            let start_lba = match read_u64_le(&sector, entry_offset + 32) {
                Some(v) => v,
                None => return result,
            };
            let end_lba = match read_u64_le(&sector, entry_offset + 40) {
                Some(v) => v,
                None => return result,
            };
            result[found] = Some(PartitionInfo {
                base_lba: start_lba,
                sector_count: end_lba - start_lba,
                partition_type: *type_guid,
            });
            found += 1;
        }
    }

    result
}
