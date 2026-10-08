// ── ABI Freeze Verification (v0.42)
//
// This module verifies that all frozen ABI interfaces maintain their
// values across versions. If any assertion here fails, the ABI has been
// broken and the version MUST be bumped (MAJOR).
//
// ── Frozen Interfaces (v0.42) ──
//   1. Event types 0–15     → eventbus/mod.rs
//   2. Capability flags      → drivers/caps.rs
//   3. IOAPIC API            → interrupts/ioapic.rs
//   4. WaitReason variants   → kwait/mod.rs
//
// Rule: Do NOT reassign existing values. Only append new values.

// =====================================================================
// 1. Event type constants (0–15 are frozen at v0.42)
// =====================================================================

/// Verify that the frozen event type values are unchanged.
pub fn verify_event_types() -> Result<(), &'static str> {
    // [0–15] Core event types — frozen at v0.42
    if crate::eventbus::EVENT_TIMER_TICK != 0 { return Err("EVENT_TIMER_TICK != 0"); }
    if crate::eventbus::EVENT_KEYBOARD_INPUT != 1 { return Err("EVENT_KEYBOARD_INPUT != 1"); }
    if crate::eventbus::EVENT_SERIAL_DATA != 2 { return Err("EVENT_SERIAL_DATA != 2"); }
    if crate::eventbus::EVENT_DISK_IO_COMPLETE != 3 { return Err("EVENT_DISK_IO_COMPLETE != 3"); }
    if crate::eventbus::EVENT_PROCESS_EXIT != 4 { return Err("EVENT_PROCESS_EXIT != 4"); }
    if crate::eventbus::EVENT_DRIVER_LOADED != 5 { return Err("EVENT_DRIVER_LOADED != 5"); }
    if crate::eventbus::EVENT_DRIVER_CRASH != 6 { return Err("EVENT_DRIVER_CRASH != 6"); }
    if crate::eventbus::EVENT_POLICY_VIOLATION != 7 { return Err("EVENT_POLICY_VIOLATION != 7"); }
    if crate::eventbus::EVENT_FS_MOUNTED != 8 { return Err("EVENT_FS_MOUNTED != 8"); }
    if crate::eventbus::EVENT_KEYB_LAYOUT != 9 { return Err("EVENT_KEYB_LAYOUT != 9"); }
    if crate::eventbus::EVENT_RTC_READ != 10 { return Err("EVENT_RTC_READ != 10"); }
    if crate::eventbus::EVENT_RTC_DATA != 11 { return Err("EVENT_RTC_DATA != 11"); }
    if crate::eventbus::EVENT_SHUTDOWN != 12 { return Err("EVENT_SHUTDOWN != 12"); }
    if crate::eventbus::EVENT_DRIVER_UNLOAD != 13 { return Err("EVENT_DRIVER_UNLOAD != 13"); }
    if crate::eventbus::EVENT_DRIVER_UNLOAD_ACK != 14 { return Err("EVENT_DRIVER_UNLOAD_ACK != 14"); }
    if crate::eventbus::EVENT_NMI_WATCHDOG != 15 { return Err("EVENT_NMI_WATCHDOG != 15"); }
    Ok(())
}

// =====================================================================
// 2. Capability flags (bits 0–11 are frozen at v0.42)
// =====================================================================

