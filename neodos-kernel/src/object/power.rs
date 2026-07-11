use crate::object::{
    ObOperations, ObType, ObId, ob_create_object,
};
use crate::object::namespace;

pub struct PowerManagerOps;

impl ObOperations for PowerManagerOps {
    fn on_destroy(&self, _id: ObId, _native_id: u64) {}
}

pub static POWER_MANAGER_OPS: PowerManagerOps = PowerManagerOps;

pub fn init_power_manager() {
    let ob_id = match ob_create_object(
        ObType::PowerManager,
        "System\\PowerManager",
        0,
        0,
        Some(&POWER_MANAGER_OPS),
    ) {
        Ok(id) => id,
        Err(_) => {
            crate::serial_println!("[POWER] Failed to create PowerManager object");
            return;
        }
    };
    let _ = namespace::ob_create_directory("\\System");
    let _ = namespace::ob_insert_object("\\System\\PowerManager", ob_id);
    crate::serial_println!("[POWER] PowerManager registered at \\System\\PowerManager (ObId={})", ob_id);
}

pub fn power_shutdown() -> ! {
    crate::serial_println!("[POWER] Shutting down...");
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

pub fn power_reboot() -> ! {
    crate::serial_println!("[POWER] Rebooting...");
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
