//! CPU identification via the `CPUID` instruction.
//!
//! We read:
//! - the vendor string (`CPUID(0)`),
//! - the brand string (`CPUID(0x8000_0002..=0x8000_0004)`) when supported,
//! - feature flags (`CPUID(1)`, `CPUID(7)`, `CPUID(0x8000_0001)`),
//! - address width (`CPUID(0x8000_0008)`).
//!
//! Note: On some toolchains/targets LLVM may reserve `RBX` (e.g. PIC codegen),
//! so the low-level `cpuid()` wrapper saves/restores `rbx` manually.

use crate::arch::x64::cpu_local;
use crate::arch::x64::smp;
use crate::timers;

// ── Legacy CpuInfo (backward compatible) ────────────────────────────

pub struct CpuInfo {
    pub vendor_id: [u8; 12],
    pub brand: [u8; 48],
}

impl CpuInfo {
    pub fn vendor_str(&self) -> &str {
        core::str::from_utf8(&self.vendor_id).unwrap_or("Unknown")
    }

    pub fn brand_str(&self) -> &str {
        let mut end = self.brand.len();
        while end > 0 && (self.brand[end - 1] == 0 || self.brand[end - 1] == b' ') {
            end -= 1;
        }
        core::str::from_utf8(&self.brand[..end]).unwrap_or("Unknown")
    }
}

pub fn get_cpu_info() -> CpuInfo {
    let mut vendor_id = [0u8; 12];
    let mut brand = [0u8; 48];

    #[cfg(target_arch = "x86_64")]
    {
        let (_max_basic, ebx, ecx, edx) = cpuid(0, 0);
        vendor_id[0..4].copy_from_slice(&ebx.to_le_bytes());
        vendor_id[4..8].copy_from_slice(&edx.to_le_bytes());
        vendor_id[8..12].copy_from_slice(&ecx.to_le_bytes());

        let (max_ext, _, _, _) = cpuid(0x8000_0000, 0);
        if max_ext >= 0x8000_0004 {
            for (i, leaf) in (0x8000_0002u32..=0x8000_0004u32).enumerate() {
                let (a, b, c, d) = cpuid(leaf, 0);
                let off = i * 16;
                brand[off + 0..off + 4].copy_from_slice(&a.to_le_bytes());
                brand[off + 4..off + 8].copy_from_slice(&b.to_le_bytes());
                brand[off + 8..off + 12].copy_from_slice(&c.to_le_bytes());
                brand[off + 12..off + 16].copy_from_slice(&d.to_le_bytes());
            }
        }
    }

    CpuInfo { vendor_id, brand }
}

// ── Full CpuInfo (for sys_getcpuinfo) ───────────────────────────────

/// Complete CPU information structure exposed to user-mode via sys_getcpuinfo.
/// Layout is #[repr(C)] for stable ABI across kernel/user boundary.
#[repr(C)]
pub struct CpuInfoFull {
    // ── Identity ──
    pub vendor_id: [u8; 12],
    pub brand: [u8; 48],
    pub family: u32,
    pub model: u32,
    pub stepping: u32,
    pub cpu_type: u32,

    // ── Feature flags ──
    pub features_edx: u32,
    pub features_ecx: u32,
    pub ext_features_edx: u32,
    pub ext_features_ecx: u32,
    pub features_ebx_leaf7: u32,

    // ── Addressing ──
    pub phys_addr_bits: u8,
    pub virt_addr_bits: u8,

    // ── SMP / Topology ──
    pub cpu_count: u32,
    pub apic_id: u32,
    pub cpu_id: u32,
    pub is_bsp: bool,

    // ── Timer / Frequency ──
    pub tsc_khz: u64,
    pub timer_source: u8,
    pub tick_rate_hz: u64,
}

impl CpuInfoFull {
    pub fn vendor_str(&self) -> &str {
        core::str::from_utf8(&self.vendor_id).unwrap_or("Unknown")
    }

    pub fn brand_str(&self) -> &str {
        let mut end = self.brand.len();
        while end > 0 && (self.brand[end - 1] == 0 || self.brand[end - 1] == b' ') {
            end -= 1;
        }
        core::str::from_utf8(&self.brand[..end]).unwrap_or("Unknown")
    }

    /// Decode CPU type field (CPUID leaf 1, EAX bits 12-13) into a human-readable string.
    pub fn cpu_type_str(&self) -> &'static str {
        match self.cpu_type {
            0 => "Reserved (overclocked)",
            1 => "Other",
            2 => "Unknown",
            3 => "Normal desktop/mobile",
            _ => "Unknown",
        }
    }
}

