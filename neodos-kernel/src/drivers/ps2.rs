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

/// Write a command byte to the PS/2 controller (port 0x64).
fn ps2_write_cmd(cmd: u8) -> bool {
    if !ps2_wait_input() { return false; }
    crate::hal::outb(0x64, cmd);
    true
}

/// Write a data byte to the PS/2 data port (0x60).
fn ps2_write_data(data: u8) -> bool {
    if !ps2_wait_input() { return false; }
    crate::hal::outb(0x60, data);
    true
}

/// Write a data byte to the second PS/2 port (mouse) by first sending
/// 0xD4 to port 0x64, then the data byte to port 0x60.
fn ps2_write_mouse(data: u8) -> bool {
    if !ps2_write_cmd(0xD4) { return false; }
    ps2_write_data(data)
}

/// Read a byte from the PS/2 data port (0x60), waiting for output buffer full.
fn ps2_read_data() -> Option<u8> {
    if ps2_wait_output() {
        Some(crate::hal::inb(0x60))
    } else {
        None
    }
}

pub fn init_ps2() {
    if !ps2_wait_input() { return; }
    crate::hal::outb(0x64, 0xADu8);           // Disable keyboard port
    if !ps2_wait_input() { return; }
    crate::hal::outb(0x64, 0xA7u8);           // Disable mouse port
    ps2_flush_output();

    // IRQ12 masked on PIC slave (will be unmasked if IOAPIC not available)
    let slave_mask = crate::hal::inb(0xA1) | 0x10;
    crate::hal::outb(0xA1, slave_mask);

    // Read and update config byte
    if !ps2_wait_input() { return; }
    crate::hal::outb(0x64, 0x20u8);
    if !ps2_wait_output() { return; }
    let config: u8 = crate::hal::inb(0x60);
    // Enable port 1 IRQ (bit 0), enable port 2 clock (bit 4=0),
    // enable port 2 IRQ (bit 1), enable translation (bit 5)
    let new_config = (config | 0x01 | 0x02) & !0x10 | 0x20;
    if !ps2_wait_input() { return; }
    crate::hal::outb(0x64, 0x60u8);
    if !ps2_wait_input() { return; }
    crate::hal::outb(0x60, new_config);

    // Enable mouse port
    if !ps2_wait_input() { return; }
    crate::hal::outb(0x64, 0xA8u8);

    // Enable keyboard port
    if !ps2_wait_input() { return; }
    crate::hal::outb(0x64, 0xAEu8);

    // Enable keyboard scanning
    if !ps2_wait_input() { return; }
    crate::hal::outb(0x60, 0xF4u8);
    if ps2_wait_output() { let _ack: u8 = crate::hal::inb(0x60); }

    // Initialize mouse
    init_mouse();
}

/// Initialize the PS/2 mouse (port 2):
///   - Reset
///   - Set defaults
///   - Enable data reporting
fn init_mouse() {
    // Reset mouse
    if !ps2_write_mouse(0xFF) { return; }
    // Expect ACK (0xFA) then self-test (0xAA) then device ID (0x00)
    let _ack = ps2_read_data();
    let _bat = ps2_read_data();
    let _dev_id = ps2_read_data();

    // Set default parameters
    if !ps2_write_mouse(0xF6) { return; }
    let _ack = ps2_read_data();

    // Enable data reporting (stream mode)
    if !ps2_write_mouse(0xF4) { return; }
    let _ack = ps2_read_data();

    crate::serial_println!("[PS2] Mouse initialized on port 2");
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
