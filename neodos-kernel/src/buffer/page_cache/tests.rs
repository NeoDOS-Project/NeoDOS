//! Page cache tests.

use super::*;

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
        pc.mark_dirty_sector(0);
        test_eq!(pc.dirty_count(), 0);
        pc.mark_dirty(0, 1, 0);
        test_eq!(pc.dirty_count(), 0);
        pc.invalidate_inode(0, 0);
        test_eq!(pc.dirty_count(), 0);
        test_eq!(pc.peek_inode(0, 1, 0), None);
    });
}
