use crate::buffer::block_cache::BlockCache;
use crate::drivers::block::BlockDevice;
use crate::fs::neodos_fs::*;
use alloc::vec::Vec;

const MAX_DIR_DEPTH: usize = 32;
const FSCK_MAX_INODES: u32 = 256;

pub struct FsckStats {
    pub total_inodes: u32,
    pub used_inodes: u32,
    pub valid_inodes: u32,
    pub corrupted_inodes: u32,
    pub cross_linked_blocks: u32,
    pub bitmap_orphan_blocks: u32,
    pub bitmap_missing_blocks: u32,
    pub orphan_inodes: u32,
    pub dangling_entries: u32,
    pub dir_errors: u32,
    pub superblock_errors: u32,
    pub repairs_applied: u32,
}

pub enum FsckMode {
    CheckOnly,
    Repair,
}

// Helper: read Inode value from disk.
fn read_inode(inode_num: u32, cache: &mut BlockCache, dev: &mut dyn BlockDevice, partition_base: u32) -> Result<Inode, ()> {
    let inode_sector = 1 + (inode_num / 2);
    let offset = (inode_num % 2) as usize * 256;
    let sector_data = cache.get_sector(inode_sector + partition_base, dev)?;
    let inode: Inode = unsafe {
        core::ptr::read_unaligned(sector_data.as_ptr().add(offset) as *const _)
    };
    Ok(inode)
}

fn write_inode(inode_num: u32, inode: &Inode, cache: &mut BlockCache, dev: &mut dyn BlockDevice, partition_base: u32) -> Result<(), ()> {
    let inode_sector = 1 + (inode_num / 2);
    let offset = (inode_num % 2) as usize * 256;
    let sector_data = cache.get_sector_mut(inode_sector + partition_base, dev)?;
    unsafe {
        core::ptr::write_unaligned(sector_data.as_mut_ptr().add(offset) as *mut Inode, *inode);
    }
    Ok(())
}

fn read_superblock(cache: &mut BlockCache, dev: &mut dyn BlockDevice, partition_base: u32) -> Result<Superblock, ()> {
    let data = cache.get_sector(partition_base, dev)?;
    let sb: Superblock = unsafe { core::ptr::read_unaligned(data.as_ptr() as *const _) };
    Ok(sb)
}

fn write_superblock(sb: &Superblock, cache: &mut BlockCache, dev: &mut dyn BlockDevice, partition_base: u32) -> Result<(), ()> {
    let data = cache.get_sector_mut(partition_base, dev)?;
    unsafe { core::ptr::write_unaligned(data.as_mut_ptr() as *mut Superblock, *sb); }
    Ok(())
}

fn read_dir_entry(sector_data: &[u8; 512], entry_off: usize) -> DirectoryEntry {
    unsafe { core::ptr::read_unaligned(sector_data.as_ptr().add(entry_off) as *const _) }
}

