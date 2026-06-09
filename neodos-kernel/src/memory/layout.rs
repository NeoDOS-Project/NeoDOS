use spin::Mutex;
use lazy_static::lazy_static;

const MAX_REGIONS: usize = 32;
const REGION_NAME_LEN: usize = 24;

#[derive(Clone, Copy, Debug)]
pub struct MemoryRegion {
    pub base: u64,
    pub size: u64,
    pub name: [u8; REGION_NAME_LEN],
    pub flags: u32,
}

impl MemoryRegion {
    pub fn end(&self) -> u64 {
        self.base.saturating_add(self.size)
    }
}

pub struct MemoryLayout {
    regions: [Option<MemoryRegion>; MAX_REGIONS],
    count: usize,
}

impl MemoryLayout {
    pub const fn new() -> Self {
        MemoryLayout { regions: [None; MAX_REGIONS], count: 0 }
    }

    pub fn reserve_region(&mut self, base: u64, size: u64, name: &[u8], flags: u32) -> bool {
        if self.count >= MAX_REGIONS {
            return false;
        }
        if size == 0 {
            return false;
        }
        if let Some(existing) = self.find_overlap(base, size) {
            panic!("MemoryLayout overlap: '{:?}' at 0x{:x}+0x{:x} overlaps with existing at 0x{:x}+0x{:x}",
                   core::str::from_utf8(name).unwrap_or("?"),
                   base, size,
                   existing.base, existing.size);
        }
        let mut region = MemoryRegion {
            base,
            size,
            name: [0u8; REGION_NAME_LEN],
            flags,
        };
        let copy_len = name.len().min(REGION_NAME_LEN - 1);
        region.name[..copy_len].copy_from_slice(&name[..copy_len]);
        self.regions[self.count] = Some(region);
        self.count += 1;
        true
    }

    pub fn reserve_at(&mut self, base: u64, size: u64, name: &[u8], flags: u32) -> bool {
        self.reserve_region(base, size, name, flags)
    }

    pub fn find_region(&self, name: &[u8]) -> Option<&MemoryRegion> {
        for i in 0..self.count {
            if let Some(reg) = &self.regions[i] {
                let reg_name = &reg.name[..reg.name.iter().position(|&c| c == 0).unwrap_or(reg.name.len())];
                if reg_name == name {
                    return Some(reg);
                }
            }
        }
        None
    }

    fn find_overlap(&self, base: u64, size: u64) -> Option<MemoryRegion> {
        let end = base.saturating_add(size);
        for i in 0..self.count {
            if let Some(reg) = &self.regions[i] {
                let r_end = reg.base.saturating_add(reg.size);
                if base < r_end && end > reg.base {
                    return Some(*reg);
                }
            }
        }
        None
    }

    pub fn region_count(&self) -> usize {
        self.count
    }

    pub fn iter(&self) -> impl Iterator<Item = &MemoryRegion> {
        self.regions[..self.count].iter().filter_map(|r| r.as_ref())
    }
}

lazy_static! {
    static ref LAYOUT: Mutex<MemoryLayout> = Mutex::new(MemoryLayout::new());
}

pub fn layout() -> &'static Mutex<MemoryLayout> {
    &LAYOUT
}

pub fn init_default() {
    let mut l = LAYOUT.lock();
    l.reserve_region(0x0010_0000, 0x0010_0000, b"kernel_image\0", 0);
    l.reserve_region(0x0040_0000, 0x0040_0000, b"user_window\0", 0);
    l.reserve_region(0x0100_0000, 0x0100_0000, b"kernel_heap\0", 0);
    l.reserve_region(0x1000_0000, 0x0200_0000, b"user_heap\0", 0);
    l.reserve_region(0x1e00_0000, 0x0020_0000, b"nxl_region\0", 0);
    l.reserve_region(0x2000_0000, 0x0200_0000, b"mmap_region\0", 0);
    l.reserve_region(0x3000_0000, 0x0100_0000, b"driver_iso\0", 0);
}

pub fn reserve_region(base: u64, size: u64, name: &[u8]) -> bool {
    LAYOUT.lock().reserve_region(base, size, name, 0)
}
