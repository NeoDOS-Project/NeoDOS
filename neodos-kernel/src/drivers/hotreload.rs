// Hot Reload — driver unload/reload with graceful drain, ownership tracking, and ABI check
//
// W2. Hot reload drivers — Runtime loading/unloading/reloading without reboot
//
// Features:
//   1. Graceful unload: calls driver_fini(), sends EVENT_DRIVER_UNLOAD, waits for ack
//   2. Force unload: skips ack wait, marks orphaned resources
//   3. Resource tracking: block devices, event handlers tracked per driver
//   4. Version check: ABI compatibility verified on reload
//   5. Clean state machine integration: Active → Unloading → Unloaded → Loaded

use alloc::string::ToString;
use crate::drivers::driver_runtime::{self, DriverId, DriverState, PipelineStep};
use crate::drivers::nem::v3loader;
use crate::drivers::nem::v3loader::NemV3LoadResult;
use crate::eventbus::{EVENT_DRIVER_UNLOAD, EVENT_DRIVER_UNLOAD_ACK, SOURCE_KERNEL, Event};
use crate::nem;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;
use lazy_static::lazy_static;
use crate::log::LogSubsys;

// ── Constants ──

/// Maximum ticks to wait for a driver ACK before force-unloading.
pub const UNLOAD_DRAIN_TIMEOUT_TICKS: u64 = 100;

/// Number of hot reload tracking entries.
const MAX_HOTRELOAD_ENTRIES: usize = 16;

// ── Resource types ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ResourceType {
    BlockDevice = 0,
    NetworkDevice = 1,
    ObNamespace = 2,
}

#[derive(Debug, Clone, Copy)]
pub struct ResourceRecord {
    pub driver_id: DriverId,
    pub resource_type: ResourceType,
    pub resource_id: u32,
}

// ── Hot reload entry (stores load result for unload cleanup) ──

pub struct HotReloadEntry {
    pub driver_id: DriverId,
    pub entry_fini: Option<unsafe extern "C" fn() -> i32>,
    pub name: Vec<u8>,
    pub category: nem::DriverCategory,
    pub isolated: bool,
    pub isolated_base: u64,
    pub isolated_size: u64,
    pub loaded_at_tick: u64,
}

// ── Resource registry ──

pub struct ResourceRegistry {
    resources: Vec<ResourceRecord>,
}

impl ResourceRegistry {
    pub const fn new() -> Self {
        ResourceRegistry { resources: Vec::new() }
    }

    pub fn track(&mut self, driver_id: DriverId, resource_type: ResourceType, resource_id: u32) {
        self.resources.push(ResourceRecord { driver_id, resource_type, resource_id });
    }

    pub fn untrack(&mut self, driver_id: DriverId, resource_type: ResourceType, resource_id: u32) {
        self.resources.retain(|r| !(r.driver_id == driver_id && r.resource_type == resource_type && r.resource_id == resource_id));
    }

    pub fn untrack_all(&mut self, driver_id: DriverId) -> Vec<ResourceRecord> {
        let mut removed = Vec::new();
        self.resources.retain(|r| {
            if r.driver_id == driver_id {
                removed.push(*r);
                false
            } else {
                true
            }
        });
        removed
    }

    pub fn resources_for(&self, driver_id: DriverId) -> Vec<ResourceRecord> {
        self.resources.iter().filter(|r| r.driver_id == driver_id).copied().collect()
    }
}

// ── Hot reload registry ──

pub struct HotReloadRegistry {
    entries: Vec<HotReloadEntry>,
}

impl HotReloadRegistry {
    pub const fn new() -> Self {
        HotReloadRegistry { entries: Vec::new() }
    }

