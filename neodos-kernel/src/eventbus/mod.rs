use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering, compiler_fence};
use spin::Mutex;
use core::ptr;
use crate::test_eq;
use crate::test_true;

// ── Event type constants ──
//
// ╔═══════════════════════════════════════════════════════════════════╗
// ║  ABI FROZEN at v0.42                                            ║
// ║  Event types 0–15 MUST NOT be reassigned.                       ║
// ║  New event types MUST start at 16+.                             ║
// ║  PCI events at 0x1000+ are reserved for hardware subsystems.    ║
// ║  User events at 0x2000+ are for NEM driver use.                ║
// ╚═══════════════════════════════════════════════════════════════════╝

pub type EventType = u32;
pub type EventSource = u32;

pub const EVENT_TIMER_TICK: EventType = 0;        // FROZEN v0.42
pub const EVENT_KEYBOARD_INPUT: EventType = 1;    // FROZEN v0.42
pub const EVENT_SERIAL_DATA: EventType = 2;       // FROZEN v0.42
pub const EVENT_DISK_IO_COMPLETE: EventType = 3;  // FROZEN v0.42
pub const EVENT_PROCESS_EXIT: EventType = 4;      // FROZEN v0.42
pub const EVENT_DRIVER_LOADED: EventType = 5;     // FROZEN v0.42
pub const EVENT_DRIVER_CRASH: EventType = 6;      // FROZEN v0.42
pub const EVENT_POLICY_VIOLATION: EventType = 7;  // FROZEN v0.42
pub const EVENT_FS_MOUNTED: EventType = 8;        // FROZEN v0.42
pub const EVENT_KEYB_LAYOUT: EventType = 9;       // FROZEN v0.42
pub const EVENT_RTC_READ: EventType = 10;         // FROZEN v0.42
pub const EVENT_RTC_DATA: EventType = 11;         // FROZEN v0.42
pub const EVENT_SHUTDOWN: EventType = 12;         // FROZEN v0.42
pub const EVENT_DRIVER_UNLOAD: EventType = 13;    // FROZEN v0.42
pub const EVENT_DRIVER_UNLOAD_ACK: EventType = 14;// FROZEN v0.42
pub const EVENT_NMI_WATCHDOG: EventType = 15;     // FROZEN v0.42
pub const EVENT_MOUSE_INPUT: EventType = 16;      // PS/2 mouse raw bytes
pub const EVENT_NETWORK_PACKET: EventType = 17;   // NIC received a packet (data0=nic_id, data1=len)
// ── PCI / MSI events (must match pci.nem constants) ──
/// Kernel → pci.nem: read a config dword.  data0[31:0] = packed BDF+offset.
pub const EVENT_PCI_READ_CONFIG: EventType    = 0x1000;
/// Kernel → pci.nem: write a config dword.  data0 = packed BDF+offset, data1 = value.
pub const EVENT_PCI_WRITE_CONFIG: EventType   = 0x1001;
/// pci.nem → kernel: result of a config read.  data0 = packed BDF+offset, data1 = value.
pub const EVENT_PCI_READ_RESULT: EventType    = 0x1002;
/// pci.nem → kernel: config write acknowledged.
pub const EVENT_PCI_WRITE_DONE: EventType     = 0x1003;
/// Kernel → pci.nem: configure MSI for a device.
/// data0[63:32] = vector, data0[31:0] = packed BDF, data1 = cap_offset.
pub const EVENT_MSI_CONFIGURE: EventType      = 0x1010;
/// pci.nem → kernel: MSI configured OK.  data0 = packed BDF.
pub const EVENT_MSI_CONFIGURED: EventType     = 0x1011;
pub const EVENT_KEYDOWN: EventType = 27;
pub const EVENT_KEYUP: EventType = 28;
pub const EVENT_KEY_CHAR: EventType = 29;
pub const EVENT_KBD_MODIFIER: EventType = 30;
pub const EVENT_KBD_REPEAT: EventType = 31;
pub const EVENT_USER: EventType = 0x2000;
pub const EVENT_WILDCARD: EventType = 0xFFFFFFFF;

pub const SOURCE_HAL: EventSource = 0;
pub const SOURCE_DRIVER: EventSource = 1;
pub const SOURCE_KERNEL: EventSource = 2;
pub const SOURCE_USERLAND: EventSource = 3;