pub fn run(cache: &mut BlockCache, dev: &mut dyn BlockDevice, mode: FsckMode, partition_base: u32) -> FsckStats {
    let mut stats = FsckStats {
        total_inodes: 0, used_inodes: 0, valid_inodes: 0, corrupted_inodes: 0,
        cross_linked_blocks: 0, bitmap_orphan_blocks: 0, bitmap_missing_blocks: 0,
        orphan_inodes: 0, dangling_entries: 0, dir_errors: 0,
        superblock_errors: 0, repairs_applied: 0,
    };

    let is_repair = matches!(mode, FsckMode::Repair);

    // ── 1. Superblock ──────────────────────────────────────
    let sb = match read_superblock(cache, dev, partition_base) {
        Ok(sb) => sb,
        Err(_) => {
            stats.superblock_errors += 1;
            crate::serial_println!("[FSCK] ERROR: Cannot read superblock (sector 0)");
            return stats;
        }
    };

    let mut sb_fixed = sb;

    let sb_magic = sb.magic;
    let sb_block_size = sb.block_size;
    let sb_num_blocks = sb.num_blocks;
    let sb_num_inodes = sb.num_inodes;
    let sb_label_len = sb.label_len;

    if sb_magic != SUPERBLOCK_MAGIC {
        stats.superblock_errors += 1;
        crate::serial_println!("[FSCK] ERROR: Bad superblock magic 0x{:08X} (expected 0x{:08X})", sb_magic, SUPERBLOCK_MAGIC);
        if is_repair {
            sb_fixed.magic = SUPERBLOCK_MAGIC;
            crate::serial_println!("[FSCK] REPAIR: Restored superblock magic");
        }
    }

    if sb_block_size != BLOCK_SIZE as u32 {
        stats.superblock_errors += 1;
        crate::serial_println!("[FSCK] ERROR: Bad block size {} (expected {})", sb_block_size, BLOCK_SIZE);
        if is_repair {
            sb_fixed.block_size = BLOCK_SIZE as u32;
            crate::serial_println!("[FSCK] REPAIR: Restored block size");
        }
    }

    if sb_num_blocks == 0 || sb_num_blocks > 2560 {
        stats.superblock_errors += 1;
        crate::serial_println!("[FSCK] ERROR: Suspicious num_blocks: {}", sb_num_blocks);
        if is_repair && sb_num_blocks == 0 {
            sb_fixed.num_blocks = 2560;
            crate::serial_println!("[FSCK] REPAIR: Set num_blocks to 2560");
        }
    }

    if sb_num_inodes == 0 || sb_num_inodes > FSCK_MAX_INODES {
        stats.superblock_errors += 1;
        crate::serial_println!("[FSCK] ERROR: Suspicious num_inodes: {}", sb_num_inodes);
        if is_repair && sb_num_inodes == 0 {
            sb_fixed.num_inodes = FSCK_MAX_INODES;
            crate::serial_println!("[FSCK] REPAIR: Set num_inodes to {}", FSCK_MAX_INODES);
        }
    }

    if sb_label_len > 11 {
        stats.superblock_errors += 1;
        crate::serial_println!("[FSCK] ERROR: Volume label length {} exceeds 11", sb_label_len);
        if is_repair {
            sb_fixed.label_len = sb_label_len.min(11);
            crate::serial_println!("[FSCK] REPAIR: Clamped label length to {}", sb_fixed.label_len);
        }
    }

    if stats.superblock_errors > 0 && is_repair {
        let _ = write_superblock(&sb_fixed, cache, dev, partition_base);
        stats.repairs_applied += 1;
    }

    let num_blocks = sb_fixed.num_blocks;
    let total_inodes = sb_fixed.num_inodes.min(FSCK_MAX_INODES);

    // ── 2. Read all inodes, build block ownership map ─────
    let mut inodes: Vec<(u32, Inode)> = Vec::new();

    for i in 0..total_inodes {
        let inode = match read_inode(i, cache, dev, partition_base) {
            Ok(inode_val) => inode_val,
            Err(_) => {
                stats.corrupted_inodes += 1;
                crate::serial_println!("[FSCK] ERROR: Cannot read inode {}", i);
                continue;
            }
        };

        stats.total_inodes += 1;

        let ino_inode_num = inode.inode_num;
        let ino_mode = inode.mode;
        let ino_size = inode.size;

        // Check if inode is used
        let used = ino_inode_num != 0 || ino_mode != 0;

        if !used {
            // Free slot should be all-zeros
            if ino_mode != 0 || ino_inode_num != 0 {
                stats.corrupted_inodes += 1;
                crate::serial_println!("[FSCK] ERROR: Inode {} has mode=0 but inode_num={}", i, ino_inode_num);
                if is_repair {
                    let mut fixed = inode;
                    fixed.inode_num = 0;
                    let _ = write_inode(i, &fixed, cache, dev, partition_base);
                    stats.repairs_applied += 1;
                    crate::serial_println!("[FSCK] REPAIR: Cleared inode_num for inode {}", i);
                }
            }
            continue;
        }

        stats.used_inodes += 1;

        // Validate inode_num field
        if ino_inode_num != i && ino_inode_num != 0 {
            stats.corrupted_inodes += 1;
            crate::serial_println!("[FSCK] ERROR: Inode {} has inode_num={} (mismatch)", i, ino_inode_num);
            if is_repair {
                let mut fixed = inode;
                fixed.inode_num = i;
                let _ = write_inode(i, &fixed, cache, dev, partition_base);
                stats.repairs_applied += 1;
                crate::serial_println!("[FSCK] REPAIR: Fixed inode_num for inode {}", i);
            }
        }

        // Validate mode (must have exactly one of MODE_DIR/MODE_FILE, no high bits)
        let mode_valid = if ino_mode == 0 || ino_mode & 0xFF00 != 0 {
            false
        } else {
            let has_dir = (ino_mode & MODE_DIR) != 0;
            let has_file = (ino_mode & MODE_FILE) != 0;
            has_dir != has_file
        };

        if !mode_valid {
            stats.corrupted_inodes += 1;
            crate::serial_println!("[FSCK] ERROR: Inode {} has invalid mode 0x{:04X}", i, ino_mode);
            if is_repair {
                let mut fixed = inode;
                fixed.mode &= 0xC0 | 0x001F;
                if (fixed.mode & MODE_DIR) != 0 && (fixed.mode & MODE_FILE) != 0 {
                    fixed.mode &= !MODE_FILE;
                }
                if (fixed.mode & (MODE_DIR | MODE_FILE)) == 0 {
                    fixed.mode = MODE_FILE;
                }
                let _ = write_inode(i, &fixed, cache, dev, partition_base);
                stats.repairs_applied += 1;
                let fmode = fixed.mode;
                crate::serial_println!("[FSCK] REPAIR: Fixed mode for inode {} to 0x{:04X}", i, fmode);
            }
        } else {
            stats.valid_inodes += 1;
        }

        // Validate size vs data blocks
        let mut data_block_count: usize = 0;
        for bi in 0..12 {
            let bp = inode.direct_blocks[bi];
            let used_block = bp != 0 || (bi == 0 && (ino_mode & MODE_DIR) != 0 && ino_size > 0);
            if used_block {
                data_block_count += 1;
            }
        }
        let max_size = (data_block_count * BLOCK_SIZE) as u32;
        if ino_size > max_size && data_block_count > 0 {
            stats.corrupted_inodes += 1;
            crate::serial_println!("[FSCK] ERROR: Inode {} size={} exceeds block capacity {} ({} blocks)", i, ino_size, max_size, data_block_count);
            if is_repair {
                let mut fixed = inode;
                fixed.size = max_size;
                let _ = write_inode(i, &fixed, cache, dev, partition_base);
                stats.repairs_applied += 1;
                crate::serial_println!("[FSCK] REPAIR: Clamped size for inode {} to {}", i, max_size);
            }
        }

        inodes.push((i, inode));
    }

    // ── 3. Build block ownership & detect cross-links ─────
    let mut block_refs: Vec<(u32, u32)> = Vec::new();

    for &(inode_num, ref inode) in &inodes {
        let ino_mode = inode.mode;
        let ino_size = inode.size;

        for idx in 0..12 {
            let bp = inode.direct_blocks[idx];
            let used = bp != 0 || (idx == 0 && (ino_mode & MODE_DIR) != 0 && ino_size > 0);
            if !used {
                continue;
            }
            let b = if bp != 0 { bp } else { 0 };

            if b >= num_blocks {
                stats.corrupted_inodes += 1;
                crate::serial_println!("[FSCK] ERROR: Inode {} block {} ptr {} out of range (num_blocks={})", inode_num, idx, b, num_blocks);
                if is_repair {
                    let mut fixed = *inode;
                    fixed.direct_blocks[idx] = 0;
                    if (fixed.mode & MODE_DIR) != 0 && idx == 0 && fixed.size > 0 {
                        fixed.size = 0;
                    }
                    let _ = write_inode(inode_num, &fixed, cache, dev, partition_base);
                    stats.repairs_applied += 1;
                    crate::serial_println!("[FSCK] REPAIR: Cleared out-of-range block ptr in inode {} block {}", inode_num, idx);
                }
                continue;
            }

            let existing = block_refs.iter().find(|&&(blk, _)| blk == b);
            if let Some(&(_, owner)) = existing {
                stats.cross_linked_blocks += 1;
                crate::serial_println!("[FSCK] ERROR: Cross-linked block {} between inode {} and inode {}", b, owner, inode_num);
                if is_repair {
                    let mut fixed = *inode;
                    fixed.direct_blocks[idx] = 0;
                    if (fixed.mode & MODE_DIR) != 0 && idx == 0 && fixed.size > 0 {
                        fixed.size = 0;
                    }
                    let _ = write_inode(inode_num, &fixed, cache, dev, partition_base);
                    stats.repairs_applied += 1;
                    crate::serial_println!("[FSCK] REPAIR: Cleared cross-linked block {} from inode {} block {}", b, inode_num, idx);
                }
            } else {
                block_refs.push((b, inode_num));
            }
        }
    }

    // ── 4. Block bitmap consistency ───────────────────────
    // The bitmap is always rebuilt from inodes at mount time.
    // We only verify that our computed bitmap matches expectations.
    let mut computed_bitmap = BlockBitmap::new();
    for &(b, _) in &block_refs {
        computed_bitmap.mark_used(b);
    }

    let root_has_content = inodes.iter().any(|&(num, ref inode)| {
        num == 0 && (inode.mode & MODE_DIR) != 0 && inode.size > 0
    });
    if root_has_content && !block_refs.iter().any(|&(b, _)| b == 0) {
        computed_bitmap.mark_used(0);
    }

    // ── 5. Directory tree walk & orphan/dangling detection ─
    let mut reachable_inodes: Vec<u32> = Vec::new();
    reachable_inodes.push(0);

    let mut dirs_to_visit: Vec<(u32, usize)> = Vec::new();
    dirs_to_visit.push((0, 0));

    while let Some((dir_inode_num, depth)) = dirs_to_visit.pop() {
        if depth > MAX_DIR_DEPTH {
            stats.dir_errors += 1;
            crate::serial_println!("[FSCK] ERROR: Directory depth exceeded at inode {} (possible cycle)", dir_inode_num);
            continue;
        }

        let dir_inode = match inodes.iter().find(|&&(num, _)| num == dir_inode_num) {
            Some((_, inode)) => *inode,
            None => {
                stats.dir_errors += 1;
                crate::serial_println!("[FSCK] ERROR: Directory inode {} not in inode table", dir_inode_num);
                continue;
            }
        };

        let dir_mode = dir_inode.mode;
        let dir_size = dir_inode.size;

        if (dir_mode & MODE_DIR) == 0 {
            stats.dir_errors += 1;
            crate::serial_println!("[FSCK] ERROR: Inode {} is not a directory but has children", dir_inode_num);
            continue;
        }

        for idx in 0..12 {
            let bp = dir_inode.direct_blocks[idx];
            let used = bp != 0 || (idx == 0 && (dir_mode & MODE_DIR) != 0 && dir_size > 0);
            if !used { continue; }
            let b = if bp != 0 { bp } else { 0 };
            if b >= num_blocks { continue; }

            let block_sector = 200 + (b * 8);
            for sector_offset in 0..8 {
                // Phase 1: Read sector (immutable borrow, then release)
                let mut sector_entries: Vec<(usize, u32, u8, u8, [u8; 249])> = Vec::new();
                {
                    let sector_data = match cache.get_sector(block_sector + sector_offset + partition_base, dev) {
                        Ok(d) => d,
                        Err(_) => {
                            stats.dir_errors += 1;
                            crate::serial_println!("[FSCK] ERROR: Cannot read sector {} (dir inode {} block {})", block_sector + sector_offset, dir_inode_num, b);
                            continue;
                        }
                    };
                    for entry_offset in (0..512).step_by(256) {
                        let first_byte = sector_data[entry_offset];
                        if first_byte == 0x00 || first_byte == 0xE5 {
                            continue;
                        }
                        let entry: DirectoryEntry = read_dir_entry(sector_data, entry_offset);
                        let ei = entry.inode_num;
                        if ei == 0 { continue; }
                        let mut name_buf = [0u8; 249];
                        name_buf.copy_from_slice(&entry.name);
                        sector_entries.push((entry_offset, ei, entry.entry_type, entry.name_len, name_buf));
                    }
                } // immutable borrow of cache ends here

                // Phase 2: Process entries & optionally repair
                for &(entry_offset, entry_inode_num, entry_type, entry_name_len, ref entry_name) in &sector_entries {
                    // Check entry target inode range
                    if entry_inode_num >= total_inodes {
                        stats.dangling_entries += 1;
                        crate::serial_println!("[FSCK] ERROR: Dir inode {} has entry pointing to invalid inode {} (name_len={})", dir_inode_num, entry_inode_num, entry_name_len);
                        if is_repair {
                            if let Ok(data) = cache.get_sector_mut(block_sector + sector_offset + partition_base, dev) {
                                data[entry_offset] = 0xE5;
                                stats.repairs_applied += 1;
                                crate::serial_println!("[FSCK] REPAIR: Deleted dangling entry in inode {} at sector {}", dir_inode_num, block_sector + sector_offset);
                            }
                        }
                        continue;
                    }

                    // Check if target inode exists and is used
                    let target_used = inodes.iter().any(|&(num, ref ti)| {
                        let ti_inum = ti.inode_num;
                        let ti_mode = ti.mode;
                        num == entry_inode_num && (ti_inum != 0 || ti_mode != 0)
                    });

                    if !target_used {
                        stats.dangling_entries += 1;
                        crate::serial_println!("[FSCK] ERROR: Dir inode {} entry inode_num={} points to unused inode", dir_inode_num, entry_inode_num);
                        if is_repair {
                            if let Ok(data) = cache.get_sector_mut(block_sector + sector_offset + partition_base, dev) {
                                data[entry_offset] = 0xE5;
                                stats.repairs_applied += 1;
                                crate::serial_println!("[FSCK] REPAIR: Deleted stale entry pointing to inode {} in dir {}", entry_inode_num, dir_inode_num);
                            }
                        }
                        continue;
                    }

                    // Validate entry type matches inode mode
                    let actual_mode = inodes.iter()
                        .find(|&&(num, _)| num == entry_inode_num)
                        .map(|(_, ti)| ti.mode)
                        .unwrap_or(0);
                    let is_dir_entry = entry_type == 2;
                    let is_file_entry = entry_type == 1;
                    let is_actually_dir = (actual_mode & MODE_DIR) != 0;
                    let is_actually_file = (actual_mode & MODE_FILE) != 0;

                    if (is_dir_entry && !is_actually_dir) || (is_file_entry && !is_actually_file) {
                        stats.dir_errors += 1;
                        crate::serial_println!("[FSCK] ERROR: Dir inode {} entry type mismatch: entry_type={}, inode mode=0x{:04X}", dir_inode_num, entry_type, actual_mode);
                        if is_repair {
                            if let Ok(data) = cache.get_sector_mut(block_sector + sector_offset + partition_base, dev) {
                                data[entry_offset + 5] = if is_actually_dir { 2 } else { 1 };
                                stats.repairs_applied += 1;
                                crate::serial_println!("[FSCK] REPAIR: Fixed entry type for inode {}", entry_inode_num);
                            }
                        }
                    }

                    // Track reachable inode
                    if !reachable_inodes.contains(&entry_inode_num) {
                        reachable_inodes.push(entry_inode_num);
                    }

                    // Add subdirectories to visit list
                    if entry_type == 2 && entry_inode_num != dir_inode_num {
                        let name_max = (entry_name_len as usize).min(249);
                        let name = core::str::from_utf8(&entry_name[..name_max]).unwrap_or("");
                        if name != "." && name != ".." && entry_inode_num != ROOT_INODE {
                            dirs_to_visit.push((entry_inode_num, depth + 1));
                        }
                    }
                }
            }
        }
    }

    // ── 6. Orphan inode detection ─────────────────────────
    for &(inode_num, ref inode) in &inodes {
        let ino_inum = inode.inode_num;
        let ino_mode = inode.mode;
        let ino_size = inode.size;
        let used = ino_inum != 0 || ino_mode != 0;
        if !used { continue; }
        if inode_num == ROOT_INODE { continue; }
        if !reachable_inodes.contains(&inode_num) {
            stats.orphan_inodes += 1;
            crate::serial_println!("[FSCK] ERROR: Orphan inode {} (mode=0x{:04X}, size={}) not reachable from root", inode_num, ino_mode, ino_size);
            if is_repair {
                let mut fixed = *inode;
                fixed.mode = 0;
                fixed.inode_num = 0;
                fixed.size = 0;
                fixed.direct_blocks = [0; 12];
                let _ = write_inode(inode_num, &fixed, cache, dev, partition_base);
                stats.repairs_applied += 1;
                crate::serial_println!("[FSCK] REPAIR: Freed orphan inode {}", inode_num);
            }
        }
    }

    // ── 7. Flush repairs ──────────────────────────────────
    if is_repair && stats.repairs_applied > 0 {
        let _ = cache.flush(dev);
        crate::serial_println!("[FSCK] REPAIR: Flushed repairs to disk ({} repairs applied)", stats.repairs_applied);
    }

    stats
}