    pub fn register(&mut self, driver_id: DriverId, result: &NemV3LoadResult) {
        // Remove old entry for same driver_id (if reloading)
        self.entries.retain(|e| e.driver_id != driver_id);
        if self.entries.len() >= MAX_HOTRELOAD_ENTRIES {
            kwarn!(LogSubsys::Hotreload, "Warning: hot reload entry limit reached for driver {}", driver_id);
            return;
        }
        self.entries.push(HotReloadEntry {
            driver_id,
            entry_fini: result.entry_fini,
            name: result.name.clone(),
            category: result.category,
            isolated: result.isolated,
            isolated_base: result.base as u64,
            isolated_size: result.total_size as u64,
            loaded_at_tick: crate::hal::get_ticks(),
        });
    }

    pub fn unregister(&mut self, driver_id: DriverId) {
        self.entries.retain(|e| e.driver_id != driver_id);
    }

    pub fn get(&self, driver_id: DriverId) -> Option<&HotReloadEntry> {
        self.entries.iter().find(|e| e.driver_id == driver_id)
    }

    pub fn get_by_name(&self, name: &str) -> Option<&HotReloadEntry> {
        self.entries.iter().find(|e| {
            let ename = core::str::from_utf8(&e.name).unwrap_or("");
            ename.eq_ignore_ascii_case(name)
        })
    }
}

// ── Global singletons ──

lazy_static! {
    pub static ref HOT_RELOAD_REGISTRY: Mutex<HotReloadRegistry> = Mutex::new(HotReloadRegistry::new());
    pub static ref RESOURCE_REGISTRY: Mutex<ResourceRegistry> = Mutex::new(ResourceRegistry::new());
}

/// Per-driver unload ack flag — set by the driver when it receives EVENT_DRIVER_UNLOAD.
static UNLOAD_ACK_FLAG: AtomicBool = AtomicBool::new(false);
static UNLOAD_ACK_DRIVER: AtomicU32 = AtomicU32::new(0);

// ── Internal: event handler for DRIVER_UNLOAD_ACK ──

fn handle_unload_ack(event: &Event) {
    if event.event_type == EVENT_DRIVER_UNLOAD_ACK {
        UNLOAD_ACK_FLAG.store(true, Ordering::SeqCst);
    }
}

// ── Resource tracking API ──

/// Track a resource as owned by a specific driver.
/// Called by hst_register_block_device / similar registration functions.
pub fn track_resource(driver_id: DriverId, resource_type: ResourceType, resource_id: u32) {
    RESOURCE_REGISTRY.lock().track(driver_id, resource_type, resource_id);
    ktrace!(LogSubsys::Hotreload, "Tracked resource type={:?} id={} for driver {}", resource_type, resource_id, driver_id);
}

/// Untrack a single resource.
pub fn untrack_resource(driver_id: DriverId, resource_type: ResourceType, resource_id: u32) {
    RESOURCE_REGISTRY.lock().untrack(driver_id, resource_type, resource_id);
}

/// Track an Ob namespace entry owned by a driver.
pub fn track_ob_entry(driver_id: DriverId, resource_id: u32) {
    track_resource(driver_id, ResourceType::ObNamespace, resource_id);
}

/// Untrack an Ob namespace entry.
pub fn untrack_ob_entry(driver_id: DriverId, resource_id: u32) {
    untrack_resource(driver_id, ResourceType::ObNamespace, resource_id);
}

/// Register a load result for future hot reload/unload use.
pub fn register_load_result(driver_id: DriverId, result: &NemV3LoadResult) {
    HOT_RELOAD_REGISTRY.lock().register(driver_id, result);
    ktrace!(LogSubsys::Hotreload, "Registered load result for driver {} ({})", driver_id,
        core::str::from_utf8(&result.name).unwrap_or("?"));
}

/// Register the unload ack handler with the event bus.
/// Called once at boot.
pub fn init_hot_reload() {
    let _ = crate::eventbus::EVENT_BUS.register_handler(
        EVENT_DRIVER_UNLOAD_ACK,
        handle_unload_ack,
        "hotreload_unload_ack",
    );
    kinfo!(LogSubsys::Hotreload, "Initialized — unload ACK handler registered");
}

// ── Unload driver ──

