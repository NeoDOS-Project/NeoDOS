use crate::kbd::{KBD_SHIFT, KBD_CTRL, KBD_ALT};
use crate::log::LogSubsys;

pub fn dispatch_hotkey(code: u8, modifiers: u8) -> bool {
    let ctrl = (modifiers & KBD_CTRL) != 0;
    let alt = (modifiers & KBD_ALT) != 0;
    let shift = (modifiers & KBD_SHIFT) != 0;

    // Ctrl+Alt+Del → shutdown
    if ctrl && alt && code == 0x53 {
        kinfo!(LogSubsys::Kbd, "Ctrl+Alt+Del — shutting down...");
        crate::object::power::power_shutdown();
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
        crate::raw_serial_println!("[CORRELATION] last_DOUBLE_RUNNING_seq={} stack_owner_mismatch={}",
            crate::scheduler::diag::dr_last_seq(),
            crate::scheduler::diag::stack_owner_mismatch_count());
        return true;
    }

    false
}
