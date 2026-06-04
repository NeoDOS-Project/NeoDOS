use crate::arch::x64::paging;
use crate::serial_println;
use crate::globals;

const DLL_REGION_BASE: u64 = 0x1e00_0000;
const DLL_REGION_SIZE: u64 = 0x20_0000;
const DLL_SLOT_SIZE: u64 = 0x4_0000;
const DLL_SLOT_COUNT: usize = 8;
const DLL_MAX_SIZE: usize = 64 * 1024;

#[derive(Clone, Copy)]
struct DllSlot {
    loaded: bool,
    base: u64,
    size: usize,
    name: [u8; 24],
}

static mut DLL_REGISTRY: [DllSlot; DLL_SLOT_COUNT] = [
    DllSlot { loaded: false, base: 0x1e00_0000, size: 0, name: [0u8; 24] },
    DllSlot { loaded: false, base: 0x1e04_0000, size: 0, name: [0u8; 24] },
    DllSlot { loaded: false, base: 0x1e08_0000, size: 0, name: [0u8; 24] },
    DllSlot { loaded: false, base: 0x1e0c_0000, size: 0, name: [0u8; 24] },
    DllSlot { loaded: false, base: 0x1e10_0000, size: 0, name: [0u8; 24] },
    DllSlot { loaded: false, base: 0x1e14_0000, size: 0, name: [0u8; 24] },
    DllSlot { loaded: false, base: 0x1e18_0000, size: 0, name: [0u8; 24] },
    DllSlot { loaded: false, base: 0x1e1c_0000, size: 0, name: [0u8; 24] },
];

pub fn init_dll_region() -> bool {
    serial_println!("[DLL] Initializing shared library region 0x{:x}..0x{:x}",
        DLL_REGION_BASE, DLL_REGION_BASE + DLL_REGION_SIZE);

    if paging::split_2mb_page(DLL_REGION_BASE).is_err() {
        serial_println!("[DLL] FAILED to split 2MB page");
        return false;
    }

    if paging::set_pd_user_accessible(DLL_REGION_BASE, true).is_err() {
        serial_println!("[DLL] FAILED to set PD USER_ACCESSIBLE");
        return false;
    }

    serial_println!("[DLL] Region ready: {} x {} KB slots",
        DLL_SLOT_COUNT, DLL_SLOT_SIZE / 1024);
    true
}

pub fn load_dll() -> bool {
    match dll_load("C:\\SYSTEM\\LIB\\libneodos.dll") {
        Some(base) => {
            serial_println!("[DLL] libneodos DLL loaded at 0x{:x}", base);
            true
        }
        None => {
            serial_println!("[DLL] WARNING: libneodos.dll not found");
            false
        }
    }
}

static mut DLL_IMAGE_BUF: [u8; DLL_MAX_SIZE] = [0u8; DLL_MAX_SIZE];

pub fn dll_load(path: &str) -> Option<u64> {
    let slot_idx = find_free_slot()?;
    let base = unsafe { DLL_REGISTRY[slot_idx].base };

    serial_println!("[DLL] Loading '{}' @ slot {} => 0x{:x}", path, slot_idx, base);

    let buf: &mut [u8] = unsafe { &mut DLL_IMAGE_BUF };
    buf.fill(0);

    let image_size = {
        let mut size = 0usize;
        let result = globals::with_vfs(|vfs| {
            match vfs.resolve_path(path) {
                Ok((drive_idx, node)) => {
                    match vfs.read(drive_idx, node.inode, 0, buf) {
                        Ok(n) => { size = n; Ok(()) }
                        Err(e) => {
                            serial_println!("[DLL] read error: {:?}", e);
                            Err(())
                        }
                    }
                }
                Err(e) => {
                    serial_println!("[DLL] resolve '{}' failed: {:?}", path, e);
                    Err(())
                }
            }
        });
        if result.is_err() || size == 0 { return None; }
        size
    };

    let data = unsafe { &*core::ptr::slice_from_raw_parts(DLL_IMAGE_BUF.as_ptr(), image_size) };

    let result = crate::elf::load_elf(data)?;
    serial_println!("[DLL] ELF entry=0x{:x}", result.entry);

    mark_slot_user_accessible(base, image_size);

    unsafe {
        DLL_REGISTRY[slot_idx] = DllSlot {
            loaded: true,
            base,
            size: image_size,
            name: {
                let mut n = [0u8; 24];
                let b = path.as_bytes();
                let l = core::cmp::min(b.len(), 23);
                n[..l].copy_from_slice(&b[..l]);
                n
            },
        };
    }

    serial_println!("[DLL] '{}' => 0x{:x} ({} bytes)", path, base, image_size);
    Some(base)
}

fn find_free_slot() -> Option<usize> {
    unsafe {
        DLL_REGISTRY.iter().position(|s| !s.loaded)
    }
}

fn mark_slot_user_accessible(base: u64, image_size: usize) {
    let start = base & !(paging::PAGE_4K - 1);
    let end = base + image_size as u64 + paging::PAGE_4K - 1;
    let end_aligned = end & !(paging::PAGE_4K - 1);

    let mut addr = start;
    while addr < end_aligned {
        if let Some(entry) = crate::hal::walk_ptes_4k(addr) {
            use x86_64::structures::paging::PageTableFlags;
            let phys = entry.addr();
            let mut flags = entry.flags();
            flags.remove(PageTableFlags::WRITABLE);
            flags |= PageTableFlags::USER_ACCESSIBLE;
            entry.set_addr(phys, flags);
            crate::hal::flush_tlb(addr);
        }
        addr += paging::PAGE_4K;
    }

    serial_println!("[DLL] Marked 0x{:x}..0x{:x} USER_ACCESSIBLE",
        start, end_aligned);
}
