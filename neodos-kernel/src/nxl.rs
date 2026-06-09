use crate::arch::x64::paging;
use crate::serial_println;
use crate::globals;
use crate::fs::vfs::{VfsNode, MODE_DIR, MODE_FILE};

const NXL_REGION_BASE: u64 = 0x1e00_0000;
const NXL_REGION_SIZE: u64 = 0x20_0000;
const NXL_SLOT_SIZE: u64 = 0x4_0000;
const NXL_SLOT_COUNT: usize = 8;
const NXL_MAX_SIZE: usize = 64 * 1024;

#[derive(Clone, Copy)]
struct NxlSlot {
    loaded: bool,
    base: u64,
    size: usize,
    name: [u8; 24],
}

static mut NXL_REGISTRY: [NxlSlot; NXL_SLOT_COUNT] = [
    NxlSlot { loaded: false, base: 0x1e00_0000, size: 0, name: [0u8; 24] },
    NxlSlot { loaded: false, base: 0x1e04_0000, size: 0, name: [0u8; 24] },
    NxlSlot { loaded: false, base: 0x1e08_0000, size: 0, name: [0u8; 24] },
    NxlSlot { loaded: false, base: 0x1e0c_0000, size: 0, name: [0u8; 24] },
    NxlSlot { loaded: false, base: 0x1e10_0000, size: 0, name: [0u8; 24] },
    NxlSlot { loaded: false, base: 0x1e14_0000, size: 0, name: [0u8; 24] },
    NxlSlot { loaded: false, base: 0x1e18_0000, size: 0, name: [0u8; 24] },
    NxlSlot { loaded: false, base: 0x1e1c_0000, size: 0, name: [0u8; 24] },
];

pub fn init_nxl_region() -> bool {
    serial_println!("[NXL] Initializing shared library region 0x{:x}..0x{:x}",
        NXL_REGION_BASE, NXL_REGION_BASE + NXL_REGION_SIZE);

    if paging::split_2mb_page(NXL_REGION_BASE).is_err() {
        serial_println!("[NXL] FAILED to split 2MB page");
        return false;
    }

    if paging::set_pd_user_accessible(NXL_REGION_BASE, true).is_err() {
        serial_println!("[NXL] FAILED to set PD USER_ACCESSIBLE");
        return false;
    }

    serial_println!("[NXL] Region ready: {} x {} KB slots",
        NXL_SLOT_COUNT, NXL_SLOT_SIZE / 1024);
    true
}

pub fn load_nxl() -> bool {
    match nxl_load("C:\\SYSTEM\\LIB\\libneodos.nxl") {
        Some(base) => {
            serial_println!("[NXL] libneodos NXL loaded at 0x{:x}", base);
            true
        }
        None => {
            serial_println!("[NXL] WARNING: libneodos.nxl not found");
            false
        }
    }
}

static mut NXL_IMAGE_BUF: [u8; NXL_MAX_SIZE] = [0u8; NXL_MAX_SIZE];

pub fn nxl_load(path: &str) -> Option<u64> {
    let slot_idx = find_free_slot()?;
    let base = unsafe { NXL_REGISTRY[slot_idx].base };

    serial_println!("[NXL] Loading '{}' @ slot {} => 0x{:x}", path, slot_idx, base);

    let buf: &mut [u8] = unsafe { &mut NXL_IMAGE_BUF };
    buf.fill(0);

    let image_size = {
        let mut size = 0usize;
        let result = globals::with_vfs(|vfs| {
            let resolved = match vfs.resolve_path(path) {
                Ok(result) => Some(result),
                Err(e) => {
                    serial_println!("[NXL] resolve '{}' failed: {:?}", path, e);
                    None
                }
            }.or_else(|| resolve_nxl_fallback(vfs, path));

            match resolved {
                Some((drive_idx, node)) => {
                    match vfs.read(drive_idx, node.inode, 0, buf) {
                        Ok(n) => { size = n; Ok(()) }
                        Err(e) => {
                            serial_println!("[NXL] read error: {:?}", e);
                            Err(())
                        }
                    }
                }
                None => Err(()),
            }
        });
        if result.is_err() || size == 0 { return None; }
        size
    };

    let data = unsafe { &*core::ptr::slice_from_raw_parts(NXL_IMAGE_BUF.as_ptr(), image_size) };

    let result = crate::elf::load_elf(data)?;
    serial_println!("[NXL] ELF entry=0x{:x}", result.entry);

    mark_slot_user_accessible(base, image_size);

    unsafe {
        NXL_REGISTRY[slot_idx] = NxlSlot {
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

    serial_println!("[NXL] '{}' => 0x{:x} ({} bytes)", path, base, image_size);
    Some(base)
}

fn resolve_nxl_fallback(vfs: &mut crate::fs::vfs::Vfs, path: &str) -> Option<(usize, VfsNode)> {
    let file_name = path
        .rsplit(|c| c == '\\' || c == '/')
        .next()
        .unwrap_or(path);

    for drive_idx in 0..vfs.drives.len() {
        if vfs.drives[drive_idx].is_none() {
            continue;
        }

        if let Some(found) = search_directory(vfs, drive_idx, 0, file_name, 0) {
            return Some(found);
        }
    }

    None
}

fn search_directory(
    vfs: &mut crate::fs::vfs::Vfs,
    drive_idx: usize,
    inode: u32,
    file_name: &str,
    depth: usize,
) -> Option<(usize, VfsNode)> {
    if depth > 16 {
        return None;
    }

    let mut index = 0usize;
    loop {
        match vfs.readdir(drive_idx, inode, index) {
            Ok(Some(entry)) => {
                if entry.name.eq_ignore_ascii_case(file_name) && (entry.node.mode & MODE_FILE) != 0 {
                    return Some((drive_idx, entry.node));
                }

                if (entry.node.mode & MODE_DIR) != 0 {
                    if let Some(found) = search_directory(vfs, drive_idx, entry.node.inode, file_name, depth + 1) {
                        return Some(found);
                    }
                }

                index += 1;
            }
            Ok(None) => break,
            Err(_) => {
                index += 1;
            }
        }
    }

    None
}

fn find_free_slot() -> Option<usize> {
    unsafe {
        NXL_REGISTRY.iter().position(|s| !s.loaded)
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

    serial_println!("[NXL] Marked 0x{:x}..0x{:x} USER_ACCESSIBLE",
        start, end_aligned);
}
