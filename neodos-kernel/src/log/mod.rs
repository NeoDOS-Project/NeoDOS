//! Configurable kernel logging subsystem.
//!
//! Replaces scattered `serial_println!` calls with level- and subsystem-filtered
//! macros. Each subsystem has both a compile-time threshold (set via environment
//! variables like `LOG_NET=DEBUG`) and an optional runtime override.

use core::sync::atomic::{AtomicU8, Ordering};
use core::mem::MaybeUninit;

include!(concat!(env!("OUT_DIR"), "/log_config.rs"));

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum LogLevel {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
    Trace = 4,
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            LogLevel::Error => "ERROR",
            LogLevel::Warn => "WARN",
            LogLevel::Info => "INFO",
            LogLevel::Debug => "DEBUG",
            LogLevel::Trace => "TRACE",
        }
    }

    pub const fn from_u8(v: u8) -> Self {
        match v {
            0 => LogLevel::Error,
            1 => LogLevel::Warn,
            2 => LogLevel::Info,
            3 => LogLevel::Debug,
            4 => LogLevel::Trace,
            _ => LogLevel::Info,
        }
    }
}

#[derive(Clone, Copy)]
pub struct LogSubsys {
    idx: usize,
    tag: &'static str,
}

#[allow(non_upper_case_globals)]
impl LogSubsys {
    pub const Kernel: LogSubsys = LogSubsys { idx: 0, tag: "KERN" };
    pub const Net: LogSubsys = LogSubsys { idx: 1, tag: "NET" };
    pub const Sched: LogSubsys = LogSubsys { idx: 2, tag: "SCHED" };
    pub const Driver: LogSubsys = LogSubsys { idx: 3, tag: "DRV" };
    pub const Power: LogSubsys = LogSubsys { idx: 4, tag: "PM" };
    pub const Services: LogSubsys = LogSubsys { idx: 5, tag: "SM" };
    pub const Cm: LogSubsys = LogSubsys { idx: 6, tag: "CM" };
    pub const Object: LogSubsys = LogSubsys { idx: 7, tag: "OB" };
    pub const Syscall: LogSubsys = LogSubsys { idx: 8, tag: "SYS" };
    pub const Kbd: LogSubsys = LogSubsys { idx: 9, tag: "KBD" };
    pub const Input: LogSubsys = LogSubsys { idx: 10, tag: "INPUT" };
    pub const Virtio: LogSubsys = LogSubsys { idx: 11, tag: "VIO" };
    pub const Timers: LogSubsys = LogSubsys { idx: 12, tag: "TIMER" };
    pub const Interrupts: LogSubsys = LogSubsys { idx: 13, tag: "IRQ" };
    pub const Exception: LogSubsys = LogSubsys { idx: 14, tag: "EXC" };
    pub const Security: LogSubsys = LogSubsys { idx: 15, tag: "SEC" };
    pub const Fat32: LogSubsys = LogSubsys { idx: 16, tag: "FAT32" };
    pub const Hotreload: LogSubsys = LogSubsys { idx: 17, tag: "HOTRELOAD" };
    pub const Isolation: LogSubsys = LogSubsys { idx: 18, tag: "ISO" };
    pub const Nem: LogSubsys = LogSubsys { idx: 19, tag: "NEM" };
    pub const Nvme: LogSubsys = LogSubsys { idx: 20, tag: "NVMe" };
    pub const Pci: LogSubsys = LogSubsys { idx: 21, tag: "PCI" };
    pub const Slab: LogSubsys = LogSubsys { idx: 22, tag: "SLAB" };
    pub const Elf: LogSubsys = LogSubsys { idx: 23, tag: "ELF" };
    pub const Nxl: LogSubsys = LogSubsys { idx: 24, tag: "NXL" };
    pub const Vfs: LogSubsys = LogSubsys { idx: 25, tag: "VFS" };
    pub const Fs: LogSubsys = LogSubsys { idx: 26, tag: "FS" };
    pub const Watchdog: LogSubsys = LogSubsys { idx: 27, tag: "WDT" };
    pub const Boot: LogSubsys = LogSubsys { idx: 28, tag: "BOOT" };
    pub const Test: LogSubsys = LogSubsys { idx: 29, tag: "TEST" };
    pub const Memory: LogSubsys = LogSubsys { idx: 30, tag: "MEM" };
    pub const Dns: LogSubsys = LogSubsys { idx: 31, tag: "DNS" };
    pub const Arp: LogSubsys = LogSubsys { idx: 32, tag: "ARP" };
    pub const Icmp: LogSubsys = LogSubsys { idx: 33, tag: "ICMP" };
    pub const Seh: LogSubsys = LogSubsys { idx: 34, tag: "SEH" };
    pub const Apic: LogSubsys = LogSubsys { idx: 35, tag: "APIC" };
    pub const Hpet: LogSubsys = LogSubsys { idx: 36, tag: "HPET" };
    pub const Ioapic: LogSubsys = LogSubsys { idx: 37, tag: "IOAPIC" };
    pub const Msi: LogSubsys = LogSubsys { idx: 38, tag: "MSI" };
    pub const Ps2: LogSubsys = LogSubsys { idx: 39, tag: "PS2" };
    pub const Ata: LogSubsys = LogSubsys { idx: 40, tag: "ATA" };
    pub const Ahci: LogSubsys = LogSubsys { idx: 41, tag: "AHCI" };
    pub const Bench: LogSubsys = LogSubsys { idx: 42, tag: "BENCH" };
    pub const Init: LogSubsys = LogSubsys { idx: 43, tag: "INIT" };
    pub const User: LogSubsys = LogSubsys { idx: 44, tag: "USER" };
    pub const Storage: LogSubsys = LogSubsys { idx: 45, tag: "STORAGE" };
}