pub const EVENT_FLAG_NONE: u32 = 0;
pub const EVENT_FLAG_URGENT: u32 = 1 << 0;
pub const EVENT_FLAG_BROADCAST: u32 = 1 << 1;
pub const EVENT_FLAG_DYN_PAYLOAD: u32 = 1 << 4;

pub const ERR_EVENT_BUS_FULL: i64 = -16;

// ── Event struct (ABI-stable, used by NEM drivers) ──

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
    event_id: 0, event_type: 0, source: 0, timestamp: 0,
    device_id: 0, driver_target: 0, data0: 0, data1: 0, flags: 0,
};

// ── Callback ──

pub type EventCallback = fn(&Event);

// ── Priority & Filter ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EventPriority {
    High = 0,
    Normal = 1,
}

#[derive(Debug, Clone, Copy)]
pub struct EventFilter {
    pub event_type: EventType,
    pub source_mask: u32,
    pub device_id: u32,
    pub match_device: bool,
}

impl EventFilter {
    pub const fn wildcard() -> Self {
        EventFilter { event_type: EVENT_WILDCARD, source_mask: 0xFFFF_FFFF, device_id: 0, match_device: false }
    }

    pub const fn by_type(event_type: EventType) -> Self {
        EventFilter { event_type, source_mask: 0xFFFF_FFFF, device_id: 0, match_device: false }
    }

    pub const fn by_type_and_source(event_type: EventType, source_mask: u32) -> Self {
        EventFilter { event_type, source_mask, device_id: 0, match_device: false }
    }

    pub const fn strict(event_type: EventType, source: EventSource, device_id: u32) -> Self {
        EventFilter { event_type, source_mask: 1 << source, device_id, match_device: true }
    }

    pub fn matches(&self, event: &Event) -> bool {
        if self.event_type != EVENT_WILDCARD && self.event_type != event.event_type {
            return false;
        }
        if self.source_mask != 0xFFFF_FFFF && (self.source_mask & (1 << event.source)) == 0 {
            return false;
        }
        if self.match_device && self.device_id != event.device_id {
            return false;
        }
        true
    }
}

// ── Handler entry ──

#[derive(Debug, Clone, Copy)]
pub struct HandlerEntry {
    pub filter: EventFilter,
    pub callback: EventCallback,
    pub name: &'static str,
}

// ── Queue sizes ──

const NORMAL_QUEUE_SIZE: usize = 64;
const HIGH_QUEUE_SIZE: usize = 16;
const MAX_HANDLERS: usize = 64;

// ── High-priority queue ──

pub struct HighPriorityQueue {
    buffer: UnsafeCell<[Event; HIGH_QUEUE_SIZE]>,
    head: AtomicUsize,
    tail: AtomicUsize,
}

unsafe impl Sync for HighPriorityQueue {}

impl HighPriorityQueue {
    pub const fn new() -> Self {
        HighPriorityQueue {
            buffer: UnsafeCell::new([EMPTY_EVENT; HIGH_QUEUE_SIZE]),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    pub fn push(&self, event: &Event) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let next = (tail + 1) % HIGH_QUEUE_SIZE;
        if next == head { return false; }
        unsafe { (*self.buffer.get())[tail] = *event; }
        compiler_fence(Ordering::Release);
        self.tail.store(next, Ordering::Release);
        true
    }

    pub fn pop(&self) -> Option<Event> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head == tail { return None; }
        let event = unsafe { (*self.buffer.get())[head] };
        compiler_fence(Ordering::Release);
        self.head.store((head + 1) % HIGH_QUEUE_SIZE, Ordering::Release);
        Some(event)
    }
}

// ── Event Bus (single unified version) ──

pub struct EventBus {
    /// Normal-priority queue (64 slots, lock-free SPSC)
    queue: UnsafeCell<[Event; NORMAL_QUEUE_SIZE]>,
    head: AtomicUsize,
    tail: AtomicUsize,

    /// High-priority queue (16 slots, lock-free SPSC)
    high_queue: HighPriorityQueue,

    /// Monotonic event ID counter
    next_id: AtomicU64,

    /// Handlers with subscription filters (up to 64)
    handlers: Mutex<[Option<HandlerEntry>; MAX_HANDLERS]>,
}

