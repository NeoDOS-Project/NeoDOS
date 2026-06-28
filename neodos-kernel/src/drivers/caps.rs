// src/drivers/caps.rs
// Capability System (X3) — fine-grained resource access control for NEM drivers.
//
// Each driver has a 64-bit capability bitmap set at load time. Every hst_*
// export function checks that the calling driver holds the required capability
// before executing. BOOT drivers inherit all capabilities; SYSTEM and DEMAND
// drivers receive a restricted set.

use crate::nem::DriverCategory;

// ── Capability flag constants ──
//
// ╔═══════════════════════════════════════════════════════════════════╗
// ║  ABI FROZEN at v0.42                                            ║
// ║  Capability bits 0–11 MUST NOT be reassigned.                   ║
// ║  New capabilities MUST use bit 12+.                             ║
// ╚═══════════════════════════════════════════════════════════════════╝

pub const CAP_NONE: u64 = 0;
pub const CAP_IRQ: u64 = 1 << 0;          // FROZEN v0.42 — bit 0
pub const CAP_DMA: u64 = 1 << 1;          // FROZEN v0.42 — bit 1
pub const CAP_MMIO: u64 = 1 << 2;         // FROZEN v0.42 — bit 2
pub const CAP_PORTIO: u64 = 1 << 3;       // FROZEN v0.42 — bit 3
pub const CAP_ALLOC_PAGE: u64 = 1 << 4;   // FROZEN v0.42 — bit 4
pub const CAP_BLOCK_DEVICE: u64 = 1 << 5; // FROZEN v0.42 — bit 5
pub const CAP_EVENT_BUS: u64 = 1 << 6;    // FROZEN v0.42 — bit 6
pub const CAP_INPUT: u64 = 1 << 7;        // FROZEN v0.42 — bit 7
pub const CAP_LOG: u64 = 1 << 8;          // FROZEN v0.42 — bit 8
pub const CAP_TIMING: u64 = 1 << 9;       // FROZEN v0.42 — bit 9
pub const CAP_MEMORY: u64 = 1 << 10;      // FROZEN v0.42 — bit 10
pub const CAP_ISOLATION: u64 = 1 << 11;   // FROZEN v0.42 — bit 11

pub const CAP_ALL: u64 = u64::MAX;

/// Human-readable name for each capability (for diagnostics).
pub fn cap_name(flag: u64) -> &'static str {
    match flag {
        CAP_IRQ => "IRQ",
        CAP_DMA => "DMA",
        CAP_MMIO => "MMIO",
        CAP_PORTIO => "PORTIO",
        CAP_ALLOC_PAGE => "ALLOC_PAGE",
        CAP_BLOCK_DEVICE => "BLOCK_DEVICE",
        CAP_EVENT_BUS => "EVENT_BUS",
        CAP_INPUT => "INPUT",
        CAP_LOG => "LOG",
        CAP_TIMING => "TIMING",
        CAP_MEMORY => "MEMORY",
        CAP_ISOLATION => "ISOLATION",
        _ => "UNKNOWN",
    }
}

/// List all defined capability flag names (for NDREG display).
pub fn all_cap_names() -> &'static [(u64, &'static str)] {
    &[
        (CAP_IRQ, "IRQ"),
        (CAP_DMA, "DMA"),
        (CAP_MMIO, "MMIO"),
        (CAP_PORTIO, "PORTIO"),
        (CAP_ALLOC_PAGE, "ALLOC_PAGE"),
        (CAP_BLOCK_DEVICE, "BLOCK_DEVICE"),
        (CAP_EVENT_BUS, "EVENT_BUS"),
        (CAP_INPUT, "INPUT"),
        (CAP_LOG, "LOG"),
        (CAP_TIMING, "TIMING"),
        (CAP_MEMORY, "MEMORY"),
        (CAP_ISOLATION, "ISOLATION"),
    ]
}

/// Capability set wrapper with introspection helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilitySet {
    pub bits: u64,
}

impl CapabilitySet {
    pub const fn new(bits: u64) -> Self {
        Self { bits }
    }

    pub const fn none() -> Self {
        Self { bits: 0 }
    }

    pub const fn all() -> Self {
        Self { bits: u64::MAX }
    }

    pub fn has(&self, required: u64) -> bool {
        required == 0 || (self.bits & required) == required
    }

    pub fn add(&mut self, mask: u64) {
        self.bits |= mask;
    }

    pub fn remove(&mut self, mask: u64) {
        self.bits &= !mask;
    }

    pub fn is_empty(&self) -> bool {
        self.bits == 0
    }

    /// Format as comma-separated string (truncated for display).
    pub fn format(&self) -> alloc::string::String {
        let mut s = alloc::string::String::new();
        for &(flag, name) in all_cap_names() {
            if self.bits & flag != 0 {
                if !s.is_empty() { s.push('|'); }
                s.push_str(name);
            }
        }
        if s.is_empty() { s.push_str("NONE"); }
        s
    }

