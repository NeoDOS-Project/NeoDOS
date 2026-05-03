// CPU identification via CPUID instruction

pub struct CpuInfo {
    pub vendor_id: [u8; 12],
    pub brand: [u8; 48],
}

impl CpuInfo {
    pub fn vendor_str(&self) -> &str {
        core::str::from_utf8(&self.vendor_id).unwrap_or("Unknown")
    }
}

pub fn get_cpu_info() -> CpuInfo {
    // For now, return empty CPU info to avoid rbx register issues
    // TODO: Implement proper CPUID reading with workaround for rbx
    let vendor_id = [0u8; 12];
    let brand = [0u8; 48];
    
    CpuInfo { vendor_id, brand }
}
