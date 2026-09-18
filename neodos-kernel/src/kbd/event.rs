use crate::eventbus::Event;

pub fn kbd_event_handler(event: &Event) {
    if event.event_type == crate::eventbus::EVENT_KEYBOARD_INPUT {
        let scancode = event.data0 as u8;
        let released = (scancode & 0x80) != 0;
        let is_make = !released;
        let code = scancode & 0x7F;
        crate::serial_println!("[KBD_EVENT] scancode=0x{:x} make={} code=0x{:x}", scancode, is_make, code);

        if let Some(mut kbd) = crate::kbd::KBD.try_lock() {
            kbd.process_scancode(code, is_make);
        } else {
            crate::serial_println!("[KBD_EVENT] KBD lock busy! Skipping or retrying");
        }
    }
}

pub fn register_kbd_event_handler() {
    let res = crate::eventbus::EVENT_BUS.register_handler(
        crate::eventbus::EVENT_KEYBOARD_INPUT,
        kbd_event_handler,
        "neokbd",
    );
    crate::serial_println!("[KBD_REG] result={:?} handler=kbd_event_handler", res);
}
