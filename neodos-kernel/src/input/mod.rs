pub mod vt;
pub mod manager;

pub use vt::VtInputQueue;
pub use manager::{init, active_vt, switch_vt, push_byte, pop_byte_from_vt};

pub type InputBuffer = VtInputQueue;

// ── Tests ──────────────────────────────────────────────────────────

pub fn register_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_ne;

    test_case!("input_empty_pop", {
        let buf = InputBuffer::new();
        test_eq!(buf.pop(), None);
    });

    test_case!("input_push_pop_one", {
        let buf = InputBuffer::new();
        test_eq!(buf.push(42), Ok(()));
        test_eq!(buf.pop(), Some(42));
        test_eq!(buf.pop(), None);
    });

    test_case!("input_buffer_capacity", {
        let buf = InputBuffer::new();
        let mut count = 0;
        while buf.push(count as u8).is_ok() {
            count += 1;
        }
        test_ne!(count, 0);
        test_eq!(buf.push(0), Err(()));
    });

    test_case!("input_wrap_around", {
        let buf = InputBuffer::new();
        for i in 0..100 { let _ = buf.push(i); }
        for i in 0..50 { test_eq!(buf.pop(), Some(i)); }
        for i in 100..150 { let _ = buf.push(i); }
        for i in 50..100 { test_eq!(buf.pop(), Some(i)); }
        for i in 100..150 { test_eq!(buf.pop(), Some(i)); }
        test_eq!(buf.pop(), None);
    });

    test_case!("input_full_then_drain", {
        let buf = InputBuffer::new();
        while buf.push(0xFF).is_ok() {}
        let mut count = 0;
        while buf.pop().is_some() {
            count += 1;
        }
        test_ne!(count, 0);
    });
}