impl EventBus {
    pub const fn new() -> Self {
        EventBus {
            queue: UnsafeCell::new([EMPTY_EVENT; NORMAL_QUEUE_SIZE]),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            high_queue: HighPriorityQueue::new(),
            next_id: AtomicU64::new(1),
            handlers: Mutex::new([None; MAX_HANDLERS]),
        }
    }

    // ── Event injection ──

    /// Push to normal-priority queue (backward-compatible API)
    pub fn push_event(
        &self,
        event_type: EventType,
        source: EventSource,
        device_id: u32,
        data0: u64,
        data1: u64,
        flags: u32,
    ) -> Result<u64, ()> {
        self.push_event_priority(event_type, source, device_id, data0, data1, flags, EventPriority::Normal)
    }

    /// Push event with explicit priority
    #[allow(clippy::too_many_arguments)]
    pub fn push_event_priority(
        &self,
        event_type: EventType,
        source: EventSource,
        device_id: u32,
        data0: u64,
        data1: u64,
        flags: u32,
        priority: EventPriority,
    ) -> Result<u64, ()> {
        let event_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let timestamp = crate::hal::get_ticks();
        let event = Event {
            event_id, event_type, source, timestamp, device_id,
            driver_target: 0, data0, data1, flags,
        };
        match priority {
            EventPriority::High => {
                if self.high_queue.push(&event) { Ok(event_id) } else { Err(()) }
            }
            EventPriority::Normal => {
                let tail = self.tail.load(Ordering::Relaxed);
                let head = self.head.load(Ordering::Acquire);
                let next = (tail + 1) % NORMAL_QUEUE_SIZE;
                if next == head { return Err(()); }
                unsafe { (*self.queue.get())[tail] = event; }
                compiler_fence(Ordering::Release);
                self.tail.store(next, Ordering::Release);
                Ok(event_id)
            }
        }
    }

    // ── Event consumption ──

    fn pop_normal(&self) -> Option<Event> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head == tail { return None; }
        let event = unsafe { (*self.queue.get())[head] };
        compiler_fence(Ordering::Release);
        self.head.store((head + 1) % NORMAL_QUEUE_SIZE, Ordering::Release);
        Some(event)
    }

    /// Pop highest-priority event available
    fn pop(&self) -> Option<Event> {
        self.high_queue.pop().or_else(|| self.pop_normal())
    }

    // ── Handler registration ──

    /// Register a handler by event type (backward-compatible wrapper that
    /// creates a v2 handler with an EventFilter::by_type filter)
    pub fn register_handler(
        &self,
        event_type: EventType,
        callback: EventCallback,
        name: &'static str,
    ) -> Result<(), ()> {
        self.register_handler_v2(EventFilter::by_type(event_type), callback, name)
    }

    /// Register a handler with a subscription filter
    pub fn register_handler_v2(
        &self,
        filter: EventFilter,
        callback: EventCallback,
        name: &'static str,
    ) -> Result<(), ()> {
        let mut handlers = self.handlers.lock();
        for slot in handlers.iter_mut() {
            if slot.is_none() {
                *slot = Some(HandlerEntry { filter, callback, name });
                return Ok(());
            }
        }
        Err(())
    }

    /// Unregister a handler by callback function pointer
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

    fn dispatch_one_event(&self, event: &Event) {
        let handlers = self.handlers.lock();
        for h in handlers.iter().flatten() {
            if h.filter.matches(event) {
                (h.callback)(event);
            }
        }
        drop(handlers);
    }

    /// Pop and dispatch the next event (high priority first)
    pub fn dispatch_one(&self) -> bool {
        self.pop().is_some_and(|e| { self.dispatch_one_event(&e); true })
    }

    /// Drain and dispatch all pending events (high first, then normal)
    pub fn dispatch_pending(&self) -> usize {
        let mut count = 0;
        while self.dispatch_one() {
            count += 1;
        }
        count
    }

}

unsafe impl Sync for EventBus {}

// ── Global singleton ──

pub static EVENT_BUS: EventBus = EventBus::new();

// ── Convenience wrappers ──

pub fn push_event(
    event_type: EventType, source: EventSource, device_id: u32,
    data0: u64, data1: u64, flags: u32,
) -> Result<u64, ()> {
    EVENT_BUS.push_event(event_type, source, device_id, data0, data1, flags)
}

// ── Tests ──

