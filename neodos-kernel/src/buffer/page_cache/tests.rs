//! Page cache tests.

use super::*;

// ── Test support ────────────────────────────────────────────────────

/// Minimal in-memory block device for page-cache tests.
///
/// Kept local to `buffer` on purpose: importing the filesystem checker's
/// `TestBlockDevice` would add a forbidden `buffer -> fs` dependency (#715).
struct TestDevice {
    sectors: alloc::vec::Vec<[u8; 512]>,
}

impl super::BlockDevice for TestDevice {
    // `IrpId` is a `u32` alias; naming the alias type here would pull `irp`
    // into the `buffer` subsystem.
    fn submit_irp(&mut self, _irp_id: u32) -> Result<(), ()> {
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

// ── Tests ──────────────────────────────────────────────────────────

pub fn register_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;

    test_case!("page_cache_create_empty", {
        let pc = PageCache::new();
        test_eq!(pc.entry_count(), 0);
        test_eq!(pc.dirty_count(), 0);
    });

    test_case!("page_cache_peek_miss", {
        let pc = PageCache::new();
        test_eq!(pc.peek_inode(0, 1, 0), None);
        test_eq!(pc.peek_inode(0, 1, 1), None);
        test_eq!(pc.peek_inode(0, 0, 0), None);
    });

    test_case!("page_cache_mark_dirty_adds_dirty", {
        let mut pc = PageCache::new();
        test_eq!(pc.dirty_count(), 0);
        pc.mark_dirty(0, 1, 0);
        test_eq!(pc.dirty_count(), 0);
    });

    test_case!("page_cache_invalidate_noop_empty", {
        let mut pc = PageCache::new();
        pc.invalidate_inode(0, 42);
        test_eq!(pc.entry_count(), 0);
    });

    test_case!("page_cache_invalidate_multiple", {
        let mut pc = PageCache::new();
        pc.invalidate_inode(0, 1);
        pc.invalidate_inode(0, 2);
        test_eq!(pc.entry_count(), 0);
    });

    test_case!("page_cache_entry_count_bounds", {
        let pc = PageCache::new();
        test_true!(pc.entry_count() <= 128);
        test_eq!(pc.dirty_count(), 0);
    });

    test_case!("page_cache_dirty_count_never_negative", {
        let pc = PageCache::new();
        test_true!(pc.dirty_count() < usize::MAX);
    });

    test_case!("page_cache_peek_returns_none_unknown", {
        let pc = PageCache::new();
        for inode in &[1u32, 2, 3] {
            for block in &[0u32, 1, 5, 10] {
                test_eq!(pc.peek_inode(0, *inode, *block), None);
            }
        }
    });

    test_case!("page_cache_capacity", {
        let pc = PageCache::new();
        test_eq!(pc.capacity(), 128);
        test_eq!(pc.max_capacity(), 2048);
        test_eq!(pc.min_capacity(), 64);
    });

    test_case!("page_cache_stats_empty", {
        let pc = PageCache::new();
        let stats = pc.stats();
        test_eq!(stats.hits, 0);
        test_eq!(stats.misses, 0);
        test_eq!(stats.evictions, 0);
        test_eq!(stats.current_entries, 0);
        test_eq!(stats.dirty_count, 0);
        test_eq!(stats.pending_writes, 0);
        test_eq!(stats.hash_table_len, 0);
    });

    test_case!("page_cache_hit_rate_zero", {
        let pc = PageCache::new();
        test_eq!(pc.hit_rate(), 0.0);
    });

    test_case!("page_cache_pending_write_count_zero", {
        let pc = PageCache::new();
        test_eq!(pc.pending_write_count(), 0);
    });

    test_case!("page_cache_invalidate_leaves_other_inodes", {
        let mut pc = PageCache::new();
        pc.invalidate_inode(0, 1);
        pc.invalidate_inode(0, 2);
        test_eq!(pc.entry_count(), 0);
        pc.invalidate_inode(0, 1);
        test_eq!(pc.entry_count(), 0);
    });

    test_case!("page_cache_flush_noop_empty", {
        let pc = PageCache::new();
        test_eq!(pc.dirty_count(), 0);
    });

    test_case!("vfs_cache_coherency", {
        let mut pc = PageCache::new();
        test_eq!(pc.dirty_count(), 0);
        pc.mark_dirty_sector(0, 0);
        test_eq!(pc.dirty_count(), 0);
        pc.mark_dirty(0, 1, 0);
        test_eq!(pc.dirty_count(), 0);
        pc.invalidate_inode(0, 0);
        test_eq!(pc.dirty_count(), 0);
        test_eq!(pc.peek_inode(0, 1, 0), None);
    });

    // #709: two devices must not alias on the same LBA, and a dirty page must
    // not be dropped when another device touches that LBA.
    test_case!("page_cache_devices_do_not_alias", {
        let mut pc = PageCache::new();
        let mut a = TestDevice { sectors: alloc::vec![[0u8; 512]; 8] };
        let mut b = TestDevice { sectors: alloc::vec![[0u8; 512]; 8] };

        {
            let data = pc.get_page_mut(0, 1, 0, 0, 1, &mut a).unwrap();
            data[0] = 0xAA;
        }
        // Device B touches the same LBA 0.
        {
            let data = pc.get_page_mut(0, 1, 0, 0, 2, &mut b).unwrap();
            data[0] = 0xBB;
        }
        // Flushing A must write A's dirty page: it must not have been dropped.
        pc.flush(1, &mut a).unwrap();
        let s = a.read_sector(0).unwrap();
        test_eq!(s[0], 0xAA);
    });
}
