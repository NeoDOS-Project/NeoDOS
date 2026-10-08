//! FSCK for FAT32.
//!
//! Validates the BPB geometry, walks the FAT chains reachable from the root
//! directory, and reports corrupt links, cross-linked clusters and orphaned
//! (allocated but unreachable) clusters. Repair frees orphaned clusters, which
//! is the only class of damage that can be fixed without risking live data.

use alloc::vec::Vec;

use crate::fs::fat32::Fat32Driver;
use crate::vfs::io::IoStack;
use super::{FsckStats, FsckTrait};

/// Free FAT entry.
const FAT_FREE: u32 = 0x0000_0000;
/// Bad-cluster marker.
const FAT_BAD: u32 = 0x0FFF_FFF7;
/// First end-of-chain value.
const FAT_EOC_MIN: u32 = 0x0FFF_FFF8;

/// Upper bound on clusters we build per-cluster state for (8 MiB of state).
const MAX_TRACKED_CLUSTERS: u64 = 8_000_000;

impl FsckTrait for Fat32Driver {
    fn check(&self) -> FsckStats {
        fat32_fsck(self, false)
    }

    fn repair(&self) -> FsckStats {
        fat32_fsck(self, true)
    }
}

struct Analysis {
    stats: FsckStats,
    orphans: Vec<u32>,
}

fn fat32_fsck(fs: &Fat32Driver, repair: bool) -> FsckStats {
    let first = analyze(fs);
    if !repair || first.orphans.is_empty() {
        let mut stats = first.stats;
        stats.total_nodes = stats.total_dirs + stats.total_files;
        return stats;
    }

    let mut fixed = 0u32;
    for &cluster in &first.orphans {
        if fs.write_fat_entry(cluster, FAT_FREE).is_ok() {
            fixed += 1;
        }
    }

    let second = analyze(fs);
    let mut stats = second.stats;
    stats.repaired = fixed;
    stats.total_nodes = stats.total_dirs + stats.total_files;
    stats
}

fn analyze(fs: &Fat32Driver) -> Analysis {
    let mut stats = FsckStats::default();
    let mut orphans = Vec::new();
    let bs = &fs.boot_sector;

    // 1. Geometry sanity.
    let spc = bs.sectors_per_cluster as u64;
    if bs.bytes_per_sector != 512 || spc == 0 || !spc.is_power_of_two() {
        stats.errors += 1;
        return Analysis { stats, orphans };
    }
    if bs.reserved_sectors == 0 || bs.num_fats == 0 || bs.sectors_per_fat == 0 {
        stats.errors += 1;
        return Analysis { stats, orphans };
    }
    let data_start = bs.data_start() as u64;
    let total_sectors = bs.total_sectors_32 as u64;
    if total_sectors <= data_start {
        stats.errors += 1;
        return Analysis { stats, orphans };
    }
    let total_clusters = (total_sectors - data_start) / spc;
    if total_clusters == 0 {
        stats.errors += 1;
        return Analysis { stats, orphans };
    }
    let max_cluster = total_clusters + 2;
    stats.total_blocks = total_clusters;

    if total_clusters > MAX_TRACKED_CLUSTERS {
        stats.warnings += 1;
        return Analysis { stats, orphans };
    }

    // FAT must be large enough to address every cluster.
    let fat_capacity = bs.sectors_per_fat as u64 * (512 / 4);
    if max_cluster > fat_capacity {
        stats.errors += 1;
        return Analysis { stats, orphans };
    }

    let root = bs.root_cluster as u64;
    if root < 2 || root >= max_cluster {
        stats.errors += 1;
        return Analysis { stats, orphans };
    }

    // Reserved FAT entries (informational).
    match fs.read_fat_entry(0) {
        Ok(v) if v & 0x0FFF_FF00 == 0x0FFF_F800 => {}
        Ok(_) => stats.warnings += 1,
        Err(_) => stats.errors += 1,
    }

    // 2. Walk reachable chains. `state`: 0 = free/unseen, 1 = claimed.
    let mut state = alloc::vec![0u8; total_clusters as usize];
    let mut dir_queue: Vec<u32> = Vec::new();
    let mut file_starts: Vec<u32> = Vec::new();

    // Root directory first.
    let root_chain = claim_chain(fs, &mut state, max_cluster, bs.root_cluster, &mut stats);
    collect_dir_entries(fs, &root_chain, spc, data_start, &mut dir_queue, &mut file_starts, &mut stats);

    let mut guard = 0u64;
    while let Some(dir) = dir_queue.pop() {
        guard += 1;
        if guard > max_cluster {
            stats.errors += 1;
            break;
        }
        let chain = claim_chain(fs, &mut state, max_cluster, dir, &mut stats);
        collect_dir_entries(fs, &chain, spc, data_start, &mut dir_queue, &mut file_starts, &mut stats);
    }

    for start in file_starts {
        let _ = claim_chain(fs, &mut state, max_cluster, start, &mut stats);
    }

    // 3. Classify every cluster from the FAT.
    let mut free: u64 = 0;
    let mut c = 2u32;
    while (c as u64) < max_cluster {
        match fs.read_fat_entry(c) {
            Ok(v) => {
                if v == FAT_FREE {
                    free += 1;
                } else if state[(c - 2) as usize] == 0 {
                    orphans.push(c);
                }
            }
            Err(_) => {
                stats.errors += 1;
                break;
            }
        }
        c += 1;
    }

    stats.used_blocks = state.iter().filter(|&&s| s == 1).count() as u64;
    stats.free_blocks = free;
    stats.errors += orphans.len() as u32;

    Analysis { stats, orphans }
}

