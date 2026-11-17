//! Raw keyboard event ring + virtual-key mapping (#466).
//!
//! A bounded, lock-free SPSC ring of [`KeyEvent`]s, one per VT. The producer is
//! the PS/2 scancode path (`kbd::process_scancode`, IRQ/DPC context); the
//! consumer is the foreground Ring-3 process via `ObInfoClass::KeyboardRead` /
//! `KeyboardPoll`. A full ring overwrites the oldest entry and bumps `dropped`
//! (a game prefers losing an old key to stalling an IRQ).

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Ring capacity (events).
pub const KEY_RING_CAP: usize = 256;

/// `KeyEvent.flags`: E0-prefixed (extended) key.
pub const KBD_EV_EXTENDED: u8 = 0x01;
/// `KeyEvent.flags`: auto-repeat.
pub const KBD_EV_REPEAT: u8 = 0x02;

// Virtual-key codes (a stable enumeration so clients need not hard-code
// scancodes). Values follow the widespread Windows VK numbering.
pub const VK_BACK: u16 = 0x08;
pub const VK_TAB: u16 = 0x09;
pub const VK_RETURN: u16 = 0x0D;
pub const VK_SHIFT: u16 = 0x10;
pub const VK_CONTROL: u16 = 0x11;
pub const VK_MENU: u16 = 0x12; // Alt
pub const VK_ESCAPE: u16 = 0x1B;
pub const VK_SPACE: u16 = 0x20;
pub const VK_PRIOR: u16 = 0x21; // PageUp
pub const VK_NEXT: u16 = 0x22; // PageDown
pub const VK_END: u16 = 0x23;
pub const VK_HOME: u16 = 0x24;
pub const VK_LEFT: u16 = 0x25;
pub const VK_UP: u16 = 0x26;
pub const VK_RIGHT: u16 = 0x27;
pub const VK_DOWN: u16 = 0x28;
pub const VK_INSERT: u16 = 0x2D;
pub const VK_DELETE: u16 = 0x2E;
pub const VK_NUMPAD0: u16 = 0x60;
pub const VK_NUMPAD1: u16 = 0x61;
pub const VK_NUMPAD2: u16 = 0x62;
pub const VK_NUMPAD3: u16 = 0x63;
pub const VK_NUMPAD4: u16 = 0x64;
pub const VK_NUMPAD5: u16 = 0x65;
pub const VK_NUMPAD6: u16 = 0x66;
pub const VK_NUMPAD7: u16 = 0x67;
pub const VK_NUMPAD8: u16 = 0x68;
pub const VK_NUMPAD9: u16 = 0x69;
pub const VK_MULTIPLY: u16 = 0x6A;
pub const VK_ADD: u16 = 0x6B;
pub const VK_SUBTRACT: u16 = 0x6D;
pub const VK_DECIMAL: u16 = 0x6E;
pub const VK_DIVIDE: u16 = 0x6F;
pub const VK_F1: u16 = 0x70;
pub const VK_F2: u16 = 0x71;
pub const VK_F3: u16 = 0x72;
pub const VK_F4: u16 = 0x73;
pub const VK_F5: u16 = 0x74;
pub const VK_F6: u16 = 0x75;
pub const VK_F7: u16 = 0x76;
pub const VK_F8: u16 = 0x77;
pub const VK_F9: u16 = 0x78;
pub const VK_F10: u16 = 0x79;
pub const VK_F11: u16 = 0x7A;
pub const VK_F12: u16 = 0x7B;

/// Map a set-1 make scancode to a virtual key (#391: the E0 prefix splits
/// extended keys from their numpad collisions).
///
/// Returns 0 for keys with no dedicated VK (letters/digits: use `codepoint`).
pub fn vk_from_scancode(code: u8, extended: bool) -> u16 {
    let code = code & 0x7F;
    if extended {
        return match code {
            0x48 => VK_UP,
            0x50 => VK_DOWN,
            0x4B => VK_LEFT,
            0x4D => VK_RIGHT,
            0x47 => VK_HOME,
            0x4F => VK_END,
            0x49 => VK_PRIOR,
            0x51 => VK_NEXT,
            0x52 => VK_INSERT,
            0x53 => VK_DELETE,
            0x1D => VK_CONTROL, // right Ctrl
            0x38 => VK_MENU,    // right Alt
            0x1C => VK_RETURN,  // keypad Enter
            0x35 => VK_DIVIDE,  // keypad /
            _ => 0,
        };
    }
    match code {
        0x01 => VK_ESCAPE,
        0x0E => VK_BACK,
        0x0F => VK_TAB,
        0x1C => VK_RETURN,
        0x1D => VK_CONTROL,
        0x2A => VK_SHIFT,
        0x36 => VK_SHIFT,
        0x38 => VK_MENU,
        0x39 => VK_SPACE,
        0x3B..=0x44 => VK_F1 + (code as u16 - 0x3B),
        0x57 => VK_F11,
        0x58 => VK_F12,
        0x37 => VK_MULTIPLY,
        0x47 => VK_NUMPAD7,
        0x48 => VK_NUMPAD8,
        0x49 => VK_NUMPAD9,
        0x4A => VK_SUBTRACT,
        0x4B => VK_NUMPAD4,
        0x4C => VK_NUMPAD5,
        0x4D => VK_NUMPAD6,
        0x4E => VK_ADD,
        0x4F => VK_NUMPAD1,
        0x50 => VK_NUMPAD2,
        0x51 => VK_NUMPAD3,
        0x52 => VK_NUMPAD0,
        0x53 => VK_DECIMAL,
        _ => 0,
    }
}

