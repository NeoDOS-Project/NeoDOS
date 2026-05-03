// src/input.rs

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

pub static mut INPUT_BUFFER: InputBuffer = InputBuffer::new();

pub fn push_byte(byte: u8) {
    unsafe {
        let _ = INPUT_BUFFER.push(byte);
    }
}

pub fn pop_byte() -> Option<u8> {
    unsafe {
        INPUT_BUFFER.pop()
    }
}