/// Follow and claim a cluster chain starting at `start`, returning the
/// ordered list of clusters. Detects cycles, cross-links and invalid links.
fn claim_chain(
    fs: &Fat32Driver,
    state: &mut [u8],
    max_cluster: u64,
    start: u32,
    stats: &mut FsckStats,
) -> Vec<u32> {
    let mut chain = Vec::new();
    let mut cluster = start;

    loop {
        if (cluster as u64) < 2 || (cluster as u64) >= max_cluster {
            stats.errors += 1;
            break;
        }
        let idx = (cluster - 2) as usize;
        if state[idx] == 1 {
            // Already owned by another chain: cross-link or cycle.
            stats.errors += 1;
            break;
        }
        state[idx] = 1;
        chain.push(cluster);

        if chain.len() as u64 > max_cluster {
            stats.errors += 1;
            break;
        }

        let next = match fs.read_fat_entry(cluster) {
            Ok(v) => v,
            Err(_) => {
                stats.errors += 1;
                break;
            }
        };
        if next == FAT_FREE {
            stats.errors += 1;
            break;
        }
        if next >= FAT_EOC_MIN {
            break;
        }
        if next == FAT_BAD {
            stats.errors += 1;
            break;
        }
        if (next as u64) < 2 || (next as u64) >= max_cluster {
            stats.errors += 1;
            break;
        }
        cluster = next;
    }

    chain
}

/// Parse the directory entries stored in `chain` and queue subdirectories and
/// file data chains for further traversal.
fn collect_dir_entries(
    fs: &Fat32Driver,
    chain: &[u32],
    spc: u64,
    data_start: u64,
    dir_queue: &mut Vec<u32>,
    file_starts: &mut Vec<u32>,
    stats: &mut FsckStats,
) {
    'outer: for &cluster in chain {
        let lba = data_start + (cluster as u64 - 2) * spc;
        for s in 0..spc {
            let sector = match fs.read_sector((lba + s) as u32) {
                Ok(x) => x,
                Err(_) => {
                    stats.errors += 1;
                    continue;
                }
            };
            for off in (0..512).step_by(32) {
                let array: &[u8; 32] = match sector[off..off + 32].try_into() {
                    Ok(a) => a,
                    Err(_) => continue,
                };
                if array[0] == 0x00 {
                    break 'outer;
                }
                // Skip long-filename and volume-label entries.
                if array[11] & 0x08 != 0 {
                    continue;
                }
                let entry = match Fat32Driver::parse_entry(array) {
                    Some(e) => e,
                    None => continue,
                };
                // Skip the "." and ".." self/parent links.
                if entry.name[0] == b'.' {
                    continue;
                }
                if entry.is_directory {
                    stats.total_dirs += 1;
                    dir_queue.push(entry.cluster);
                } else {
                    stats.total_files += 1;
                    if entry.cluster >= 2 {
                        file_starts.push(entry.cluster);
                    }
                }
            }
        }
    }
}

// ── FSCK tests ──────────────────────────────────────────────────────