const COMPILE_TIME_LEVELS: [u8; LOG_SUBSYS_COUNT] = {
    let mut arr = [2u8; LOG_SUBSYS_COUNT];
    arr[0] = BUILD_KERNEL_LEVEL;
    arr[1] = BUILD_NET_LEVEL;
    arr[2] = BUILD_SCHED_LEVEL;
    arr[3] = BUILD_DRIVER_LEVEL;
    arr[4] = BUILD_POWER_LEVEL;
    arr[5] = BUILD_SERVICES_LEVEL;
    arr[6] = BUILD_CM_LEVEL;
    arr[7] = BUILD_OBJECT_LEVEL;
    arr[8] = BUILD_SYSCALL_LEVEL;
    arr[9] = BUILD_KBD_LEVEL;
    arr[10] = BUILD_INPUT_LEVEL;
    arr[11] = BUILD_VIRTIO_LEVEL;
    arr[12] = BUILD_TIMERS_LEVEL;
    arr[13] = BUILD_INTERRUPTS_LEVEL;
    arr[14] = BUILD_EXCEPTION_LEVEL;
    arr[15] = BUILD_SECURITY_LEVEL;
    arr[16] = BUILD_FAT32_LEVEL;
    arr[17] = BUILD_HOTRELOAD_LEVEL;
    arr[18] = BUILD_ISOLATION_LEVEL;
    arr[19] = BUILD_NEM_LEVEL;
    arr[20] = BUILD_NVME_LEVEL;
    arr[21] = BUILD_PCI_LEVEL;
    arr[22] = BUILD_SLAB_LEVEL;
    arr[23] = BUILD_ELF_LEVEL;
    arr[24] = BUILD_NXL_LEVEL;
    arr[25] = BUILD_VFS_LEVEL;
    arr[26] = BUILD_FS_LEVEL;
    arr[27] = BUILD_WATCHDOG_LEVEL;
    arr[28] = BUILD_BOOT_LEVEL;
    arr[29] = BUILD_TEST_LEVEL;
    arr[30] = BUILD_MEMORY_LEVEL;
    arr[31] = BUILD_DNS_LEVEL;
    arr[32] = BUILD_ARP_LEVEL;
    arr[33] = BUILD_ICMP_LEVEL;
    arr[34] = BUILD_SEH_LEVEL;
    arr[35] = BUILD_APIC_LEVEL;
    arr[36] = BUILD_HPET_LEVEL;
    arr[37] = BUILD_IOAPIC_LEVEL;
    arr[38] = BUILD_MSI_LEVEL;
    arr[39] = BUILD_PS2_LEVEL;
    arr[40] = BUILD_ATA_LEVEL;
    arr[41] = BUILD_AHCI_LEVEL;
    arr[42] = BUILD_BENCH_LEVEL;
    arr[43] = BUILD_INIT_LEVEL;
    arr[44] = BUILD_USER_LEVEL;
    arr[45] = BUILD_STORAGE_LEVEL;
    arr
};

