//! Boot Benchmark System — TSC-based boot pipeline profiling
//!
//! Measures storage init, first read, FS mount, shell load, and total boot
//! time.  Collects AHCI-specific debug counters (poll loops, DMA failures,
//! etc.) and enforces a 60 s watchdog so the kernel never hard-freezes.

use core::sync::atomic::{AtomicU64, AtomicU32, AtomicBool, Ordering};

// ── TSC-based high-precision timer ──────────────────────────────────

/// Read the CPU Time-Stamp Counter (RDTSC).
#[inline(always)]
pub fn rdtsc() -> u64 {
    unsafe { crate::hal::raw::raw_read_tsc() }
}

/// Estimate TSC frequency by timing against the PIT (Channel 2).
/// Returns approximate ticks-per-millisecond.
fn calibrate_tsc_khz() -> u64 {
    // Use PIT channel 2 for a ~10 ms delay (11932 ticks @ 1.193182 MHz)
    const PIT_HZ: u64 = 1_193_182;
    const DELAY_TICKS: u16 = 11932; // ~10 ms

    // Gate PIT channel 2 on
    let gate = crate::hal::inb(0x61);
    crate::hal::outb(0x61, (gate & 0xFD) | 0x01);
    // Mode 0, channel 2
    crate::hal::outb(0x43, 0xB0);
    crate::hal::outb(0x42, (DELAY_TICKS & 0xFF) as u8);
    crate::hal::outb(0x42, (DELAY_TICKS >> 8) as u8);

    // Reset gate to start countdown
    let gate2 = crate::hal::inb(0x61);
    crate::hal::outb(0x61, gate2 & 0xFE);
    crate::hal::outb(0x61, gate2 | 0x01);

    let start = rdtsc();
    // Wait for PIT output bit (bit 5 of port 0x61)
    while (crate::hal::inb(0x61) & 0x20) == 0 {}
    let end = rdtsc();

    let elapsed = end.saturating_sub(start);
    // elapsed ticks in ~10 ms → ticks/ms = elapsed / 10
    let khz = elapsed / 10;
    if khz == 0 { 1000 } else { khz } // fallback 1 GHz
}

static TSC_KHZ: AtomicU64 = AtomicU64::new(0);

/// Initialise TSC calibration.  Call once, early in boot.
pub fn init() {
    let khz = calibrate_tsc_khz();
    TSC_KHZ.store(khz, Ordering::Relaxed);
    crate::serial_println!("[BENCH] TSC calibrated: {} ticks/ms", khz);
}

/// Get calibrated TSC frequency in ticks per millisecond.
pub fn get_tsc_khz() -> u64 {
    TSC_KHZ.load(Ordering::Relaxed)
}

/// Current timestamp in TSC ticks.
#[inline]
pub fn boot_time_now() -> u64 {
    rdtsc()
}

/// Convert tick delta to milliseconds.
#[inline]
pub fn elapsed_ms(start: u64, end: u64) -> u64 {
    let khz = TSC_KHZ.load(Ordering::Relaxed);
    if khz == 0 { return 0; }
    end.saturating_sub(start) / khz
}

// ── Boot stages ─────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BootStage {
    KernelEntry    = 0,
    StorageInit    = 1,
    StorageReady   = 2,
    FirstRead      = 3,
    FsMounted      = 4,
    ShellReady     = 5,
}

impl BootStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::KernelEntry  => "kernel_entry",
            Self::StorageInit  => "init",
            Self::StorageReady => "storage_ready",
            Self::FirstRead    => "first_read",
            Self::FsMounted    => "fs_mount",
            Self::ShellReady   => "shell_load",
        }
    }
}

const NUM_STAGES: usize = 6;

static STAGE_TS: [AtomicU64; NUM_STAGES] = [
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
];

/// Record a timestamp for the given stage.
pub fn mark(stage: BootStage) {
    STAGE_TS[stage as usize].store(boot_time_now(), Ordering::Relaxed);
}

