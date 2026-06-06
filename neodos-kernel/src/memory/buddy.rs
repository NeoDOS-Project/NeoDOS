use spin::Mutex;
use lazy_static::lazy_static;
use core::cmp;

const PAGE_SIZE: u64 = 4096;
pub const MAX_ORDER: usize = 10;
pub const BITMAP_WORDS: usize = 16384;
pub const MAX_FREE_SLOTS: usize = 512;

pub struct BuddyAllocator {
    free_lists: [u64; MAX_ORDER + 1],
    free_counts: [u16; MAX_ORDER + 1],
    free_slots: [[u64; MAX_FREE_SLOTS]; MAX_ORDER + 1],
    bitmap: [u64; BITMAP_WORDS],
    free_pages: u64,
    total_pages: u64,
}

unsafe impl Send for BuddyAllocator {}
unsafe impl Sync for BuddyAllocator {}

impl BuddyAllocator {
    const fn new() -> Self {
        BuddyAllocator {
            free_lists: [0; 11],
            free_counts: [0; 11],
            free_slots: [[0; MAX_FREE_SLOTS]; 11],
            bitmap: [u64::MAX; BITMAP_WORDS],
            free_pages: 0,
            total_pages: 0,
        }
    }

    fn bitmap_set(&mut self, frame: usize) {
        if frame < BITMAP_WORDS * 64 {
            self.bitmap[frame / 64] |= 1u64 << (frame % 64);
        }
    }

    fn bitmap_clear(&mut self, frame: usize) {
        if frame < BITMAP_WORDS * 64 {
            self.bitmap[frame / 64] &= !(1u64 << (frame % 64));
        }
    }

    fn bitmap_test(&self, frame: usize) -> bool {
        if frame < BITMAP_WORDS * 64 {
            (self.bitmap[frame / 64] & (1u64 << (frame % 64))) != 0
        } else {
            true
        }
    }

    fn page_to_frame(page: u64) -> usize {
        (page / PAGE_SIZE) as usize
    }

    fn frame_to_page(frame: usize) -> u64 {
        (frame as u64) * PAGE_SIZE
    }

    fn buddy_of(frame: usize, order: usize) -> usize {
        frame ^ (1usize << order)
    }

    fn push_slot(&mut self, order: usize, addr: u64) {
        let count = self.free_counts[order] as usize;
        if count < MAX_FREE_SLOTS {
            self.free_slots[order][count] = addr;
            self.free_counts[order] = (count + 1) as u16;
        }
    }

    fn pop_slot(&mut self, order: usize) -> Option<u64> {
        let count = self.free_counts[order] as usize;
        if count == 0 {
            return None;
        }
        let new_count = count - 1;
        self.free_counts[order] = new_count as u16;
        Some(self.free_slots[order][new_count])
    }

    fn scan_for_buddy(&self, order: usize, buddy_addr: u64) -> Option<usize> {
        let count = self.free_counts[order] as usize;
        for i in 0..count {
            if self.free_slots[order][i] == buddy_addr {
                return Some(i);
            }
        }
        None
    }

    fn remove_slot_at(&mut self, order: usize, idx: usize) {
        let count = self.free_counts[order] as usize;
        if idx < count {
            let last = count - 1;
            self.free_slots[order][idx] = self.free_slots[order][last];
            self.free_counts[order] = last as u16;
        }
    }

    pub fn init_from_regions(&mut self, regions: &[(u64, u64)], phys_max: u64) {
        let max_frames = (phys_max.max(1) + PAGE_SIZE - 1) / PAGE_SIZE;
        self.total_pages = max_frames;
        let limit = (max_frames as usize).min(BITMAP_WORDS * 64);

        for &(start, end) in regions {
            let first = (start / PAGE_SIZE) as usize;
            let last = ((end + PAGE_SIZE - 1) / PAGE_SIZE) as usize;
            for frame in first..last.min(limit) {
                self.bitmap_clear(frame);
            }
        }

        let mut run_start: Option<usize> = None;
        for frame in 0..=limit {
            let is_free = if frame < limit { !self.bitmap_test(frame) } else { false };
            if is_free && run_start.is_none() {
                run_start = Some(frame);
            } else if !is_free {
                if let Some(s) = run_start {
                    let count = frame - s;
                    if count > 0 {
                        self.free_pages += count as u64;
                        self.free_add_run(s, count);
                    }
                    run_start = None;
                }
            }
        }
    }

