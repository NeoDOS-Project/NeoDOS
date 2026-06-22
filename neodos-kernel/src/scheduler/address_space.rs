// A4.3 — Address space validation for ELF loader
//
// Tracks loaded ELF segments per-EPROCESS and validates against
// protected memory regions to prevent malicious ELF exploits.

use alloc::vec::Vec;

// ── Memory layout constants (from paging.rs) ──

pub const USER_BASE: u64 = 0x0040_0000;
pub const USER_LIMIT: u64 = 0x0240_0000; // 32 MB window (v0.40)

// ── Protected region ranges ──

struct ProtectedRegion {
    start: u64,
    end: u64,
    name: &'static str,
}

const PROTECTED_REGIONS: &[ProtectedRegion] = &[
    ProtectedRegion { start: 0x400_0000, end: 0x402_0000, name: "kernel_image" },
    ProtectedRegion { start: 0x0240_0000, end: 0x0340_0000, name: "kernel_heap" },
    ProtectedRegion { start: 0x1000_0000, end: 0x1200_0000, name: "user_heap" },
    ProtectedRegion { start: 0x1E00_0000, end: 0x1E20_0000, name: "nxl_region" },
    ProtectedRegion { start: 0x2000_0000, end: 0x2200_0000, name: "mmap_region" },
    ProtectedRegion { start: 0x3000_0000, end: 0x3100_0000, name: "driver_isolation" },
];

// ── Collision error codes (from spec) ──

pub const ELF_ERR_VADDR_OUT_OF_RANGE: i64 = -1;
pub const ELF_ERR_ZERO_VADDR: i64 = -2;
pub const ELF_ERR_SEGMENT_OVERLAP: i64 = -3;
pub const ELF_ERR_KERNEL_COLLISION: i64 = -4;
pub const ELF_ERR_HEAP_COLLISION: i64 = -5;
pub const ELF_ERR_MMAP_COLLISION: i64 = -6;

// ── Segment info ──

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SegmentInfo {
    pub vaddr: u64,
    pub memsz: u64,
    pub flags: u32,
}

// ── AddressSpace ──

pub struct AddressSpace {
    pub loaded_segments: Vec<SegmentInfo>,
}

impl AddressSpace {
    pub fn new() -> Self {
        AddressSpace {
            loaded_segments: Vec::new(),
        }
    }

    /// Validate a single segment range against protected regions and user window.
    /// Returns Ok(()) if valid, Err(error_code) if not.
    /// Order: null → protected regions → user window.
    pub fn validate_segment(vaddr: u64, memsz: u64) -> Result<(), i64> {
        // Check 2: prohibit null vaddr
        if vaddr == 0 {
            return Err(ELF_ERR_ZERO_VADDR);
        }

        // Check 4: no collision with protected regions (before general range check
        // so that specific error codes are returned for kernel/heap/mmap violations)
        let seg_end = vaddr.saturating_add(memsz);
        for region in PROTECTED_REGIONS {
            if vaddr < region.end && seg_end > region.start {
                return Err(match region.name {
                    "kernel_image" => ELF_ERR_KERNEL_COLLISION,
                    "kernel_heap" => ELF_ERR_KERNEL_COLLISION,
                    "user_heap" => ELF_ERR_HEAP_COLLISION,
                    "mmap_region" => ELF_ERR_MMAP_COLLISION,
                    "nxl_region" => ELF_ERR_HEAP_COLLISION,
                    "driver_isolation" => ELF_ERR_KERNEL_COLLISION,
                    _ => ELF_ERR_KERNEL_COLLISION,
                });
            }
        }

        // Check 1: must be within user window
        if vaddr < USER_BASE || seg_end > USER_LIMIT {
            return Err(ELF_ERR_VADDR_OUT_OF_RANGE);
        }

        Ok(())
    }

    /// Check for overlap between two segments.
    pub fn segments_overlap(a: &SegmentInfo, b: &SegmentInfo) -> bool {
        a.vaddr < b.vaddr + b.memsz && b.vaddr < a.vaddr + a.memsz
    }

    /// Full validation: range, null, protected regions, and inter-segment overlap.
    /// Returns Ok(()) if all checks pass, Err(error_code) on failure.
    pub fn add_segment(&mut self, vaddr: u64, memsz: u64, flags: u32) -> Result<(), i64> {
        Self::validate_segment(vaddr, memsz)?;

        let new_seg = SegmentInfo { vaddr, memsz, flags };

        // Check 3: no overlap with existing segments
        for existing in &self.loaded_segments {
            if Self::segments_overlap(&new_seg, existing) {
                return Err(ELF_ERR_SEGMENT_OVERLAP);
            }
        }

        self.loaded_segments.push(new_seg);
        Ok(())
    }

    /// Clear all tracked segments (used on process exit).
    pub fn clear(&mut self) {
        self.loaded_segments.clear();
    }

    pub fn segment_count(&self) -> usize {
        self.loaded_segments.len()
    }
}