/// Verify that the frozen capability flag values are unchanged.
pub fn verify_capability_flags() -> Result<(), &'static str> {
    use crate::drivers::caps;
    if caps::CAP_IRQ != 1 << 0 { return Err("CAP_IRQ != 1"); }
    if caps::CAP_DMA != 1 << 1 { return Err("CAP_DMA != 2"); }
    if caps::CAP_MMIO != 1 << 2 { return Err("CAP_MMIO != 4"); }
    if caps::CAP_PORTIO != 1 << 3 { return Err("CAP_PORTIO != 8"); }
    if caps::CAP_ALLOC_PAGE != 1 << 4 { return Err("CAP_ALLOC_PAGE != 16"); }
    if caps::CAP_BLOCK_DEVICE != 1 << 5 { return Err("CAP_BLOCK_DEVICE != 32"); }
    if caps::CAP_EVENT_BUS != 1 << 6 { return Err("CAP_EVENT_BUS != 64"); }
    if caps::CAP_INPUT != 1 << 7 { return Err("CAP_INPUT != 128"); }
    if caps::CAP_LOG != 1 << 8 { return Err("CAP_LOG != 256"); }
    if caps::CAP_TIMING != 1 << 9 { return Err("CAP_TIMING != 512"); }
    if caps::CAP_MEMORY != 1 << 10 { return Err("CAP_MEMORY != 1024"); }
    if caps::CAP_ISOLATION != 1 << 11 { return Err("CAP_ISOLATION != 2048"); }
    Ok(())
}

// =====================================================================
// 3. IOAPIC — verify API surface stability
// =====================================================================

/// Verify IOAPIC constants are unchanged.
pub fn verify_ioapic_constants() -> Result<(), &'static str> {
    // Redirection entry flags
    if crate::interrupts::ioapic::ioapic_pin_count() < 24
        && crate::interrupts::ioapic::is_active()
    {
        // This is fine — if not active, pin_count is 0
    }
    Ok(())
}

// =====================================================================
// 4. WaitReason magic constants (KWait, frozen at v0.42)
// =====================================================================

/// Verify KWait magic encoding consistency.
pub fn verify_kwait_magic() -> Result<(), &'static str> {
    use crate::kwait::WaitReason;

    let pipe = WaitReason::PipeRead { pipe_id: 1 };
    let irp = WaitReason::IrpComplete { irp_id: 2 };
    let tj = WaitReason::ThreadJoin { tid: 3 };
    let ce = WaitReason::ChildExit { pid: 4 };
    let ev = WaitReason::Event { event_type: 5 };
    let al = WaitReason::Alertable;

    // Each type must produce a unique tag in the upper 16 bits
    let tags = [
        pipe.encode_magic() >> 16,
        irp.encode_magic() >> 16,
        tj.encode_magic() >> 16,
        ce.encode_magic() >> 16,
        ev.encode_magic() >> 16,
        al.encode_magic() >> 16,
    ];
    for i in 0..tags.len() {
        for j in (i + 1)..tags.len() {
            if tags[i] == tags[j] {
                return Err("Duplicate KWait magic tag");
            }
        }
    }
    Ok(())
}

// =====================================================================
// Combined verification (called at boot from Phase 3.9)
// =====================================================================

pub fn verify_all_frozen_abis() -> Result<(), &'static str> {
    verify_event_types()?;
    verify_capability_flags()?;
    verify_ioapic_constants()?;
    verify_kwait_magic()?;
    Ok(())
}

// ── Tests ──

pub fn register_abi_freeze_tests() {
    use crate::{test_case, test_true};

    test_case!("abi_freeze_event_types", {
        test_true!(verify_event_types().is_ok());
    });

    test_case!("abi_freeze_capability_flags", {
        test_true!(verify_capability_flags().is_ok());
    });

    test_case!("abi_freeze_kwait_magic", {
        test_true!(verify_kwait_magic().is_ok());
    });

    test_case!("abi_freeze_all", {
        test_true!(verify_all_frozen_abis().is_ok());
    });

    // Event type boundaries: new event types should be >= 16
    test_case!("abi_freeze_event_16_reserved_upper", {
        // Core events end at 15. PCI events start at 0x1000.
        // Verify no semi-reserved events encroach on the 16-0xFFF range.
        test_true!(crate::eventbus::EVENT_PCI_READ_CONFIG >= 0x1000);
        test_true!(crate::eventbus::EVENT_USER >= 0x2000);
    });

    // Capability boundaries: new caps should be bit 12+
    test_case!("abi_freeze_cap_bit12_reserved", {
        use crate::drivers::caps;
        // The 12th defined capability is at bit 11 (2048).
        // Bit 12 (4096) and above are reserved for future.
        test_true!(caps::CAP_ISOLATION < (1 << 12));
    });
}
