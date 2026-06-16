use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_fsck(&mut self, args: &[&str]) {
        let repair = args.iter().any(|a| a.eq_ignore_ascii_case("/F") || a.eq_ignore_ascii_case("-F") || a.eq_ignore_ascii_case("/REPAIR"));

        let drive = if let Some(first) = args.first() {
            if first.len() >= 2 && first.as_bytes()[1] == b':' {
                first.chars().next().unwrap_or('C')
            } else {
                self.current_drive
            }
        } else {
            self.current_drive
        };

        if repair {
            println!("FSCK /F: Checking and repairing drive {}...", drive);
        } else {
            println!("FSCK: Checking drive {}... (use /F to repair errors)", drive);
        }

        let mut cache_lock = crate::globals::BLOCK_CACHE.lock();
        let cache = match cache_lock.as_mut() {
            Some(c) => c,
            None => {
                println!("ERROR: Block cache not initialized");
                return;
            }
        };
        let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
        let dev = match bdevs_lock.get(0) {
            Some(d) => d,
            None => {
                println!("ERROR: No block device available");
                return;
            }
        };

        let mode = if repair {
            crate::fs::fsck::FsckMode::Repair
        } else {
            crate::fs::fsck::FsckMode::CheckOnly
        };

        let partition_base = crate::globals::PRIMARY_PARTITION_BASE.load(core::sync::atomic::Ordering::Relaxed) as u32;
        let stats = crate::fs::fsck::run(cache, dev, mode, partition_base);

        crate::fs::fsck::print_report(&stats);

        if stats.repairs_applied > 0 {
            crate::globals::NEED_CACHE_FLUSH.store(true, core::sync::atomic::Ordering::Relaxed);
            crate::globals::flush_cache_if_needed();
            println!("Cache flushed after repairs.");
        }
    }
}
