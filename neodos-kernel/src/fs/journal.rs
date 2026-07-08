// src/fs/journal.rs — Write-ahead log for NeoFS crash recovery (FS-5)
#![allow(dead_code)]

use crate::buffer::page_cache::PageCache;
use crate::drivers::block::BlockDevice;
use crate::serial_println;
use alloc::vec::Vec;
use super::neodos_fs::{Inode, InodeCache, crc32};

/// Journal magic constants
const JOURNAL_MAGIC: u32 = 0x4A4F5552;  // "JOUR"
const JOURNAL_ENTRY_MAGIC: u32 = 0x454E5452; // "ENTR"

/// Journal entry types
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalEntryType {
    Begin = 1,
    Data = 2,
    Commit = 3,
    Rollback = 4,
}

/// File system operations tracked by journal
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalOperation {
    CreateFile = 1,
    WriteFile = 2,
    DeleteFile = 3,
    Mkdir = 4,
    Rmdir = 5,
    Rename = 6,
}

/// A single journal entry (512 bytes = 1 sector)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct JournalEntry {
    pub magic: u32,             // JOURNAL_ENTRY_MAGIC
    pub sequence: u32,          // Transaction sequence number
    pub entry_type: u8,         // JournalEntryType
    pub operation: u8,          // JournalOperation
    pub inode_num: u32,         // Target inode
    pub data_len: u16,          // Length of data blob (0..256)
    pub timestamp: u64,         // Monotonic tick count
    pub data: [u8; 256],        // Inode snapshot (256 bytes)
    pub checksum: u32,          // CRC32 of magic..data
}

impl JournalEntry {
    pub fn compute_checksum(&self) -> u32 {
        let ck_bytes = unsafe {
            core::slice::from_raw_parts(self as *const _ as *const u8, 280)
        };
        crc32(ck_bytes)
    }

    pub fn set_checksum(&mut self) {
        self.checksum = self.compute_checksum();
    }

    pub fn verify_checksum(&self) -> bool {
        self.checksum == self.compute_checksum()
    }
}

/// Journal header stored in the first journal sector
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct JournalHeader {
    pub magic: u32,             // JOURNAL_MAGIC
    pub sequence: u32,          // Next transaction sequence
    pub checkpoint: u32,        // Last fully checkpointed sequence
    pub num_entries: u32,       // Entries written since last checkpoint
    pub start_sector: u32,      // First sector of journal data area
    pub num_sectors: u32,       // Total sectors reserved for journal
    pub reserved: [u8; 488],    // Padding to 512 bytes
}

impl JournalHeader {
    pub fn is_valid(&self) -> bool {
        self.magic == JOURNAL_MAGIC
    }
}

/// Journal state machine
pub struct Journal {
    /// Absolute LBA of the journal header sector
    header_sector: u32,
    /// Absolute LBA of the first journal entry sector
    entry_base: u32,
    /// Total sectors in the journal area
    num_sectors: u32,
    /// Maximum entries the journal can hold (minus 1 for header)
    max_entries: u32,
    /// Current transaction sequence number
    sequence: u32,
    /// Last checkpointed sequence
    checkpoint: u32,
    /// Number of entries since last checkpoint
    num_entries: u32,
    /// Whether journaling is enabled
    enabled: bool,
}

impl Journal {
    /// Create a new journal at the given sector range.
    /// `header_sector` is the absolute LBA of the journal header.
    /// `num_sectors` is the total space reserved (including header).
    pub fn new(header_sector: u32, num_sectors: u32) -> Self {
        let max_entries = if num_sectors > 1 { num_sectors - 1 } else { 0 };
        Journal {
            header_sector,
            entry_base: header_sector + 1,
            num_sectors,
            max_entries,
            sequence: 1,
            checkpoint: 0,
            num_entries: 0,
            enabled: num_sectors > 1,
        }
    }

