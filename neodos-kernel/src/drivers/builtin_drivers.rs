// Built-in .nem driver behaviour callbacks
// Each maps a NemDriverType to one or more Event Bus handlers.

use crate::eventbus::{self, Event, EVENT_TIMER_TICK, EVENT_KEYBOARD_INPUT};
use crate::nem::NemDriverType;
use crate::drivers::driver_runtime::DRIVER_RUNTIME;

// ── null driver callback ──

pub fn null_callback(event: &Event) {
    let mut r = DRIVER_RUNTIME.lock();
    let drv_id = r.get_by_driver_type(NemDriverType::Null).map(|d| d.id);
    if let Some(id) = drv_id {
        r.record_event(id, event.event_type, event.timestamp);
        if event.event_type == EVENT_TIMER_TICK {
            r.increment_tick(id);
        }
    }
}

// ── echo driver callback ──

pub fn echo_callback(event: &Event) {
    let mut r = DRIVER_RUNTIME.lock();
    let drv_id = r.get_by_driver_type(NemDriverType::Echo).map(|d| d.id);
    drop(r);

    let mut r = DRIVER_RUNTIME.lock();
    if let Some(id) = drv_id {
        r.record_event(id, event.event_type, event.timestamp);
        if event.event_type == EVENT_TIMER_TICK {
            r.increment_tick(id);
        }
    }
    drop(r);

    // Keep runtime accounting for echo driver, but avoid serial spam.
}

// ── timer_listener driver callback ──

pub fn timer_listener_callback(event: &Event) {
    if event.event_type != EVENT_TIMER_TICK {
        return;
    }

    let _ = {
        let mut r = DRIVER_RUNTIME.lock();
        let drv_id = r.get_by_driver_type(NemDriverType::Lifecycle).map(|d| d.id);
        if let Some(id) = drv_id {
            r.record_event(id, event.event_type, event.timestamp);
            r.increment_tick(id);
            r.get(id).map(|d| d.tick_count).unwrap_or(0)
        } else {
            0
        }
    };
}

// ── Boot-time init ──
// Registers each built-in driver's callback for the event types it cares about.

pub fn init() {
    // null: receives TIMER_TICK
    let _ = eventbus::EVENT_BUS.register_handler(EVENT_TIMER_TICK, null_callback, "null");

    // echo: receives TIMER_TICK + KEYBOARD_INPUT
    let _ = eventbus::EVENT_BUS.register_handler(EVENT_TIMER_TICK, echo_callback, "echo");
    let _ = eventbus::EVENT_BUS.register_handler(EVENT_KEYBOARD_INPUT, echo_callback, "echo");

    // timer_listener: receives TIMER_TICK
    let _ = eventbus::EVENT_BUS.register_handler(EVENT_TIMER_TICK, timer_listener_callback, "timer");

    crate::serial_println!("[DRV] Built-in driver callbacks registered");
}
