//! Directory operations para NeoFS v2.
//! Cada directorio es un B-tree de DirEntryV2.

use alloc::vec::Vec;
use crate::fs::btree::{BTreeEntry, BTree, BTreeIO};

pub const DIRENTRY_SIZE: usize = 128;
pub const NAME_MAX: usize = 48;
pub const INLINE_MAX: usize = 16;

/// Entrada de directorio (128 bytes, almacenada en B-tree leaf).
#[derive(Debug, Clone)]
pub struct DirEntryV2 {
    pub name: Vec<u8>,
    pub mode: u16,
    pub size: u64,
    pub created: u64,
    pub modified: u64,
    pub checksum: u32,
    pub inline_len: u32,
    pub inline_data: [u8; INLINE_MAX],
    pub extent_lba: u64,
    pub extent_count: u32,
}

pub use super::vfs::{MODE_DIR, MODE_FILE};
pub const PERM_R: u16 = 1;
pub const PERM_W: u16 = 2;
pub const PERM_X: u16 = 4;
pub const PERM_S: u16 = 8;
pub const PERM_D: u16 = 16;

impl DirEntryV2 {
    pub fn new_file(name: &str) -> Self {
        DirEntryV2 {
            name: name.as_bytes().to_vec(),
            mode: MODE_FILE | PERM_R | PERM_W | PERM_X | PERM_D,
            size: 0,
            created: 0,
            modified: 0,
            checksum: 0,
            inline_len: 0,
            inline_data: [0u8; INLINE_MAX],
            extent_lba: 0,
            extent_count: 0,
        }
    }

    pub fn new_dir(name: &str) -> Self {
        DirEntryV2 {
            name: name.as_bytes().to_vec(),
            mode: MODE_DIR | PERM_R | PERM_W | PERM_X | PERM_D,
            size: 0,
            created: 0,
            modified: 0,
            checksum: 0,
            inline_len: 0,
            inline_data: [0u8; INLINE_MAX],
            extent_lba: 0,
            extent_count: 0,
        }
    }

    pub fn is_dir(&self) -> bool { self.mode & MODE_DIR != 0 }
    pub fn is_file(&self) -> bool { self.mode & MODE_FILE != 0 }

    pub fn serialize(&self, buf: &mut [u8; DIRENTRY_SIZE]) {
        buf.fill(0);
        let nl = self.name.len().min(NAME_MAX);
        buf[0] = nl as u8;
        buf[1..1 + nl].copy_from_slice(&self.name[..nl]);
        // name[1..1+NAME_MAX] = 49 bytes total for name section
        // inline_data follows name section
        let off_name = 1 + NAME_MAX; // 49
        let il = self.inline_len.min(INLINE_MAX as u32) as usize;
        buf[off_name..off_name + il].copy_from_slice(&self.inline_data[..il]);
        let mut off = off_name + INLINE_MAX; // 49 + 16 = 65
        buf[off..off + 2].copy_from_slice(&self.mode.to_le_bytes()); off += 2; // 67
        buf[off..off + 8].copy_from_slice(&self.size.to_le_bytes()); off += 8; // 75
        buf[off..off + 8].copy_from_slice(&self.created.to_le_bytes()); off += 8; // 83
        buf[off..off + 8].copy_from_slice(&self.modified.to_le_bytes()); off += 8; // 91
        buf[off..off + 4].copy_from_slice(&self.checksum.to_le_bytes()); off += 4; // 95
        buf[off..off + 4].copy_from_slice(&self.inline_len.to_le_bytes()); off += 4; // 99
        buf[off..off + 8].copy_from_slice(&self.extent_lba.to_le_bytes()); off += 8; // 107
        buf[off..off + 4].copy_from_slice(&self.extent_count.to_le_bytes()); // 111
        // padding to 128
    }

