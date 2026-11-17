//! Bounded, TTL-aware cache for resolver answers.
//!
//! The cache never reads a clock: callers pass a monotonic millisecond value.
//! That keeps it independent of the Ring 3 uptime syscall and lets the host
//! tests drive expiry deterministically.

use alloc::string::String;
use alloc::vec::Vec;

/// Longest TTL an answer may be cached for, in seconds. A stale or hostile
/// answer must not pin an address for the life of the process.
pub const DEFAULT_MAX_TTL_S: u32 = 3_600;

struct Entry {
    host: String,
    server: [u8; 4],
    addr: [u8; 4],
    /// Monotonic time (ms) at which the entry stops being valid.
    expires_ms: u64,
}

/// Fixed-capacity cache keyed by `(hostname, server)`.
pub struct AnswerCache {
    entries: Vec<Entry>,
    capacity: usize,
    max_ttl_s: u32,
}

impl AnswerCache {
    pub const fn new(capacity: usize, max_ttl_s: u32) -> Self {
        AnswerCache {
            entries: Vec::new(),
            capacity,
            max_ttl_s,
        }
    }

    /// Look up a live answer. Returns the address and the remaining TTL in
    /// seconds (rounded up). An expired entry is dropped and reported as a
    /// miss.
    pub fn lookup(&mut self, host: &str, server: [u8; 4], now_ms: u64) -> Option<([u8; 4], u32)> {
        let idx = self
            .entries
            .iter()
            .position(|e| e.host == host && e.server == server)?;
        let entry = &self.entries[idx];
        if now_ms >= entry.expires_ms {
            self.entries.remove(idx);
            return None;
        }
        let left_ms = entry.expires_ms - now_ms;
        let ttl = ((left_ms + 999) / 1000).min(u32::MAX as u64) as u32;
        Some((entry.addr, ttl))
    }

    /// Cache `addr` for `ttl_s`. A zero TTL removes any previous entry and
    /// stores nothing; longer TTLs are capped at [`Self::max_ttl_s`].
    pub fn insert(
        &mut self,
        host: &str,
        server: [u8; 4],
        addr: [u8; 4],
        ttl_s: u32,
        now_ms: u64,
    ) {
        let ttl = ttl_s.min(self.max_ttl_s);
        if ttl == 0 || self.capacity == 0 {
            self.remove(host, server);
            return;
        }
        let expires_ms = now_ms.saturating_add(ttl as u64 * 1000);
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|e| e.host == host && e.server == server)
        {
            entry.addr = addr;
            entry.expires_ms = expires_ms;
            return;
        }
        if self.entries.len() >= self.capacity {
            // Full: replace the entry that expires first.
            if let Some((idx, _)) = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.expires_ms)
            {
                self.entries.swap_remove(idx);
            }
        }
        self.entries.push(Entry {
            host: String::from(host),
            server,
            addr,
            expires_ms,
        });
    }

    /// Drop the entry for `(host, server)`, if any.
    pub fn remove(&mut self, host: &str, server: [u8; 4]) {
        self.entries
            .retain(|e| !(e.host == host && e.server == server));
    }

    /// Drop every entry.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
