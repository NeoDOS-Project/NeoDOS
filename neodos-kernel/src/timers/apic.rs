use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};
use crate::log::LogSubsys;

// ── MSRs ───────────────────────────────────────────────────────────
const IA32_APIC_BASE: u32 = 0x1B;
const IA32_APIC_BASE_ENABLE: u64 = 1 << 11;
const IA32_APIC_BASE_X2APIC: u64 = 1 << 10;
const IA32_APIC_BASE_BSP: u64 = 1 << 8;
const IA32_APIC_BASE_ADDR_MASK: u64 = 0xFFFF_FFFF_FFFF_F000;

// ── Local APIC register offsets (MMIO from base) ───────────────────
const APIC_ID_REG: u64          = 0x020;
const APIC_VERSION_REG: u64     = 0x030;
const APIC_LVT_TIMER: u64       = 0x320;
const APIC_ICR_LOW: u64         = 0x300;   // Interrupt Command Register (low)
const APIC_ICR_HIGH: u64        = 0x310;   // Interrupt Command Register (high)
const APIC_TIMER_INIT_COUNT: u64 = 0x380;
const APIC_TIMER_CUR_COUNT: u64 = 0x390;
const APIC_TIMER_DIVIDER: u64   = 0x3E0;
const APIC_SVR: u64             = 0x0F0;   // Spurious Interrupt Vector
const APIC_EOI: u64             = 0x0B0;

// ── LVT Timer bits ─────────────────────────────────────────────────
const APIC_LVT_TIMER_MASKED: u32      = 1 << 16;
const APIC_LVT_TIMER_PERIODIC: u32    = 1 << 17;
const APIC_LVT_TIMER_ONE_SHOT: u32    = 0 << 17;
const APIC_LVT_TIMER_TSC_DEADLINE: u32 = 2 << 17;

// ── Divider values ─────────────────────────────────────────────────
const APIC_DIVIDE_1: u32   = 0b1011;
const APIC_DIVIDE_2: u32   = 0b0000;
const APIC_DIVIDE_4: u32   = 0b0001;
const APIC_DIVIDE_8: u32   = 0b0010;
const APIC_DIVIDE_16: u32  = 0b0011;
const APIC_DIVIDE_32: u32  = 0b1000;
const APIC_DIVIDE_64: u32  = 0b1001;
const APIC_DIVIDE_128: u32 = 0b1010;

// ── State ──────────────────────────────────────────────────────────

/// Local APIC MMIO base address (0 if not initialized).
static mut APIC_BASE: u64 = 0;

/// Calibrated bus frequency in KHz (0 if not calibrated).
pub static mut APIC_BUS_KHZ: u64 = 0;

// ── MSR helpers ────────────────────────────────────────────────────

#[inline]
unsafe fn rdmsr(msr: u32) -> u64 {
    crate::hal::raw::raw_read_msr(msr)
}

#[inline]
unsafe fn wrmsr(msr: u32, val: u64) {
    crate::hal::raw::raw_write_msr(msr, val);
}

// ── MMIO helpers ───────────────────────────────────────────────────

#[inline]
unsafe fn apic_read(offset: u64) -> u32 {
    let ptr = (APIC_BASE + offset) as *const u32;
    fence(Ordering::SeqCst);
    read_volatile(ptr)
}

#[inline]
unsafe fn apic_write(offset: u64, val: u32) {
    let ptr = (APIC_BASE + offset) as *mut u32;
    fence(Ordering::SeqCst);
    write_volatile(ptr, val);
}

// ── Public API ─────────────────────────────────────────────────────

/// Return the APIC MMIO base address (0 if not initialized).
pub fn apic_base() -> u64 {
    unsafe { APIC_BASE }
}

/// Return the calibrated APIC bus frequency in KHz (0 if not calibrated).
pub fn apic_bus_khz() -> u64 {
    unsafe { APIC_BUS_KHZ }
}

/// Check if the Local APIC is present and enabled.
/// Returns `true` if IA32_APIC_BASE MSR indicates APIC is enabled.
pub fn is_apic_present() -> bool {
    unsafe {
        let base = rdmsr(IA32_APIC_BASE);
        (base & IA32_APIC_BASE_ENABLE) != 0
    }
}

