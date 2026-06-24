#![no_std]
#![no_main]

use libneodos::println;
use libneodos::syscall::{self, ob_access, ObInfoClass, sys_ob_open, sys_ob_query_info, sys_close};
use core::mem::size_of;

#[repr(C)]
struct CpuInfoFull {
    pub vendor_id: [u8; 12],
    pub brand: [u8; 48],
    pub family: u32,
    pub model: u32,
    pub stepping: u32,
    pub cpu_type: u32,
    pub features_edx: u32,
    pub features_ecx: u32,
    pub ext_features_edx: u32,
    pub ext_features_ecx: u32,
    pub features_ebx_leaf7: u32,
    pub phys_addr_bits: u8,
    pub virt_addr_bits: u8,
    pub cpu_count: u32,
    pub apic_id: u32,
    pub cpu_id: u32,
    pub is_bsp: bool,
    pub tsc_khz: u64,
    pub timer_source: u8,
    pub tick_rate_hz: u64,
}

impl CpuInfoFull {
    fn vendor_str(&self) -> &str {
        let end = self.vendor_id.iter().position(|&b| b == 0).unwrap_or(12);
        core::str::from_utf8(&self.vendor_id[..end]).unwrap_or("Unknown")
    }
    fn brand_str(&self) -> &str {
        let mut end = self.brand.len();
        while end > 0 && (self.brand[end - 1] == 0 || self.brand[end - 1] == b' ') { end -= 1; }
        core::str::from_utf8(&self.brand[..end]).unwrap_or("Unknown")
    }
    fn has_feature(&self, feat: &str) -> bool {
        match feat {
            "SSE"    => (self.features_edx >> 25) & 1 == 1,
            "SSE2"   => (self.features_edx >> 26) & 1 == 1,
            "SSE3"   => (self.features_ecx >> 0) & 1 == 1,
            "SSSE3"  => (self.features_ecx >> 9) & 1 == 1,
            "SSE4.1" => (self.features_ecx >> 19) & 1 == 1,
            "SSE4.2" => (self.features_ecx >> 20) & 1 == 1,
            "AVX"    => (self.features_ecx >> 28) & 1 == 1,
            "AVX2"   => (self.features_ebx_leaf7 >> 5) & 1 == 1,
            "AES"    => (self.features_ecx >> 25) & 1 == 1,
            "FMA"    => (self.features_ecx >> 12) & 1 == 1,
            "POPCNT" => (self.features_ecx >> 23) & 1 == 1,
            "RDRAND" => (self.features_ecx >> 30) & 1 == 1,
            "NX"     => (self.ext_features_edx >> 20) & 1 == 1,
            "x86-64" => (self.ext_features_edx >> 29) & 1 == 1,
            "SYSCALL"=> (self.ext_features_edx >> 11) & 1 == 1,
            "MMX"    => (self.features_edx >> 23) & 1 == 1,
            _ => false,
        }
    }
}

fn timer_name(source: u8) -> &'static str {
    match source { 0 => "PIT", 1 => "HPET", 2 => "APIC Timer", _ => "Unknown" }
}

fn cpu_type_name(t: u32) -> &'static str {
    match t { 0 => "Reserved", 1 => "Other", 2 => "Unknown", 3 => "Normal", _ => "Unknown" }
}

#[used]
#[link_section = ".rodata"]
static CPUINFO_HELP: &[u8] = b"::HELP::\
CPUINFO\r\n\
  Displays CPU information: vendor, brand, features, topology, timers.\r\n\
::END::";

fn print_help() {
    libneodos::println!("CPUINFO");
    libneodos::println!("  Displays CPU information: vendor, brand, features, topology, timers.");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    if libneodos::args::is_help_flag(&libneodos::args::read_args()) {
        print_help();
        syscall::sys_exit(0);
    }

    let fd = match sys_ob_open("\\Global\\Info\\CpuInfo", ob_access::READ) {
        Ok(f) => f,
        Err(_) => {
            libneodos::println!("CPU info not available");
            syscall::sys_exit(1);
        }
    };

    let mut info: CpuInfoFull = unsafe { core::mem::zeroed() };
    let buf = unsafe {
        core::slice::from_raw_parts_mut(&mut info as *mut CpuInfoFull as *mut u8, size_of::<CpuInfoFull>())
    };
    let n = match sys_ob_query_info(fd, ObInfoClass::Cpu, buf) {
        Ok(n) => n,
        Err(_) => {
            let _ = sys_close(fd);
            libneodos::println!("CPU info query failed");
            syscall::sys_exit(1);
        }
    };
    let _ = sys_close(fd);

    if n < size_of::<CpuInfoFull>() {
        libneodos::println!("CPU info truncated");
        syscall::sys_exit(1);
    }

    println!("======================================================");
    println!("              CPU INFORMATION");
    println!("======================================================");
    println!("Vendor:     {}", info.vendor_str());
    println!("Brand:      {}", info.brand_str());
    println!("Family: {}  Model: {}  Stepping: {}",
        info.family, info.model, info.stepping);
    println!("Type: {} ({})", info.cpu_type, cpu_type_name(info.cpu_type));
    println!("------------------------------------------------------");
    println!("TOPOLOGY");
    println!("  Logical CPUs:  {}", info.cpu_count);
    println!("  APIC ID:       {}", info.apic_id);
    println!("  BSP:           {}", if info.is_bsp { "Yes" } else { "No" });
    println!("------------------------------------------------------");
    println!("ADDRESSING");
    println!("  Physical:  {} bits", info.phys_addr_bits);
    println!("  Virtual:   {} bits", info.virt_addr_bits);
    println!("------------------------------------------------------");
    println!("TIMERS");
    if info.tsc_khz >= 1_000_000 {
        let ghz = info.tsc_khz / 1_000_000;
        let mhz = (info.tsc_khz % 1_000_000) / 1000;
        println!("  TSC:       {}.{:03} GHz", ghz, mhz);
    } else {
        println!("  TSC:       {} KHz", info.tsc_khz);
    }
    println!("  Source:    {}", timer_name(info.timer_source));
    println!("  Tick rate: {} Hz", info.tick_rate_hz);
    println!("------------------------------------------------------");
    println!("FEATURES");

    let features = ["MMX", "SSE", "SSE2", "SSE3", "SSSE3", "SSE4.1", "SSE4.2",
        "AVX", "AVX2", "AES", "FMA", "POPCNT", "RDRAND", "NX", "x86-64", "SYSCALL"];
    let mut detected = 0u32;
    for &f in &features {
        if info.has_feature(f) {
            println!("  {}", f);
            detected += 1;
        }
    }

    println!();
    println!("  {} features detected", detected);
    println!("======================================================");

    syscall::sys_exit(0)
}
