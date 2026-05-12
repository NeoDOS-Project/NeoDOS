// src/drivers/usb_hid/hid.rs
//
// HID (Human Interface Device) protocol support.
//
// HID devices communicate via:
// 1. Control transfers (for configuration)
// 2. Interrupt transfers (for data - used by keyboards/mice)
//
// HID Report Descriptors define the data format.
// Keyboard reports are typically 8 bytes:
//   Byte 0: modifiers (bit 0=LCtrl, 1=LShift, 2=LAlt, 3=LCtrl, 4=RShift, 5=RCtrl)
//   Byte 1: reserved
//   Bytes 2-7: up to 6 key codes

/// Convert HID key code to ASCII character (no modifiers considered)
/// Returns the ASCII byte that should be pushed to the input buffer
pub fn hid_key_to_ascii(hid_code: u8) -> Option<u8> {
    match hid_code {
        // Letters (HID codes 0x04-0x1D = A-Z)
        0x04..=0x1D => Some(b'a' + (hid_code - 0x04)),

        // Numbers
        0x1E => Some(b'1'),
        0x1F => Some(b'2'),
        0x20 => Some(b'3'),
        0x21 => Some(b'4'),
        0x22 => Some(b'5'),
        0x23 => Some(b'6'),
        0x24 => Some(b'7'),
        0x25 => Some(b'8'),
        0x26 => Some(b'9'),
        0x27 => Some(b'0'),

        // Punctuation and symbols
        0x28 => Some(b'\n'),      // Enter
        0x29 => None,             // Escape
        0x2A => Some(b'\x08'),    // Backspace
        0x2B => Some(b'\t'),      // Tab
        0x2C => Some(b' '),       // Space
        0x2D => Some(b'-'),
        0x2E => Some(b'='),
        0x2F => Some(b'['),
        0x30 => Some(b']'),
        0x31 => Some(b'\\'),
        0x32 => None,             // Non-US # and ~
        0x33 => Some(b';'),
        0x34 => Some(b'\''),
        0x35 => Some(b'`'),
        0x36 => Some(b','),
        0x37 => Some(b'.'),
        0x38 => Some(b'/'),

        // Keypad
        0x59 => Some(b'1'),
        0x5A => Some(b'2'),
        0x5B => Some(b'3'),
        0x5C => Some(b'4'),
        0x5D => Some(b'5'),
        0x5E => Some(b'6'),
        0x5F => Some(b'7'),
        0x60 => Some(b'8'),
        0x61 => Some(b'9'),
        0x62 => Some(b'0'),
        0x58 => Some(b'\n'),     // Numpad Enter

        _ => None,
    }
}

/// Parse a USB HID keyboard report (8 bytes) and push keystrokes
pub fn parse_hid_report(report: &[u8; 8]) {
    if report[0] == 0 && report[1] == 0 && report[2] == 0 {
        return; // Empty report
    }

    for i in 2..8 {
        if report[i] != 0 {
            if let Some(ascii) = hid_key_to_ascii(report[i]) {
                super::usb_push_byte(ascii);
            }
        }
    }
}