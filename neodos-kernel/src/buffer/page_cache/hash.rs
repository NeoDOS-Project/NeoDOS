//! FNV key helpers, open-addressing hash table and readahead state.

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

const fn fnv_hash(key: u64) -> u64 {
    let mut h = FNV_OFFSET;
    let bytes = key.to_le_bytes();
    let mut i = 0;
    while i < 8 {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

pub(crate) const fn make_inode_key(drive_id: u8, inode: u32, block: u32) -> u64 {
    (drive_id as u64) << 56 | (inode as u64) << 32 | block as u64
}

pub(crate) const HT_EMPTY: u64 = u64::MAX;
pub(crate) const HT_TOMBSTONE: u64 = u64::MAX - 1;

#[derive(Copy, Clone)]
pub(crate) struct HashEntry {
    pub(crate) key: u64,
    pub(crate) value: u16,
}

pub(crate) const EMPTY_HASH_ENTRY: HashEntry = HashEntry {
    key: HT_EMPTY,
    value: 0,
};

pub(crate) struct HashTable<const N: usize> {
    pub(crate) entries: [HashEntry; N],
    pub(crate) len: usize,
}

impl<const N: usize> HashTable<N> {
    pub(crate) const fn new() -> Self {
        HashTable {
            entries: [EMPTY_HASH_ENTRY; N],
            len: 0,
        }
    }

    pub(crate) fn insert(&mut self, key: u64, value: u16) -> bool {
        if self.len * 2 >= N * 7 {
            return false;
        }
        let mut idx = (fnv_hash(key) as usize) % N;
        let mut first_tombstone = None;
        for _ in 0..N {
            match self.entries[idx].key {
                HT_EMPTY => {
                    let slot = first_tombstone.unwrap_or(idx);
                    self.entries[slot] = HashEntry { key, value };
                    if first_tombstone.is_none() {
                        self.len += 1;
                    }
                    return true;
                }
                HT_TOMBSTONE if first_tombstone.is_none() => {
                    first_tombstone = Some(idx);
                }
                k if k == key => {
                    self.entries[idx].value = value;
                    return true;
                }
                _ => {}
            }
            idx = (idx + 1) % N;
        }
        false
    }

    pub(crate) fn get(&self, key: u64) -> Option<u16> {
        let mut idx = (fnv_hash(key) as usize) % N;
        for _ in 0..N {
            match self.entries[idx].key {
                HT_EMPTY => return None,
                HT_TOMBSTONE => {}
                k if k == key => return Some(self.entries[idx].value),
                _ => {}
            }
            idx = (idx + 1) % N;
        }
        None
    }

    pub(crate) fn remove(&mut self, key: u64) -> bool {
        let mut idx = (fnv_hash(key) as usize) % N;
        for _ in 0..N {
            match self.entries[idx].key {
                HT_EMPTY => return false,
                HT_TOMBSTONE => {}
                k if k == key => {
                    self.entries[idx].key = HT_TOMBSTONE;
                    self.entries[idx].value = 0;
                    self.len = self.len.saturating_sub(1);
                    return true;
                }
                _ => {}
            }
            idx = (idx + 1) % N;
        }
        false
    }

    pub(crate) fn contains(&self, key: u64) -> bool {
        self.get(key).is_some()
    }
}

#[derive(Copy, Clone)]
pub(crate) struct ReadaheadState {
    pub(crate) last_block: i64,
    pub(crate) consecutive_count: u32,
    pub(crate) window_size: u32,
    pub(crate) direction: i8,
}

pub(crate) const EMPTY_READAHEAD: ReadaheadState = ReadaheadState {
    last_block: -1,
    consecutive_count: 0,
    window_size: 4,
    direction: 0,
};
