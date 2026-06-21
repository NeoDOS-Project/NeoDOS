use crate::test_case;
use crate::test_eq;
use crate::test_true;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;
use crate::fs::vfs::{FileSystem, VfsError, VfsNode, DirEntry, MODE_DIR, MODE_FILE};
use crate::drivers::driver_runtime::{self, DriverInstance};

// ── Inode constants ──────────────────────────────────────────────────────────
const ROOT_INODE: u32 = 0;
const PROCESSES_DIR: u32 = 1;
const DRIVERS_DIR: u32 = 2;
const MEMORY_FILE: u32 = 3;
const INTERRUPTS_FILE: u32 = 4;
const PID_INODE_BASE: u32 = 1000;
const DRIVER_INODE_BASE: u32 = 2000;

/// Virtual filesystem K:\ that exposes kernel objects as read-only files.
pub struct KDrive;

impl KDrive {
    pub fn new() -> Self {
        KDrive
    }

    fn is_pid_inode(inode: u32) -> bool {
        inode >= PID_INODE_BASE && inode < PID_INODE_BASE + 256
    }

    fn is_driver_inode(inode: u32) -> bool {
        inode >= DRIVER_INODE_BASE && inode < DRIVER_INODE_BASE + 64
    }

    fn pid_from_inode(inode: u32) -> u32 {
        inode - PID_INODE_BASE
    }

    fn driver_idx_from_inode(inode: u32) -> usize {
        (inode - DRIVER_INODE_BASE) as usize
    }

    fn generate_memory_stats() -> String {
        let stats = crate::memory::stats();
        format!(
            "Physical Memory: {} KB\r\nTotal: {} KB\r\nUsable: {} KB\r\nFree: {} KB\r\nUsed: {} KB\r\nReserved: {} KB\r\n",
            stats.phys_max, stats.total_kib, stats.usable_kib,
            stats.free_kib, stats.used_kib, stats.reserved_kib,
        )
    }

    fn generate_process_info(pid: u32) -> String {
        let sched = crate::scheduler::current_scheduler().lock();
        if let Some(eproc) = sched.find_eprocess(pid) {
            let threads = sched.thread_tids_for_pid(pid);
            let state_str = if let Some(tid) = threads.first() {
                if let Some(kthread) = sched.find_kthread(*tid) {
                    match kthread.state {
                        crate::scheduler::ThreadState::Ready => "Ready",
                        crate::scheduler::ThreadState::Running => "Running",
                        crate::scheduler::ThreadState::Blocked { .. } => "Blocked",
                        crate::scheduler::ThreadState::Terminated => "Terminated",
                    }
                } else { "Unknown" }
            } else { "Terminated" };
            let prio_str = if let Some(tid) = threads.first() {
                if let Some(kthread) = sched.find_kthread(*tid) {
                    match kthread.priority {
                        0 => "HIGH",
                        1 => "ABOVE_NORMAL",
                        2 => "NORMAL",
                        3 => "IDLE",
                        _ => "OTHER",
                    }
                } else { "N/A" }
            } else { "N/A" };
            let is_admin = if eproc.token.is_admin_token() { "Yes" } else { "No" };
            let cwd_letter = (eproc.cwd_drive + b'A') as char;
            format!(
                "PID: {}\r\nParent PID: {}\r\nThreads: {}\r\nState: {}\r\nPriority: {}\r\nCWD: {}:{}\r\nHeap Base: 0x{:x}\r\nHeap Break: 0x{:x}\r\nAdmin: {}\r\n",
                pid, eproc.parent_pid, threads.len(), state_str, prio_str,
                cwd_letter, eproc.cwd_path, eproc.heap_base, eproc.heap_break, is_admin,
            )
        } else {
            format!("PID {} not found\r\n", pid)
        }
    }