    pub fn deserialize(buf: &[u8; DIRENTRY_SIZE]) -> Self {
        let nl = buf[0] as usize;
        let name = if nl > 0 { buf[1..1 + nl.min(NAME_MAX)].to_vec() } else { Vec::new() };
        let off_name = 1 + NAME_MAX;
        let off_inline = off_name;
        let mut inline_data = [0u8; INLINE_MAX];
        inline_data.copy_from_slice(&buf[off_inline..off_inline + INLINE_MAX]);
        let mut off = off_inline + INLINE_MAX;
        let mode = u16::from_le_bytes(buf[off..off+2].try_into().unwrap_or([0;2])); off += 2;
        let size = u64::from_le_bytes(buf[off..off+8].try_into().unwrap_or([0;8])); off += 8;
        let created = u64::from_le_bytes(buf[off..off+8].try_into().unwrap_or([0;8])); off += 8;
        let modified = u64::from_le_bytes(buf[off..off+8].try_into().unwrap_or([0;8])); off += 8;
        let checksum = u32::from_le_bytes(buf[off..off+4].try_into().unwrap_or([0;4])); off += 4;
        let inline_len = u32::from_le_bytes(buf[off..off+4].try_into().unwrap_or([0;4])); off += 4;
        let extent_lba = u64::from_le_bytes(buf[off..off+8].try_into().unwrap_or([0;8])); off += 8;
        let extent_count = u32::from_le_bytes(buf[off..off+4].try_into().unwrap_or([0;4]));
        DirEntryV2 { name, mode, size, created, modified, checksum, inline_len, inline_data, extent_lba, extent_count }
    }

    /// Serializar a entrada de B-tree (key=name, value=128 bytes).
    pub fn to_btree_entry(&self) -> BTreeEntry {
        let mut val = [0u8; DIRENTRY_SIZE];
        self.serialize(&mut val);
        BTreeEntry {
            key: self.name.clone(),
            value: val.to_vec(),
        }
    }

    pub fn from_btree_entry(e: &BTreeEntry) -> Self {
        let mut buf = [0u8; DIRENTRY_SIZE];
        let len = e.value.len().min(DIRENTRY_SIZE);
        buf[..len].copy_from_slice(&e.value[..len]);
        Self::deserialize(&buf)
    }
}

/// Cargar un DirEntry desde el B-tree de un directorio.
pub fn dir_lookup(io: &impl BTreeIO, dir_root_lba: u64, name: &str) -> Option<DirEntryV2> {
    let key = name.as_bytes();
    let result = BTree::lookup(io, dir_root_lba, key)?;
    let mut buf = [0u8; DIRENTRY_SIZE];
    let len = result.len().min(DIRENTRY_SIZE);
    buf[..len].copy_from_slice(&result[..len]);
    Some(DirEntryV2::deserialize(&buf))
}

/// Enumerar entradas de un directorio (índice secuencial).
pub fn dir_readdir(io: &impl BTreeIO, dir_root_lba: u64, index: usize) -> Option<DirEntryV2> {
    let mut i = 0usize;
    let mut result: Option<DirEntryV2> = None;
    BTree::walk(io, dir_root_lba, &mut |entry: &BTreeEntry| {
        if i == index {
            result = Some(DirEntryV2::from_btree_entry(entry));
        }
        i += 1;
    });
    result
}

/// Contar entradas de un directorio.
pub fn dir_count(io: &impl BTreeIO, dir_root_lba: u64) -> usize {
    let mut count = 0usize;
    BTree::walk(io, dir_root_lba, &mut |_| { count += 1; });
    count
}

// ── Tests ──

pub fn register_dir_tests() {
    crate::test_case!("neodos_dir_serialize_roundtrip", {
        let d = DirEntryV2::new_file("test.txt");
        let mut buf = [0u8; DIRENTRY_SIZE];
        d.serialize(&mut buf);
        let loaded = DirEntryV2::deserialize(&buf);
        crate::test_eq!(core::str::from_utf8(&loaded.name).unwrap(), "test.txt");
        crate::test_true!(loaded.is_file());
    });

    crate::test_case!("neodos_dir_mode_bits", {
        let f = DirEntryV2::new_file("a");
        crate::test_true!(f.is_file());
        crate::test_true!(!f.is_dir());
        let d = DirEntryV2::new_dir("b");
        crate::test_true!(d.is_dir());
        crate::test_true!(!d.is_file());
    });
}