    /// Number of set bits.
    pub fn count(&self) -> u32 {
        self.bits.count_ones()
    }
}

/// Return the default capability set for a given driver category.
///
/// Inheritance rules:
///   BOOT   → All capabilities (full hardware + kernel access)
///   SYSTEM → Port I/O, IRQ, MMIO, DMA, Event Bus, Input, Log, Timing
///   DEMAND → Event Bus, Log, Timing (sandboxed)
pub fn capability_for_category(cat: DriverCategory) -> CapabilitySet {
    match cat {
        DriverCategory::Boot => CapabilitySet::all(),
        DriverCategory::System => CapabilitySet::new(
            CAP_PORTIO | CAP_IRQ | CAP_MMIO | CAP_DMA |
            CAP_EVENT_BUS | CAP_INPUT | CAP_LOG | CAP_TIMING,
        ),
        DriverCategory::Demand => CapabilitySet::new(
            CAP_EVENT_BUS | CAP_LOG | CAP_TIMING,
        ),
    }
}

/// Check whether `driver_bits` (the driver's capability set) includes every
/// capability in `required`. Returns `Ok(())` or an error listing the missing
/// capabilities.
pub fn check_capabilities(driver_bits: u64, required: u64) -> Result<(), &'static str> {
    if required == 0 {
        return Ok(());
    }
    if (driver_bits & required) == required {
        Ok(())
    } else {
        let missing = required & !driver_bits;
        let msg: &[u8] = b"Missing caps: ";
        let mut buf = [0u8; 128];
        let mut pos = msg.len();
        buf[..pos].copy_from_slice(msg);
        let mut first = true;
        for &(flag, name) in all_cap_names() {
            if missing & flag != 0 {
                if !first { buf[pos] = b'|'; pos += 1; }
                let n = name.as_bytes();
                let nlen = n.len().min(buf.len().saturating_sub(pos));
                buf[pos..pos + nlen].copy_from_slice(&n[..nlen]);
                pos += nlen;
                first = false;
            }
        }
        // We can't return a stack-based buffer, so return a static string for now.
        // The detailed missing-caps is available via NDREG DEBUG.
        Err("Capability denied")
    }
}

// ── Capability escalation ──

/// Event type for capability escalation requests.
pub const EVENT_CAP_ESCALATION: u32 = 0x2000;

/// Escalation request data (sent as Event data0/data1):
///   data0 = driver_id
///   data1 = requested capabilities bitmask
///
/// The kernel audits the request (in `handle_cap_escalation`) and may grant
/// additional capabilities by calling `grant_capabilities`.
///
/// Maximum additional capabilities a non-BOOT driver can request.
pub const CAP_ESCALATION_LIMIT: u64 = CAP_ALLOC_PAGE | CAP_BLOCK_DEVICE | CAP_MEMORY;

/// Handle a capability escalation request from a driver.
/// Called from Event Bus dispatch when EVENT_CAP_ESCALATION is received.
///
/// Policy:
///   - BOOT drivers already have all caps — escalation is a no-op.
///   - SYSTEM drivers may request CAP_ALLOC_PAGE, CAP_BLOCK_DEVICE, or CAP_MEMORY.
///   - DEMAND drivers may NOT escalate (security boundary).
///   - Only capabilities in ESCALATION_LIMIT can be granted.
pub fn handle_cap_escalation(driver_id: u32, requested: u64) -> Result<u64, &'static str> {
    use crate::drivers::driver_runtime::DRIVER_RUNTIME;
    let mut rt = DRIVER_RUNTIME.lock();
    let drv = rt.get_mut(driver_id).ok_or("Driver not found")?;

    // BOOT already has everything
    if drv.category == DriverCategory::Boot {
        return Ok(drv.caps);
    }

    // DEMAND may not escalate
    if drv.category == DriverCategory::Demand {
        return Err("DEMAND drivers cannot request capability escalation");
    }

    // Only caps within the escalation limit are grantable
    let grantable = requested & CAP_ESCALATION_LIMIT;
    if grantable == 0 || grantable != requested {
        return Err("Request includes non-grantable capabilities");
    }

    // Grant: merge into existing caps
    let new_caps = drv.caps | grantable;
    drv.caps = new_caps;

    crate::serial_println!(
        "[CAP] Escalation granted: driver {} gained {:016x}, now {:016x}",
        drv.name_str(), grantable, new_caps,
    );

    Ok(new_caps)
}

/// Grant specific additional capabilities to a driver (used by escalation handler
/// or kernel policy code).
pub fn grant_capabilities(driver_id: u32, additional: u64) -> Result<u64, &'static str> {
    use crate::drivers::driver_runtime::DRIVER_RUNTIME;
    let mut rt = DRIVER_RUNTIME.lock();
    let drv = rt.get_mut(driver_id).ok_or("Driver not found")?;
    drv.caps |= additional;
    Ok(drv.caps)
}