/// Unload a driver by name.
///
/// `force`: if true, skips waiting for the driver's ACK and force-cleanup resources.
///
/// Returns Ok(message) or Err(error_message).
pub fn unload_driver(name: &str, force: bool) -> Result<alloc::string::String, &'static str> {
    let drv = driver_runtime::get_driver_by_name(name).ok_or("Driver not found in runtime")?;
    let id = drv.id;

    // Check if we have hot reload info for this driver
    let entry = HOT_RELOAD_REGISTRY.lock().get(id).map(|e| HotReloadEntry {
        driver_id: e.driver_id,
        entry_fini: e.entry_fini,
        name: e.name.clone(),
        category: e.category,
        isolated: e.isolated,
        isolated_base: e.isolated_base,
        isolated_size: e.isolated_size,
        loaded_at_tick: e.loaded_at_tick,
    });

    // Step 1: Transition to Unloading state
    {
        let mut rt = driver_runtime::DRIVER_RUNTIME.lock();
        if rt.try_transition(id, DriverState::Unloading).is_err() {
            return Err("Cannot transition to Unloading state");
        }
    }
    kinfo!(LogSubsys::Hotreload, "Driver {} ({}) transitioning to UNLOADING", name, id);

    // Step 2: Call driver_fini() if available
    if let Some(ref entry) = entry {
        if let Some(fini_fn) = entry.entry_fini {
            kinfo!(LogSubsys::Hotreload, "Calling driver_fini() for {}", name);
            unsafe {
                crate::drivers::nem::driver::set_current_driver(id);
                fini_fn();
                crate::drivers::nem::driver::clear_current_driver();
            }
        }
    }

    // Step 3: Send EVENT_DRIVER_UNLOAD
    UNLOAD_ACK_FLAG.store(false, Ordering::SeqCst);
    UNLOAD_ACK_DRIVER.store(id, Ordering::SeqCst);
    let _ = crate::eventbus::EVENT_BUS.push_event(
        EVENT_DRIVER_UNLOAD,
        SOURCE_KERNEL,
        0,
        id as u64,
        name.len() as u64,
        0,
    );
    kinfo!(LogSubsys::Hotreload, "Sent EVENT_DRIVER_UNLOAD to {}", name);

    // Step 4: Wait for ack (unless force)
    if !force {
        let start_tick = crate::hal::get_ticks();
        loop {
            let tick = crate::hal::get_ticks();
            if tick.wrapping_sub(start_tick) >= UNLOAD_DRAIN_TIMEOUT_TICKS {
                kerror!(LogSubsys::Hotreload, "Timeout waiting for DRIVER_UNLOAD_ACK from {} — forcing unload", name);
                break;
            }
            if UNLOAD_ACK_FLAG.load(Ordering::SeqCst) {
                kinfo!(LogSubsys::Hotreload, "Received DRIVER_UNLOAD_ACK from {}", name);
                break;
            }
            // Yield to allow driver to process the event
            crate::hal::hlt_once();
        }
    }

    // Step 5: Clean up resources
    let resources = RESOURCE_REGISTRY.lock().untrack_all(id);
    for res in &resources {
        match res.resource_type {
            ResourceType::BlockDevice => {
                crate::drivers::block::unregister_nem_block_device(res.resource_id as usize);
                kinfo!(LogSubsys::Hotreload, "Unregistered block device idx={} for driver {}", res.resource_id, id);
            }
            ResourceType::NetworkDevice => {
                crate::net::nic::nic_unregister(res.resource_id);
                kinfo!(LogSubsys::Hotreload, "Unregistered network device id={} for driver {}", res.resource_id, id);
            }
            ResourceType::ObNamespace => {
                // Remove the Ob object by ID
                let _ = crate::object::namespace::ob_remove_by_id(res.resource_id as u64);
                kinfo!(LogSubsys::Hotreload, "Removed Ob namespace entry id={} for driver {}", res.resource_id, id);
            }
        }
    }

    // Step 6: Free isolated memory
    if let Some(ref entry) = entry {
        if entry.isolated {
            v3loader::free_isolated_driver(id);
            kinfo!(LogSubsys::Hotreload, "Freed isolated memory for driver {}", id);
        }
    }

    // Step 7: Remove from runtime registry
    let removed = {
        let mut rt = driver_runtime::DRIVER_RUNTIME.lock();
        rt.remove(id)
    };
    if removed.is_none() {
        return Err("Failed to remove driver from runtime");
    }

    // Step 8: Remove from hot reload registry
    HOT_RELOAD_REGISTRY.lock().unregister(id);

    let msg = alloc::format!("Driver '{}' (id={}) unloaded successfully{}", name, id,
        if force { " (forced)" } else { "" });
    kinfo!(LogSubsys::Hotreload, "{}", msg);
    Ok(msg)
}