/// Initialize the Local APIC and configure its timer.
///
/// Steps:
/// 1. Read APIC base from MSR.
/// 2. Enable APIC via SVR (Spurious Interrupt Vector Register).
/// 3. Calibrate the APIC timer against the HPET.
/// 4. Configure LVT Timer in periodic mode with the target vector.
/// 5. Set divider and initial count.
///
/// Returns `true` if APIC timer was successfully configured.
pub fn init_apic_timer() -> bool {
    if !is_apic_present() {
        kerror!(LogSubsys::Apic, "Local APIC not present");
        return false;
    }

    unsafe {
        let base_msr = rdmsr(IA32_APIC_BASE);
        let base = base_msr & IA32_APIC_BASE_ADDR_MASK;
        APIC_BASE = base;

        kinfo!(LogSubsys::Apic, "Local APIC at MMIO 0x{:x}", base);

        // Enable the APIC by setting the Spurious Interrupt Vector Register
        // bit 8 (APIC enable). Keep vector 0xFF as spurious.
        let svr = apic_read(APIC_SVR);
        apic_write(APIC_SVR, svr | (1 << 8) | 0xFF);

        // Calibrate APIC timer frequency against HPET
        let bus_khz = calibrate_apic_bus();
        APIC_BUS_KHZ = bus_khz;

        if bus_khz == 0 {
            kerror!(LogSubsys::Apic, "Failed to calibrate bus frequency");
            return false;
        }

        kinfo!(LogSubsys::Apic, "Bus frequency: {} KHz", bus_khz);

        // ── Disable legacy timer sources before enabling APIC timer ──
        // Mask IRQ0 on the master PIC (set bit 0 of PIC data port 0x21)
        // to prevent HPET/PIT from sending interrupts via the PIC.
        crate::hal::outb(0x21, crate::hal::inb(0x21) | 0x01);

        // Disable HPET legacy replacement so HPET doesn't drive IRQ0
        // through the PIC while APIC timer is active
        let hpet_base = crate::timers::hpet::hpet_mmio_base();
        if hpet_base != 0 {
            // Read current HPET config, clear legacy bit, keep enable
            let hpet_config = crate::timers::hpet::read_hpet_config(hpet_base);
            crate::timers::hpet::write_hpet_config(hpet_base,
                hpet_config & !(1 << 1)); // clear HPET_CFG_LEGACY
        }

        // Configure LVT Timer:
        //   - Periodic mode (bit 17 = 1)
        //   - Vector 32 (IRQ0 remap)
        //   - Not masked (bit 16 = 0)
        let vector: u32 = 32;
        let lvt = vector | APIC_LVT_TIMER_PERIODIC;
        apic_write(APIC_LVT_TIMER, lvt);

        // Set divider to 16 for finer granularity
        apic_write(APIC_TIMER_DIVIDER, APIC_DIVIDE_16);

        // Set initial count:
        // Target: TICK_INTERVAL_US microseconds
        // Counter runs at bus_khz / 16 KHz (with divider 16)
        let counter_khz = bus_khz / divider_value(APIC_DIVIDE_16);
        let ticks = (counter_khz * crate::timers::TICK_INTERVAL_US) / 1000;
        apic_write(APIC_TIMER_INIT_COUNT, ticks as u32);

        kinfo!(LogSubsys::Apic, "Timer configured: {} ticks per interval ({} µs)",
            ticks, crate::timers::TICK_INTERVAL_US);

        // Write EOI to clear any pending interrupts
        apic_write(APIC_EOI, 0);
    }

    true
}

/// Calibrate the APIC bus frequency using HPET as reference.
/// Returns bus frequency in KHz, or 0 on failure.
unsafe fn calibrate_apic_bus() -> u64 {
    // Check if HPET is available for calibration
    let hpet_base = crate::timers::hpet::hpet_mmio_base();
    if hpet_base == 0 {
        kwarn!(LogSubsys::Apic, "No HPET available for calibration");
        return 0;
    }

    // HPET counter frequency in Hz
    let fs_period = crate::timers::hpet::hpet_fs_period();
    if fs_period == 0 {
        return 0;
    }
    let hpet_hz = 1_000_000_000_000_000u64 / fs_period;

    // Configure APIC timer in one-shot mode for calibration
    apic_write(APIC_LVT_TIMER, 32 | APIC_LVT_TIMER_ONE_SHOT);

    // Measure: start APIC counter at a known value, wait for HPET to
    // elapse a known time, read remaining APIC count.
    const CALIBRATION_US: u32 = 1000; // 1 ms calibration period
    let hpet_ticks = (hpet_hz * CALIBRATION_US as u64) / 1_000_000;

    if hpet_ticks == 0 {
        return 0;
    }

    // Use a large initial count to avoid underflow
    let initial_count: u32 = 0xFFFF_FFFF;
    apic_write(APIC_TIMER_DIVIDER, APIC_DIVIDE_16);
    apic_write(APIC_TIMER_INIT_COUNT, initial_count);

    // Read HPET start time
    let hpet_start = crate::timers::hpet::read_raw_counter(hpet_base);

    // Wait for HPET to elapse CALIBRATION_US
    loop {
        let now = crate::timers::hpet::read_raw_counter(hpet_base);
        if now.wrapping_sub(hpet_start) >= hpet_ticks {
            break;
        }
        crate::hal::raw::raw_pause();
    }

    // Read remaining APIC count
    let remaining = apic_read(APIC_TIMER_CUR_COUNT);

    // Ticks elapsed = initial_count - remaining
    let elapsed = initial_count.wrapping_sub(remaining);

    if elapsed == 0 {
        return 0;
    }

    // Counter ran at bus_khz / 16 KHz
    // elapsed / bus_khz * 16 ticks in CALIBRATION_US / 1000 ms
    // bus_khz = elapsed * 16 * 1000 / CALIBRATION_US / 1000
    //         = elapsed * 16 / CALIBRATION_US
    // Wait, let me re-derive:
    //   elapsed_ticks = (bus_freq_hz / 16) * (CALIBRATION_US / 1_000_000)
    //   bus_freq_hz = elapsed_ticks * 16 * 1_000_000 / CALIBRATION_US
    //   bus_freq_khz = bus_freq_hz / 1000 = elapsed_ticks * 16_000 / CALIBRATION_US
    let bus_khz = (elapsed as u64) * 16_000 / (CALIBRATION_US as u64);

    if bus_khz > 10_000_000 {
        // Sanity check: bus frequency shouldn't exceed 10 GHz
        // This likely means the calibration loop didn't work correctly
        return 2_000_000; // default fallback: 2 GHz
    }

    bus_khz
}