fn stage_ts(stage: BootStage) -> u64 {
    STAGE_TS[stage as usize].load(Ordering::Relaxed)
}

// ── Benchmark result ────────────────────────────────────────────────

pub struct BootBenchmarkResult {
    pub driver_name: &'static str,
    pub storage_init_ms: u64,
    pub first_read_ms: u64,
    pub fs_mount_ms: u64,
    pub shell_load_ms: u64,
    pub total_boot_ms: u64,
    pub avg_io_latency_ms: u64,
    pub max_io_latency_ms: u64,
    pub command_count: u64,
    pub polling_loops: u64,
    pub timeout_count: u64,
    pub timed_out: bool,
    pub timeout_stage: &'static str,
}

impl BootBenchmarkResult {
    pub fn from_stages(name: &'static str) -> Self {
        let t0 = stage_ts(BootStage::KernelEntry);
        BootBenchmarkResult {
            driver_name: name,
            storage_init_ms: elapsed_ms(
                stage_ts(BootStage::StorageInit),
                stage_ts(BootStage::StorageReady),
            ),
            first_read_ms: elapsed_ms(
                stage_ts(BootStage::StorageReady),
                stage_ts(BootStage::FirstRead),
            ),
            fs_mount_ms: elapsed_ms(
                stage_ts(BootStage::FirstRead),
                stage_ts(BootStage::FsMounted),
            ),
            shell_load_ms: elapsed_ms(
                stage_ts(BootStage::FsMounted),
                stage_ts(BootStage::ShellReady),
            ),
            total_boot_ms: elapsed_ms(t0, stage_ts(BootStage::ShellReady)),
            avg_io_latency_ms: 0,
            max_io_latency_ms: 0,
            command_count: 0,
            polling_loops: 0,
            timeout_count: 0,
            timed_out: false,
            timeout_stage: "",
        }
    }

    pub fn print(&self) {
        crate::serial_println!("{}:", self.driver_name);
        if self.timed_out {
            crate::serial_println!("  Status: FAILED_TIMEOUT at stage '{}'", self.timeout_stage);
        }
        crate::serial_println!("  - init:       {} ms", self.storage_init_ms);
        crate::serial_println!("  - first read: {} ms", self.first_read_ms);
        crate::serial_println!("  - mount:      {} ms", self.fs_mount_ms);
        crate::serial_println!("  - shell:      {} ms", self.shell_load_ms);
        crate::serial_println!("  - TOTAL:      {} ms", self.total_boot_ms);
        crate::println!("{}:", self.driver_name);
        if self.timed_out {
            crate::println!("  Status: FAILED_TIMEOUT at stage '{}'", self.timeout_stage);
        }
        crate::println!("  - init:       {} ms", self.storage_init_ms);
        crate::println!("  - first read: {} ms", self.first_read_ms);
        crate::println!("  - mount:      {} ms", self.fs_mount_ms);
        crate::println!("  - shell:      {} ms", self.shell_load_ms);
        crate::println!("  - TOTAL:      {} ms", self.total_boot_ms);
    }
}

// ── AHCI debug instrumentation ──────────────────────────────────────

pub static AHCI_COMMANDS:     AtomicU64 = AtomicU64::new(0);
pub static AHCI_POLL_LOOPS:   AtomicU64 = AtomicU64::new(0);
pub static AHCI_TOTAL_WAIT:   AtomicU64 = AtomicU64::new(0);
pub static AHCI_MAX_WAIT:     AtomicU64 = AtomicU64::new(0);
pub static AHCI_TIMEOUTS:     AtomicU64 = AtomicU64::new(0);
pub static AHCI_DMA_FAILURES: AtomicU64 = AtomicU64::new(0);
pub static AHCI_RETRIES:      AtomicU64 = AtomicU64::new(0);

// ── VirtIO debug instrumentation ────────────────────────────────────
pub static VIRTIO_COMMANDS: AtomicU64 = AtomicU64::new(0);

/// Call at the start of each AHCI DMA command.
pub fn ahci_cmd_start() {
    AHCI_COMMANDS.fetch_add(1, Ordering::Relaxed);
}