// ── Reload driver ──

/// Reload a driver from a NeoFS path.
///
/// Steps:
///   1. Read new binary from NeoFS
///   2. Parse NEM v3 and validate ABI compatibility
///   3. Find existing driver by name, unload gracefully
///   4. Load new driver through v3loader
///   5. Register, initialize, bind event handlers, activate
///
/// Returns Ok(message) or Err(error_message).
pub fn reload_driver(path: &str) -> Result<alloc::string::String, &'static str> {
    // Step 1: Read and parse the new binary
    let data = read_whole_file(path).map_err(|_| "Cannot read driver file")?;
    let parsed = nem::parse_nem_v3(&data).ok_or("Invalid NEM v3 format")?;

    let driver_name = parsed.name.to_ascii_uppercase();
    let name_upper = driver_name.clone();

    // Step 2: Check ABI compatibility
    let abi_result = crate::drivers::abi::negotiate_default(
        parsed.header.abi_min,
        parsed.header.abi_target,
        parsed.header.abi_max,
    );
    if !abi_result.is_compatible() {
        return Err("New driver binary is not ABI-compatible with current kernel");
    }

    // Step 3: Find existing driver and unload it
    let existing_id = {
        let rt = driver_runtime::DRIVER_RUNTIME.lock();
        rt.get_by_name(&name_upper).map(|d| d.id)
    };

    if let Some(old_id) = existing_id {
        let old_name = {
            let rt = driver_runtime::DRIVER_RUNTIME.lock();
            rt.get(old_id).map(|d| alloc::string::String::from(d.name_str()))
        };
        kinfo!(LogSubsys::Hotreload, "Found existing driver '{}' (id={}), unloading...", old_name.as_deref().unwrap_or("?"), old_id);
        // Unload with force=true to ensure clean removal
        unload_driver(&name_upper, true)?;
    }

    // Step 4: Load the new driver via v3loader
    kinfo!(LogSubsys::Hotreload, "Loading new driver '{}' from {}", name_upper, path);
    let load_result = v3loader::load_nem_v3(&data)?;

    // Step 5: Register with runtime
    let name_str = core::str::from_utf8(&load_result.name)
        .map(|s| s.split('\0').next().unwrap_or("?"))
        .unwrap_or("?");
    let name_upper_str = name_str.to_ascii_uppercase();

    let rt_id = driver_runtime::register_driver_ext(
        &name_upper_str,
        nem::NemDriverType::Lifecycle,
        nem::NEM_API_VERSION,
        0,
        parsed.header.abi_min,
        parsed.header.abi_target,
        parsed.header.abi_max,
        load_result.category,
    ).map_err(|_| "Failed to register reloaded driver in runtime")?;

    // Step 6: Bind isolation and hot reload tracking
    v3loader::bind_isolated_driver(rt_id, &load_result);
    register_load_result(rt_id, &load_result);

    // Step 7: Initialize the driver
    unsafe { crate::drivers::nem::driver::set_current_driver(rt_id); }

    let init_ok = match load_result.entry_init {
        Some(init_fn) => unsafe { init_fn() == 0 },
        None => true,
    };

    if !init_ok {
        driver_runtime::DRIVER_RUNTIME.lock()
            .set_error(rt_id, driver_runtime::ERR_INIT_FAILED, true);
        unsafe { crate::drivers::nem::driver::clear_current_driver(); }
        return Err("Driver init() failed on reload");
    }

    let _ = driver_runtime::DRIVER_RUNTIME.lock()
        .try_transition(rt_id, DriverState::Initialized);

    // Step 8: Register with event bus
    let _ = driver_runtime::DRIVER_RUNTIME.lock()
        .try_transition(rt_id, DriverState::Registered);

    // Step 9: Bind event handlers (simple pass-through — drivers using
    // hst_push_event or register_event handle this themselves)
    let _ = driver_runtime::DRIVER_RUNTIME.lock()
        .try_transition(rt_id, DriverState::Bound);

    // Step 10: Activate
    let activate_ok = match load_result.entry_activate {
        Some(activate_fn) => unsafe { activate_fn() == 0 },
        None => true,
    };

    if !activate_ok {
        driver_runtime::DRIVER_RUNTIME.lock()
            .set_error(rt_id, driver_runtime::ERR_INIT_FAILED, true);
        unsafe { crate::drivers::nem::driver::clear_current_driver(); }
        return Err("Driver activate() failed on reload");
    }

    if driver_runtime::DRIVER_RUNTIME.lock()
        .certify_and_activate(rt_id).is_ok()
    {
        unsafe { crate::drivers::nem::driver::clear_current_driver(); }
        let msg = alloc::format!("Driver '{}' reloaded successfully (new id={})", name_upper_str, rt_id);
        kinfo!(LogSubsys::Hotreload, "{}", msg);
        Ok(msg)
    } else {
        driver_runtime::DRIVER_RUNTIME.lock()
            .set_error(rt_id, driver_runtime::ERR_CERTIFICATION_FAILED, true);
        unsafe { crate::drivers::nem::driver::clear_current_driver(); }
        Err("Driver certification failed on reload")
    }
}