    fn generate_driver_info_for_instance(drv: &DriverInstance) -> String {
        format!(
            "Name: {}\r\nID: {}\r\nState: {}\r\nCategory: {}\r\nType: {}\r\nABI: min={} target={} max={}\r\nCapabilities: 0x{:016x}\r\nLast Error: {} ({})\r\nEvents Received: {}\r\nTick Count: {}\r\nIsolation: mode={} base=0x{:x} size=0x{:x}\r\n",
            drv.name_str(), drv.id, drv.state.to_str(), drv.category.to_str(),
            drv.driver_type.to_str(), drv.abi_min, drv.abi_target, drv.abi_max,
            drv.caps, drv.last_error, driver_runtime::err_to_str(drv.last_error),
            drv.events_received, drv.tick_count,
            drv.isolation_mode, drv.isolated_base, drv.isolated_size,
        )
    }

    fn generate_interrupts_info() -> String {
        let count = unsafe {
            crate::arch::x64::cpu_local::gs_read_u64(
                crate::arch::x64::cpu_local::OFFSET_INTERRUPT_COUNT)
        };
        let cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
        let total_cpus = crate::arch::x64::cpu_local::cpu_count();
        format!(
            "Current CPU: {}\r\nTotal CPUs: {}\r\nInterrupts on this CPU: {}\r\n",
            cpu, total_cpus, count,
        )
    }

    fn generate_driver_info_by_idx(idx: usize) -> Option<String> {
        let drivers = driver_runtime::driver_names();
        if let Some((_name, id, _state)) = drivers.get(idx) {
            if let Some(drv) = driver_runtime::get_driver(*id) {
                return Some(Self::generate_driver_info_for_instance(&drv));
            }
        }
        None
    }

    fn get_content(&self, inode: u32) -> Result<String, VfsError> {
        match inode {
            MEMORY_FILE => Ok(Self::generate_memory_stats()),
            INTERRUPTS_FILE => Ok(Self::generate_interrupts_info()),
            pid if Self::is_pid_inode(pid) => Ok(Self::generate_process_info(Self::pid_from_inode(pid))),
            did if Self::is_driver_inode(did) => {
                let idx = Self::driver_idx_from_inode(did);
                Self::generate_driver_info_by_idx(idx).ok_or(VfsError::NotFound)
            }
            _ => Err(VfsError::NotFound),
        }
    }
}

