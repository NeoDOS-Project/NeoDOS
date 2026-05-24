const PS2_TIMEOUT: u32 = 100_000;

fn ps2_wait_input() -> bool {
    for _ in 0..PS2_TIMEOUT {
        let s: u8 = crate::hal::inb(0x64);
        if (s & 0x02) == 0 { return true; }
    }
    false
}

fn ps2_wait_output() -> bool {
    for _ in 0..PS2_TIMEOUT {
        let s: u8 = crate::hal::inb(0x64);
        if (s & 0x01) != 0 { return true; }
    }
    false
}

fn ps2_flush_output() {
    for _ in 0..PS2_TIMEOUT {
        let s: u8 = crate::hal::inb(0x64);
        if (s & 0x01) == 0 { break; }
        let _: u8 = crate::hal::inb(0x60);
    }
}

pub fn init_ps2() {
    if !ps2_wait_input() { return; }
    crate::hal::outb(0x64, 0xADu8);
    if !ps2_wait_input() { return; }
    crate::hal::outb(0x64, 0xA7u8);
    ps2_flush_output();
    let slave_mask = crate::hal::inb(0xA1) | 0x10;
    crate::hal::outb(0xA1, slave_mask);
    if !ps2_wait_input() { return; }
    crate::hal::outb(0x64, 0x20u8);
    if !ps2_wait_output() { return; }
    let config: u8 = crate::hal::inb(0x60);
    let new_config = (config | 0x01) & !0x10 & !0x02 | 0x20;
    if !ps2_wait_input() { return; }
    crate::hal::outb(0x64, 0x60u8);
    if !ps2_wait_input() { return; }
    crate::hal::outb(0x60, new_config);
    if !ps2_wait_input() { return; }
    crate::hal::outb(0x64, 0xAEu8);
    if !ps2_wait_input() { return; }
    crate::hal::outb(0x60, 0xF4u8);
    if ps2_wait_output() { let _ack: u8 = crate::hal::inb(0x60); }
}

pub fn set_leds(leds: u8) -> bool {
    if !ps2_wait_input() { return false; }
    crate::hal::outb(0x60, 0xEDu8);
    if !ps2_wait_input() { return false; }
    crate::hal::outb(0x60, leds & 0x07);
    true
}

pub fn wait_for_key() {
    loop {
        let status: u8 = crate::hal::inb(0x64);
        if (status & 0x01) != 0 {
            let _: u8 = crate::hal::inb(0x60);
            break;
        }
    }
}

pub fn read_scancode() -> Option<u8> {
    let status: u8 = crate::hal::inb(0x64);
    if (status & 0x01) != 0 {
        Some(crate::hal::inb(0x60))
    } else {
        None
    }
}