/// A single raw keyboard event (16 bytes, `#[repr(C)]` — the Ring-3 ABI).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    /// Raw set-1 scancode (E0-prefixed keys keep the base code).
    pub scancode: u16,
    /// Virtual key ([`vk_from_scancode`]); 0 if none.
    pub vk: u16,
    /// Layout translation (0 on key-up / non-character keys).
    pub codepoint: u32,
    /// Modifier bitmask (KBD_SHIFT/CTRL/ALT/ALTGR/CAPS/NUMLOCK/SCROLLLOCK).
    pub modifiers: u8,
    /// 1 = make (down), 0 = break (up).
    pub pressed: u8,
    /// [`KBD_EV_EXTENDED`] / [`KBD_EV_REPEAT`].
    pub flags: u8,
    /// Padding to make the struct exactly 16 bytes (ABI).
    pub _pad: u32,
}

impl KeyEvent {
    pub const NONE: KeyEvent = KeyEvent {
        scancode: 0,
        vk: 0,
        codepoint: 0,
        modifiers: 0,
        pressed: 0,
        flags: 0,
        _pad: 0,
    };
}

/// Bounded lock-free SPSC ring of [`KeyEvent`]s.
pub struct KeyEventRing {
    buf: UnsafeCell<[KeyEvent; KEY_RING_CAP]>,
    head: AtomicUsize,
    tail: AtomicUsize,
    dropped: AtomicU64,
}

// SAFETY: single producer (IRQ/DPC) and single consumer (one Ring-3 thread or
// the kernel test thread); indices are atomic and the slot is published with a
// Release store / read with an Acquire load.
unsafe impl Sync for KeyEventRing {}

impl KeyEventRing {
    pub const fn new() -> Self {
        KeyEventRing {
            buf: UnsafeCell::new([KeyEvent::NONE; KEY_RING_CAP]),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            dropped: AtomicU64::new(0),
        }
    }

    /// Push an event. Full ring: overwrite the oldest entry and count a drop.
    pub fn push(&self, ev: KeyEvent) {
        let t = self.tail.load(Ordering::Relaxed);
        let h = self.head.load(Ordering::Acquire);
        if t.wrapping_sub(h) >= KEY_RING_CAP {
            self.head.store(h.wrapping_add(1), Ordering::Release);
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: `t % CAP` is in bounds; the producer owns `tail`.
        unsafe {
            (*self.buf.get())[t % KEY_RING_CAP] = ev;
        }
        self.tail.store(t.wrapping_add(1), Ordering::Release);
    }

    pub fn pop(&self) -> Option<KeyEvent> {
        let h = self.head.load(Ordering::Relaxed);
        let t = self.tail.load(Ordering::Acquire);
        if h == t {
            return None;
        }
        // SAFETY: `h % CAP` is in bounds; the consumer owns `head`.
        let ev = unsafe { (*self.buf.get())[h % KEY_RING_CAP] };
        self.head.store(h.wrapping_add(1), Ordering::Release);
        Some(ev)
    }

    pub fn has_data(&self) -> bool {
        self.head.load(Ordering::Acquire) != self.tail.load(Ordering::Acquire)
    }

    pub fn len(&self) -> usize {
        self.tail
            .load(Ordering::Acquire)
            .wrapping_sub(self.head.load(Ordering::Acquire))
    }

    pub fn is_empty(&self) -> bool {
        !self.has_data()
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn clear(&self) {
        let t = self.tail.load(Ordering::Acquire);
        self.head.store(t, Ordering::Release);
    }
}

pub fn register_tests() {
    use crate::{test_case, test_eq, test_true};

    test_case!("kbd_ring_push_pop_fifo", {
        let r = KeyEventRing::new();
        test_true!(r.is_empty());
        for i in 0..10u16 {
            let mut ev = KeyEvent::NONE;
            ev.scancode = i;
            r.push(ev);
        }
        test_eq!(r.len(), 10);
        for i in 0..10u16 {
            test_eq!(r.pop().unwrap().scancode, i);
        }
        test_true!(r.pop().is_none());
    });

    test_case!("kbd_ring_overflow_drops_oldest", {
        let r = KeyEventRing::new();
        for i in 0..(KEY_RING_CAP as u16 + 1) {
            let mut ev = KeyEvent::NONE;
            ev.scancode = i;
            r.push(ev);
        }
        test_eq!(r.len(), KEY_RING_CAP);
        test_eq!(r.dropped(), 1);
        // The oldest (scancode 0) was overwritten; the next pop is 1.
        test_eq!(r.pop().unwrap().scancode, 1);
    });

    test_case!("kbd_key_event_is_16_bytes", {
        test_eq!(core::mem::size_of::<KeyEvent>(), 16);
    });

    test_case!("kbd_vk_extended_split", {
        // #391: E0 arrows must not collide with numpad.
        test_eq!(vk_from_scancode(0x48, false), VK_NUMPAD8);
        test_eq!(vk_from_scancode(0x48, true), VK_UP);
        test_eq!(vk_from_scancode(0x50, true), VK_DOWN);
        test_eq!(vk_from_scancode(0x4B, true), VK_LEFT);
        test_eq!(vk_from_scancode(0x4D, true), VK_RIGHT);
        test_eq!(vk_from_scancode(0x47, true), VK_HOME);
        test_eq!(vk_from_scancode(0x4F, true), VK_END);
        test_eq!(vk_from_scancode(0x49, true), VK_PRIOR);
        test_eq!(vk_from_scancode(0x51, true), VK_NEXT);
        test_eq!(vk_from_scancode(0x52, true), VK_INSERT);
        test_eq!(vk_from_scancode(0x53, true), VK_DELETE);
        test_eq!(vk_from_scancode(0x3B, false), VK_F1);
        test_eq!(vk_from_scancode(0x44, false), VK_F10);
        test_eq!(vk_from_scancode(0x01, false), VK_ESCAPE);
    });
}
