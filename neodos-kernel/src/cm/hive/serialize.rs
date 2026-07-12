use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::types::{Cell, KeyCell, ValueCell, SecurityCell, NULL_CELL, MAX_CELLS};
use super::core::Hive;

impl Hive {
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        let mut entries: Vec<(u32, &Cell)> = Vec::new();
        for (i, slot) in self.cells.iter().enumerate() {
            if let Some(cell) = slot {
                match cell {
                    Cell::Free => {}
                    _ => entries.push((i as u32, cell)),
                }
            }
        }

        let header_off = buf.len();
        buf.extend_from_slice(b"NEOH");
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());

        let mut checksum: u32 = 0;
        for (cell_idx, cell) in &entries {
            checksum = checksum.wrapping_add(*cell_idx);
            buf.extend_from_slice(&cell_idx.to_le_bytes());

            match cell {
                Cell::Key(k) => {
                    let cell_type: u8 = 1;
                    checksum = checksum.wrapping_add(cell_type as u32);
                    buf.push(cell_type);

                    let name_bytes = k.name.as_bytes();
                    let name_len = name_bytes.len() as u16;
                    checksum = checksum.wrapping_add(name_len as u32);
                    buf.extend_from_slice(&name_len.to_le_bytes());
                    for &b in name_bytes {
                        checksum = checksum.wrapping_add(b as u32);
                    }
                    buf.extend_from_slice(name_bytes);

                    checksum = checksum.wrapping_add(k.parent_cell);
                    buf.extend_from_slice(&k.parent_cell.to_le_bytes());
                    checksum = checksum.wrapping_add(k.subkeys_head);
                    buf.extend_from_slice(&k.subkeys_head.to_le_bytes());
                    checksum = checksum.wrapping_add(k.subkeys_sibling);
                    buf.extend_from_slice(&k.subkeys_sibling.to_le_bytes());
                    checksum = checksum.wrapping_add(k.values_head);
                    buf.extend_from_slice(&k.values_head.to_le_bytes());
                    checksum = checksum.wrapping_add(k.sec_desc_cell);
                    buf.extend_from_slice(&k.sec_desc_cell.to_le_bytes());
                    let lw_low = k.last_write as u32;
                    let lw_high = (k.last_write >> 32) as u32;
                    checksum = checksum.wrapping_add(lw_low);
                    checksum = checksum.wrapping_add(lw_high);
                    buf.extend_from_slice(&k.last_write.to_le_bytes());
                }
                Cell::Value(v) => {
                    let cell_type: u8 = 2;
                    checksum = checksum.wrapping_add(cell_type as u32);
                    buf.push(cell_type);

                    let name_bytes = v.name.as_bytes();
                    let name_len = name_bytes.len() as u16;
                    checksum = checksum.wrapping_add(name_len as u32);
                    buf.extend_from_slice(&name_len.to_le_bytes());
                    for &b in name_bytes {
                        checksum = checksum.wrapping_add(b as u32);
                    }
                    buf.extend_from_slice(name_bytes);

                    checksum = checksum.wrapping_add(v.value_type);
                    buf.extend_from_slice(&v.value_type.to_le_bytes());

                    let data_len = v.data.len() as u32;
                    checksum = checksum.wrapping_add(data_len);
                    buf.extend_from_slice(&data_len.to_le_bytes());
                    for &b in &v.data {
                        checksum = checksum.wrapping_add(b as u32);
                    }
                    buf.extend_from_slice(&v.data);

                    let nxt = v.next;
                    checksum = checksum.wrapping_add(nxt);
                    buf.extend_from_slice(&nxt.to_le_bytes());
                }
                Cell::Security(s) => {
                    let cell_type: u8 = 3;
                    checksum = checksum.wrapping_add(cell_type as u32);
                    buf.push(cell_type);

                    let sd_len = s.sd_data.len() as u32;
                    checksum = checksum.wrapping_add(sd_len);
                    buf.extend_from_slice(&sd_len.to_le_bytes());
                    for &b in &s.sd_data {
                        checksum = checksum.wrapping_add(b as u32);
                    }
                    buf.extend_from_slice(&s.sd_data);

                    let nxt = s.next;
                    checksum = checksum.wrapping_add(nxt);
                    buf.extend_from_slice(&nxt.to_le_bytes());
                }
                Cell::Free => {}
            }
        }

        let checksum_off = header_off + 4 + 4 + 4;
        buf[checksum_off..checksum_off + 4].copy_from_slice(&checksum.to_le_bytes());

        buf
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, ()> {
        if data.len() < 16 {
            return Err(());
        }
        if &data[0..4] != b"NEOH" {
            return Err(());
        }
        let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        if version != 1 {
            return Err(());
        }
        let entry_count = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;
        let stored_checksum = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

        let mut cells: Vec<Option<Cell>> = Vec::with_capacity(MAX_CELLS);
        cells.resize(MAX_CELLS, None);

        let mut count: usize = 0;
        let mut pos = 16usize;
        let mut computed_checksum: u32 = 0;

        for _ in 0..entry_count {
            if pos + 5 > data.len() {
                return Err(());
            }
            let cell_idx = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
            computed_checksum = computed_checksum.wrapping_add(cell_idx);
            let cell_type = data[pos + 4];
            computed_checksum = computed_checksum.wrapping_add(cell_type as u32);
            pos += 5;

            match cell_type {
                1 => {
                    if pos + 2 > data.len() { return Err(()); }
                    let name_len = u16::from_le_bytes([data[pos], data[pos+1]]) as usize;
                    computed_checksum = computed_checksum.wrapping_add(name_len as u32);
                    pos += 2;
                    if pos + name_len > data.len() { return Err(()); }
                    let name = core::str::from_utf8(&data[pos..pos + name_len]).map_err(|_| ())?;
                    for &b in &data[pos..pos + name_len] {
                        computed_checksum = computed_checksum.wrapping_add(b as u32);
                    }
                    pos += name_len;
                    if pos + 4 > data.len() { return Err(()); }
                    let parent_cell = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                    computed_checksum = computed_checksum.wrapping_add(parent_cell);
                    pos += 4;
                    if pos + 4 > data.len() { return Err(()); }
                    let subkeys_head = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                    computed_checksum = computed_checksum.wrapping_add(subkeys_head);
                    pos += 4;
                    if pos + 4 > data.len() { return Err(()); }
                    let subkeys_sibling = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                    computed_checksum = computed_checksum.wrapping_add(subkeys_sibling);
                    pos += 4;
                    if pos + 4 > data.len() { return Err(()); }
                    let values_head = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                    computed_checksum = computed_checksum.wrapping_add(values_head);
                    pos += 4;
                    if pos + 4 > data.len() { return Err(()); }
                    let sec_desc_cell = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                    computed_checksum = computed_checksum.wrapping_add(sec_desc_cell);
                    pos += 4;
                    if pos + 8 > data.len() { return Err(()); }
                    let last_write = u64::from_le_bytes([
                        data[pos], data[pos+1], data[pos+2], data[pos+3],
                        data[pos+4], data[pos+5], data[pos+6], data[pos+7],
                    ]);
                    let lw_low = last_write as u32;
                    let lw_high = (last_write >> 32) as u32;
                    computed_checksum = computed_checksum.wrapping_add(lw_low);
                    computed_checksum = computed_checksum.wrapping_add(lw_high);
                    pos += 8;

                    if (cell_idx as usize) >= cells.len() {
                        return Err(());
                    }
                    cells[cell_idx as usize] = Some(Cell::Key(KeyCell {
                        name: name.to_string(),
                        parent_cell,
                        subkeys_head,
                        subkeys_sibling,
                        values_head,
                        sec_desc_cell,
                        last_write,
                    }));
                    count += 1;
                }
                2 => {
                    if pos + 2 > data.len() { return Err(()); }
                    let name_len = u16::from_le_bytes([data[pos], data[pos+1]]) as usize;
                    computed_checksum = computed_checksum.wrapping_add(name_len as u32);
                    pos += 2;
                    if pos + name_len > data.len() { return Err(()); }
                    let name_str = core::str::from_utf8(&data[pos..pos + name_len]).map_err(|_| ())?;
                    for &b in &data[pos..pos + name_len] {
                        computed_checksum = computed_checksum.wrapping_add(b as u32);
                    }
                    pos += name_len;
                    if pos + 4 > data.len() { return Err(()); }
                    let value_type = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                    computed_checksum = computed_checksum.wrapping_add(value_type);
                    pos += 4;
                    if pos + 4 > data.len() { return Err(()); }
                    let data_len = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                    computed_checksum = computed_checksum.wrapping_add(data_len);
                    pos += 4;
                    if pos + data_len as usize > data.len() { return Err(()); }
                    let val_data = data[pos..pos + data_len as usize].to_vec();
                    for &b in &val_data {
                        computed_checksum = computed_checksum.wrapping_add(b as u32);
                    }
                    pos += data_len as usize;
                    if pos + 4 > data.len() { return Err(()); }
                    let nxt = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                    computed_checksum = computed_checksum.wrapping_add(nxt);
                    pos += 4;

                    if (cell_idx as usize) >= cells.len() {
                        return Err(());
                    }
                    cells[cell_idx as usize] = Some(Cell::Value(ValueCell {
                        name: name_str.to_string(),
                        value_type,
                        data: val_data,
                        next: nxt,
                    }));
                    count += 1;
                }
                3 => {
                    if pos + 4 > data.len() { return Err(()); }
                    let sd_len = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                    computed_checksum = computed_checksum.wrapping_add(sd_len);
                    pos += 4;
                    if pos + sd_len as usize > data.len() { return Err(()); }
                    let sd_data = data[pos..pos + sd_len as usize].to_vec();
                    for &b in &sd_data {
                        computed_checksum = computed_checksum.wrapping_add(b as u32);
                    }
                    pos += sd_len as usize;
                    if pos + 4 > data.len() { return Err(()); }
                    let nxt = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                    computed_checksum = computed_checksum.wrapping_add(nxt);
                    pos += 4;

                    if (cell_idx as usize) >= cells.len() {
                        return Err(());
                    }
                    cells[cell_idx as usize] = Some(Cell::Security(SecurityCell {
                        sd_data,
                        next: nxt,
                    }));
                    count += 1;
                }
                _ => return Err(()),
            }
        }

        if computed_checksum != stored_checksum {
            return Err(());
        }

        let mut hive = Hive {
            name: String::new(),
            cells,
            next_alloc_hint: 1,
            count,
            dirty: false,
        };
        if hive.cells[0].is_none() {
            hive.cells[0] = Some(Cell::Key(KeyCell::new("", NULL_CELL)));
            hive.count += 1;
        }
        Ok(hive)
    }

    pub fn flush_to_io(&self, _io: &crate::vfs::io::IoStack) -> Result<(), ()> {
        let data = self.serialize();
        if data.len() > 4096 {
            return Err(());
        }
        let mut sector = [0u8; 512];
        let chunk_len = core::cmp::min(data.len(), 512);
        sector[..chunk_len].copy_from_slice(&data[..chunk_len]);
        _io.write_sector(0, &sector)
    }

    pub fn load_from_io(_io: &crate::vfs::io::IoStack, name: &str) -> Result<Self, ()> {
        let sector = _io.read_sector(0)?;
        if &sector[0..4] == b"NEOH" {
            let size = {
                let entry_count = u32::from_le_bytes([sector[8], sector[9], sector[10], sector[11]]);
                let est = 16u32 + entry_count * 48;
                core::cmp::min(512, est as usize)
            };
            if let Ok(hive) = Self::deserialize(&sector[..size]) {
                let mut h = hive;
                h.name = name.to_string();
                return Ok(h);
            }
        }
        Err(())
    }
}