    fn free_add_run(&mut self, start_frame: usize, count: usize) {
        let mut addr = start_frame;
        let mut remaining = count;
        while remaining > 0 {
            let mut order = MAX_ORDER;
            while order > 0 && (addr & ((1usize << order) - 1)) != 0 {
                order -= 1;
            }
            while order > 0 && (1usize << order) > remaining {
                order -= 1;
            }
            if (1usize << order) > remaining {
                order = 0;
                while order < MAX_ORDER && (1usize << (order + 1)) <= remaining
                    && (addr & ((1usize << (order + 1)) - 1)) == 0 {
                    order += 1;
                }
            }
            let block_pages = 1usize << order;
            let phys = Self::frame_to_page(addr);
            self.push_slot(order, phys);
            remaining -= block_pages;
            addr += block_pages;
        }
    }

    pub fn alloc_frames(&mut self, order: usize) -> Option<u64> {
        let order = order.min(MAX_ORDER);
        for o in order..=MAX_ORDER {
            if self.free_counts[o] > 0 {
                let addr = self.pop_slot(o)?;
                let frame = Self::page_to_frame(addr);
                self.bitmap_set(frame);

                let allocated_pages = 1usize << o;
                let needed_pages = 1usize << order;
                if allocated_pages > needed_pages {
                    let remaining_start = frame + needed_pages;
                    let remaining_count = allocated_pages - needed_pages;
                    self.free_add_run(remaining_start, remaining_count);
                }

                self.free_pages -= needed_pages as u64;
                return Some(addr);
            }
        }
        None
    }

    pub fn free_frames(&mut self, addr: u64, order: usize) {
        let order = order.min(MAX_ORDER);
        let needed_pages = 1usize << order;
        let mut frame = Self::page_to_frame(addr);
        let mut cur_order = order;

        self.bitmap_set(frame);

        while cur_order < MAX_ORDER {
            let buddy = Self::buddy_of(frame, cur_order);
            let buddy_addr = Self::frame_to_page(buddy);

            if let Some(idx) = self.scan_for_buddy(cur_order, buddy_addr) {
                self.remove_slot_at(cur_order, idx);
                self.bitmap_set(buddy);
                frame = cmp::min(frame, buddy);
                cur_order += 1;
            } else {
                break;
            }
        }

        let merged_addr = Self::frame_to_page(frame);
        self.push_slot(cur_order, merged_addr);
        self.free_pages += needed_pages as u64;
    }

    pub fn allocate_frame(&mut self) -> Option<u64> {
        self.alloc_frames(0)
    }

    pub fn free_frame(&mut self, addr: u64) {
        self.free_frames(addr, 0);
    }

    pub fn mark_used_region(&mut self, start: u64, size: u64) {
        let first = (start / PAGE_SIZE) as usize;
        let last = ((start + size + PAGE_SIZE - 1) / PAGE_SIZE) as usize;
        let limit = (self.total_pages as usize).min(BITMAP_WORDS * 64);
        for frame in first..last.min(limit) {
            if !self.bitmap_test(frame) {
                self.bitmap_set(frame);
                self.free_pages = self.free_pages.saturating_sub(1);
            }
        }
    }

    pub fn free_pages(&self) -> u64 {
        self.free_pages
    }
}

lazy_static! {
    pub static ref ALLOCATOR: Mutex<BuddyAllocator> = Mutex::new(BuddyAllocator::new());
}

pub fn init_from_regions(regions: &[(u64, u64)], phys_max: u64) {
    ALLOCATOR.lock().init_from_regions(regions, phys_max);
}

pub fn allocate_frame() -> Option<u64> {
    ALLOCATOR.lock().allocate_frame()
}

pub fn free_frame(addr: u64) {
    ALLOCATOR.lock().free_frame(addr);
}

pub fn alloc_frames(order: usize) -> Option<u64> {
    ALLOCATOR.lock().alloc_frames(order)
}

pub fn free_frames(addr: u64, order: usize) {
    ALLOCATOR.lock().free_frames(addr, order);
}

pub fn mark_used_region(start: u64, size: u64) {
    ALLOCATOR.lock().mark_used_region(start, size);
}

pub fn free_pages() -> u64 {
    ALLOCATOR.lock().free_pages()
}
