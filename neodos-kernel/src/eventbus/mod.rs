#![allow(dead_code)]
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering, compiler_fence};
use spin::Mutex;
use crate::test_eq;
use crate::test_true;
use core::ptr;

// ── Event type constants ──

pub type EventType = u32;
pub type EventSource = u32;

pub const EVENT_TIMER_TICK: EventType = 0;
pub const EVENT_KEYBOARD_INPUT: EventType = 1;
pub const EVENT_SERIAL_DATA: EventType = 2;
pub const EVENT_DISK_IO_COMPLETE: EventType = 3;
pub const EVENT_PROCESS_EXIT: EventType = 4;
pub const EVENT_DRIVER_LOADED: EventType = 5;
pub const EVENT_DRIVER_CRASH: EventType = 6;
pub const EVENT_POLICY_VIOLATION: EventType = 7;
pub const EVENT_FS_MOUNTED: EventType = 8;
pub const EVENT_KEYB_LAYOUT: EventType = 9;
pub const EVENT_USER: EventType = 0x1000;
pub const EVENT_WILDCARD: EventType = 0xFFFFFFFF;

pub const SOURCE_HAL: EventSource = 0;
pub const SOURCE_DRIVER: EventSource = 1;
pub const SOURCE_KERNEL: EventSource = 2;
pub const SOURCE_USERLAND: EventSource = 3;

pub const EVENT_FLAG_NONE: u32 = 0;
pub const EVENT_FLAG_URGENT: u32 = 1 << 0;
pub const EVENT_FLAG_BROADCAST: u32 = 1 << 1;

// ── Event structure ──

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Event {
    pub event_id: u64,
    pub event_type: EventType,
    pub source: EventSource,
    pub timestamp: u64,
    pub device_id: u32,
    pub driver_target: u32,
    pub data0: u64,
    pub data1: u64,
    pub flags: u32,
}

const EMPTY_EVENT: Event = Event {
    event_id: 0,
    event_type: 0,
    source: 0,
    timestamp: 0,
    device_id: 0,
    driver_target: 0,
    data0: 0,
    data1: 0,
    flags: 0,
};

// ── Callback model ──

pub type EventCallback = fn(&Event);

#[derive(Clone, Copy)]
struct RegisteredHandler {
    event_type: EventType,
    callback: EventCallback,
    name: &'static str,
}

// ── Queue sizes ──

const QUEUE_SIZE: usize = 64;
const MAX_HANDLERS: usize = 32;

// ── Event Bus ──

pub struct EventBus {
    queue: UnsafeCell<[Event; QUEUE_SIZE]>,
    head: AtomicUsize,
    tail: AtomicUsize,
    next_id: AtomicU64,
    handlers: Mutex<[Option<RegisteredHandler>; MAX_HANDLERS]>,
}

impl EventBus {
    pub const fn new() -> Self {
        EventBus {
            queue: UnsafeCell::new([EMPTY_EVENT; QUEUE_SIZE]),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            next_id: AtomicU64::new(1),
            handlers: Mutex::new([None; MAX_HANDLERS]),
        }
    }

    // ── Event injection (IRQ‑safe, lock‑free) ──

    pub fn push_event(
        &self,
        event_type: EventType,
        source: EventSource,
        device_id: u32,
        data0: u64,
        data1: u64,
        flags: u32,
    ) -> Result<u64, ()> {
        let event_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let timestamp = crate::hal::get_ticks();
        let event = Event {
            event_id,
            event_type,
            source,
            timestamp,
            device_id,
            driver_target: 0,
            data0,
            data1,
            flags,
        };
        self.push_raw(&event)
    }

