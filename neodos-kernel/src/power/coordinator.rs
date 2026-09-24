use crate::log::LogSubsys;

pub fn shutdown() -> ! {
    kinfo!(LogSubsys::Power, "Shutting down...");
    crate::cm::cm_flush_all_hives();
    crate::globals::flush_cache_if_needed();
    let _ = crate::eventbus::EVENT_BUS.push_event(
        crate::eventbus::EVENT_SHUTDOWN,
        crate::eventbus::SOURCE_KERNEL,
        0, 0, 0, 0,
    );
    crate::eventbus::EVENT_BUS.dispatch_pending();
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