/// Record polling iterations for a single command.
pub fn ahci_cmd_polled(loops: u64) {
    AHCI_POLL_LOOPS.fetch_add(loops, Ordering::Relaxed);
}

/// Record command completion wait time in ms.
pub fn ahci_cmd_done(wait_ms: u64) {
    AHCI_TOTAL_WAIT.fetch_add(wait_ms, Ordering::Relaxed);
    loop {
        let cur = AHCI_MAX_WAIT.load(Ordering::Relaxed);
        if wait_ms <= cur { break; }
        if AHCI_MAX_WAIT.compare_exchange(cur, wait_ms, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
            break;
        }
    }
}

pub fn ahci_cmd_timeout() {
    AHCI_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
}

pub fn ahci_dma_failure() {
    AHCI_DMA_FAILURES.fetch_add(1, Ordering::Relaxed);
}

pub fn print_ahci_debug() {
    let cmds  = AHCI_COMMANDS.load(Ordering::Relaxed);
    let polls = AHCI_POLL_LOOPS.load(Ordering::Relaxed);
    let total = AHCI_TOTAL_WAIT.load(Ordering::Relaxed);
    let max   = AHCI_MAX_WAIT.load(Ordering::Relaxed);
    let tmo   = AHCI_TIMEOUTS.load(Ordering::Relaxed);
    let dma   = AHCI_DMA_FAILURES.load(Ordering::Relaxed);
    let avg   = total.checked_div(cmds).unwrap_or(0);

    let msg = "[AHCI DEBUG]";
    crate::serial_println!("{}", msg);
    crate::serial_println!("  commands:      {}", cmds);
    crate::serial_println!("  avg wait:      {} ms", avg);
    crate::serial_println!("  max wait:      {} ms", max);
    crate::serial_println!("  poll loops:    {}", polls);
    crate::serial_println!("  timeouts:      {}", tmo);
    crate::serial_println!("  dma_failures:  {}", dma);

    crate::println!("{}", msg);
    crate::println!("  commands:      {}", cmds);
    crate::println!("  avg wait:      {} ms", avg);
    crate::println!("  max wait:      {} ms", max);
    crate::println!("  poll loops:    {}", polls);
    crate::println!("  timeouts:      {}", tmo);
    crate::println!("  dma_failures:  {}", dma);
}

// ── Watchdog ────────────────────────────────────────────────────────

/// Maximum allowed boot time in milliseconds.
pub const MAX_BOOT_TIME_MS: u64 = 60_000;

/// Per-stage timeout in milliseconds.
pub const MAX_STAGE_TIME_MS: u64 = 15_000;

static WATCHDOG_START: AtomicU64 = AtomicU64::new(0);
static WATCHDOG_STAGE_START: AtomicU64 = AtomicU64::new(0);
static WATCHDOG_TRIPPED: AtomicBool = AtomicBool::new(false);
static WATCHDOG_STAGE: AtomicU32 = AtomicU32::new(0);

/// Arm the watchdog at boot start.
pub fn watchdog_arm() {
    let now = boot_time_now();
    WATCHDOG_START.store(now, Ordering::Relaxed);
    WATCHDOG_STAGE_START.store(now, Ordering::Relaxed);
    WATCHDOG_TRIPPED.store(false, Ordering::Relaxed);
}

/// Notify watchdog of a new stage.
pub fn watchdog_enter_stage(stage: BootStage) {
    WATCHDOG_STAGE_START.store(boot_time_now(), Ordering::Relaxed);
    WATCHDOG_STAGE.store(stage as u32, Ordering::Relaxed);
}

