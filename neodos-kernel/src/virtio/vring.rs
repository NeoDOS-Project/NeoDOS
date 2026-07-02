// src/virtio/vring.rs — Split Virtqueue (vring) 1.0
// Descriptor table, available ring, used ring management.
// ALL shared-memory writes use volatile stores (critical for DMA).

#![allow(dead_code)]

use core::sync::atomic::{fence, Ordering};

pub const VRING_DESC_F_NEXT: u16 = 1;
pub const VRING_DESC_F_WRITE: u16 = 2;

/// virtq_desc — 16 bytes
#[derive(Clone, Copy)]
#[repr(C, align(16))]
pub struct VringDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

/// Split virtqueue state.
pub struct SplitVring {
    pub num: u16,
    pub desc_phys: u64,
    pub avail_phys: u64,
    pub used_phys: u64,
    pub last_used_idx: u16,
}

impl SplitVring {
    /// Calculate the vring used ring offset for legacy mode.
    /// QEMU uses: align(num*16 + 4 + 2 + num*2, 4096/2)
    pub fn legacy_used_offset(num: u16) -> usize {
        let desc_sz = num as usize * 16;
        let avail_sz = 4 + num as usize * 2;
        let total = desc_sz + avail_sz;
        // align to 2048 (half page)
        (total + 2047) & !2047
    }

    pub fn new(num: u16, queue_phys: u64, legacy: bool) -> Self {
        let desc_sz = num as usize * 16;
        let avail_sz = 4 + num as usize * 2;

        let (desc_phys, avail_phys, used_phys) = if legacy {
            let used_off = Self::legacy_used_offset(num);
            (queue_phys, queue_phys + desc_sz as u64, queue_phys + used_off as u64)
        } else {
            (queue_phys, queue_phys + desc_sz as u64,
             queue_phys + desc_sz as u64 + avail_sz as u64)
        };

        SplitVring { num, desc_phys, avail_phys, used_phys, last_used_idx: 0 }
    }

    /// Write a single descriptor entry (volatile store).
    pub unsafe fn write_desc(&self, index: u16, addr: u64, len: u32, flags: u16, next: u16) {
        let p = self.desc_phys as *mut u64;
        let base = index as usize * 2;
        p.add(base).write_volatile(addr);
        p.add(base + 1).write_volatile(
            (len as u64) | ((flags as u64) << 32) | ((next as u64) << 48)
        );
    }

    /// Submit descriptor chain head to the available ring (volatile stores).
    /// Returns the old avail index for subsequent completion polling.
    pub unsafe fn submit_chain(&mut self, head: u16) -> u16 {
        fence(Ordering::SeqCst);
        let flags_ptr = self.avail_phys as *mut u16;
        let idx_ptr = (self.avail_phys + 2) as *mut u16;
        let ring_ptr = (self.avail_phys + 4) as *mut u16;

        let old = idx_ptr.read_volatile();
        let slot = (old as usize) % (self.num as usize);
        ring_ptr.add(slot).write_volatile(head);
        fence(Ordering::SeqCst);
        idx_ptr.write_volatile(old.wrapping_add(1));
        fence(Ordering::SeqCst);

        // Ensure flags is 0 (already zeroed from init)
        flags_ptr.write_volatile(0);
        fence(Ordering::SeqCst);

        old
    }

    /// Poll for completion. Returns True if used ring idx changed.
    /// The used ring is written by the device via DMA — must use volatile read.
    pub unsafe fn poll_completed(&mut self) -> bool {
        let used_idx_ptr = (self.used_phys + 2) as *const u16;
        let new_idx = used_idx_ptr.read_volatile();
        if new_idx == self.last_used_idx {
            return false;
        }
        self.last_used_idx = self.last_used_idx.wrapping_add(1);
        true
    }
}
