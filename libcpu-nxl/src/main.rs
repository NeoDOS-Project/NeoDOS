#![no_std]
#![no_main]

use core::arch::asm;

#[no_mangle]
pub extern "C" fn nxl_entry() -> ! {
    loop { unsafe { asm!("hlt"); } }
}

unsafe fn syscall_2(n: u64, a0: u64, a1: u64) -> u64 {
    let r: u64;
    asm!(
        "push rbx", "push rcx",
        "mov rax, {n}", "mov rbx, {a0}", "mov rcx, {a1}", "int 0x80",
        "pop rcx", "pop rbx",
        n = in(reg) n, a0 = in(reg) a0, a1 = in(reg) a1,
        out("rax") r,
    );
    r
}

#[repr(C)]
pub struct CpuInfoFull {
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

static mut CPU_INFO: CpuInfoFull = CpuInfoFull {
    vendor_id: [0u8; 12], brand: [0u8; 48],
    family: 0, model: 0, stepping: 0, cpu_type: 0,
    features_edx: 0, features_ecx: 0,
    ext_features_edx: 0, ext_features_ecx: 0,
    features_ebx_leaf7: 0,
    phys_addr_bits: 0, virt_addr_bits: 0,
    cpu_count: 0, apic_id: 0, cpu_id: 0, is_bsp: false,
    tsc_khz: 0, timer_source: 0, tick_rate_hz: 0,
};
static mut INITIALIZED: bool = false;

fn ensure_initialized() {
    unsafe {
        if !INITIALIZED {
            let buf_ptr = &mut CPU_INFO as *mut CpuInfoFull as *mut u8;
            let buf_len = core::mem::size_of::<CpuInfoFull>();
            syscall_2(24, buf_ptr as u64, buf_len as u64);
            INITIALIZED = true;
        }
    }
}

fn has_bit(value: u32, bit: u32) -> bool {
    (value >> bit) & 1 == 1
}

#[no_mangle] pub extern "C" fn cpuinfo_always_true() -> bool { true }

// ── Exported functions ──

#[no_mangle]
pub extern "C" fn cpuinfo_get_info() -> *const CpuInfoFull {
    ensure_initialized();
    unsafe { &CPU_INFO as *const CpuInfoFull }
}

#[no_mangle]
pub extern "C" fn cpuinfo_get_vendor() -> *const u8 {
    ensure_initialized();
    unsafe { CPU_INFO.vendor_id.as_ptr() }
}

#[no_mangle]
pub extern "C" fn cpuinfo_get_brand() -> *const u8 {
    ensure_initialized();
    unsafe { CPU_INFO.brand.as_ptr() }
}

#[no_mangle] pub extern "C" fn cpuinfo_get_family() -> u32 { ensure_initialized(); unsafe { CPU_INFO.family } }
#[no_mangle] pub extern "C" fn cpuinfo_get_model() -> u32 { ensure_initialized(); unsafe { CPU_INFO.model } }
#[no_mangle] pub extern "C" fn cpuinfo_get_stepping() -> u32 { ensure_initialized(); unsafe { CPU_INFO.stepping } }
#[no_mangle] pub extern "C" fn cpuinfo_get_type() -> u32 { ensure_initialized(); unsafe { CPU_INFO.cpu_type } }

// ── Feature queries ──

#[no_mangle] pub extern "C" fn cpuinfo_has_sse() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_edx, 25) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_sse2() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_edx, 26) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_sse3() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_ecx, 0) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_ssse3() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_ecx, 9) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_sse41() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_ecx, 19) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_sse42() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_ecx, 20) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_avx() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_ecx, 28) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_avx2() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_ebx_leaf7, 5) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_aes() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_ecx, 25) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_fma() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_ecx, 12) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_f16c() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_ecx, 29) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_popcnt() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_ecx, 23) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_xsave() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_ecx, 26) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_rdrand() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_ecx, 30) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_pclmulqdq() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_ecx, 1) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_fsgsbase() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_ebx_leaf7, 0) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_bmi1() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_ebx_leaf7, 3) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_bmi2() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_ebx_leaf7, 8) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_hle() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_ebx_leaf7, 4) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_rtm() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_ebx_leaf7, 11) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_smep() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_ebx_leaf7, 7) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_erms() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_ebx_leaf7, 9) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_x2apic() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_ecx, 21) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_htt() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_edx, 28) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_nx() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.ext_features_edx, 20) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_long_mode() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.ext_features_edx, 29) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_syscall() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.ext_features_edx, 11) } }
#[no_mangle] pub extern "C" fn cpuinfo_has_mmx() -> bool { ensure_initialized(); unsafe { has_bit(CPU_INFO.features_edx, 23) } }

// ── Topology ──

#[no_mangle] pub extern "C" fn cpuinfo_get_cpu_count() -> u32 { ensure_initialized(); unsafe { CPU_INFO.cpu_count } }
#[no_mangle] pub extern "C" fn cpuinfo_get_apic_id() -> u32 { ensure_initialized(); unsafe { CPU_INFO.apic_id } }
#[no_mangle] pub extern "C" fn cpuinfo_is_bsp() -> bool { ensure_initialized(); unsafe { CPU_INFO.is_bsp } }

// ── Timer ──

#[no_mangle] pub extern "C" fn cpuinfo_get_tsc_khz() -> u64 { ensure_initialized(); unsafe { CPU_INFO.tsc_khz } }
#[no_mangle] pub extern "C" fn cpuinfo_get_timer_source() -> u8 { ensure_initialized(); unsafe { CPU_INFO.timer_source } }
#[no_mangle] pub extern "C" fn cpuinfo_get_tick_rate_hz() -> u64 { ensure_initialized(); unsafe { CPU_INFO.tick_rate_hz } }

// ── Addressing ──

#[no_mangle] pub extern "C" fn cpuinfo_get_phys_addr_bits() -> u8 { ensure_initialized(); unsafe { CPU_INFO.phys_addr_bits } }
#[no_mangle] pub extern "C" fn cpuinfo_get_virt_addr_bits() -> u8 { ensure_initialized(); unsafe { CPU_INFO.virt_addr_bits } }

// ── Feature name table ──

static FEATURES: &[(&[u8], extern "C" fn() -> bool)] = &[
    (b"FPU\0",        cpuinfo_always_true),
    (b"TSC\0",        cpuinfo_always_true),
    (b"MMX\0",        cpuinfo_has_mmx),
    (b"SSE\0",        cpuinfo_has_sse),
    (b"SSE2\0",       cpuinfo_has_sse2),
    (b"SSE3\0",       cpuinfo_has_sse3),
    (b"SSSE3\0",      cpuinfo_has_ssse3),
    (b"SSE4.1\0",     cpuinfo_has_sse41),
    (b"SSE4.2\0",     cpuinfo_has_sse42),
    (b"AVX\0",        cpuinfo_has_avx),
    (b"AVX2\0",       cpuinfo_has_avx2),
    (b"AES-NI\0",     cpuinfo_has_aes),
    (b"FMA\0",        cpuinfo_has_fma),
    (b"F16C\0",       cpuinfo_has_f16c),
    (b"POPCNT\0",     cpuinfo_has_popcnt),
    (b"XSAVE\0",      cpuinfo_has_xsave),
    (b"RDRAND\0",     cpuinfo_has_rdrand),
    (b"CLMUL\0",      cpuinfo_has_pclmulqdq),
    (b"FSGSBASE\0",   cpuinfo_has_fsgsbase),
    (b"BMI1\0",       cpuinfo_has_bmi1),
    (b"BMI2\0",       cpuinfo_has_bmi2),
    (b"HLE\0",        cpuinfo_has_hle),
    (b"RTM\0",        cpuinfo_has_rtm),
    (b"SMEP\0",       cpuinfo_has_smep),
    (b"ERMS\0",       cpuinfo_has_erms),
    (b"X2APIC\0",     cpuinfo_has_x2apic),
    (b"HTT\0",        cpuinfo_has_htt),
    (b"NX\0",         cpuinfo_has_nx),
    (b"LONG_MODE\0",  cpuinfo_has_long_mode),
    (b"SYSCALL\0",    cpuinfo_has_syscall),
];

#[no_mangle]
pub extern "C" fn cpuinfo_feature_count() -> u32 {
    FEATURES.len() as u32
}

#[no_mangle]
pub extern "C" fn cpuinfo_feature_name(index: u32) -> *const u8 {
    if (index as usize) < FEATURES.len() {
        let bytes: &[u8] = FEATURES[index as usize].0;
        bytes.as_ptr()
    } else {
        core::ptr::null()
    }
}

#[no_mangle]
pub extern "C" fn cpuinfo_feature_enabled(index: u32) -> bool {
    if (index as usize) < FEATURES.len() {
        FEATURES[index as usize].1()
    } else {
        false
    }
}

// ── Timer source name ──

static TIMER_NAMES: &[&[u8]] = &[b"PIT\0", b"HPET\0", b"APIC Timer\0"];

#[no_mangle]
pub extern "C" fn cpuinfo_timer_source_name() -> *const u8 {
    let idx = unsafe { CPU_INFO.timer_source } as usize;
    if idx < TIMER_NAMES.len() {
        let bytes: &[u8] = TIMER_NAMES[idx];
        bytes.as_ptr()
    } else {
        b"Unknown\0".as_ptr()
    }
}

// ── CPU type name ──

#[no_mangle]
pub extern "C" fn cpuinfo_type_name() -> *const u8 {
    let t = unsafe { CPU_INFO.cpu_type };
    match t {
        0 => b"Reserved\0".as_ptr(),
        1 => b"Other\0".as_ptr(),
        2 => b"Unknown\0".as_ptr(),
        3 => b"Normal\0".as_ptr(),
        _ => b"Unknown\0".as_ptr(),
    }
}

// ============================================================
// Export Table — placed in .export_table section at known offset
// ============================================================
#[repr(C)]
pub struct CpuInfoAbiTable {
    pub version: u32,
    pub get_vendor: extern "C" fn() -> *const u8,
    pub get_brand: extern "C" fn() -> *const u8,
    pub get_family: extern "C" fn() -> u32,
    pub get_model: extern "C" fn() -> u32,
    pub get_stepping: extern "C" fn() -> u32,
    pub get_type: extern "C" fn() -> u32,
    pub has_sse: extern "C" fn() -> bool,
    pub has_sse2: extern "C" fn() -> bool,
    pub has_sse3: extern "C" fn() -> bool,
    pub has_ssse3: extern "C" fn() -> bool,
    pub has_sse41: extern "C" fn() -> bool,
    pub has_sse42: extern "C" fn() -> bool,
    pub has_avx: extern "C" fn() -> bool,
    pub has_avx2: extern "C" fn() -> bool,
    pub has_aes: extern "C" fn() -> bool,
    pub has_fma: extern "C" fn() -> bool,
    pub has_f16c: extern "C" fn() -> bool,
    pub has_popcnt: extern "C" fn() -> bool,
    pub has_xsave: extern "C" fn() -> bool,
    pub has_rdrand: extern "C" fn() -> bool,
    pub has_pclmulqdq: extern "C" fn() -> bool,
    pub has_fsgsbase: extern "C" fn() -> bool,
    pub has_bmi1: extern "C" fn() -> bool,
    pub has_bmi2: extern "C" fn() -> bool,
    pub has_hle: extern "C" fn() -> bool,
    pub has_rtm: extern "C" fn() -> bool,
    pub has_smep: extern "C" fn() -> bool,
    pub has_erms: extern "C" fn() -> bool,
    pub has_x2apic: extern "C" fn() -> bool,
    pub has_htt: extern "C" fn() -> bool,
    pub has_nx: extern "C" fn() -> bool,
    pub has_long_mode: extern "C" fn() -> bool,
    pub has_syscall: extern "C" fn() -> bool,
    pub has_mmx: extern "C" fn() -> bool,
    pub get_cpu_count: extern "C" fn() -> u32,
    pub get_apic_id: extern "C" fn() -> u32,
    pub is_bsp: extern "C" fn() -> bool,
    pub get_tsc_khz: extern "C" fn() -> u64,
    pub get_timer_source: extern "C" fn() -> u8,
    pub get_tick_rate_hz: extern "C" fn() -> u64,
    pub get_phys_addr_bits: extern "C" fn() -> u8,
    pub get_virt_addr_bits: extern "C" fn() -> u8,
    pub feature_count: extern "C" fn() -> u32,
    pub feature_name: extern "C" fn(u32) -> *const u8,
    pub feature_enabled: extern "C" fn(u32) -> bool,
    pub timer_source_name: extern "C" fn() -> *const u8,
    pub type_name: extern "C" fn() -> *const u8,
    pub get_info: extern "C" fn() -> *const CpuInfoFull,
    pub _reserved: [u64; 8],
}

#[no_mangle]
#[link_section = ".export_table"]
pub static CPUINFO_EXPORT_TABLE: CpuInfoAbiTable = CpuInfoAbiTable {
    version: 1,
    get_vendor: cpuinfo_get_vendor,
    get_brand: cpuinfo_get_brand,
    get_family: cpuinfo_get_family,
    get_model: cpuinfo_get_model,
    get_stepping: cpuinfo_get_stepping,
    get_type: cpuinfo_get_type,
    has_sse: cpuinfo_has_sse,
    has_sse2: cpuinfo_has_sse2,
    has_sse3: cpuinfo_has_sse3,
    has_ssse3: cpuinfo_has_ssse3,
    has_sse41: cpuinfo_has_sse41,
    has_sse42: cpuinfo_has_sse42,
    has_avx: cpuinfo_has_avx,
    has_avx2: cpuinfo_has_avx2,
    has_aes: cpuinfo_has_aes,
    has_fma: cpuinfo_has_fma,
    has_f16c: cpuinfo_has_f16c,
    has_popcnt: cpuinfo_has_popcnt,
    has_xsave: cpuinfo_has_xsave,
    has_rdrand: cpuinfo_has_rdrand,
    has_pclmulqdq: cpuinfo_has_pclmulqdq,
    has_fsgsbase: cpuinfo_has_fsgsbase,
    has_bmi1: cpuinfo_has_bmi1,
    has_bmi2: cpuinfo_has_bmi2,
    has_hle: cpuinfo_has_hle,
    has_rtm: cpuinfo_has_rtm,
    has_smep: cpuinfo_has_smep,
    has_erms: cpuinfo_has_erms,
    has_x2apic: cpuinfo_has_x2apic,
    has_htt: cpuinfo_has_htt,
    has_nx: cpuinfo_has_nx,
    has_long_mode: cpuinfo_has_long_mode,
    has_syscall: cpuinfo_has_syscall,
    has_mmx: cpuinfo_has_mmx,
    get_cpu_count: cpuinfo_get_cpu_count,
    get_apic_id: cpuinfo_get_apic_id,
    is_bsp: cpuinfo_is_bsp,
    get_tsc_khz: cpuinfo_get_tsc_khz,
    get_timer_source: cpuinfo_get_timer_source,
    get_tick_rate_hz: cpuinfo_get_tick_rate_hz,
    get_phys_addr_bits: cpuinfo_get_phys_addr_bits,
    get_virt_addr_bits: cpuinfo_get_virt_addr_bits,
    feature_count: cpuinfo_feature_count,
    feature_name: cpuinfo_feature_name,
    feature_enabled: cpuinfo_feature_enabled,
    timer_source_name: cpuinfo_timer_source_name,
    type_name: cpuinfo_type_name,
    get_info: cpuinfo_get_info,
    _reserved: [0; 8],
};

// ── Panic handler ──

#[panic_handler]
fn nxl_panic(_info: &core::panic::PanicInfo) -> ! {
    loop { unsafe { asm!("hlt"); } }
}
