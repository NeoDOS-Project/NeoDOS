#![allow(dead_code)]

use crate::drivers::block::BlockDevice;

const GPT_SIGNATURE: [u8; 8] = *b"EFI PART";
const PART_TYPE_NEODOS: [u8; 16] = [
    0xA2, 0xA0, 0xD0, 0xEB, 0xE5, 0xB9, 0x33, 0x44,
    0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26, 0x99, 0xC7,
];

/// GUID for EFI System Partition (ESP / FAT32)
pub const PART_TYPE_ESP: [u8; 16] = [
    0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11,
    0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B,
];

pub const MAX_NEODOS_PARTITIONS: usize = 4;

#[derive(Clone, Copy)]
pub struct GptPartition {
    pub start_lba: u64,
    pub end_lba: u64,
}

fn read_u64_le(buf: &[u8], offset: usize) -> Option<u64> {
    let arr: [u8; 8] = buf[offset..offset + 8].try_into().ok()?;
    Some(u64::from_le_bytes(arr))
}

fn read_u32_le(buf: &[u8], offset: usize) -> Option<u32> {
    let arr: [u8; 4] = buf[offset..offset + 4].try_into().ok()?;
    Some(u32::from_le_bytes(arr))
}

pub fn find_neodos_partition(dev: &mut dyn BlockDevice) -> Option<GptPartition> {
    let mut partitions = find_all_neodos_partitions(dev);
    partitions[0].take()
}

fn read_sector_from_dev(dev: &mut dyn BlockDevice, lba: u32) -> Result<[u8; 512], ()> {
    let saved_base = dev.base_lba();
    dev.set_base_lba(0);
    let result = dev.read_sector(lba as u64);
    dev.set_base_lba(saved_base);
    result
}

fn parse_gpt<F>(mut read_sector: F) -> [Option<GptPartition>; MAX_NEODOS_PARTITIONS]
where
    F: FnMut(u32) -> Result<[u8; 512], ()>,
{
    let mut result = [None; MAX_NEODOS_PARTITIONS];

    let gpt_header = match read_sector(1) {
        Ok(h) => h,
        Err(_) => return result,
    };
    if gpt_header[0..8] != GPT_SIGNATURE {
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
        if found >= MAX_NEODOS_PARTITIONS {
            break;
        }
        let sector_idx = i as u64 / entries_per_sector;
        let offset_in_sector = (i as u64 % entries_per_sector) * entry_size;
        let lba = part_entry_lba + sector_idx;

        let sector = match read_sector(lba as u32) {
            Ok(s) => s,
            Err(_) => return result,
        };
        let entry_offset = offset_in_sector as usize;

        if sector[entry_offset..entry_offset + 16] == PART_TYPE_NEODOS {
            let start_lba = match read_u64_le(&sector, entry_offset + 32) {
                Some(v) => v,
                None => return result,
            };
            let end_lba = match read_u64_le(&sector, entry_offset + 40) {
                Some(v) => v,
                None => return result,
            };
            result[found] = Some(GptPartition { start_lba, end_lba });
            found += 1;
        }
    }

    result
}

pub fn find_all_neodos_partitions(dev: &mut dyn BlockDevice) -> [Option<GptPartition>; MAX_NEODOS_PARTITIONS] {
    parse_gpt(|lba| read_sector_from_dev(dev, lba))
}

/// Find all ESP (EFI System Partition) partitions matching the ESP GUID.
pub fn find_all_esp_partitions(dev: &mut dyn BlockDevice) -> [Option<GptPartition>; MAX_NEODOS_PARTITIONS] {
    parse_gpt_filter(|lba| read_sector_from_dev(dev, lba), &PART_TYPE_ESP)
}

/// Generic GPT parser that filters by any partition type GUID.
fn parse_gpt_filter<F>(mut read_sector: F, target_type: &[u8; 16]) -> [Option<GptPartition>; MAX_NEODOS_PARTITIONS]
where
    F: FnMut(u32) -> Result<[u8; 512], ()>,
{
    let mut result = [None; MAX_NEODOS_PARTITIONS];

    let gpt_header = match read_sector(1) {
        Ok(h) => h,
        Err(_) => return result,
    };
    if &gpt_header[0..8] != b"EFI PART" {
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
        if found >= MAX_NEODOS_PARTITIONS {
            break;
        }
        let sector_idx = i as u64 / entries_per_sector;
        let offset_in_sector = (i as u64 % entries_per_sector) * entry_size;
        let lba = part_entry_lba + sector_idx;

        let sector = match read_sector(lba as u32) {
            Ok(s) => s,
            Err(_) => return result,
        };
        let entry_offset = offset_in_sector as usize;

        if &sector[entry_offset..entry_offset + 16] == target_type {
            let start_lba = match read_u64_le(&sector, entry_offset + 32) {
                Some(v) => v,
                None => return result,
            };
            let end_lba = match read_u64_le(&sector, entry_offset + 40) {
                Some(v) => v,
                None => return result,
            };
            result[found] = Some(GptPartition { start_lba, end_lba });
            found += 1;
        }
    }

    result
}
