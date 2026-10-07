//! FSCK for FAT32.
//!
//! Validates the BPB geometry, walks the FAT chains reachable from the root
//! directory, and reports corrupt links, cross-linked clusters and orphaned
//! (allocated but unreachable) clusters. Repair frees orphaned clusters, which
//! is the only class of damage that can be fixed without risking live data.

use alloc::vec::Vec;

use crate::drivers::fat32::Fat32Driver;
use crate::drivers::fsck::{FsckStats, FsckTrait};

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