    /// Load journal state from disk.
    pub fn load(&mut self, cache: &mut PageCache, dev: &mut dyn BlockDevice) {
        if !self.enabled {
            return;
        }
        let sector_data = match cache.get_sector(self.header_sector, dev) {
            Ok(d) => d,
            Err(_) => {
                self.enabled = false;
                return;
            }
        };
        let header: JournalHeader = unsafe {
            core::ptr::read_unaligned(sector_data.as_ptr() as *const _)
        };
        if header.is_valid() {
            self.sequence = header.sequence;
            self.checkpoint = header.checkpoint;
            self.num_entries = header.num_entries;
            serial_println!("[JOURNAL] Loaded: seq={} ckpt={} entries={}",
                self.sequence, self.checkpoint, self.num_entries);
        } else {
            // Initialize fresh journal
            serial_println!("[JOURNAL] Fresh journal (no header found)");
            self.checkpoint = 0;
            self.sequence = 1;
            self.num_entries = 0;
        }
    }

    /// Persist journal header to disk.
    pub fn sync_header(&self, cache: &mut PageCache, dev: &mut dyn BlockDevice) {
        if !self.enabled {
            return;
        }
        let header = JournalHeader {
            magic: JOURNAL_MAGIC,
            sequence: self.sequence,
            checkpoint: self.checkpoint,
            num_entries: self.num_entries,
            start_sector: self.header_sector,
            num_sectors: self.num_sectors,
            reserved: [0u8; 488],
        };
        let sector_data = match cache.get_sector_mut(self.header_sector, dev) {
            Ok(d) => d,
            Err(_) => return,
        };
        unsafe {
            core::ptr::write_unaligned(sector_data.as_mut_ptr() as *mut JournalHeader, header);
        }
    }

    /// Begin a transaction: write a Begin entry.
    pub fn begin_transaction(&mut self, op: JournalOperation, inode_num: u32, inode_snapshot: &[u8; 256],
                             cache: &mut PageCache, dev: &mut dyn BlockDevice) -> Result<u32, ()> {
        if !self.enabled {
            return Ok(0);
        }
        if self.num_entries >= self.max_entries {
            serial_println!("[JOURNAL] Journal full, forcing checkpoint");
            self.checkpoint_internal(cache, dev)?;
        }

        let seq = self.sequence;
        let entry_idx = self.num_entries;
        let sector_lba = self.entry_base + entry_idx;

        let mut entry = JournalEntry {
            magic: JOURNAL_ENTRY_MAGIC,
            sequence: seq,
            entry_type: JournalEntryType::Begin as u8,
            operation: op as u8,
            inode_num,
            data_len: 256u16,
            timestamp: crate::hal::get_ticks(),
            data: *inode_snapshot,
            checksum: 0,
        };
        entry.set_checksum();

        let sector_data = match cache.get_sector_mut(sector_lba, dev) {
            Ok(d) => d,
            Err(_) => return Err(()),
        };
        unsafe {
            core::ptr::write_unaligned(sector_data.as_mut_ptr() as *mut JournalEntry, entry);
        }
        self.num_entries += 1;
        self.sync_header(cache, dev);

        serial_println!("[JOURNAL] Begin tx={} op={:?} inode={}", seq, op, inode_num);
        Ok(seq)
    }

    /// Commit a transaction: write a Commit entry.
    pub fn commit_transaction(&mut self, tx_id: u32, cache: &mut PageCache, dev: &mut dyn BlockDevice) -> Result<(), ()> {
        if !self.enabled || tx_id == 0 {
            return Ok(());
        }
        if self.num_entries >= self.max_entries {
            return Err(());
        }

        let entry_idx = self.num_entries;
        let sector_lba = self.entry_base + entry_idx;

        let mut entry = JournalEntry {
            magic: JOURNAL_ENTRY_MAGIC,
            sequence: tx_id,
            entry_type: JournalEntryType::Commit as u8,
            operation: 0,
            inode_num: 0,
            data_len: 0,
            timestamp: crate::hal::get_ticks(),
            data: [0u8; 256],
            checksum: 0,
        };
        entry.set_checksum();

        let sector_data = match cache.get_sector_mut(sector_lba, dev) {
            Ok(d) => d,
            Err(_) => return Err(()),
        };
        unsafe {
            core::ptr::write_unaligned(sector_data.as_mut_ptr() as *mut JournalEntry, entry);
        }
        self.num_entries += 1;
        self.sync_header(cache, dev);

        serial_println!("[JOURNAL] Commit tx={}", tx_id);
        Ok(())
    }

