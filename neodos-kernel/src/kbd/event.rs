use crate::eventbus::Event;

pub fn kbd_event_handler(event: &Event) {
    if event.event_type == crate::eventbus::EVENT_KEYBOARD_INPUT {
        let scancode = event.data0 as u8;
        let released = (scancode & 0x80) != 0;
        let is_make = !released;
        let code = scancode & 0x7F;

        let mut kbd = crate::kbd::KBD.lock();
        kbd.process_scancode(code, is_make);
    }
}

pub fn register_kbd_event_handler() {
    let _ = crate::eventbus::EVENT_BUS.register_handler(
        crate::eventbus::EVENT_KEYBOARD_INPUT,
        kbd_event_handler,
        "neokbd",
    );
}