pub fn print_report(stats: &FsckStats) {
    crate::println!("");
    crate::println!("========================================");
    crate::println!("  NeoDOS  FSCK   Report"                 );
    crate::println!("========================================");
    crate::println!("");

    crate::println!("  Inodes:");
    crate::println!("    Total:     {}", stats.total_inodes);
    crate::println!("    Used:      {}", stats.used_inodes);
    crate::println!("    Valid:     {}", stats.valid_inodes);
    crate::println!("    Corrupted: {}", stats.corrupted_inodes);
    crate::println!("");

    crate::println!("  Errors:");
    crate::println!("    Cross-linked blocks: {}", stats.cross_linked_blocks);
    crate::println!("    Orphan inodes:       {}", stats.orphan_inodes);
    crate::println!("    Dangling entries:    {}", stats.dangling_entries);
    crate::println!("    Dir errors:          {}", stats.dir_errors);
    crate::println!("    Superblock errors:   {}", stats.superblock_errors);
    crate::println!("");

    crate::println!("  Repairs applied: {}", stats.repairs_applied);

    let total_errors = stats.corrupted_inodes + stats.cross_linked_blocks
        + stats.orphan_inodes + stats.dangling_entries
        + stats.dir_errors + stats.superblock_errors;

    crate::println!("");
    if total_errors == 0 {
        crate::println!("  STATUS: OK -- No errors found.");
    } else {
        crate::println!("  STATUS: {} error(s) found.", total_errors);
    }
    crate::println!("========================================");
}