// ── Helper: read a whole file from NeoFS ──

fn read_whole_file(path: &str) -> Result<alloc::vec::Vec<u8>, ()> {
    crate::globals::with_vfs(|vfs| {
        let (drive_idx, node) = vfs.resolve_path(path).map_err(|_| ())?;
        if node.mode & crate::fs::vfs::MODE_FILE == 0 {
            return Err(());
        }
        let size = node.size as usize;
        if size == 0 || size > 65536 {
            return Err(());
        }
    let mut buf = alloc::vec![0u8; size];
        let read = vfs.read(drive_idx, node.inode, 0, &mut buf).map_err(|_| ())?;
        buf.truncate(read);
        Ok(buf)
    })
}

// ── Tests ──

pub fn register_hotreload_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;

    test_case!("hotreload_resource_track_untrack", {
        let mut registry = ResourceRegistry::new();
        registry.track(1, ResourceType::BlockDevice, 0);
        registry.track(1, ResourceType::BlockDevice, 1);
        registry.track(2, ResourceType::BlockDevice, 2);
        let res1 = registry.resources_for(1);
        test_eq!(res1.len(), 2);
        let res2 = registry.resources_for(2);
        test_eq!(res2.len(), 1);
        // Untrack one
        registry.untrack(1, ResourceType::BlockDevice, 0);
        let res1 = registry.resources_for(1);
        test_eq!(res1.len(), 1);
        test_eq!(res1[0].resource_id, 1);
        // Untrack all
        let removed = registry.untrack_all(2);
        test_eq!(removed.len(), 1);
        test_eq!(registry.resources_for(2).len(), 0);
    });

    test_case!("hotreload_resource_track_max", {
        let mut registry = ResourceRegistry::new();
        for i in 0..100 {
            registry.track(1, ResourceType::BlockDevice, i);
        }
        test_eq!(registry.resources_for(1).len(), 100);
        let removed = registry.untrack_all(1);
        test_eq!(removed.len(), 100);
    });

    test_case!("hotreload_registry_register", {
        let mut registry = HotReloadRegistry::new();
        let result = NemV3LoadResult {
            base: core::ptr::null_mut(),
            total_size: 4096,
            text_base: core::ptr::null_mut(),
            rodata_base: core::ptr::null_mut(),
            data_base: core::ptr::null_mut(),
            text_size: 1024,
            rodata_size: 1024,
            data_size: 1024,
            bss_size: 1024,
            entry_init: None,
            entry_event: None,
            entry_activate: None,
            entry_fini: None,
            name: b"TEST".to_vec(),
            category: nem::DriverCategory::System,
            isolated: false,
            driver_id: 0,
        };
        registry.register(42, &result);
        let entry = registry.get(42);
        test_true!(entry.is_some());
        test_eq!(entry.unwrap().driver_id, 42);
        registry.unregister(42);
        test_true!(registry.get(42).is_none());
    });

    test_case!("hotreload_registry_reregister", {
        let mut registry = HotReloadRegistry::new();
        let result1 = NemV3LoadResult {
            base: core::ptr::null_mut(),
            total_size: 4096,
            text_base: core::ptr::null_mut(),
            rodata_base: core::ptr::null_mut(),
            data_base: core::ptr::null_mut(),
            text_size: 1024,
            rodata_size: 1024,
            data_size: 1024,
            bss_size: 1024,
            entry_init: None,
            entry_event: None,
            entry_activate: None,
            entry_fini: None,
            name: b"RELOAD".to_vec(),
            category: nem::DriverCategory::System,
            isolated: false,
            driver_id: 0,
        };
        registry.register(1, &result1);
        let name1 = core::str::from_utf8(&registry.get(1).unwrap().name).unwrap_or("").to_string();
        test_eq!(name1.as_str(), "RELOAD");
        // Re-register same ID — replaces old entry
        let result2 = NemV3LoadResult {
            base: core::ptr::null_mut(),
            total_size: 2048,
            text_base: core::ptr::null_mut(),
            rodata_base: core::ptr::null_mut(),
            data_base: core::ptr::null_mut(),
            text_size: 512,
            rodata_size: 512,
            data_size: 512,
            bss_size: 512,
            entry_init: None,
            entry_event: None,
            entry_activate: None,
            entry_fini: None,
            name: b"V2".to_vec(),
            category: nem::DriverCategory::Boot,
            isolated: true,
            driver_id: 0,
        };
        registry.register(1, &result2);
        let entry = registry.get(1).unwrap();
        let name2 = core::str::from_utf8(&entry.name).unwrap_or("");
        test_eq!(name2, "V2");
        test_eq!(entry.isolated, true);
        test_eq!(entry.isolated_size, 2048);
    });

    test_case!("hotreload_init_event_handler", {
        // Verify the handler registration doesn't crash
        let id = driver_runtime::register_driver("HOTRELOAD_TEST", crate::nem::NemDriverType::Null, 1, 0).unwrap();
        let result = NemV3LoadResult {
            base: core::ptr::null_mut(),
            total_size: 100,
            text_base: core::ptr::null_mut(),
            rodata_base: core::ptr::null_mut(),
            data_base: core::ptr::null_mut(),
            text_size: 0,
            rodata_size: 0,
            data_size: 0,
            bss_size: 0,
            entry_init: None,
            entry_event: None,
            entry_activate: None,
            entry_fini: None,
            name: b"HOTRELOAD_TEST".to_vec(),
            category: nem::DriverCategory::System,
            isolated: false,
            driver_id: 0,
        };
        register_load_result(id, &result);
        let reg = HOT_RELOAD_REGISTRY.lock();
        let entry = reg.get(id);
        test_true!(entry.is_some());
        test_eq!(entry.unwrap().driver_id, id);
        drop(reg);
        HOT_RELOAD_REGISTRY.lock().unregister(id);
        driver_runtime::DRIVER_RUNTIME.lock().remove(id);
    });

    test_case!("hotreload_state_transitions", {
        // Test that the new state transitions work
        let mut rt = driver_runtime::DriverRuntime::new();
        let id = rt.register("HOTRELOAD", crate::nem::NemDriverType::Null, 1, 0).unwrap();
        // Full pipeline to Active
        rt.try_transition(id, DriverState::Initialized).unwrap();
        rt.try_transition(id, DriverState::Registered).unwrap();
        rt.try_transition(id, DriverState::Bound).unwrap();
        rt.try_transition(id, DriverState::Active).unwrap();
        test_eq!(rt.get(id).unwrap().state, DriverState::Active);
        // Active → Unloading (new transition)
        test_true!(rt.try_transition(id, DriverState::Unloading).is_ok());
        test_eq!(rt.get(id).unwrap().state, DriverState::Unloading);
        // Unloading → Unloaded (new transition)
        test_true!(rt.try_transition(id, DriverState::Unloaded).is_ok());
        test_eq!(rt.get(id).unwrap().state, DriverState::Unloaded);
        // Unloaded → Loaded (reload path, new transition)
        test_true!(rt.try_transition(id, DriverState::Loaded).is_ok());
        test_eq!(rt.get(id).unwrap().state, DriverState::Loaded);
        rt.remove(id);
    });

    test_case!("hotreload_invalid_transitions", {
        let mut rt = driver_runtime::DriverRuntime::new();
        let id = rt.register("BAD", crate::nem::NemDriverType::Null, 1, 0).unwrap();
        // Cannot go from Loaded to Unloading (must be Active first)
        test_true!(rt.try_transition(id, DriverState::Unloading).is_err());
        // Cannot go from Unloaded to Active (must go through pipeline)
        rt.try_transition(id, DriverState::Unloaded).ok();
        test_true!(rt.try_transition(id, DriverState::Active).is_err());
        // Unloaded → Loaded is valid (reload path)
        test_true!(rt.try_transition(id, DriverState::Loaded).is_ok());
        rt.remove(id);
    });

    test_case!("hotreload_state_counts_with_unloading", {
        let mut rt = driver_runtime::DriverRuntime::new();
        let id = rt.register("COUNT1", crate::nem::NemDriverType::Null, 1, 0).unwrap();
        let _id2 = rt.register("COUNT2", crate::nem::NemDriverType::Echo, 1, 0).unwrap();
        // id through pipeline to Active, then to Unloading
        rt.try_transition(id, DriverState::Initialized).unwrap();
        rt.try_transition(id, DriverState::Registered).unwrap();
        rt.try_transition(id, DriverState::Bound).unwrap();
        rt.try_transition(id, DriverState::Active).unwrap();
        rt.try_transition(id, DriverState::Unloading).unwrap();
        let counts = rt.state_counts();
        // Should have: 1 Unloading, 1 Loaded
        let mut found_unloading = false;
        let mut found_loaded = false;
        for (state, count) in &counts {
            if *state == DriverState::Unloading { found_unloading = true; test_eq!(*count, 1); }
            if *state == DriverState::Loaded { found_loaded = true; test_eq!(*count, 1); }
        }
        test_true!(found_unloading);
        test_true!(found_loaded);
        rt.remove(id);
        rt.remove(_id2);
    });

    test_case!("hotreload_unload_non_existent", {
        let result = unload_driver("NONEXISTENT_DRIVER_XYZ", false);
        test_true!(result.is_err());
    });

    test_case!("hotreload_error_codes", {
        // Verify new error codes exist
        test_eq!(driver_runtime::ERR_UNLOAD_FAILED, 10);
        test_eq!(driver_runtime::ERR_UNLOAD_TIMEOUT, 11);
        test_eq!(driver_runtime::err_to_str(driver_runtime::ERR_UNLOAD_FAILED), "UNLOAD_FAILED");
        test_eq!(driver_runtime::err_to_str(driver_runtime::ERR_UNLOAD_TIMEOUT), "UNLOAD_TIMEOUT");
    });

    test_case!("hotreload_pipeline_step_unloading", {
        test_eq!(PipelineStep::Unloading as u8, 6);
        test_eq!(PipelineStep::Unloading.to_str(), "UNLOAD");
    });
}
