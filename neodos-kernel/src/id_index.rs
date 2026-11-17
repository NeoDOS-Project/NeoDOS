//! NEODOS-05 (#635): a small open-addressing `u64 -> usize` index with **O(1)
//! average** lookup, used by the Ob table and the scheduler id lookups.
//!
//! The kernel is `no_std` (no `std::collections::HashMap`, no `hashbrown`), so
//! this is a compact linear-probing table. Keys are dense counters starting at
//! 1; `0` marks an empty slot, `u64::MAX` a tombstone. It grows at 75% load, so
//! there is always at least one empty slot and `get` always terminates.

use alloc::vec::Vec;

const EMPTY: u64 = 0;
const TOMBSTONE: u64 = u64::MAX;

pub struct IdIndex {
    keys: Vec<u64>,
    vals: Vec<usize>,
    len: usize,
    /// Non-empty slots (live + tombstones); drives the load factor.
    used: usize,
    mask: usize,
}

impl IdIndex {
    pub fn new() -> Self {
        let cap = 16usize;
        IdIndex {
            keys: alloc::vec![EMPTY; cap],
            vals: alloc::vec![0usize; cap],
            len: 0,
            used: 0,
            mask: cap - 1,
        }
    }

    #[inline]
    pub fn len(&self) -> usize { self.len }

    #[inline]
    pub fn is_empty(&self) -> bool { self.len == 0 }

    #[inline]
    fn hash(k: u64) -> usize {
        // Fibonacci hashing over the multiplicative inverse of the golden ratio.
        (k.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32) as usize
    }

    /// Look up the value for `k` (`None` if absent). O(1) average.
    #[inline]
    pub fn get(&self, k: u64) -> Option<usize> {
        let mut i = Self::hash(k) & self.mask;
        loop {
            let kk = self.keys[i];
            if kk == EMPTY { return None; }
            if kk == k { return Some(self.vals[i]); }
            i = (i + 1) & self.mask;
        }
    }

    /// Insert or update `k -> v`.
    pub fn insert(&mut self, k: u64, v: usize) {
        debug_assert!(k != EMPTY && k != TOMBSTONE, "IdIndex key must be 1..u64::MAX");
        if (self.used + 1) * 4 >= self.keys.len() * 3 {
            self.grow();
        }
        self.insert_no_grow(k, v);
    }

    fn insert_no_grow(&mut self, k: u64, v: usize) {
        let mut i = Self::hash(k) & self.mask;
        loop {
            let kk = self.keys[i];
            if kk == EMPTY || kk == TOMBSTONE {
                self.keys[i] = k;
                self.vals[i] = v;
                self.len += 1;
                if kk == EMPTY { self.used += 1; }
                return;
            }
            if kk == k {
                self.vals[i] = v;
                return;
            }
            i = (i + 1) & self.mask;
        }
    }

    /// Remove `k` if present (leaves a tombstone).
    pub fn remove(&mut self, k: u64) {
        let mut i = Self::hash(k) & self.mask;
        loop {
            let kk = self.keys[i];
            if kk == EMPTY { return; }
            if kk == k {
                self.keys[i] = TOMBSTONE;
                self.len -= 1;
                return;
            }
            i = (i + 1) & self.mask;
        }
    }

    fn grow(&mut self) {
        let new_cap = self.keys.len() * 2;
        let old_keys = core::mem::replace(&mut self.keys, alloc::vec![EMPTY; new_cap]);
        let old_vals = core::mem::replace(&mut self.vals, alloc::vec![0usize; new_cap]);
        self.mask = new_cap - 1;
        self.len = 0;
        self.used = 0;
        for (i, &k) in old_keys.iter().enumerate() {
            if k != EMPTY && k != TOMBSTONE {
                self.insert_no_grow(k, old_vals[i]);
            }
        }
    }
}

impl Default for IdIndex {
    fn default() -> Self { Self::new() }
}

pub fn register_tests() {
    use crate::{test_case, test_eq, test_true};

    test_case!("neodos05_id_index_basic", {
        let mut idx = IdIndex::new();
        test_true!(idx.get(1).is_none());
        test_true!(idx.is_empty());
        for k in 1..=200u64 {
            idx.insert(k, (k * 3) as usize);
        }
        test_eq!(idx.len(), 200);
        for k in 1..=200u64 {
            test_eq!(idx.get(k), Some((k * 3) as usize));
        }
        // Remove half (tombstones), verify both halves.
        for k in (1..=200u64).step_by(2) {
            idx.remove(k);
        }
        test_eq!(idx.len(), 100);
        for k in 1..=200u64 {
            test_eq!(idx.get(k).is_some(), k % 2 == 0);
        }
        // Reinsert into tombstoned slots and re-grow.
        for k in (1..=200u64).step_by(2) {
            idx.insert(k, 1);
        }
        for k in 1..=200u64 {
            test_true!(idx.get(k).is_some());
        }
        // Update in place.
        idx.insert(100, 42);
        test_eq!(idx.get(100), Some(42));
    });

    test_case!("neodos05_lookup_benchmark", {
        use crate::infra::boot_benchmark::rdtsc;
        let mut idx = IdIndex::new();
        let keys: alloc::vec::Vec<u64> = (1..=1000u64).collect();
        for &k in &keys {
            idx.insert(k, k as usize);
        }
        let n = 100_000u64;
        let mut acc = 0usize;
        // Indexed lookups (O(1)).
        for i in 0..n {
            acc += idx.get((i % 1000) + 1).unwrap_or(0);
        }
        let t0 = rdtsc();
        for i in 0..n {
            acc += idx.get((i % 1000) + 1).unwrap_or(0);
        }
        let t1 = rdtsc();
        // Linear-scan baseline over the same 1000 keys (O(n)).
        let mut acc2 = 0usize;
        let t2 = rdtsc();
        for i in 0..n {
            let key = (i % 1000) + 1;
            acc2 += keys.iter().position(|&k| k == key).unwrap_or(0);
        }
        let t3 = rdtsc();
        let per_idx = (t1 - t0) / n;
        let per_lin = (t3 - t2) / n;

        // Latency distribution: per-op cycles -> p50 / p99.
        let m = 2000usize;
        let mut samples: alloc::vec::Vec<u64> = alloc::vec::Vec::with_capacity(m);
        for i in 0..m {
            let key = ((i % 1000) + 1) as u64;
            let s = rdtsc();
            let v = idx.get(key).unwrap_or(0);
            let e = rdtsc();
            acc += v;
            samples.push(e - s);
        }
        samples.sort_unstable();
        let p50 = samples[m / 2];
        let p99 = samples[m * 99 / 100];
        crate::serial_println!(
            "[LOOKUP_BENCH] id_index avg={} p50={} p99={} cyc/lookup; linear avg={} (n={}); {} {}",
            per_idx, p50, p99, per_lin, n, acc, acc2);
        // Indexed must be strictly faster than a linear scan over 1000 entries.
        test_true!(per_idx < per_lin);
    });
}