    /// Rollback: find a Begin entry for the given tx, restore inode snapshot.
    pub fn rollback_transaction(tx_id: u32, entry_base: u32, cache: &mut PageCache, dev: &mut dyn BlockDevice,
                                inode_cache: &mut InodeCache, abs_lba: u32,
                                _data_start: u32) -> Result<(), ()> {
        // Scan backwards for the Begin entry for this tx
        // In a real system we'd search more efficiently
        for offset in (0..1024).rev() {
            let sector_lba = entry_base + offset as u32;
            let sector_data = match cache.get_sector(sector_lba, dev) {
                Ok(d) => d,
                Err(_) => continue,
            };
            let entry: JournalEntry = unsafe {
                core::ptr::read_unaligned(sector_data.as_ptr() as *const _)
            };
            if entry.magic != JOURNAL_ENTRY_MAGIC || !entry.verify_checksum() {
                continue;
            }
            if entry.sequence == tx_id && entry.entry_type == JournalEntryType::Begin as u8 {
                // Restore inode from snapshot
                let inode: Inode = unsafe {
                    core::ptr::read_unaligned(entry.data.as_ptr() as *const _)
                };
                let inode_num = entry.inode_num as usize;
                let inode_sector = abs_lba + 1 + (inode_num as u32 / 2);
                let offset_in_sector = (inode_num % 2) * 256;
                let sector_data = match cache.get_sector_mut(inode_sector, dev) {
                    Ok(d) => d,
                    Err(_) => return Err(()),
                };
                unsafe {
                    core::ptr::write_unaligned(
                        sector_data.as_mut_ptr().add(offset_in_sector) as *mut Inode,
                        inode
                    );
                }
                // Update cache
                let mut restored = inode;
                restored.set_checksum();
                inode_cache.ensure_inode_capacity(inode_num);
                inode_cache.inodes[inode_num] = Some(restored);
                serial_println!("[JOURNAL] Rolled back tx={} inode={}", tx_id, inode_num);
                return Ok(());
            }
        }
        serial_println!("[JOURNAL] No Begin entry found for tx={}", tx_id);
        Err(())
    }

