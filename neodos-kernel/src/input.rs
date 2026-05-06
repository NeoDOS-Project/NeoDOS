// src/input.rs

use spin::Mutex;

pub struct InputBuffer {
    buffer: [u8; 256],
    head: usize,
    tail: usize,
}

impl InputBuffer {
    pub const fn new() -> Self {
        InputBuffer {
            buffer: [0; 256],
            head: 0,
            tail: 0,
        }
    }
    
    pub fn push(&mut self, byte: u8) -> Result<(), ()> {
        let next = (self.tail + 1) % 256;
        if next == self.head {
            return Err(());  // Buffer full
        }
        self.buffer[self.tail] = byte;
        self.tail = next;
        Ok(())
    }
    
    pub fn pop(&mut self) -> Option<u8> {
        if self.head == self.tail {
            return None;
        }
        let byte = self.buffer[self.head];
        self.head = (self.head + 1) % 256;
        Some(byte)
    }
    
    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }
}

pub static INPUT_BUFFER: Mutex<InputBuffer> = Mutex::new(InputBuffer::new());

/// Push byte from IRQ context (interrupts already disabled by hardware during handler)
pub fn push_byte(byte: u8) {
    let mut buffer = INPUT_BUFFER.lock();
    let _ = buffer.push(byte);
}

/// Pop byte from main context (disable interrupts to avoid race with IRQ1)
pub fn pop_byte() -> Option<u8> {
    crate::arch::x64::disable_interrupts();
    let result = {
        let mut buffer = INPUT_BUFFER.lock();
        buffer.pop()
    };
    crate::arch::x64::enable_interrupts();
    result
}
