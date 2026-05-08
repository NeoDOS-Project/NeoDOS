//! CPU identification via the `CPUID` instruction.
//!
//! We read:
//! - the vendor string (`CPUID(0)`),
//! - the brand string (`CPUID(0x8000_0002..=0x8000_0004)`) when supported.
//!
//! Note: On some toolchains/targets LLVM may reserve `RBX` (e.g. PIC codegen),
//! so the low-level `cpuid()` wrapper saves/restores `rbx` manually.

pub struct CpuInfo {
    pub vendor_id: [u8; 12],
    pub brand: [u8; 48],
}

impl CpuInfo {
    pub fn vendor_str(&self) -> &str {
        core::str::from_utf8(&self.vendor_id).unwrap_or("Unknown")
    }

    pub fn brand_str(&self) -> &str {
        // Brand strings are usually NUL-padded; trim trailing NULs/spaces.
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
        let (max_basic, ebx, ecx, edx) = cpuid(0, 0);
        vendor_id[0..4].copy_from_slice(&ebx.to_le_bytes());
        vendor_id[4..8].copy_from_slice(&edx.to_le_bytes());
        vendor_id[8..12].copy_from_slice(&ecx.to_le_bytes());

        // Extended CPUID leaves for brand string (0x80000002..=0x80000004)
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
        } else if max_basic > 0 {
            // Vendor only (brand unsupported) is still useful.
        }
    }

    CpuInfo { vendor_id, brand }
}

#[cfg(target_arch = "x86_64")]
fn cpuid(leaf: u32, subleaf: u32) -> (u32, u32, u32, u32) {
    let mut eax = leaf;
    let mut ecx = subleaf;
    let ebx: u32;
    let edx: u32;
    unsafe {
        core::arch::asm!(
            // LLVM may reserve RBX (e.g. for PIC); avoid using it as an operand.
            // Save/restore it manually and copy EBX into a normal output register.
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