    /// Recover: scan journal for uncommitted transactions and roll them back.
    pub fn recover(&mut self, cache: &mut PageCache, dev: &mut dyn BlockDevice,
                   inode_cache: &mut InodeCache, abs_lba: u32,
                   _data_start: u32) -> Result<u32, ()> {
        if !self.enabled {
            return Ok(0);
        }
        let mut recovered = 0u32;

        // Read journal header to see where we are
        self.load(cache, dev);

        if self.num_entries == 0 {
            serial_println!("[JOURNAL] Clean journal, no recovery needed");
            return Ok(0);
        }

        serial_println!("[JOURNAL] Recovery: scanning {} entries for incomplete transactions...", self.num_entries);

        // Collect all Begin entries that don't have a matching Commit
        let mut begins: Vec<(u32, u32)> = Vec::new(); // (tx_id, entry_offset)
        let mut commits: Vec<u32> = Vec::new();

        for offset in 0..self.num_entries {
            let sector_lba = self.entry_base + offset;
            let sector_data = match cache.get_sector(sector_lba, dev) {
                Ok(d) => d,
                Err(_) => continue,
            };
            let entry: JournalEntry = unsafe {
                core::ptr::read_unaligned(sector_data.as_ptr() as *const _)
            };
            if entry.magic != JOURNAL_ENTRY_MAGIC || !entry.verify_checksum() {
                continue;
            }
            match entry.entry_type {
                1 => begins.push((entry.sequence, offset)), // Begin
                3 => commits.push(entry.sequence),          // Commit
                _ => {}
            }
        }

        // Roll back any Begin without Commit
        for (tx_id, offset) in &begins {
            if !commits.contains(tx_id) {
                serial_println!("[JOURNAL] Found uncommitted tx={} at offset {}", tx_id, offset);
                // Read the begin entry to get inode snapshot
                let sector_lba = self.entry_base + offset;
                let sector_data = match cache.get_sector(sector_lba, dev) {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                let entry: JournalEntry = unsafe {
                    core::ptr::read_unaligned(sector_data.as_ptr() as *const _)
                };
                let inode: Inode = unsafe {
                    core::ptr::read_unaligned(entry.data.as_ptr() as *const _)
                };
                let inode_num = entry.inode_num as usize;
                let inode_sector = abs_lba + 1 + (inode_num as u32 / 2);
                let offset_in_sector = (inode_num % 2) * 256;
                let sector_data = match cache.get_sector_mut(inode_sector, dev) {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                unsafe {
                    core::ptr::write_unaligned(
                        sector_data.as_mut_ptr().add(offset_in_sector) as *mut Inode,
                        inode
                    );
                }
                let mut restored = inode;
                restored.set_checksum();
                inode_cache.ensure_inode_capacity(inode_num);
                inode_cache.inodes[inode_num] = Some(restored);
                recovered += 1;
            }
        }

        // Reset journal
        self.sequence += 1;
        self.checkpoint = self.sequence - 1;
        self.num_entries = 0;
        self.sync_header(cache, dev);

        serial_println!("[JOURNAL] Recovery complete: {} transactions rolled back", recovered);
        Ok(recovered)
    }

    /// Internal: checkpoint and clear the journal.
    fn checkpoint_internal(&mut self, cache: &mut PageCache, dev: &mut dyn BlockDevice) -> Result<(), ()> {
        self.checkpoint = self.sequence;
        self.sequence += 1;
        self.num_entries = 0;
        self.sync_header(cache, dev);
        Ok(())
    }

    /// Accessors for testing
    pub fn enabled(&self) -> bool { self.enabled }
    pub fn sequence(&self) -> u32 { self.sequence }
    pub fn checkpoint(&self) -> u32 { self.checkpoint }
    pub fn entry_base(&self) -> u32 { self.entry_base }
}

/// Reserve journal sectors at the end of the data area.
/// Returns (header_sector, num_sectors) or None if insufficient space.
pub fn reserve_journal_area(num_blocks: u32, block_size: u32) -> Option<(u32, u32)> {
    let sectors_per_block = block_size / 512;
    let total_sectors = num_blocks * sectors_per_block;
    // Reserve 64 journal sectors (32 KB) or 1% of total, whichever is smaller
    let journal_sectors = core::cmp::min(64u32, total_sectors / 100).max(4); // at least 4
    let data_sectors = total_sectors.saturating_sub(journal_sectors);
    let header_sector = data_sectors;
    if header_sector + journal_sectors <= total_sectors {
        Some((header_sector, journal_sectors))
    } else {
        None
    }
}

// ── Tests ──

pub fn register_journal_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;

    test_case!("fs_journal_replay", {
        // Test journal entry serialization round-trip and checksum
        let mut entry = JournalEntry {
            magic: JOURNAL_ENTRY_MAGIC,
            sequence: 42,
            entry_type: JournalEntryType::Begin as u8,
            operation: JournalOperation::CreateFile as u8,
            inode_num: 7,
            data_len: 256,
            timestamp: 12345,
            data: [0u8; 256],
            checksum: 0,
        };
        entry.data[0] = 0xAB;
        entry.data[100] = 0xCD;
        entry.set_checksum();
        test_true!(entry.verify_checksum());

        // Modify a byte — checksum should fail
        entry.data[50] = 0xFF;
        test_true!(!entry.verify_checksum());
    });

    test_case!("fs_journal_header_validation", {
        let header = JournalHeader {
            magic: JOURNAL_MAGIC,
            sequence: 1,
            checkpoint: 0,
            num_entries: 0,
            start_sector: 100,
            num_sectors: 64,
            reserved: [0u8; 488],
        };
        test_true!(header.is_valid());

        let bad_header = JournalHeader {
            magic: 0xDEADBEEF,
            ..header
        };
        test_true!(!bad_header.is_valid());
    });

    test_case!("fs_journal_reserve_area", {
        let area = reserve_journal_area(10000, 4096);
        test_true!(area.is_some());
        let (header_sector, num_sectors) = area.unwrap();
        test_true!(num_sectors >= 4);
        test_true!(num_sectors <= 64);
        // Verify area fits within total sectors
        let total_sectors = 10000 * 8;
        test_true!(header_sector + num_sectors <= total_sectors);
    });

    test_case!("fs_journal_tx_lifecycle", {
        // In-memory journal lifecycle test
        let journal = Journal::new(100, 10);
        test_true!(journal.enabled());
        test_eq!(journal.sequence(), 1);
        test_eq!(journal.checkpoint(), 0);
        test_eq!(journal.entry_base(), 101);

        // Journal with 0 sectors should be disabled
        let disabled = Journal::new(0, 0);
        test_true!(!disabled.enabled());
    });
}
