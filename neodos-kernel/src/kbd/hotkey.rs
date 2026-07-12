use crate::kbd::{KBD_SHIFT, KBD_CTRL, KBD_ALT};

pub fn dispatch_hotkey(code: u8, modifiers: u8) -> bool {
    let ctrl = (modifiers & KBD_CTRL) != 0;
    let alt = (modifiers & KBD_ALT) != 0;
    let shift = (modifiers & KBD_SHIFT) != 0;

    // Ctrl+Alt+Del → shutdown
    if ctrl && alt && code == 0x53 {
        crate::serial_println!("[NeoKBD] Ctrl+Alt+Del — shutting down...");
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

    false
}