/// Check if we've exceeded time limits.  Returns `true` if timed out.
pub fn watchdog_check() -> bool {
    if WATCHDOG_TRIPPED.load(Ordering::Relaxed) {
        return true;
    }
    let now = boot_time_now();
    let total = elapsed_ms(WATCHDOG_START.load(Ordering::Relaxed), now);
    let stage_elapsed = elapsed_ms(WATCHDOG_STAGE_START.load(Ordering::Relaxed), now);

    if total > MAX_BOOT_TIME_MS || stage_elapsed > MAX_STAGE_TIME_MS {
        WATCHDOG_TRIPPED.store(true, Ordering::Relaxed);
        let stage_id = WATCHDOG_STAGE.load(Ordering::Relaxed);
        let stage_name = match stage_id {
            1 => "init",
            2 => "storage_ready",
            3 => "first_read",
            4 => "fs_mount",
            5 => "shell_load",
            _ => "unknown",
        };
        crate::serial_println!("[BOOT BENCHMARK TIMEOUT]");
        crate::serial_println!("  Stage:   {}", stage_name);
        crate::serial_println!("  Elapsed: {} ms (stage {} ms)", total, stage_elapsed);
        crate::serial_println!("  Status:  FAILED_TIMEOUT");
        crate::serial_println!("  Likely causes:");
        crate::serial_println!("    - infinite polling loop");
        crate::serial_println!("    - DMA never completes");
        crate::serial_println!("    - invalid PRDT");
        crate::serial_println!("    - missing IRQ / interrupt stall");
        crate::serial_println!("    - deadlock in storage layer");

        crate::println!("[BOOT BENCHMARK TIMEOUT]");
        crate::println!("  Stage: {} | Elapsed: {} ms | FAILED_TIMEOUT", stage_name, total);
        return true;
    }
    false
}

/// Has the watchdog tripped?
pub fn watchdog_tripped() -> bool {
    WATCHDOG_TRIPPED.load(Ordering::Relaxed)
}

// ── Configuration flags ──────────────────────────────────────────────

/// Controls whether to print boot benchmark results.
/// Default: true. Can be disabled by setting BENCHMARK_REPORT=0 in BOOT.CFG.
pub static ENABLE_BOOT_BENCHMARK_REPORT: AtomicBool = AtomicBool::new(true);

/// Controls whether to print detailed AHCI debug stats.
/// Default: true. Can be disabled by setting AHCI_DEBUG=0 in BOOT.CFG.
pub static ENABLE_AHCI_DEBUG_OUTPUT: AtomicBool = AtomicBool::new(true);



/// Initialize boot configuration with default values.
/// Configuration can be changed at runtime using the BENCH shell command.
pub fn load_config() {
    // Both flags default to true (original behavior)
    // Try to read from C:\SYSTEM\BOOT.CFG if VFS is available
    
    // Attempt to read BOOT.CFG from the filesystem
    if let Ok(content) = read_boot_config() {
        parse_boot_config(&content);
    }
    
    crate::serial_println!("[BENCH] Config flags: BENCHMARK_REPORT={}, AHCI_DEBUG={}",
        if ENABLE_BOOT_BENCHMARK_REPORT.load(Ordering::Relaxed) { 1 } else { 0 },
        if ENABLE_AHCI_DEBUG_OUTPUT.load(Ordering::Relaxed) { 1 } else { 0 });
}

/// Try to read BOOT.CFG from the filesystem
fn read_boot_config() -> Result<alloc::string::String, &'static str> {
    use alloc::string::String;
    
    let path = "C:\\System\\Kernel\\boot.cfg";
    
    crate::globals::with_vfs(|vfs| {
        // Resolve path to get drive index and file metadata
        let (drive_idx, node) = vfs.resolve_path(path).map_err(|_| "Failed to resolve BOOT.CFG")?;
        
        // Verify it's a file, not a directory
        const MODE_FILE: u16 = 0x80;
        if node.mode & MODE_FILE == 0 {
            return Err("BOOT.CFG is not a regular file");
        }
        
        // Read file content (up to 1024 bytes)
        let size = (node.size as usize).min(1024);
        let mut buf = alloc::vec![0u8; size];
        
        vfs.read(drive_idx, node.inode, 0, &mut buf)
            .map_err(|_| "Failed to read BOOT.CFG")?;
        
        // Convert to string
        String::from_utf8(buf).map_err(|_| "BOOT.CFG is not valid UTF-8")
    })
}