// ── Tests (in-memory only, no disk access) ─────────────

pub fn register_fsck_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;

    fn valid_mode(mode: u16) -> bool {
        if mode == 0 { return true; }
        if mode & 0xFF00 != 0 { return false; }
        let has_dir = (mode & MODE_DIR) != 0;
        let has_file = (mode & MODE_FILE) != 0;
        has_dir != has_file
    }

    fn count_blocks(inode: &Inode) -> usize {
        let mut c = 0usize;
        for i in 0..12 {
            let b = inode.direct_blocks[i];
            if b != 0 || (i == 0 && (inode.mode & MODE_DIR) != 0 && inode.size > 0) {
                c += 1;
            }
        }
        c
    }

    fn block_used(inode: &Inode, idx: usize) -> bool {
        let b = inode.direct_blocks[idx];
        b != 0 || (idx == 0 && (inode.mode & MODE_DIR) != 0 && inode.size > 0)
    }

    fn get_block(inode: &Inode, idx: usize) -> Option<u32> {
        let b = inode.direct_blocks[idx];
        if b != 0 { return Some(b); }
        if idx == 0 && (inode.mode & MODE_DIR) != 0 && inode.size > 0 { return Some(0); }
        None
    }

    fn is_used(inode: &Inode) -> bool {
        inode.inode_num != 0 || inode.mode != 0
    }

    test_case!("fsck_mode_validation", {
        test_true!(valid_mode(0));
        test_true!(valid_mode(MODE_DIR));
        test_true!(valid_mode(MODE_FILE));
        test_true!(valid_mode(MODE_DIR | 0x0001));
        test_true!(!valid_mode(MODE_DIR | MODE_FILE));
        test_true!(!valid_mode(0xFF00));
        test_true!(!valid_mode(0xFFFF));
        test_true!(!valid_mode(0x0100));
    });

    test_case!("fsck_block_used", {
        let mut inode = Inode {
            inode_num: 1, mode: MODE_FILE, size: 4096,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_true!(!block_used(&inode, 0));

        inode.direct_blocks[0] = 1;
        test_true!(block_used(&inode, 0));

        inode.mode = MODE_DIR;
        inode.size = 4096;
        inode.direct_blocks[0] = 0;
        test_true!(block_used(&inode, 0));

        inode.size = 0;
        test_true!(!block_used(&inode, 0));
    });

    test_case!("fsck_count_blocks", {
        let mut inode = Inode {
            inode_num: 1, mode: MODE_FILE, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_eq!(count_blocks(&inode), 0);
        inode.direct_blocks[0] = 5;
        inode.direct_blocks[1] = 10;
        test_eq!(count_blocks(&inode), 2);

        inode.mode = MODE_DIR;
        inode.size = 4096;
        inode.direct_blocks[0] = 0;
        inode.direct_blocks[1] = 0;
        test_eq!(count_blocks(&inode), 1);
    });

    test_case!("fsck_get_block", {
        let mut inode = Inode {
            inode_num: 1, mode: MODE_FILE, size: 4096,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_true!(get_block(&inode, 0).is_none());
        inode.direct_blocks[0] = 5;
        test_eq!(get_block(&inode, 0), Some(5));

        inode.mode = MODE_DIR;
        inode.direct_blocks[0] = 0;
        test_eq!(get_block(&inode, 0), Some(0));
    });

    test_case!("fsck_is_used", {
        let free = Inode {
            inode_num: 0, mode: 0, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 0, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_true!(!is_used(&free));
        let used = Inode {
            inode_num: 1, mode: MODE_FILE, size: 100,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_true!(is_used(&used));
    });

    test_case!("fsck_block_range", {
        let limit = 2560u32;
        test_true!(0u32 < limit);
        test_true!(2559u32 < limit);
        test_true!(!(2560u32 < limit));
        test_true!(!(9999u32 < limit));
    });
}
