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

// ── Tests ──────────────────────────────────────────────────────────

pub fn register_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;

    test_case!("vt_queue_create", {
        let q = VtInputQueue::new();
        test_eq!(q.pop(), None);
    });

    test_case!("vt_queue_push_pop", {
        let q = VtInputQueue::new();
        test_eq!(q.push(0x41), Ok(()));
        test_eq!(q.pop(), Some(0x41));
        test_eq!(q.pop(), None);
    });

    test_case!("vt_queue_capacity", {
        let q = VtInputQueue::new();
        let mut count = 0;
        while q.push(count as u8).is_ok() {
            count += 1;
        }
        test_true!(count > 0);
        test_eq!(count, VT_QUEUE_SIZE - 1);
    });

    test_case!("vt_queue_wrap_around", {
        let q = VtInputQueue::new();
        for i in 0..(VT_QUEUE_SIZE - 1) { let _ = q.push(i as u8); }
        for i in 0..50 { test_eq!(q.pop(), Some(i as u8)); }
        for i in (VT_QUEUE_SIZE - 1)..(VT_QUEUE_SIZE - 1 + 50) { let _ = q.push(i as u8); }
        for i in 50..(VT_QUEUE_SIZE - 1) { test_eq!(q.pop(), Some(i as u8)); }
        for i in (VT_QUEUE_SIZE - 1)..(VT_QUEUE_SIZE - 1 + 50) { test_eq!(q.pop(), Some(i as u8)); }
        test_eq!(q.pop(), None);
    });

    test_case!("vt_push_to_all_queues", {
        use crate::input::manager::{push_byte, pop_byte_from_vt};
        for _vt in 0..VT_COUNT {
            let _ = push_byte(b'X');
            let active = crate::input::active_vt();
            test_eq!(pop_byte_from_vt(active), Some(b'X'));
        }
    });

    test_case!("vt_count_at_least_2", {
        test_true!(VT_COUNT >= 2);
    });
}