/// Build a minimal single-FAT, 1-sector-per-cluster FAT32 volume:
/// cluster 2 = root dir holding HELLO.TXT, cluster 3 = its data.
fn build_fat32_image() -> alloc::vec::Vec<[u8; 512]> {
    let mut sectors = alloc::vec![[0u8; 512]; 40];

    let bs = &mut sectors[0];
    bs[11..13].copy_from_slice(&512u16.to_le_bytes()); // bytes/sector
    bs[13] = 1; // sectors/cluster
    bs[14..16].copy_from_slice(&1u16.to_le_bytes()); // reserved sectors
    bs[16] = 1; // num FATs
    bs[32..36].copy_from_slice(&40u32.to_le_bytes()); // total sectors
    bs[36..40].copy_from_slice(&1u32.to_le_bytes()); // sectors/FAT
    bs[44..48].copy_from_slice(&2u32.to_le_bytes()); // root cluster
    bs[71..82].copy_from_slice(b"TESTVOL    ");
    bs[82..90].copy_from_slice(b"FAT32   ");
    bs[510] = 0x55;
    bs[511] = 0xAA;

    set_fat(&mut sectors[1], 0, 0x0FFF_FFF8);
    set_fat(&mut sectors[1], 1, 0x0FFF_FFFF);
    set_fat(&mut sectors[1], 2, 0x0FFF_FFFF); // root dir EOC
    set_fat(&mut sectors[1], 3, 0x0FFF_FFFF); // HELLO.TXT data EOC

    let root = &mut sectors[2];
    root[0..11].copy_from_slice(b"HELLO   TXT");
    root[11] = 0x20; // archive
    root[26..28].copy_from_slice(&3u16.to_le_bytes()); // cluster low
    root[28..32].copy_from_slice(&100u32.to_le_bytes()); // size

    sectors
}

fn set_fat(sector: &mut [u8; 512], cluster: u32, value: u32) {
    let off = cluster as usize * 4;
    sector[off..off + 4].copy_from_slice(&value.to_le_bytes());
}

pub fn register_tests() {
    use crate::fs::fsck::FsckTrait;

    // Clean volume: one root dir + one file, everything reachable.
    crate::test_case!("fat32_fsck_clean", {
        let dev_id = crate::fs::fsck::register_test_device(build_fat32_image());
        let io = IoStack::new(dev_id);
        let fs = Fat32Driver::new(io).unwrap();

        let stats = fs.check();
        crate::test_eq!(stats.errors, 0);
        crate::test_eq!(stats.total_blocks, 38);
        crate::test_eq!(stats.used_blocks, 2);
        crate::test_eq!(stats.free_blocks, 36);
        crate::test_eq!(stats.total_dirs, 1);
        crate::test_eq!(stats.total_files, 1);
        crate::test_eq!(stats.total_nodes, 2);

        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });

    // Orphaned cluster: allocated in the FAT but unreachable. Repair frees it.
    crate::test_case!("fat32_fsck_orphan_repair", {
        let mut sectors = build_fat32_image();
        set_fat(&mut sectors[1], 5, 0x0FFF_FFFF);
        let dev_id = crate::fs::fsck::register_test_device(sectors);
        let io = IoStack::new(dev_id);
        let fs = Fat32Driver::new(io).unwrap();

        let checked = fs.check();
        crate::test_true!(checked.errors >= 1);
        crate::test_eq!(checked.free_blocks, 35);

        let repaired = fs.repair();
        crate::test_eq!(repaired.repaired, 1);
        crate::test_eq!(repaired.errors, 0);
        crate::test_eq!(repaired.free_blocks, 36);

        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });

    // Cross-link: two directory entries reference the same data cluster.
    crate::test_case!("fat32_fsck_crosslink_detect", {
        let mut sectors = build_fat32_image();
        let root = &mut sectors[2];
        root[32..43].copy_from_slice(b"WORLD   TXT");
        root[32 + 11] = 0x20;
        root[32 + 26..32 + 28].copy_from_slice(&3u16.to_le_bytes());
        root[32 + 28..32 + 32].copy_from_slice(&50u32.to_le_bytes());

        let dev_id = crate::fs::fsck::register_test_device(sectors);
        let io = IoStack::new(dev_id);
        let fs = Fat32Driver::new(io).unwrap();

        let stats = fs.check();
        crate::test_true!(stats.errors >= 1);
        crate::test_eq!(stats.total_files, 2);

        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });

    // Invalid root cluster must be rejected.
    crate::test_case!("fat32_fsck_bad_root_cluster", {
        let mut sectors = build_fat32_image();
        sectors[0][44..48].copy_from_slice(&0u32.to_le_bytes());

        let dev_id = crate::fs::fsck::register_test_device(sectors);
        let io = IoStack::new(dev_id);
        let fs = Fat32Driver::new(io).unwrap();

        let stats = fs.check();
        crate::test_true!(stats.errors >= 1);

        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });
}