lazy_static::lazy_static! {
    static ref RUNTIME_LEVELS: [AtomicU8; LOG_SUBSYS_COUNT] = {
        let mut arr: [MaybeUninit<AtomicU8>; LOG_SUBSYS_COUNT] =
            unsafe { MaybeUninit::uninit().assume_init() };
        for elem in &mut arr[..] {
            elem.write(AtomicU8::new(0xFF));
        }
        unsafe { core::mem::transmute::<_, [AtomicU8; LOG_SUBSYS_COUNT]>(arr) }
    };
}

#[inline(always)]
fn effective_level(subsys: LogSubsys) -> u8 {
    let rt = RUNTIME_LEVELS[subsys.idx].load(Ordering::Relaxed);
    if rt == 0xFF {
        COMPILE_TIME_LEVELS[subsys.idx]
    } else {
        rt
    }
}

#[inline(always)]
pub fn log_enabled(subsys: LogSubsys, level: LogLevel) -> bool {
    level as u8 <= effective_level(subsys)
}

pub fn init() {
    for i in 0..LOG_SUBSYS_COUNT {
        RUNTIME_LEVELS[i].store(0xFF, Ordering::Relaxed);
    }
}

pub fn set_level(subsys: LogSubsys, level: LogLevel) {
    RUNTIME_LEVELS[subsys.idx].store(level as u8, Ordering::Relaxed);
}

pub fn get_level(subsys: LogSubsys) -> LogLevel {
    LogLevel::from_u8(effective_level(subsys))
}

pub fn reset_level(subsys: LogSubsys) {
    RUNTIME_LEVELS[subsys.idx].store(0xFF, Ordering::Relaxed);
}

pub fn reset_all_levels() {
    for i in 0..LOG_SUBSYS_COUNT {
        RUNTIME_LEVELS[i].store(0xFF, Ordering::Relaxed);
    }
}

#[doc(hidden)]
pub fn _log(subsys: LogSubsys, level: LogLevel, args: core::fmt::Arguments) {
    let tag = subsys.tag;
    crate::serial_print!("[{}] {}: {}\r\n", tag, level.as_str(), args);
}

#[doc(hidden)]
pub fn _log_simple(subsys: LogSubsys, args: core::fmt::Arguments) {
    let tag = subsys.tag;
    crate::serial_print!("[{}] {}\r\n", tag, args);
}

#[doc(hidden)]
pub fn _log_raw(args: core::fmt::Arguments) {
    crate::serial_print!("{}\r\n", args);
}

macro_rules! kerror {
    ($subsys:expr, $($arg:tt)*) => {
        if $crate::log::log_enabled($subsys, $crate::log::LogLevel::Error) {
            $crate::log::_log($subsys, $crate::log::LogLevel::Error, format_args!($($arg)*));
        }
    };
}

macro_rules! kwarn {
    ($subsys:expr, $($arg:tt)*) => {
        if $crate::log::log_enabled($subsys, $crate::log::LogLevel::Warn) {
            $crate::log::_log($subsys, $crate::log::LogLevel::Warn, format_args!($($arg)*));
        }
    };
}

macro_rules! kinfo {
    ($subsys:expr, $($arg:tt)*) => {
        if $crate::log::log_enabled($subsys, $crate::log::LogLevel::Info) {
            $crate::log::_log_simple($subsys, format_args!($($arg)*));
        }
    };
}

macro_rules! kdebug {
    ($subsys:expr, $($arg:tt)*) => {
        if $crate::log::log_enabled($subsys, $crate::log::LogLevel::Debug) {
            $crate::log::_log($subsys, $crate::log::LogLevel::Debug, format_args!($($arg)*));
        }
    };
}

macro_rules! ktrace {
    ($subsys:expr, $($arg:tt)*) => {
        if $crate::log::log_enabled($subsys, $crate::log::LogLevel::Trace) {
            $crate::log::_log($subsys, $crate::log::LogLevel::Trace, format_args!($($arg)*));
        }
    };
}

macro_rules! klog_raw {
    ($($arg:tt)*) => {
        $crate::log::_log_raw(format_args!($($arg)*));
    };
}
