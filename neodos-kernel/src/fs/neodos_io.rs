//! Extent read/write + inline data para NeoFS v2.
//! Cada archivo tiene extents (start_lba, length) almacenados en un B-tree
//! de tipo extent_list (node_type=2), o datos inline si caben en 208 bytes.

use alloc::vec::Vec;
use crate::drivers::block::BlockDevice;
use crate::buffer::page_cache::PageCache;
use super::neodos_dir::{DirEntryV2, INLINE_MAX};
use super::freelist::FreeList;

pub const BLOCK_SIZE: usize = 4096;

/// Leer datos de un archivo.
/// Intenta inline first, luego extents.
pub fn file_read(
    entry: &DirEntryV2,
    offset: u64,
    buf: &mut [u8],
    cache: &mut PageCache,
    dev: &mut dyn BlockDevice,
) -> Result<usize, ()> {
    if entry.inline_len > 0 {
        // Inline data
        let data = &entry.inline_data[..entry.inline_len as usize];
        if offset as usize >= data.len() {
            return Ok(0);
        }
        let to_copy = core::cmp::min(buf.len(), data.len() - offset as usize);
        buf[..to_copy].copy_from_slice(&data[offset as usize..offset as usize + to_copy]);
        return Ok(to_copy);
    }

    if entry.extent_lba == 0 {
        return Ok(0); // archivo vacío
    }

    // Extent-based read: el primer extent está en entry.extent_lba
    // (formato: [start_lba: u64, length: u32] repetido)
    // Por simplicidad: un solo extent directo
    let start_lba = entry.extent_lba;
    let total_blocks = ((entry.size + BLOCK_SIZE as u64 - 1) / BLOCK_SIZE as u64) as u32;

    let start_block = (offset / BLOCK_SIZE as u64) as u32;
    let block_offset = (offset % BLOCK_SIZE as u64) as usize;
    let mut bytes_read = 0usize;
    let mut block = start_block;

    while bytes_read < buf.len() && block < total_blocks {
        let lba = start_lba + block as u64 * 8;
        let data = cache.read_page(0, 0, block, lba, dev)?;
        let to_copy = (BLOCK_SIZE - block_offset)
            .min(buf.len() - bytes_read)
            .min((entry.size - offset - bytes_read as u64) as usize);
        buf[bytes_read..bytes_read + to_copy].copy_from_slice(&data[block_offset..block_offset + to_copy]);
        bytes_read += to_copy;
        block += 1;
    }
    Ok(bytes_read)
}

/// Escribir datos a un archivo (COW: nuevos bloques, nuevos extents).
/// Devuelve un nuevo DirEntry con los extents actualizados.
/// `partition_base_sector` es el offset de partición en sectores (translate_lba(0)).
pub fn file_write(
    entry: &DirEntryV2,
    offset: u64,
    data: &[u8],
    freelist: &mut FreeList,
    cache: &mut PageCache,
    dev: &mut dyn BlockDevice,
    partition_base_sector: u64,
) -> Result<DirEntryV2, ()> {
    let mut new_entry = entry.clone();

    let new_size = (offset as usize + data.len()).max(new_entry.size as usize);

    // Si cabe inline
    if new_size <= INLINE_MAX {
        let mut inline_data = [0u8; INLINE_MAX];
        inline_data[..new_size].copy_from_slice(&data[..data.len().min(INLINE_MAX)]);
        if offset as usize + data.len() > entry.inline_len as usize {
            let copy_start = offset as usize;
            for i in 0..data.len() {
                if copy_start + i < INLINE_MAX {
                    inline_data[copy_start + i] = data[i];
                }
            }
        }
        new_entry.inline_len = new_size as u32;
        new_entry.inline_data = inline_data;
        new_entry.extent_lba = 0;
        new_entry.extent_count = 0;
        new_entry.size = new_size as u64;
        new_entry.modified = crate::hal::get_ticks();
        new_entry.checksum = crc32(&inline_data[..new_size]);
        return Ok(new_entry);
    }

    // Necesitamos extents. Alocar bloques.
    let total_blocks = ((new_size + BLOCK_SIZE - 1) / BLOCK_SIZE) as u32;
    let start_block = freelist.alloc_blocks(total_blocks).ok_or(())?;
    let start_sector = partition_base_sector + start_block * 8;

    // Escribir datos bloque por bloque
    let mut written = 0usize;
    for block_idx in 0..total_blocks {
        let sector_lba = start_sector + block_idx as u64 * 8;
        let page = cache.get_page_mut(0, 0, block_idx, sector_lba, dev)?;
        let block_start = block_idx as usize * BLOCK_SIZE;
        let to_write = core::cmp::min(BLOCK_SIZE, new_size - block_start);
        if to_write > 0 {
            if block_start >= offset as usize {
                let src_start = block_start - offset as usize;
                let src_end = core::cmp::min(src_start + to_write, data.len());
                page[..to_write].copy_from_slice(&data[src_start..src_end]);
            } else {
                // Partial overlap at start
                let overlap = (offset as usize - block_start).min(to_write);
                if overlap > 0 {
                    // Copy old data for non-overlapping part
                }
                let data_start = 0usize;
                let data_end = core::cmp::min(to_write - overlap, data.len());
                page[overlap..overlap + data_end].copy_from_slice(&data[data_start..data_end]);
            }
        }
        written += to_write;
    }

    new_entry.extent_lba = start_block;
    new_entry.extent_count = total_blocks;
    new_entry.inline_len = 0;
    new_entry.size = new_size as u64;
    new_entry.modified = crate::hal::get_ticks();

    // Checksum del contenido completo
    if new_size <= 65536 {
        let mut full = alloc::vec![0u8; new_size];
        let mut ck_entry = new_entry.clone();
        ck_entry.extent_lba = start_sector;
        let _ = file_read(&ck_entry, 0, &mut full, cache, dev);
        new_entry.checksum = crc32(&full);
        ck_entry.extent_lba = start_block;
    }

    Ok(new_entry)
}

/// Liberar los extents de un archivo a la freelist.
pub fn file_free_extents(entry: &DirEntryV2, freelist: &mut FreeList) {
    if entry.extent_lba != 0 && entry.extent_count > 0 {
        freelist.free(entry.extent_lba, entry.extent_count);
    }
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

pub fn register_io_tests() {
    crate::test_case!("neodos_io_crc32", {
        // Re-export crc32 from neodos_fs
    });
}