fn tevent_create() -> Result<(), &'static str> {
    let event = Event {
        event_id: 42, event_type: EVENT_TIMER_TICK, source: SOURCE_HAL,
        timestamp: 1000, device_id: 1, driver_target: 0,
        data0: 7, data1: 0, flags: EVENT_FLAG_NONE,
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
    for i in 0..NORMAL_QUEUE_SIZE + 10 {
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
    test_true!(bus.unregister_handler(dummy));
    test_true!(!bus.unregister_handler(dummy));
    Ok(())
}

fn tevent_empty_queue() -> Result<(), &'static str> {
    let bus = EventBus::new();
    test_true!(bus.pop().is_none());
    test_eq!(bus.dispatch_pending(), 0);
    Ok(())
}

fn tevent_v2_filter_by_type() -> Result<(), &'static str> {
    static CALLED: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
    fn handler(e: &Event) { CALLED.store(e.event_type as u64, core::sync::atomic::Ordering::Relaxed); }
    let bus = EventBus::new();
    bus.register_handler_v2(EventFilter::by_type(EVENT_TIMER_TICK), handler, "v2_timer").unwrap();
    bus.push_event(EVENT_KEYBOARD_INPUT, SOURCE_HAL, 3, 0, 0, EVENT_FLAG_NONE).unwrap();
    bus.dispatch_pending();
    test_eq!(CALLED.load(core::sync::atomic::Ordering::Relaxed), 0);
    bus.push_event(EVENT_TIMER_TICK, SOURCE_HAL, 1, 0, 0, EVENT_FLAG_NONE).unwrap();
    bus.dispatch_pending();
    test_eq!(CALLED.load(core::sync::atomic::Ordering::Relaxed), EVENT_TIMER_TICK as u64);
    Ok(())
}

fn tevent_strict_filter() -> Result<(), &'static str> {
    static CALLED: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
    fn handler(e: &Event) { CALLED.store(e.device_id as u64, core::sync::atomic::Ordering::Relaxed); }
    let bus = EventBus::new();
    bus.register_handler_v2(EventFilter::strict(EVENT_KEYBOARD_INPUT, SOURCE_HAL, 3), handler, "v2_kbd").unwrap();
    bus.push_event(EVENT_KEYBOARD_INPUT, SOURCE_HAL, 4, 0, 0, EVENT_FLAG_NONE).unwrap();
    bus.dispatch_pending();
    test_eq!(CALLED.load(core::sync::atomic::Ordering::Relaxed), 0);
    bus.push_event(EVENT_KEYBOARD_INPUT, SOURCE_HAL, 3, 0, 0, EVENT_FLAG_NONE).unwrap();
    bus.dispatch_pending();
    test_eq!(CALLED.load(core::sync::atomic::Ordering::Relaxed), 3);
    Ok(())
}

fn tevent_filter_wildcard() -> Result<(), &'static str> {
    let filter = EventFilter::wildcard();
    let e1 = Event { event_id: 1, event_type: EVENT_TIMER_TICK, source: SOURCE_HAL, timestamp: 0, device_id: 0, driver_target: 0, data0: 0, data1: 0, flags: 0 };
    let e2 = Event { event_id: 2, event_type: EVENT_SHUTDOWN, source: SOURCE_KERNEL, timestamp: 0, device_id: 0, driver_target: 0, data0: 0, data1: 0, flags: 0 };
    test_true!(filter.matches(&e1));
    test_true!(filter.matches(&e2));
    Ok(())
}

fn tevent_filter_source_mask() -> Result<(), &'static str> {
    let filter = EventFilter::by_type_and_source(EVENT_TIMER_TICK, 1 << SOURCE_HAL);
    let hal = Event { event_id: 1, event_type: EVENT_TIMER_TICK, source: SOURCE_HAL, timestamp: 0, device_id: 0, driver_target: 0, data0: 0, data1: 0, flags: 0 };
    let kern = Event { event_id: 2, event_type: EVENT_TIMER_TICK, source: SOURCE_KERNEL, timestamp: 0, device_id: 0, driver_target: 0, data0: 0, data1: 0, flags: 0 };
    test_true!(filter.matches(&hal));
    test_true!(!filter.matches(&kern));
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
    crate::testing::register("ev_filter_by_type", tevent_v2_filter_by_type);
    crate::testing::register("ev_strict_filter", tevent_strict_filter);
    crate::testing::register("ev_filter_wildcard", tevent_filter_wildcard);
    crate::testing::register("ev_filter_source_mask", tevent_filter_source_mask);
}
