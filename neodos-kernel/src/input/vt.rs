use core::sync::atomic::{AtomicUsize, Ordering};

pub const VT_COUNT: usize = 4;
pub const VT_QUEUE_SIZE: usize = 4096;
pub const VT_CONSOLE_COLS: usize = 160;
pub const VT_CONSOLE_ROWS: usize = 50;

pub use crate::console::ConsoleState;

pub struct VtInputQueue {
    buffer: [u8; VT_QUEUE_SIZE],
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl VtInputQueue {
    pub const fn new() -> Self {
        VtInputQueue {
            buffer: [0; VT_QUEUE_SIZE],
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    pub fn push(&self, byte: u8) -> Result<(), ()> {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed);
        let next = (tail + 1) % VT_QUEUE_SIZE;
        if next == head {
            return Err(());
        }
        unsafe {
            (self.buffer.as_ptr() as *mut u8).add(tail).write(byte);
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
        let byte = unsafe { self.buffer.as_ptr().add(head).read() };
        let next = (head + 1) % VT_QUEUE_SIZE;
        self.head.store(next, Ordering::Release);
        Some(byte)
    }

    pub fn has_data(&self) -> bool {
        self.head.load(Ordering::Relaxed) != self.tail.load(Ordering::Acquire)
    }
}

pub struct VtShadowBuffer {
    pub chars: [[u8; VT_CONSOLE_COLS]; VT_CONSOLE_ROWS],
}

impl VtShadowBuffer {
    pub const fn new() -> Self {
        VtShadowBuffer {
            chars: [[0u8; VT_CONSOLE_COLS]; VT_CONSOLE_ROWS],
        }
    }
}
