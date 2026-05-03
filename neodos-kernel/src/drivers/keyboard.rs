// src/drivers/keyboard.rs

use x86_64::instructions::port::Port;

pub struct KeyboardDriver;

impl KeyboardDriver {
    pub fn read_scancode() -> Option<u8> {
        let mut status_port = Port::new(0x64u16);
        let mut data_port = Port::new(0x60u16);
        
        unsafe {
            let status: u8 = status_port.read();
            if (status & 0x01) != 0 {
                let scancode: u8 = data_port.read();
                return Some(scancode);
            }
        }
        None
    }
    
    pub fn scancode_to_ascii(scancode: u8) -> Option<u8> {
        match scancode {
            // Letters (simplified, no shift for now)
            0x1E => Some(b'a'), 0x30 => Some(b'b'), 0x2E => Some(b'c'),
            0x20 => Some(b'd'), 0x12 => Some(b'e'), 0x21 => Some(b'f'),
            0x22 => Some(b'g'), 0x23 => Some(b'h'), 0x17 => Some(b'i'),
            0x24 => Some(b'j'), 0x25 => Some(b'k'), 0x26 => Some(b'l'),
            0x32 => Some(b'm'), 0x31 => Some(b'n'), 0x18 => Some(b'o'),
            0x19 => Some(b'p'), 0x10 => Some(b'q'), 0x13 => Some(b'r'),
            0x1F => Some(b's'), 0x14 => Some(b't'), 0x16 => Some(b'u'),
            0x2F => Some(b'v'), 0x11 => Some(b'w'), 0x2D => Some(b'x'),
            0x15 => Some(b'y'), 0x2C => Some(b'z'),
            
            // Numbers
            0x02 => Some(b'1'), 0x03 => Some(b'2'), 0x04 => Some(b'3'),
            0x05 => Some(b'4'), 0x06 => Some(b'5'), 0x07 => Some(b'6'),
            0x08 => Some(b'7'), 0x09 => Some(b'8'), 0x0A => Some(b'9'),
            0x0B => Some(b'0'),
            
            // Special
            0x0E => Some(b'\x08'),  // Backspace
            0x1C => Some(b'\n'),    // Enter
            0x39 => Some(b' '),     // Space
            0x34 => Some(b'.'),     // Period
            0x35 => Some(b'/'),     // Slash
            0x0C => Some(b'-'),     // Minus
            0x0D => Some(b'='),     // Equals
            0x1A => Some(b'['),     // Left bracket
            0x1B => Some(b']'),     // Right bracket
            0x27 => Some(b';'),     // Semicolon
            0x28 => Some(b'\''),    // Quote
            0x2B => Some(b'\\'),    // Backslash
            0x33 => Some(b','),     // Comma
            
            _ => None,
        }
    }
}