// ── Tests ──

pub fn register_cap_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;
    use crate::test_ne;

    test_case!("caps_basic_flags", {
        test_eq!(CAP_IRQ, 1);
        test_eq!(CAP_DMA, 2);
        test_eq!(CAP_MMIO, 4);
        test_eq!(CAP_PORTIO, 8);
        test_eq!(CAP_ALLOC_PAGE, 16);
        test_eq!(CAP_BLOCK_DEVICE, 32);
        test_eq!(CAP_EVENT_BUS, 64);
        test_eq!(CAP_INPUT, 128);
        test_eq!(CAP_LOG, 256);
        test_eq!(CAP_TIMING, 512);
        test_eq!(CAP_MEMORY, 1024);
    });

    test_case!("caps_set_has", {
        let c = CapabilitySet::new(CAP_IRQ | CAP_DMA | CAP_PORTIO);
        test_true!(c.has(CAP_IRQ));
        test_true!(c.has(CAP_DMA));
        test_true!(c.has(CAP_PORTIO));
        test_true!(c.has(CAP_IRQ | CAP_DMA));
        test_eq!(c.has(CAP_MMIO), false);
        test_eq!(c.has(CAP_IRQ | CAP_MMIO), false);
    });

    test_case!("caps_set_add_remove", {
        let mut c = CapabilitySet::none();
        test_eq!(c.is_empty(), true);
        c.add(CAP_IRQ | CAP_DMA);
        test_true!(c.has(CAP_IRQ));
        c.remove(CAP_IRQ);
        test_eq!(c.has(CAP_IRQ), false);
        test_true!(c.has(CAP_DMA));
    });

    test_case!("caps_set_all", {
        let c = CapabilitySet::all();
        test_eq!(c.has(CAP_ALL), true);
        test_ne!(c.count(), 0);
    });

    test_case!("caps_category_boot", {
        let c = capability_for_category(DriverCategory::Boot);
        test_eq!(c, CapabilitySet::all());
        test_eq!(c.has(CAP_ALL), true);
    });

    test_case!("caps_category_system", {
        let c = capability_for_category(DriverCategory::System);
        test_true!(c.has(CAP_PORTIO));
        test_true!(c.has(CAP_IRQ));
        test_true!(c.has(CAP_EVENT_BUS));
        test_true!(c.has(CAP_TIMING));
        test_eq!(c.has(CAP_ALLOC_PAGE), false);
        test_eq!(c.has(CAP_BLOCK_DEVICE), false);
    });

    test_case!("caps_category_demand", {
        let c = capability_for_category(DriverCategory::Demand);
        test_true!(c.has(CAP_EVENT_BUS));
        test_true!(c.has(CAP_LOG));
        test_true!(c.has(CAP_TIMING));
        test_eq!(c.has(CAP_PORTIO), false);
        test_eq!(c.has(CAP_IRQ), false);
        test_eq!(c.has(CAP_ALLOC_PAGE), false);
        test_eq!(c.has(CAP_BLOCK_DEVICE), false);
    });

    test_case!("caps_check_ok", {
        let bits = CAP_IRQ | CAP_PORTIO | CAP_DMA;
        test_true!(check_capabilities(bits, CAP_IRQ).is_ok());
        test_true!(check_capabilities(bits, CAP_IRQ | CAP_PORTIO).is_ok());
        test_true!(check_capabilities(bits, 0).is_ok());
    });

    test_case!("caps_check_denied", {
        let bits = CAP_IRQ | CAP_PORTIO;
        test_true!(check_capabilities(bits, CAP_DMA).is_err());
        test_true!(check_capabilities(bits, CAP_DMA | CAP_IRQ).is_err());
        test_true!(check_capabilities(bits, CAP_ALLOC_PAGE).is_err());
    });

    test_case!("caps_escalation_limit", {
        // Should include alloc_page, block_device, memory
        test_ne!(CAP_ESCALATION_LIMIT & CAP_ALLOC_PAGE, 0);
        test_ne!(CAP_ESCALATION_LIMIT & CAP_BLOCK_DEVICE, 0);
        test_ne!(CAP_ESCALATION_LIMIT & CAP_MEMORY, 0);
        // Should NOT include IRQ, DMA, etc.
        test_eq!(CAP_ESCALATION_LIMIT & CAP_IRQ, 0);
        test_eq!(CAP_ESCALATION_LIMIT & CAP_DMA, 0);
        test_eq!(CAP_ESCALATION_LIMIT & CAP_PORTIO, 0);
    });

    test_case!("caps_set_format", {
        let c = CapabilitySet::new(CAP_IRQ | CAP_DMA | CAP_LOG);
        let s = c.format();
        test_true!(s.contains("IRQ"));
        test_true!(s.contains("DMA"));
        test_true!(s.contains("LOG"));
    });
}
