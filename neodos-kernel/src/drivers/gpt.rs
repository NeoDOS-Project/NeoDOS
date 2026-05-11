use crate::drivers::ata::AtaDriver;

const GPT_SIGNATURE: [u8; 8] = *b"EFI PART";
const PART_TYPE_NEODOS: [u8; 16] = [
    0xA2, 0xA0, 0xD0, 0xEB, 0xE5, 0xB9, 0x33, 0x44,
    0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26, 0x99, 0xC7,
];

pub struct GptPartition {
    pub start_lba: u64,
    pub end_lba: u64,
}

pub fn find_neodos_partition(ata: &mut AtaDriver) -> Option<GptPartition> {
    let gpt_header = ata.read_sector_master(1).ok()?;
    if &gpt_header[0..8] != GPT_SIGNATURE {
        return None;
    }

    let part_entry_lba = u64::from_le_bytes(
        gpt_header[72..80].try_into().ok()?,
    );
    let num_entries = u32::from_le_bytes(
        gpt_header[80..84].try_into().ok()?,
    );
    let entry_size = u32::from_le_bytes(
        gpt_header[84..88].try_into().ok()?,
    );

    let num_entries = num_entries.min(128);
    let entry_size = entry_size.max(128) as u64;

    let entries_per_sector = 512 / entry_size;
    let _num_sectors = (num_entries as u64 + entries_per_sector - 1) / entries_per_sector;

    for i in 0..num_entries {
        let sector_idx = i as u64 / entries_per_sector;
        let offset_in_sector = (i as u64 % entries_per_sector) * entry_size;
        let lba = part_entry_lba + sector_idx;

        let sector = ata.read_sector_master(lba as u32).ok()?;
        let entry_offset = offset_in_sector as usize;

        let type_guid = &sector[entry_offset..entry_offset + 16];
        if type_guid == PART_TYPE_NEODOS {
            let start_lba = u64::from_le_bytes(
                sector[entry_offset + 32..entry_offset + 40].try_into().ok()?,
            );
            let end_lba = u64::from_le_bytes(
                sector[entry_offset + 40..entry_offset + 48].try_into().ok()?,
            );
            return Some(GptPartition { start_lba, end_lba });
        }
    }

    None
}
