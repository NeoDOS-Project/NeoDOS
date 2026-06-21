#![no_std]
#![no_main]

use libneodos::println;
use libneodos::syscall;

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

#[repr(C)]
struct CpuInfoAbiTable {
    version: u32,
    get_vendor: extern "C" fn() -> *const u8,
    get_brand: extern "C" fn() -> *const u8,
    get_family: extern "C" fn() -> u32,
    get_model: extern "C" fn() -> u32,
    get_stepping: extern "C" fn() -> u32,
    get_type: extern "C" fn() -> u32,
    has_sse: extern "C" fn() -> bool,
    has_sse2: extern "C" fn() -> bool,
    has_sse3: extern "C" fn() -> bool,
    has_ssse3: extern "C" fn() -> bool,
    has_sse41: extern "C" fn() -> bool,
    has_sse42: extern "C" fn() -> bool,
    has_avx: extern "C" fn() -> bool,
    has_avx2: extern "C" fn() -> bool,
    has_aes: extern "C" fn() -> bool,
    has_fma: extern "C" fn() -> bool,
    has_f16c: extern "C" fn() -> bool,
    has_popcnt: extern "C" fn() -> bool,
    has_xsave: extern "C" fn() -> bool,
    has_rdrand: extern "C" fn() -> bool,
    has_pclmulqdq: extern "C" fn() -> bool,
    has_fsgsbase: extern "C" fn() -> bool,
    has_bmi1: extern "C" fn() -> bool,
    has_bmi2: extern "C" fn() -> bool,
    has_hle: extern "C" fn() -> bool,
    has_rtm: extern "C" fn() -> bool,
    has_smep: extern "C" fn() -> bool,
    has_erms: extern "C" fn() -> bool,
    has_x2apic: extern "C" fn() -> bool,
    has_htt: extern "C" fn() -> bool,
    has_nx: extern "C" fn() -> bool,
    has_long_mode: extern "C" fn() -> bool,
    has_syscall: extern "C" fn() -> bool,
    has_mmx: extern "C" fn() -> bool,
    get_cpu_count: extern "C" fn() -> u32,
    get_apic_id: extern "C" fn() -> u32,
    is_bsp: extern "C" fn() -> bool,
    get_tsc_khz: extern "C" fn() -> u64,
    get_timer_source: extern "C" fn() -> u8,
    get_tick_rate_hz: extern "C" fn() -> u64,
    get_phys_addr_bits: extern "C" fn() -> u8,
    get_virt_addr_bits: extern "C" fn() -> u8,
    feature_count: extern "C" fn() -> u32,
    feature_name: extern "C" fn(u32) -> *const u8,
    feature_enabled: extern "C" fn(u32) -> bool,
    timer_source_name: extern "C" fn() -> *const u8,
    type_name: extern "C" fn() -> *const u8,
    get_info: extern "C" fn() -> *const CpuInfoFull,
    _reserved: [u64; 8],
}

fn trim_fixed(s: *const u8, max_len: usize) -> &'static str {
    let slice = unsafe { core::slice::from_raw_parts(s, max_len) };
    let end = slice.iter().position(|&b| b == 0).unwrap_or(max_len);
    let valid = &slice[..end];
    let trimmed = valid.iter().rev().position(|&b| b != b' ').map(|i| &valid[..end - i]).unwrap_or(valid);
    core::str::from_utf8(trimmed).unwrap_or("")
}

fn trim_cstr(s: *const u8) -> &'static str {
    let mut len = 0usize;
    unsafe {
        while *s.add(len) != 0 { len += 1; }
    }
    unsafe { core::str::from_utf8(core::slice::from_raw_parts(s, len)).unwrap_or("") }
}

fn timer_name(source: u8) -> &'static str {
    match source {
        0 => "PIT",
        1 => "HPET",
        2 => "APIC Timer",
        _ => "Unknown",
    }
}

fn type_name_str(t: u32) -> &'static str {
    match t {
        0 => "Reserved (overclocked)",
        1 => "Other",
        2 => "Unknown",
        3 => "Normal desktop/mobile",
        _ => "Unknown",
    }
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
    libneodos::println!("  Requires cpuinfo.nxl to be loaded.");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    if libneodos::args::is_help_flag(&libneodos::args::read_args()) {
        print_help();
        syscall::sys_exit(0);
    }

    println!("======================================================");
    println!("              CPU INFORMATION");
    println!("======================================================");

    println!("[TEST] Loading cpuinfo.nxl...");

    let base = match libneodos::syscall::sys_loadlib("C:\\System\\Libraries\\cpuinfo.nxl") {
        Ok(b) => b,
        Err(e) => {
            println!("[FAIL] sys_loadlib error code {}", e);
            println!();
            println!("RESULT: FAILURE");
            println!("FAILED AT: LOAD TEST");
            println!("REASON: Could not load cpuinfo.nxl");
            libneodos::syscall::sys_exit(1)
        }
    };
    println!("[OK] cpuinfo.nxl loaded at 0x{:x}", base);

    let table = unsafe { &*(base as *const CpuInfoAbiTable) };

    if table.version != 1 {
        println!("[FAIL] ABI version mismatch: expected 1, got {}", table.version);
        libneodos::syscall::sys_exit(1)
    }

    println!();

    let vendor = trim_fixed((table.get_vendor)(), 12);
    let brand = trim_fixed((table.get_brand)(), 48);
    let family = (table.get_family)();
    let model = (table.get_model)();
    let stepping = (table.get_stepping)();
    let cpu_type = (table.get_type)();
    let cpu_count = (table.get_cpu_count)();
    let apic_id = (table.get_apic_id)();
    let bsp = (table.is_bsp)();
    let phys_bits = (table.get_phys_addr_bits)();
    let virt_bits = (table.get_virt_addr_bits)();
    let tsc = (table.get_tsc_khz)();
    let timer_src = (table.get_timer_source)();
    let tick_rate = (table.get_tick_rate_hz)();

    println!("Vendor:     {}", vendor);
    println!("Brand:      {}", brand);
    println!("Family: {}  Model: {}  Stepping: {}", family, model, stepping);
    println!("Type: {} ({})", cpu_type, type_name_str(cpu_type));

    println!("------------------------------------------------------");
    println!("TOPOLOGY");
    println!("  Logical CPUs:  {}", cpu_count);
    println!("  APIC ID:       {}", apic_id);
    println!("  BSP:           {}", if bsp { "Yes" } else { "No" });

    println!("------------------------------------------------------");
    println!("ADDRESSING");
    println!("  Physical:  {} bits", phys_bits);
    println!("  Virtual:   {} bits", virt_bits);

    println!("------------------------------------------------------");
    println!("TIMERS");
    if tsc >= 1_000_000 {
        let ghz = tsc / 1_000_000;
        let mhz = (tsc % 1_000_000) / 1000;
        println!("  TSC:       {}.{:03} GHz", ghz, mhz);
    } else {
        println!("  TSC:       {} KHz", tsc);
    }
    println!("  Source:    {}", timer_name(timer_src));
    println!("  Tick rate: {} Hz", tick_rate);

    println!("------------------------------------------------------");
    println!("FEATURES");

    let count = (table.feature_count)();
    let mut detected = 0u32;
    for i in 0..count {
        let enabled = (table.feature_enabled)(i);
        let name_ptr = (table.feature_name)(i);
        let name = trim_cstr(name_ptr);
        if enabled {
            println!("  {}", name);
            detected += 1;
        }
    }

    println!();
    println!("  {} features detected", detected);

    println!("======================================================");

    libneodos::syscall::sys_exit(0)
}