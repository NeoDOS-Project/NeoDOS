use alloc::vec::Vec;
use spin::Mutex;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::object::{ObError, ObOperations, ObId};
use crate::slot_table::SlotTable;

const MAX_SECTIONS: usize = 32;

#[derive(Debug, Clone, Copy)]
struct SectionView {
    base: u64,
    size: u64,
}

#[derive(Debug, Clone)]
struct SectionEntry {
    size: u64,
    prot: u32,
    ob_id: ObId,
    views: Vec<SectionView>,
}

pub struct SectionManager {
    sections: SlotTable<SectionEntry, MAX_SECTIONS>,
}

impl SectionManager {
    const fn new() -> Self {
        SectionManager {
            sections: SlotTable::new(),
        }
    }

    pub fn alloc(&mut self, ob_id: ObId, size: u64, prot: u32) -> Result<u32, ObError> {
        if size == 0 || size > 0x100000 {
            return Err(ObError::InvalidParam);
        }
        let entry = SectionEntry {
            size,
            prot,
            ob_id,
            views: Vec::new(),
        };
        self.sections
            .alloc(entry)
            .map(|index| index as u32)
            .map_err(|_| ObError::TableFull)
    }

    pub fn free(&mut self, section_id: u32) {
        if let Some(mut section) = self.sections.free(section_id as usize) {
            for view in section.views.drain(..) {
                unmap_view_pages(view.base, view.size);
            }
        }
    }

    pub fn map_view(&mut self, section_id: u32) -> Option<u64> {
        let size = self.sections.get(section_id as usize)?.size;

        let aligned_size = (size + 0xFFF) & !0xFFF;
        let base = allocate_mmap_region(aligned_size)?;

        if !map_section_view(base, aligned_size, true) {
            return None;
        }

        if let Some(s) = self.sections.get_mut(section_id as usize) {
            s.views.push(SectionView { base, size: aligned_size });
        }

        Some(base)
    }

    pub fn unmap_view(&mut self, section_id: u32, base: u64) -> bool {
        if let Some(s) = self.sections.get_mut(section_id as usize) {
            if let Some(pos) = s.views.iter().position(|v| v.base == base) {
                let view = s.views.remove(pos);
                unmap_view_pages(view.base, view.size);
                return true;
            }
        }
        false
    }

    pub fn size(&self, section_id: u32) -> u64 {
        self.sections.get(section_id as usize).map_or(0, |s| s.size)
    }

    pub fn prot(&self, section_id: u32) -> u32 {
        self.sections.get(section_id as usize).map_or(0, |s| s.prot)
    }
}

fn unmap_view_pages(base: u64, size: u64) {
    let mut addr = base;
    while addr < base + size {
        crate::arch::x64::paging::mmap_free_page(addr);
        addr += 0x1000;
    }
}

fn allocate_mmap_region(size: u64) -> Option<u64> {
    let mmap_base = crate::arch::x64::paging::MMAP_BASE;
    let mmap_end = mmap_base + crate::arch::x64::paging::MMAP_TOTAL_SIZE;
    let section_start = mmap_base + 0x100_0000;

    static SECTION_NEXT: AtomicU64 = AtomicU64::new(0);

    let base = if SECTION_NEXT.load(Ordering::Relaxed) == 0 {
        SECTION_NEXT.store(section_start, Ordering::Relaxed);
        section_start
    } else {
        SECTION_NEXT.load(Ordering::Relaxed)
    };

    if base + size > mmap_end {
        return None;
    }

    SECTION_NEXT.store(base + size, Ordering::Relaxed);
    Some(base)
}

fn map_section_view(base: u64, size: u64, _writable: bool) -> bool {
    let mut addr = base;
    while addr < base + size {
        // Split 2MB page if needed (4K PTE doesn't exist yet)
        if crate::hal::walk_ptes_4k(addr).is_none()
            && crate::arch::x64::paging::split_2mb_page(addr).is_err() {
            unmap_view_pages(base, addr - base);
            return false;
        }
        if crate::arch::x64::paging::mmap_alloc_page(addr).is_none() {
            unmap_view_pages(base, addr - base);
            return false;
        }
        addr += 0x1000;
    }
    true
}

static SECTION_MANAGER: Mutex<SectionManager> = Mutex::new(SectionManager::new());

pub struct SectionObOps;

impl ObOperations for SectionObOps {
    fn on_destroy(&self, _id: ObId, native_id: u64) {
        SECTION_MANAGER.lock().free(native_id as u32);
    }
}

pub static SECTION_OPS: SectionObOps = SectionObOps;

pub fn alloc_section(ob_id: ObId, size: u64, prot: u32) -> Result<u32, ObError> {
    SECTION_MANAGER.lock().alloc(ob_id, size, prot)
}

pub fn map_view(section_id: u32) -> Option<u64> {
    SECTION_MANAGER.lock().map_view(section_id)
}

pub fn unmap_view(section_id: u32, base: u64) -> bool {
    SECTION_MANAGER.lock().unmap_view(section_id, base)
}

pub fn register_section_tests() {
    use crate::{test_case, test_eq, test_true};

    test_case!("section_alloc_free", {
        let mut mgr = SectionManager::new();
        let id = mgr.alloc(42, 4096, 3).unwrap();
        test_eq!(id, 0);
        test_eq!(mgr.size(id), 4096);
        test_eq!(mgr.prot(id), 3);
        mgr.free(id);
    });

    test_case!("section_table_full", {
        let mut mgr = SectionManager::new();
        for i in 0..MAX_SECTIONS {
            test_true!(mgr.alloc(i as u64, 4096, 3).is_ok());
        }
        test_eq!(mgr.alloc(999, 4096, 3), Err(ObError::TableFull));
    });

    test_case!("section_invalid_size", {
        let mut mgr = SectionManager::new();
        test_true!(mgr.alloc(42, 0, 3).is_err());
        test_true!(mgr.alloc(42, 0x200000, 3).is_err());
    });

    test_case!("section_uses_mmap_page", {
        let mut mgr = SectionManager::new();
        let id = mgr.alloc(42, 4096, 3).unwrap();
        test_true!(mgr.map_view(id).is_some());
        mgr.free(id);
    });

    test_case!("section_double_free_safe", {
        let mut mgr = SectionManager::new();
        let id = mgr.alloc(42, 4096, 3).unwrap();
        mgr.free(id);
        mgr.free(id);
    });

    test_case!("section_multiple_allocs", {
        let mut mgr = SectionManager::new();
        let id1 = mgr.alloc(42, 4096, 3).unwrap();
        let id2 = mgr.alloc(43, 8192, 1).unwrap();
        test_eq!(mgr.size(id1), 4096);
        test_eq!(mgr.size(id2), 8192);
        test_eq!(mgr.prot(id1), 3);
        test_eq!(mgr.prot(id2), 1);
        mgr.free(id1);
        mgr.free(id2);
    });

    test_case!("section_reuse_slot", {
        let mut mgr = SectionManager::new();
        let id1 = mgr.alloc(42, 4096, 3).unwrap();
        mgr.free(id1);
        let id2 = mgr.alloc(43, 4096, 1).unwrap();
        test_eq!(id2, id1);
    });
}