    fn push_raw(&self, event: &Event) -> Result<u64, ()> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let next = (tail + 1) % QUEUE_SIZE;
        if next == head {
            return Err(());
        }
        unsafe {
            let queue = &mut *self.queue.get();
            (*queue)[tail] = *event;
        }
        compiler_fence(Ordering::Release);
        self.tail.store(next, Ordering::Release);
        Ok(event.event_id)
    }

    // ── Event consumption (scheduler / shell context) ──

    fn pop(&self) -> Option<Event> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        let event = unsafe {
            let queue = &*self.queue.get();
            (*queue)[head]
        };
        compiler_fence(Ordering::Release);
        self.head.store((head + 1) % QUEUE_SIZE, Ordering::Release);
        Some(event)
    }

    // ── Handler registration ──

    pub fn register_handler(
        &self,
        event_type: EventType,
        callback: EventCallback,
        name: &'static str,
    ) -> Result<(), ()> {
        let mut handlers = self.handlers.lock();
        for slot in handlers.iter_mut() {
            if slot.is_none() {
                *slot = Some(RegisteredHandler { event_type, callback, name });
                return Ok(());
            }
        }
        Err(())
    }

    pub fn unregister_handler(&self, callback: EventCallback) -> bool {
        let mut handlers = self.handlers.lock();
        for slot in handlers.iter_mut() {
            if let Some(h) = slot {
                if ptr::fn_addr_eq(h.callback, callback) {
                    *slot = None;
                    return true;
                }
            }
        }
        false
    }

    // ── Dispatch ──

    pub fn dispatch_one(&self) -> bool {
        let event = match self.pop() {
            Some(e) => e,
            None => return false,
        };
        let handlers = self.handlers.lock();
        for h in handlers.iter().flatten() {
            if h.event_type == event.event_type
                || h.event_type == EVENT_USER
                || h.event_type == EVENT_WILDCARD
            {
                (h.callback)(&event);
            }
        }
        true
    }

    pub fn dispatch_pending(&self) -> usize {
        let mut count = 0;
        while self.dispatch_one() {
            count += 1;
        }
        count
    }

    // ── Diagnostics ──

    pub fn queue_available(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);
        if tail >= head {
            QUEUE_SIZE - 1 - (tail - head)
        } else {
            head - tail - 1
        }
    }

    pub fn handler_count(&self) -> usize {
        let handlers = self.handlers.lock();
        handlers.iter().filter(|s| s.is_some()).count()
    }

    pub fn next_event_id(&self) -> u64 {
        self.next_id.load(Ordering::Relaxed)
    }
}

unsafe impl Sync for EventBus {}

// ── Global singleton ──

pub static EVENT_BUS: EventBus = EventBus::new();

// ── Convenience wrappers ──

pub fn push_event(
    event_type: EventType,
    source: EventSource,
    device_id: u32,
    data0: u64,
    data1: u64,
    flags: u32,
) -> Result<u64, ()> {
    EVENT_BUS.push_event(event_type, source, device_id, data0, data1, flags)
}

pub fn register_handler(
    event_type: EventType,
    callback: EventCallback,
    name: &'static str,
) -> Result<(), ()> {
    EVENT_BUS.register_handler(event_type, callback, name)
}

pub fn dispatch_pending() -> usize {
    EVENT_BUS.dispatch_pending()
}

// ── Tests ──

fn tevent_create() -> Result<(), &'static str> {
    let event = Event {
        event_id: 42,
        event_type: EVENT_TIMER_TICK,
        source: SOURCE_HAL,
        timestamp: 1000,
        device_id: 1,
        driver_target: 0,
        data0: 7,
        data1: 0,
        flags: EVENT_FLAG_NONE,
    };
    test_eq!(event.event_id, 42);
    test_eq!(event.event_type, EVENT_TIMER_TICK);
    test_eq!(event.source, SOURCE_HAL);
    test_eq!(event.timestamp, 1000);
    test_eq!(event.device_id, 1);
    test_eq!(event.data0, 7);
    Ok(())
}

fn tevent_push_pop() -> Result<(), &'static str> {
    let bus = EventBus::new();
    let id = bus.push_event(EVENT_TIMER_TICK, SOURCE_HAL, 1, 0, 0, EVENT_FLAG_NONE).unwrap();
    test_true!(id > 0);
    let popped = bus.pop().unwrap();
    test_eq!(popped.event_id, id);
    test_eq!(popped.event_type, EVENT_TIMER_TICK);
    test_eq!(popped.source, SOURCE_HAL);
    test_eq!(popped.device_id, 1);
    Ok(())
}

fn tevent_queue_order() -> Result<(), &'static str> {
    let bus = EventBus::new();
    let id1 = bus.push_event(EVENT_TIMER_TICK, SOURCE_HAL, 1, 10, 0, EVENT_FLAG_NONE).unwrap();
    let id2 = bus.push_event(EVENT_KEYBOARD_INPUT, SOURCE_HAL, 3, 0x1E, 0, EVENT_FLAG_NONE).unwrap();
    let id3 = bus.push_event(EVENT_SERIAL_DATA, SOURCE_HAL, 2, b'A' as u64, 0, EVENT_FLAG_NONE).unwrap();
    test_eq!(bus.pop().unwrap().event_id, id1);
    test_eq!(bus.pop().unwrap().event_id, id2);
    test_eq!(bus.pop().unwrap().event_id, id3);
    test_true!(bus.pop().is_none());
    Ok(())
}