impl FileSystem for KDrive {
    fn read(&mut self, inode: u32, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError> {
        let content = self.get_content(inode)?;
        let bytes = content.as_bytes();
        let len = bytes.len();
        if (offset as usize) >= len {
            return Ok(0);
        }
        let start = offset as usize;
        let end = core::cmp::min(start + buf.len(), len);
        let count = end - start;
        buf[..count].copy_from_slice(&bytes[start..end]);
        Ok(count)
    }

    fn write(&mut self, _inode: u32, _offset: u64, _buf: &[u8]) -> Result<usize, VfsError> {
        Err(VfsError::NotImplemented)
    }

    fn lookup(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError> {
        match dir_inode {
            ROOT_INODE => {
                let upper = name.to_ascii_uppercase();
                match upper.as_str() {
                    "PROCESSES" => Ok(VfsNode { inode: PROCESSES_DIR, mode: MODE_DIR, size: 0 }),
                    "DRIVERS" => Ok(VfsNode { inode: DRIVERS_DIR, mode: MODE_DIR, size: 0 }),
                    "MEMORY" => {
                        let sz = Self::generate_memory_stats().len() as u32;
                        Ok(VfsNode { inode: MEMORY_FILE, mode: MODE_FILE, size: sz })
                    }
                    "INTERRUPTS" => {
                        let sz = Self::generate_interrupts_info().len() as u32;
                        Ok(VfsNode { inode: INTERRUPTS_FILE, mode: MODE_FILE, size: sz })
                    }
                    _ => Err(VfsError::NotFound),
                }
            }
            PROCESSES_DIR => {
                let pid: u32 = name.parse().map_err(|_| VfsError::NotFound)?;
                let sched = crate::scheduler::current_scheduler().lock();
                if sched.find_eprocess(pid).is_some() {
                    let content = Self::generate_process_info(pid);
                    Ok(VfsNode { inode: PID_INODE_BASE + pid, mode: MODE_FILE, size: content.len() as u32 })
                } else {
                    Err(VfsError::NotFound)
                }
            }
            DRIVERS_DIR => {
                let drivers = driver_runtime::driver_names();
                for (i, (dname, id, _state)) in drivers.iter().enumerate() {
                    if dname.eq_ignore_ascii_case(name) {
                        if let Some(drv) = driver_runtime::get_driver(*id) {
                            let content = Self::generate_driver_info_for_instance(&drv);
                            return Ok(VfsNode {
                                inode: DRIVER_INODE_BASE + i as u32,
                                mode: MODE_FILE,
                                size: content.len() as u32,
                            });
                        }
                    }
                }
                Err(VfsError::NotFound)
            }
            _ => Err(VfsError::NotFound),
        }
    }

    fn readdir(&mut self, dir_inode: u32, index: usize) -> Result<Option<DirEntry>, VfsError> {
        match dir_inode {
            ROOT_INODE => {
                const ENTRIES: &[(&str, u32, u16)] = &[
                    ("Processes", PROCESSES_DIR, MODE_DIR),
                    ("Drivers", DRIVERS_DIR, MODE_DIR),
                    ("Memory", MEMORY_FILE, MODE_FILE),
                    ("Interrupts", INTERRUPTS_FILE, MODE_FILE),
                ];
                if index < ENTRIES.len() {
                    let (name, ino, mode) = ENTRIES[index];
                    let size = if mode == MODE_FILE {
                        match ino {
                            MEMORY_FILE => Self::generate_memory_stats().len() as u32,
                            INTERRUPTS_FILE => Self::generate_interrupts_info().len() as u32,
                            _ => 0,
                        }
                    } else {
                        0
                    };
                    Ok(Some(DirEntry {
                        name: name.to_string(),
                        node: VfsNode { inode: ino, mode, size },
                    }))
                } else {
                    Ok(None)
                }
            }
            PROCESSES_DIR => {
                let sched = crate::scheduler::current_scheduler().lock();
                let pids: Vec<u32> = sched.eprocesses.iter()
                    .filter_map(|e| e.as_ref().map(|ep| ep.pid))
                    .collect();
                drop(sched);
                if index < pids.len() {
                    let pid = pids[index];
                    let content = Self::generate_process_info(pid);
                    Ok(Some(DirEntry {
                        name: alloc::format!("{}", pid),
                        node: VfsNode {
                            inode: PID_INODE_BASE + pid,
                            mode: MODE_FILE,
                            size: content.len() as u32,
                        },
                    }))
                } else {
                    Ok(None)
                }
            }
            DRIVERS_DIR => {
                let drivers = driver_runtime::driver_names();
                if index < drivers.len() {
                    let (name, id, _state) = &drivers[index];
                    if let Some(drv) = driver_runtime::get_driver(*id) {
                        let content = Self::generate_driver_info_for_instance(&drv);
                        Ok(Some(DirEntry {
                            name: name.clone(),
                            node: VfsNode {
                                inode: DRIVER_INODE_BASE + index as u32,
                                mode: MODE_FILE,
                                size: content.len() as u32,
                            },
                        }))
                    } else {
                        Err(VfsError::NotFound)
                    }
                } else {
                    Ok(None)
                }
            }
            _ => Err(VfsError::NotADirectory),
        }
    }

    fn mkdir(&mut self, _dir_inode: u32, _name: &str) -> Result<VfsNode, VfsError> {
        Err(VfsError::NotImplemented)
    }

    fn create(&mut self, _dir_inode: u32, _name: &str) -> Result<VfsNode, VfsError> {
        Err(VfsError::NotImplemented)
    }

    fn stat(&mut self, inode: u32) -> Result<VfsNode, VfsError> {
        match inode {
            ROOT_INODE => Ok(VfsNode { inode: ROOT_INODE, mode: MODE_DIR, size: 0 }),
            PROCESSES_DIR => Ok(VfsNode { inode: PROCESSES_DIR, mode: MODE_DIR, size: 0 }),
            DRIVERS_DIR => Ok(VfsNode { inode: DRIVERS_DIR, mode: MODE_DIR, size: 0 }),
            MEMORY_FILE => {
                let sz = Self::generate_memory_stats().len() as u32;
                Ok(VfsNode { inode: MEMORY_FILE, mode: MODE_FILE, size: sz })
            }
            INTERRUPTS_FILE => {
                let sz = Self::generate_interrupts_info().len() as u32;
                Ok(VfsNode { inode: INTERRUPTS_FILE, mode: MODE_FILE, size: sz })
            }
            pid if Self::is_pid_inode(pid) => {
                let p = Self::pid_from_inode(pid);
                let sched = crate::scheduler::current_scheduler().lock();
                if sched.find_eprocess(p).is_some() {
                    let content = Self::generate_process_info(p);
                    Ok(VfsNode { inode: pid, mode: MODE_FILE, size: content.len() as u32 })
                } else {
                    Err(VfsError::NotFound)
                }
            }
            did if Self::is_driver_inode(did) => {
                let idx = Self::driver_idx_from_inode(did);
                Self::generate_driver_info_by_idx(idx).map(|content| {
                    VfsNode { inode: did, mode: MODE_FILE, size: content.len() as u32 }
                }).ok_or(VfsError::NotFound)
            }
            _ => Err(VfsError::NotFound),
        }
    }

    fn remove_file(&mut self, _dir_inode: u32, _name: &str) -> Result<(), VfsError> {
        Err(VfsError::NotImplemented)
    }

    fn remove_dir(&mut self, _dir_inode: u32, _name: &str) -> Result<(), VfsError> {
        Err(VfsError::NotImplemented)
    }

    fn fs_type(&self) -> &'static str {
        "KDrive"
    }

    fn total_sectors(&self) -> u64 {
        0
    }
}

/// Mount K:\ drive (virtual kernel object filesystem).
/// Called during boot after VFS is initialized.
pub fn init_kdrive() {
    crate::globals::with_vfs(|vfs| {
        match vfs.mount('K', alloc::boxed::Box::new(KDrive::new())) {
            Ok(()) => crate::println!("[+] K:\\ mounted (kernel object namespace)"),
            Err(e) => crate::println!("[!] Failed to mount K:\\: {:?}", e),
        }
    });
    let _ = crate::vfs::mount::vfs_mount("\\Device\\KDrive", 'K', crate::vfs::mount::FilesystemType::Iso9660);
}

pub fn register_kdrive_tests() {
    test_case!("kdrive_root_readdir", {
        let mut kd = KDrive::new();
        let r0 = kd.readdir(ROOT_INODE, 0).unwrap().unwrap();
        test_eq!(r0.name, "Processes");
        let r1 = kd.readdir(ROOT_INODE, 1).unwrap().unwrap();
        test_eq!(r1.name, "Drivers");
        let r2 = kd.readdir(ROOT_INODE, 2).unwrap().unwrap();
        test_eq!(r2.name, "Memory");
        let r3 = kd.readdir(ROOT_INODE, 3).unwrap().unwrap();
        test_eq!(r3.name, "Interrupts");
        test_true!(kd.readdir(ROOT_INODE, 4).unwrap().is_none());
    });

    test_case!("kdrive_lookup_root_entries", {
        let mut kd = KDrive::new();
        let p = kd.lookup(ROOT_INODE, "Processes").unwrap();
        test_eq!(p.inode, PROCESSES_DIR);
        test_eq!(p.mode, MODE_DIR);
        let d = kd.lookup(ROOT_INODE, "Drivers").unwrap();
        test_eq!(d.inode, DRIVERS_DIR);
        test_eq!(d.mode, MODE_DIR);
        let m = kd.lookup(ROOT_INODE, "Memory").unwrap();
        test_eq!(m.inode, MEMORY_FILE);
        test_eq!(m.mode, MODE_FILE);
        let i = kd.lookup(ROOT_INODE, "Interrupts").unwrap();
        test_eq!(i.inode, INTERRUPTS_FILE);
        test_eq!(i.mode, MODE_FILE);
    });

    test_case!("kdrive_lookup_case_insensitive", {
        let mut kd = KDrive::new();
        let p = kd.lookup(ROOT_INODE, "processes").unwrap();
        test_eq!(p.inode, PROCESSES_DIR);
        let p2 = kd.lookup(ROOT_INODE, "PROCESSES").unwrap();
        test_eq!(p2.inode, PROCESSES_DIR);
    });

    test_case!("kdrive_lookup_not_found", {
        let mut kd = KDrive::new();
        test_true!(kd.lookup(ROOT_INODE, "Nonexistent").is_err());
        test_true!(kd.lookup(ROOT_INODE, "Foo").is_err());
    });

    test_case!("kdrive_memory_stats", {
        let mut kd = KDrive::new();
        let mut buf = [0u8; 512];
        let n = kd.read(MEMORY_FILE, 0, &mut buf).unwrap();
        test_true!(n > 0);
        let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
        test_true!(text.contains("Physical Memory"));
        test_true!(text.contains("Total"));
        test_true!(text.contains("Free"));
    });

    test_case!("kdrive_interrupts_stats", {
        let mut kd = KDrive::new();
        let mut buf = [0u8; 256];
        let n = kd.read(INTERRUPTS_FILE, 0, &mut buf).unwrap();
        test_true!(n > 0);
        let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
        test_true!(text.contains("Interrupts"));
    });

    test_case!("kdrive_write_rejected", {
        let mut kd = KDrive::new();
        test_true!(kd.write(MEMORY_FILE, 0, b"data").is_err());
        test_true!(kd.create(ROOT_INODE, "NewFile").is_err());
        test_true!(kd.mkdir(ROOT_INODE, "NewDir").is_err());
    });

    test_case!("kdrive_stat_root_is_dir", {
        let mut kd = KDrive::new();
        let node = kd.stat(ROOT_INODE).unwrap();
        test_eq!(node.mode, MODE_DIR);
    });

    test_case!("kdrive_read_memory_at_offset", {
        let mut kd = KDrive::new();
        let mut full = [0u8; 512];
        let n = kd.read(MEMORY_FILE, 0, &mut full).unwrap();
        // Read at offset 10
        let mut partial = [0u8; 32];
        let m = kd.read(MEMORY_FILE, 10, &mut partial).unwrap();
        test_true!(m > 0);
        // Should match the substring of the full content
        if m > 0 && n > 10 {
            let expected = &full[10..10 + m];
            test_eq!(&partial[..m], expected);
        }
    });

    test_case!("kdrive_pid_inode_encoding", {
        test_eq!(KDrive::pid_from_inode(PID_INODE_BASE + 42), 42);
        test_eq!(KDrive::pid_from_inode(PID_INODE_BASE + 5), 5);
        test_true!(KDrive::is_pid_inode(PID_INODE_BASE + 1));
        test_true!(!KDrive::is_pid_inode(PID_INODE_BASE - 1));
    });

    test_case!("kdrive_driver_inode_encoding", {
        test_eq!(KDrive::driver_idx_from_inode(DRIVER_INODE_BASE + 3), 3);
        test_true!(KDrive::is_driver_inode(DRIVER_INODE_BASE));
        test_true!(!KDrive::is_driver_inode(DRIVER_INODE_BASE - 1));
        test_true!(!KDrive::is_driver_inode(DRIVER_INODE_BASE + 64));
    });
}
