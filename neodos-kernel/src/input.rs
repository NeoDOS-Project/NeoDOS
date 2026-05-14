// src/input.rs
//
// Lock-free single-producer (IRQ1) / single-consumer (shell) input buffer.
// The producer runs with interrupts disabled (hardware), so the consumer
// masks interrupts around its critical section to prevent the producer
// from observing an inconsistent state.

use core::sync::atomic::{AtomicUsize, Ordering};

const INPUT_BUFFER_SIZE: usize = 1024;

pub struct InputBuffer {
    buffer: [u8; INPUT_BUFFER_SIZE],
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl InputBuffer {
    pub const fn new() -> Self {
        InputBuffer {
            buffer: [0; INPUT_BUFFER_SIZE],
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    pub fn push(&self, byte: u8) -> Result<(), ()> {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed);
        
        let next = (tail + 1) % INPUT_BUFFER_SIZE;
        if next == head {
            return Err(());
        }

        unsafe {
            let ptr = self.buffer.as_ptr() as *mut u8;
            ptr.add(tail).write(byte);
        }
        
        self.tail.store(next, Ordering::Release);
        Ok(())
    }

    pub fn pop(&self) -> Option<u8> {
        let tail = self.tail.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Relaxed);
        
        if head == tail {
            return None;
        }

        let byte = unsafe {
            let ptr = self.buffer.as_ptr();
            ptr.add(head).read()
        };
        
        let next = (head + 1) % INPUT_BUFFER_SIZE;
        self.head.store(next, Ordering::Release);
        
        Some(byte)
    }
}

static INPUT_BUFFER: InputBuffer = InputBuffer::new();

pub fn push_byte(byte: u8) {
    let _ = INPUT_BUFFER.push(byte);
}

pub fn pop_byte() -> Option<u8> {
    INPUT_BUFFER.pop()
}
