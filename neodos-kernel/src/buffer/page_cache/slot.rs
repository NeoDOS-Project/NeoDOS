//! Cache slot layout.

// ── Cache slot — unified 4KB page with sub-sector dirty tracking ──────

#[derive(Copy, Clone)]
pub(crate) struct CacheSlot {
    pub(crate) valid: bool,
    pub(crate) dirty: bool,
    pub(crate) write_pending: bool,
    pub(crate) lba: u64,
    pub(crate) dirty_sectors: u8,
    pub(crate) inode_key: u64,
    pub(crate) dirty_since_tick: u64,
    pub(crate) data: [u8; 4096],
    pub(crate) lru_prev: Option<u16>,
    pub(crate) lru_next: Option<u16>,
}

pub(crate) const EMPTY_SLOT: CacheSlot = CacheSlot {
    valid: false,
    dirty: false,
    write_pending: false,
    lba: 0,
    dirty_sectors: 0,
    inode_key: 0,
    dirty_since_tick: 0,
    data: [0u8; 4096],
    lru_prev: None,
    lru_next: None,
};
