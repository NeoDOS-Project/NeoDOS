use crate::log::LogSubsys;

pub fn shutdown() -> ! {
    ktrace!(LogSubsys::Power, "Shutting down: start");
    kinfo!(LogSubsys::Power, "Shutting down...");
    ktrace!(LogSubsys::Power, "Shutting down: flushing hives");
    crate::cm::cm_flush_all_hives();
    ktrace!(LogSubsys::Power, "Shutting down: hives flushed; flushing cache");
    crate::globals::flush_cache_if_needed();
    ktrace!(LogSubsys::Power, "Shutting down: cache flushed; dispatching EVENT_SHUTDOWN");
    let _ = crate::eventbus::EVENT_BUS.push_event(
        crate::eventbus::EVENT_SHUTDOWN,
        crate::eventbus::SOURCE_KERNEL,
        0, 0, 0, 0,
    );
    crate::eventbus::EVENT_BUS.dispatch_pending();
    ktrace!(LogSubsys::Power, "Shutting down: powering off via hal");
    crate::hal::poweroff();
}

pub fn reboot() -> ! {
    kinfo!(LogSubsys::Power, "Rebooting...");
    crate::cm::cm_flush_all_hives();
    crate::globals::flush_cache_if_needed();
    let _ = crate::eventbus::EVENT_BUS.push_event(
        crate::eventbus::EVENT_SHUTDOWN,
        crate::eventbus::SOURCE_KERNEL,
        0, 0, 0, 0,
    );
    crate::eventbus::EVENT_BUS.dispatch_pending();
    crate::hal::reboot();
}

/// Emergency reset used from IRQ/terminal context (watchdog expiry). Skips the
/// registry and page-cache flush, which take blocking locks that the interrupted
/// CPU may hold (INV-3, Rule 3.5.1) — the machine is already declared hung, so
/// resetting without flushing is preferable to deadlocking the timer IRQ.
pub fn emergency_reboot() -> ! {
    kerror!(LogSubsys::Power, "Emergency reboot (no registry/cache flush)");
    crate::hal::reboot();
}