/// Parse BOOT.CFG content and set configuration flags
fn parse_boot_config(content: &str) {
    for line in content.lines() {
        let trimmed = line.trim();
        
        // Skip comments and empty lines
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        
        // Parse KEY=VALUE
        if let Some(pos) = trimmed.find('=') {
            let key = trimmed[..pos].trim();
            let value = trimmed[pos+1..].trim();
            
            match key {
                "BENCHMARK_REPORT" => {
                    if value == "1" || value.eq_ignore_ascii_case("ON") || value.eq_ignore_ascii_case("YES") {
                        ENABLE_BOOT_BENCHMARK_REPORT.store(true, Ordering::Relaxed);
                    } else if value == "0" || value.eq_ignore_ascii_case("OFF") || value.eq_ignore_ascii_case("NO") {
                        ENABLE_BOOT_BENCHMARK_REPORT.store(false, Ordering::Relaxed);
                    }
                }
                "AHCI_DEBUG" => {
                    if value == "1" || value.eq_ignore_ascii_case("ON") || value.eq_ignore_ascii_case("YES") {
                        ENABLE_AHCI_DEBUG_OUTPUT.store(true, Ordering::Relaxed);
                    } else if value == "0" || value.eq_ignore_ascii_case("OFF") || value.eq_ignore_ascii_case("NO") {
                        ENABLE_AHCI_DEBUG_OUTPUT.store(false, Ordering::Relaxed);
                    }
                }

                _ => {} // Ignore unknown keys
            }
        }
    }
}

/// Set benchmark report flag (for testing/dynamic config).
pub fn set_benchmark_report_enabled(enabled: bool) {
    ENABLE_BOOT_BENCHMARK_REPORT.store(enabled, Ordering::Relaxed);
}

/// Set AHCI debug output flag (for testing/dynamic config).
pub fn set_ahci_debug_enabled(enabled: bool) {
    ENABLE_AHCI_DEBUG_OUTPUT.store(enabled, Ordering::Relaxed);
}

// ── Full report ─────────────────────────────────────────────────────

/// Detect which storage driver is active and print the benchmark report.
pub fn print_report(driver_name: &'static str) {
    // Check if benchmark reporting is enabled
    if !ENABLE_BOOT_BENCHMARK_REPORT.load(Ordering::Relaxed) {
        return;
    }

    crate::serial_println!();
    crate::serial_println!("[BOOT BENCHMARK RESULTS]");
    crate::println!();
    crate::println!("[BOOT BENCHMARK RESULTS]");

    let mut result = BootBenchmarkResult::from_stages(driver_name);
    if watchdog_tripped() {
        result.timed_out = true;
        let s = WATCHDOG_STAGE.load(Ordering::Relaxed);
        result.timeout_stage = match s {
            1 => "init", 2 => "storage_ready", 3 => "first_read",
            4 => "fs_mount", 5 => "shell_load", _ => "unknown",
        };
    }

    // Fill AHCI stats if applicable
    let cmds = AHCI_COMMANDS.load(Ordering::Relaxed);
    if cmds > 0 {
        result.command_count = cmds;
        result.polling_loops = AHCI_POLL_LOOPS.load(Ordering::Relaxed);
        result.max_io_latency_ms = AHCI_MAX_WAIT.load(Ordering::Relaxed);
        result.avg_io_latency_ms = AHCI_TOTAL_WAIT.load(Ordering::Relaxed) / cmds.max(1);
        result.timeout_count = AHCI_TIMEOUTS.load(Ordering::Relaxed);
    }

    result.print();

    // Print detailed AHCI debug stats only if both benchmark AND ahci_debug are enabled
    if cmds > 0 && ENABLE_AHCI_DEBUG_OUTPUT.load(Ordering::Relaxed) {
        crate::serial_println!();
        crate::println!();
        print_ahci_debug();
    }

    crate::serial_println!();
    crate::println!();
}