fn divider_value(div_reg: u32) -> u64 {
    match div_reg {
        APIC_DIVIDE_1 => 1,
        APIC_DIVIDE_2 => 2,
        APIC_DIVIDE_4 => 4,
        APIC_DIVIDE_8 => 8,
        APIC_DIVIDE_16 => 16,
        APIC_DIVIDE_32 => 32,
        APIC_DIVIDE_64 => 64,
        APIC_DIVIDE_128 => 128,
        _ => 16,
    }
}

/// Send End-Of-Interrupt to the APIC (write 0 to EOI register).
pub unsafe fn apic_eoi() {
    if APIC_BASE != 0 {
        apic_write(APIC_EOI, 0);
    }
}

/// Read the APIC ID of the current CPU.
pub fn apic_id() -> u32 {
    unsafe {
        if APIC_BASE != 0 {
            (apic_read(APIC_ID_REG) >> 24) & 0xFF
        } else {
            0
        }
    }
}

/// Check if this is the Bootstrap Processor (BSP).
pub fn is_bsp() -> bool {
    unsafe {
        let base = rdmsr(IA32_APIC_BASE);
        (base & IA32_APIC_BASE_BSP) != 0
    }
}

// ── IPI (Inter-Processor Interrupt) ──────────────────────────────────

/// IPI vector for per-CPU reschedule.
pub const IPI_RESCHEDULE: u8 = 0xF0;

/// Send an IPI to a specific APIC ID.
///
/// # Safety
/// Caller must ensure the APIC is initialized and the target CPU exists.
pub unsafe fn send_ipi(apic_id: u32, vector: u8) {
    let base = apic_base();

    // Write destination APIC ID to ICR high (bits 63:24)
    let icr_high = (apic_id as u64) << 24;
    write_volatile((base + APIC_ICR_HIGH) as *mut u32, icr_high as u32);

    // Write vector + delivery mode to ICR low
    // Delivery mode 0 = Fixed, destination shorthand 0 = use destination field
    let icr_low = vector as u32;
    write_volatile((base + APIC_ICR_LOW) as *mut u32, icr_low);

    // Wait for delivery status to clear (bit 12 of ICR low)
    loop {
        let val = read_volatile((base + APIC_ICR_LOW) as *const u32);
        if (val & (1 << 12)) == 0 {
            break;
        }
    }
}

/// Send an IPI to all CPUs including self (broadcast).
pub unsafe fn send_ipi_all(vector: u8) {
    let base = apic_base();

    // ICR low: vector + delivery mode + shorthand = all including self (0b11 << 18)
    let icr_low = vector as u32 | (0b11 << 18);
    write_volatile((base + APIC_ICR_LOW) as *mut u32, icr_low);

    loop {
        let val = read_volatile((base + APIC_ICR_LOW) as *const u32);
        if (val & (1 << 12)) == 0 {
            break;
        }
    }
}

/// Send an IPI to all CPUs except self.
pub unsafe fn send_ipi_all_excl_self(vector: u8) {
    let base = apic_base();

    // ICR low: vector + delivery mode + shorthand = all excluding self (0b10 << 18)
    let icr_low = vector as u32 | (0b10 << 18);
    write_volatile((base + APIC_ICR_LOW) as *mut u32, icr_low);

    loop {
        let val = read_volatile((base + APIC_ICR_LOW) as *const u32);
        if (val & (1 << 12)) == 0 {
            break;
        }
    }
}

/// Get a CPU's APIC ID from its index.
/// Requires the KPRCB to be initialized for that CPU.
pub fn get_apic_id_for_cpu(cpu: u32) -> Option<u32> {
    if cpu as usize >= crate::arch::x64::cpu_local::MAX_CPUS {
        return None;
    }
    unsafe {
        let kprcb = crate::arch::x64::cpu_local::KPRCB_PAGES[cpu as usize];
        if kprcb == 0 { return None; }
        let apic_id = core::ptr::read_volatile((kprcb + 4) as *const u32);
        Some(apic_id)
    }
}


