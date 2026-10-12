use crate::kbd::{KBD_SHIFT, KBD_CTRL, KBD_ALT};
use crate::log::LogSubsys;

/// Deferred Ctrl+Alt+Del shutdown.
///
/// `power_shutdown()` flushes the registry hives, the page cache and the VFS —
/// with blocking `spin::Mutex` acquisitions. Running it in the keyboard IRQ
/// (IF=0) could spin forever on a lock an interrupted thread holds with IF=1
/// (INV-3, Rule 3.5.1). It is therefore queued and executed from a safe context
/// by the work queue (the same pattern as Ctrl+C / `interrupt_foreground_work`).
fn deferred_power_shutdown(_: *mut u8) {
    crate::object::power::power_shutdown(); // -> ! (powers off the machine)
}

pub fn dispatch_hotkey(code: u8, modifiers: u8) -> bool {
    let ctrl = (modifiers & KBD_CTRL) != 0;
    let alt = (modifiers & KBD_ALT) != 0;
    let shift = (modifiers & KBD_SHIFT) != 0;

    // Ctrl+Alt+Del → request shutdown (deferred out of IRQ context)
    if ctrl && alt && code == 0x53 {
        kinfo!(LogSubsys::Kbd, "Ctrl+Alt+Del — requesting shutdown (deferred)");
        let _ = crate::work_queue::WORK_QUEUE
            .push_high(deferred_power_shutdown, core::ptr::null_mut());
        return true;
    }

    // Alt+F1-F4 → VT switch (scancodes 0x3B-0x3E)
    if alt && !ctrl && !shift && (0x3B..=0x3E).contains(&code) {
        let vt_num = (code - 0x3B) as usize;
        crate::input::switch_vt(vt_num);
        return true;
    }

    // Alt+F5-F8 → VT switch to VT 4-7 (scancodes 0x3F-0x42)
    if alt && !ctrl && !shift && (0x3F..=0x42).contains(&code) {
        let vt_num = (code - 0x3B) as usize;
        crate::input::switch_vt(vt_num);
        return true;
    }

    // Ctrl+Alt+V → VT queue diagnostic dump (Fase 1) + Phase 6 IRQ33 ownership
    if ctrl && alt && code == 0x2F {
        crate::input::vt::vt_diag_dump();
        crate::arch::x64::idt::kbd_irq_dump();
        crate::interrupts::ioapic::dump_irq_routing(1);
        // Phase 293-B: dump current_thread / rsp / double-running rings on demand.
        crate::serial_println!("[293B_DUMP] on-demand forensic dump");
        crate::scheduler::diag::ctx_dump_raw();
        crate::scheduler::diag::rsp_dump_raw();
        crate::scheduler::diag::dr_dump_raw();
        crate::scheduler::diag::kcpu_dump_raw();
        crate::raw_serial_println!("[CORRELATION] last_DOUBLE_RUNNING_seq={} stack_owner_mismatch={}",
            crate::scheduler::diag::dr_last_seq(),
            crate::scheduler::diag::stack_owner_mismatch_count());
        return true;
    }

    false
}