fn tevent_queue_overflow() -> Result<(), &'static str> {
    let bus = EventBus::new();
    let mut last_ok = 0u64;
    for i in 0..QUEUE_SIZE + 10 {
        match bus.push_event(EVENT_TIMER_TICK, SOURCE_HAL, 1, i as u64, 0, EVENT_FLAG_NONE) {
            Ok(id) => last_ok = id,
            Err(_) => break,
        }
    }
    test_true!(last_ok > 0);
    test_eq!(bus.pop().unwrap().data0, 0);
    Ok(())
}

fn tevent_monotonic_id() -> Result<(), &'static str> {
    let bus = EventBus::new();
    let id1 = bus.push_event(EVENT_TIMER_TICK, SOURCE_HAL, 1, 0, 0, EVENT_FLAG_NONE).unwrap();
    let id2 = bus.push_event(EVENT_TIMER_TICK, SOURCE_HAL, 1, 0, 0, EVENT_FLAG_NONE).unwrap();
    let id3 = bus.push_event(EVENT_TIMER_TICK, SOURCE_HAL, 1, 0, 0, EVENT_FLAG_NONE).unwrap();
    test_true!(id1 < id2);
    test_true!(id2 < id3);
    Ok(())
}

fn tevent_handler_register_dispatch() -> Result<(), &'static str> {
    static CALLED: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
    fn handler(e: &Event) { CALLED.store(e.data0, core::sync::atomic::Ordering::Relaxed); }
    let bus = EventBus::new();
    bus.register_handler(EVENT_TIMER_TICK, handler, "test_handler").unwrap();
    bus.push_event(EVENT_TIMER_TICK, SOURCE_HAL, 1, 42, 0, EVENT_FLAG_NONE).unwrap();
    bus.dispatch_one();
    test_eq!(CALLED.load(core::sync::atomic::Ordering::Relaxed), 42);
    Ok(())
}

fn tevent_handler_type_filter() -> Result<(), &'static str> {
    static TIMER_CALLED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    static KBD_CALLED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    fn timer_h(_: &Event) { TIMER_CALLED.store(true, core::sync::atomic::Ordering::Relaxed); }
    fn kbd_h(_: &Event) { KBD_CALLED.store(true, core::sync::atomic::Ordering::Relaxed); }
    let bus = EventBus::new();
    bus.register_handler(EVENT_TIMER_TICK, timer_h, "timer").unwrap();
    bus.register_handler(EVENT_KEYBOARD_INPUT, kbd_h, "kbd").unwrap();
    bus.push_event(EVENT_TIMER_TICK, SOURCE_HAL, 1, 0, 0, EVENT_FLAG_NONE).unwrap();
    bus.dispatch_one();
    test_true!(TIMER_CALLED.load(core::sync::atomic::Ordering::Relaxed));
    test_true!(!KBD_CALLED.load(core::sync::atomic::Ordering::Relaxed));
    Ok(())
}

fn tevent_unregister_handler() -> Result<(), &'static str> {
    fn dummy(_: &Event) {}
    let bus = EventBus::new();
    bus.register_handler(EVENT_TIMER_TICK, dummy, "dummy").unwrap();
    test_eq!(bus.handler_count(), 1);
    test_true!(bus.unregister_handler(dummy));
    test_eq!(bus.handler_count(), 0);
    Ok(())
}

fn tevent_empty_queue() -> Result<(), &'static str> {
    let bus = EventBus::new();
    test_true!(bus.pop().is_none());
    test_eq!(bus.dispatch_pending(), 0);
    Ok(())
}

pub fn register_tests() {
    crate::testing::register("event_create", tevent_create);
    crate::testing::register("event_push_pop", tevent_push_pop);
    crate::testing::register("event_queue_order", tevent_queue_order);
    crate::testing::register("event_queue_overflow", tevent_queue_overflow);
    crate::testing::register("event_monotonic_id", tevent_monotonic_id);
    crate::testing::register("event_handler_register_dispatch", tevent_handler_register_dispatch);
    crate::testing::register("event_handler_type_filter", tevent_handler_type_filter);
    crate::testing::register("event_unregister_handler", tevent_unregister_handler);
    crate::testing::register("event_empty_queue", tevent_empty_queue);
}
