//! File-backed mmap page loader.
//!
//! Lives in the VFS layer (not `arch`) so the page-fault path can load a page
//! without `arch` depending on `globals`/VFS. `boot` registers it via
//! `paging::set_mmap_file_loader` after mounting the filesystems (#823).

use crate::log::LogSubsys;

/// Load one 4 KB page of a file-backed mapping.
///
/// Checks the global PageCache first; on a miss, reads through the VFS (which
/// populates the cache) and maps the new frame with the VMA's permissions.
#[allow(clippy::too_many_arguments)]
pub fn load_file_mmap_page(
    virt: u64,
    drive: u8,
    inode: u32,
    file_size: u32,
    base: u64,
    prot: u16,
) -> bool {
    let offset_in_file = (virt - base) as usize;
    let file_size = file_size as usize;
    let block_num = offset_in_file / 4096;

    let phys = crate::hal::alloc_page();
    if phys.is_null() {
        return false;
    }
    let phys_addr = phys as u64;
    unsafe {
        core::ptr::write_bytes(phys_addr as *mut u8, 0, 4096);
    }

    let bytes_to_read = core::cmp::min(4096, file_size.saturating_sub(offset_in_file));
    let mut from_cache = false;

    if bytes_to_read > 0 {
        let dest_slice =
            unsafe { core::slice::from_raw_parts_mut(phys_addr as *mut u8, bytes_to_read) };

        {
            let pc_lock = crate::globals::PAGE_CACHE.lock();
            if let Some(cached) = pc_lock.peek_inode(drive, inode, block_num as u32) {
                let to_copy = bytes_to_read.min(4096);
                dest_slice[..to_copy].copy_from_slice(&cached[..to_copy]);
                from_cache = true;
            }
        }

        if !from_cache {
            let result = crate::globals::with_vfs(|vfs| {
                vfs.read(drive as usize, inode, offset_in_file as u64, dest_slice)
            });
            if result.is_err() {
                crate::hal::free_page(phys);
                return false;
            }
        }
    }

    let rc = crate::hal::map_page(
        phys_addr,
        virt,
        crate::arch::x64::paging::mmap_pte_flags(prot & 2 != 0),
    );
    if rc != 0 {
        crate::hal::free_page(phys);
        return false;
    }

    kdebug!(
        LogSubsys::Memory,
        "Demand-load file 4K @ 0x{:x} (inode={}, offset={}, cache={})",
        virt,
        inode,
        offset_in_file,
        from_cache
    );
    true
}