/// Collect all CPU information into a CpuInfoFull struct.
pub fn get_cpu_info_full() -> CpuInfoFull {
    let mut info = CpuInfoFull {
        vendor_id: [0u8; 12],
        brand: [0u8; 48],
        family: 0,
        model: 0,
        stepping: 0,
        cpu_type: 0,
        features_edx: 0,
        features_ecx: 0,
        ext_features_edx: 0,
        ext_features_ecx: 0,
        features_ebx_leaf7: 0,
        phys_addr_bits: 0,
        virt_addr_bits: 0,
        cpu_count: 0,
        apic_id: 0,
        cpu_id: 0,
        is_bsp: false,
        tsc_khz: 0,
        timer_source: 0,
        tick_rate_hz: 0,
    };

    #[cfg(target_arch = "x86_64")]
    {
        // CPUID(0): vendor string + max basic leaf
        let (max_basic, ebx, ecx, edx) = cpuid(0, 0);
        info.vendor_id[0..4].copy_from_slice(&ebx.to_le_bytes());
        info.vendor_id[4..8].copy_from_slice(&edx.to_le_bytes());
        info.vendor_id[8..12].copy_from_slice(&ecx.to_le_bytes());

        // CPUID(1): family, model, stepping, type, features
        if max_basic >= 1 {
            let (eax, _ebx1, ecx1, edx1) = cpuid(1, 0);

            let base_family = (eax >> 8) & 0xF;
            let base_model = (eax >> 4) & 0xF;
            let ext_family = (eax >> 20) & 0xFF;
            let ext_model = (eax >> 16) & 0xF;

            // Intel: if family==0xF, family += ext_family; else family = base_family
            // AMD:   if family==0xF, family += ext_family; else family = base_family
            info.family = if base_family == 0xF {
                base_family + ext_family
            } else {
                base_family
            };

            // Model: if family==0xF || family==6, model |= ext_model<<4; else model = base_model
            info.model = if base_family == 0xF || base_family == 6 {
                base_model | (ext_model << 4)
            } else {
                base_model
            };

            info.stepping = eax & 0xF;
            info.cpu_type = (eax >> 12) & 0x3;

            info.features_edx = edx1;
            info.features_ecx = ecx1;
        }

        // CPUID(7,0): structured extended features (AVX2, BMI, etc.)
        if max_basic >= 7 {
            let (_, ebx7, _, _) = cpuid(7, 0);
            info.features_ebx_leaf7 = ebx7;
        }

        // CPUID(0x80000000): max extended leaf
        let (max_ext, _, _, _) = cpuid(0x8000_0000, 0);

        // CPUID(0x80000001): extended features (SYSCALL, NX, long mode)
        if max_ext >= 0x8000_0001 {
            let (_, _, ecx_ext, edx_ext) = cpuid(0x8000_0001, 0);
            info.ext_features_edx = edx_ext;
            info.ext_features_ecx = ecx_ext;
        }

        // CPUID(0x80000002..=0x80000004): brand string
        if max_ext >= 0x8000_0004 {
            for (i, leaf) in (0x8000_0002u32..=0x8000_0004u32).enumerate() {
                let (a, b, c, d) = cpuid(leaf, 0);
                let off = i * 16;
                info.brand[off + 0..off + 4].copy_from_slice(&a.to_le_bytes());
                info.brand[off + 4..off + 8].copy_from_slice(&b.to_le_bytes());
                info.brand[off + 8..off + 12].copy_from_slice(&c.to_le_bytes());
                info.brand[off + 12..off + 16].copy_from_slice(&d.to_le_bytes());
            }
        }

        // CPUID(0x80000008): address width
        if max_ext >= 0x8000_0008 {
            let (eax_addr, _, _, _) = cpuid(0x8000_0008, 0);
            info.phys_addr_bits = (eax_addr & 0xFF) as u8;
            info.virt_addr_bits = ((eax_addr >> 8) & 0xFF) as u8;
        }
    }

    // SMP / topology
    info.cpu_count = cpu_local::cpu_count();
    unsafe {
        info.apic_id = cpu_local::this_cpu_apic_id();
        info.cpu_id = cpu_local::this_cpu_id();
    }
    info.is_bsp = smp::is_bsp();

    // Timer / frequency
    info.tsc_khz = crate::boot_benchmark::get_tsc_khz();
    info.timer_source = timers::active() as u8;
    info.tick_rate_hz = crate::hal::get_tick_rate();

    info
}

// ── Low-level CPUID wrapper ─────────────────────────────────────────

#[cfg(target_arch = "x86_64")]
fn cpuid(leaf: u32, subleaf: u32) -> (u32, u32, u32, u32) {
    let mut eax = leaf;
    let mut ecx = subleaf;
    let ebx: u32;
    let edx: u32;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "cpuid",
            "mov {ebx_out:e}, ebx",
            "pop rbx",
            inout("eax") eax,
            inout("ecx") ecx,
            ebx_out = lateout(reg) ebx,
            lateout("edx") edx,
            options(nomem),
        );
    }
    (eax, ebx, ecx, edx)
}
