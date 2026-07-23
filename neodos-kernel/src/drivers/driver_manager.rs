use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;
use alloc::collections::BTreeMap;
use crate::drivers::device::{DeviceInfo, DeviceRegistry, scan_all_devices};
use crate::drivers::manifest::{self, DriverDescriptor};
use crate::nem::{self, NEM_API_VERSION};
use crate::drivers::nem::v3loader;
use crate::drivers::driver_runtime::{self, DriverState};
use crate::eventbus::{EVENT_KEYBOARD_INPUT, EVENT_KEYB_LAYOUT, EVENT_MOUSE_INPUT};
use crate::eventbus::{EVENT_SERIAL_DATA, EVENT_RTC_READ, EVENT_SHUTDOWN};
use crate::log::LogSubsys;

#[derive(Debug, Clone)]
pub struct LoadedDriverInfo {
    pub id: u32,
    pub name: String,
    pub driver_class: super::device::DeviceClass,
    pub bound_device: Option<DeviceInfo>,
}

pub struct DriverManager {
    device_registry: DeviceRegistry,
    matched_drivers: Vec<(DriverDescriptor, Vec<DeviceInfo>)>,
    loaded_drivers: Vec<LoadedDriverInfo>,
    unmatched_devices: Vec<DeviceInfo>,
    available_nem_files: Vec<String>,
}

impl DriverManager {
    pub fn new() -> Self {
        DriverManager {
            device_registry: DeviceRegistry::new(),
            matched_drivers: Vec::new(),
            loaded_drivers: Vec::new(),
            unmatched_devices: Vec::new(),
            available_nem_files: Vec::new(),
        }
    }

    pub fn init(&mut self) {
        kinfo!(LogSubsys::Driver, "=== Driver Manager v1.0 ===");

        self.discover_devices();
        self.scan_available_drivers();
        self.match_devices_to_drivers();
        self.collect_platform_drivers();
        self.print_discovery_summary();
        self.load_matched_drivers();
        self.print_loading_summary();
    }

    fn discover_devices(&mut self) {
        kinfo!(LogSubsys::Driver, "Phase 1: Device Discovery");
        self.device_registry = scan_all_devices();
        kinfo!(LogSubsys::Driver, "{} device(s) found on PCI bus", self.device_registry.count());
    }

    fn scan_available_drivers(&mut self) {
        kinfo!(LogSubsys::Driver, "Phase 2: Scanning available NEM drivers");
        let files = super::boot_loader::driver_scan("C:\\System\\Drivers");
        self.available_nem_files = files;
        kinfo!(LogSubsys::Driver, "{} NEM driver(s) available", self.available_nem_files.len());
    }

    fn match_devices_to_drivers(&mut self) {
        kinfo!(LogSubsys::Driver, "Phase 3: Matching devices to drivers");
        let mut used_descriptors: Vec<String> = Vec::new();
        let mut matched_device_indices: Vec<usize> = Vec::new();

        for dev in self.device_registry.devices() {
            let mut best: Option<&'static DriverDescriptor> = None;
            let mut best_prio: i32 = -1;

            for mdesc in manifest::all_descriptors() {
                if mdesc.matches_pci(dev.vendor_id, dev.device_id) {
                    if (mdesc.priority as i32) > best_prio {
                        best = Some(mdesc);
                        best_prio = mdesc.priority as i32;
                    }
                }
            }

            if let Some(desc) = best {
                let name_upper = desc.name.to_ascii_uppercase();
                if !used_descriptors.contains(&name_upper) {
                    kinfo!(LogSubsys::Driver, "MATCH: {} -> driver '{}' (v{:04x}:{:04x}, class={:?})", format_pci_location(dev),
                        desc.name,
                        dev.vendor_id, dev.device_id,
                        dev.class,
                    );
                    used_descriptors.push(name_upper.clone());

                    self.matched_drivers.push((
                        DriverDescriptor {
                            name: desc.name,
                            version_major: desc.version_major,
                            version_minor: desc.version_minor,
                            version_patch: desc.version_patch,
                            device_class: desc.device_class,
                            bus_type: desc.bus_type,
                            pci_devices: desc.pci_devices,
                            priority: desc.priority,
                        },
                        {
                            let mut v = Vec::new();
                            v.push(dev.clone());
                            v
                        },
                    ));
                } else {
                    let found = self.matched_drivers.iter_mut()
                        .find(|(d, _)| d.name.to_ascii_uppercase() == name_upper);
                    if let Some((_, devs)) = found {
                        devs.push(dev.clone());
                    }
                }
                matched_device_indices.push(0);
            }
        }

        let mut matched_count = 0usize;
        for (_, devs) in &self.matched_drivers {
            matched_count += devs.len();
        }

        self.unmatched_devices = self.device_registry.devices().iter()
            .filter(|d| {
                !self.matched_drivers.iter().any(|(desc, _)| {
                    desc.matches_pci(d.vendor_id, d.device_id)
                })
            })
            .cloned()
            .collect();

        kinfo!(LogSubsys::Driver, "{} device(s) matched, {} unmatched", matched_count, self.unmatched_devices.len());
    }

    fn collect_platform_drivers(&mut self) {
        for desc in manifest::all_descriptors() {
            if desc.bus_type != super::device::BusType::Platform {
                continue;
            }
            let name_upper = desc.name.to_ascii_uppercase();
            if self.matched_drivers.iter().any(|(d, _)| d.name.to_ascii_uppercase() == name_upper) {
                continue;
            }
            kinfo!(LogSubsys::Driver, "PLATFORM: driver '{}' (class={:?})", desc.name, desc.device_class,
            );
            self.matched_drivers.push((
                DriverDescriptor {
                    name: desc.name,
                    version_major: desc.version_major,
                    version_minor: desc.version_minor,
                    version_patch: desc.version_patch,
                    device_class: desc.device_class,
                    bus_type: desc.bus_type,
                    pci_devices: desc.pci_devices,
                    priority: desc.priority,
                },
                Vec::new(),
            ));
        }
    }

    fn print_discovery_summary(&self) {
        kinfo!(LogSubsys::Driver, "=== Discovery Summary ===");
        kinfo!(LogSubsys::Driver, "PCI devices found: {}", self.device_registry.count());
        kinfo!(LogSubsys::Driver, "Matched driver(s): {}", self.matched_drivers.len());
        kinfo!(LogSubsys::Driver, "Unmatched device(s): {}", self.unmatched_devices.len());

        for (desc, devs) in &self.matched_drivers {
            let dev_list: Vec<String> = devs.iter().map(|d| format_pci_location(d)).collect();
            kinfo!(LogSubsys::Driver, "  '{}' -> {}", desc.name, dev_list.join(", "));
        }

        for d in &self.unmatched_devices {
            kinfo!(LogSubsys::Driver, "UNMATCHED: {} (v{:04x}:d{:04x} class={:?})", format_pci_location(d), d.vendor_id, d.device_id, d.class
            );
        }
    }

    fn load_matched_drivers(&mut self) {
        kinfo!(LogSubsys::Driver, "Phase 4: Loading matched drivers");

        let mut all_nem_data: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        for file in &self.available_nem_files {
            if let Ok(data) = super::boot_loader::read_nem_file(file) {
                if let Some(parsed) = nem::parse_nem_v3(&data) {
                    let name = parsed.name.to_ascii_uppercase();
                    all_nem_data.insert(name, data);
                }
            }
        }

        for (desc, devices) in &self.matched_drivers {
            let name_upper = desc.name.to_ascii_uppercase();
            let data = match all_nem_data.get(&name_upper) {
                Some(d) => d.clone(),
                None => {
                    kwarn!(LogSubsys::Driver, "SKIP '{}': NEM file not found", desc.name);
                    continue;
                }
            };

            let parsed = match nem::parse_nem_v3(&data) {
                Some(p) => p,
                None => {
                    kwarn!(LogSubsys::Driver, "SKIP '{}': invalid NEM format", desc.name);
                    continue;
                }
            };

            let cat = parsed.category;
            crate::serial_print!("[DRVMGR]   Loading {} ({} device(s)) ... ", desc.name, devices.len());

            let load_result = match v3loader::load_nem_v3(&data) {
                Ok(r) => r,
                Err(e) => {
                    kerror!(LogSubsys::Driver, "FAIL (load: {})", e);
                    continue;
                }
            };

            let name_str = String::from_utf8_lossy(&load_result.name);
            let rt_name = name_str.to_ascii_uppercase();

            let rt_id = driver_runtime::register_driver_ext(
                &rt_name,
                nem::NemDriverType::Lifecycle,
                NEM_API_VERSION,
                0,
                parsed.header.abi_min,
                parsed.header.abi_target,
                parsed.header.abi_max,
                cat,
            );

            match rt_id {
                Ok(id) => {
                    v3loader::bind_isolated_driver(id, &load_result);
                    crate::drivers::hotreload::register_load_result(id, &load_result);
                    unsafe { crate::drivers::nem::driver::set_current_driver(id); }

                    let init_ok = match load_result.entry_init {
                        Some(init_fn) => unsafe { init_fn() == 0 },
                        None => true,
                    };

                    if init_ok {
                        let _ = driver_runtime::DRIVER_RUNTIME.lock()
                            .try_transition(id, DriverState::Initialized);
                        let _ = driver_runtime::DRIVER_RUNTIME.lock()
                            .try_transition(id, DriverState::Registered);

                        let bind_ok = match rt_name.as_str() {
                            "PS2KBD" => {
                                let a = v3loader::register_v3_event_bus_handler(
                                    load_result.entry_event, EVENT_KEYBOARD_INPUT, id
                                ).is_ok();
                                let b = v3loader::register_v3_event_bus_handler(
                                    load_result.entry_event, EVENT_KEYB_LAYOUT, id
                                ).is_ok();
                                a && b
                            }
                            "PS2MOUSE" => {
                                v3loader::register_v3_event_bus_handler(
                                    load_result.entry_event, EVENT_MOUSE_INPUT, id
                                ).is_ok()
                            }
                            "SERIAL" => {
                                v3loader::register_v3_event_bus_handler(
                                    load_result.entry_event, EVENT_SERIAL_DATA, id
                                ).is_ok()
                            }
                            "RTC" => {
                                v3loader::register_v3_event_bus_handler(
                                    load_result.entry_event, EVENT_RTC_READ, id
                                ).is_ok()
                            }
                            "ACPI" => {
                                v3loader::register_v3_event_bus_handler(
                                    load_result.entry_event, EVENT_SHUTDOWN, id
                                ).is_ok()
                            }
                            _ => true,
                        };

                        if bind_ok {
                            let _ = driver_runtime::DRIVER_RUNTIME.lock()
                                .try_transition(id, DriverState::Bound);

                            let activate_ok = match load_result.entry_activate {
                                Some(activate_fn) => unsafe { activate_fn() == 0 },
                                None => true,
                            };

                            if activate_ok {
                                if driver_runtime::DRIVER_RUNTIME.lock()
                                    .certify_and_activate(id).is_ok()
                                {
                                    let bound_device = devices.first().cloned();
                                    self.loaded_drivers.push(LoadedDriverInfo {
                                        id,
                                        name: desc.name.to_string(),
                                        driver_class: desc.device_class,
                                        bound_device,
                                    });
                                    kinfo!(LogSubsys::Driver, "ACTIVE");
                                } else {
                                    kerror!(LogSubsys::Driver, "CERT FAIL");
                                    driver_runtime::DRIVER_RUNTIME.lock()
                                        .set_error(id, driver_runtime::ERR_CERTIFICATION_FAILED, true);
                                }
                            } else {
                                kerror!(LogSubsys::Driver, "ACT FAIL");
                                driver_runtime::DRIVER_RUNTIME.lock()
                                    .set_error(id, driver_runtime::ERR_INIT_FAILED, true);
                            }
                        } else {
                            kerror!(LogSubsys::Driver, "BIND FAIL");
                            driver_runtime::DRIVER_RUNTIME.lock()
                                .set_error(id, driver_runtime::ERR_BIND_FAILED, true);
                        }
                    } else {
                        kerror!(LogSubsys::Driver, "INIT FAIL");
                        driver_runtime::DRIVER_RUNTIME.lock()
                            .set_error(id, driver_runtime::ERR_INIT_FAILED, true);
                    }

                    unsafe { crate::drivers::nem::driver::clear_current_driver(); }
                }
                Err(e) => {
                    kerror!(LogSubsys::Driver, "REG FAIL ({})", e);
                }
            }
        }
    }

    fn print_loading_summary(&self) {
        let rt = driver_runtime::DRIVER_RUNTIME.lock();
        let total = rt.count();
        let active = rt.active_count();
        let faulted = rt.faulted_count();
        drop(rt);

        kinfo!(LogSubsys::Driver, "=== Loading Summary: {} total, {} loaded, {} active, {} faulted ===", total, self.loaded_drivers.len(), active, faulted
        );
    }

    pub fn loaded_count(&self) -> usize {
        self.loaded_drivers.len()
    }

    pub fn loaded_drivers(&self) -> &[LoadedDriverInfo] {
        &self.loaded_drivers
    }

    pub fn unmatched_devices(&self) -> &[DeviceInfo] {
        &self.unmatched_devices
    }

    pub fn device_registry(&self) -> &DeviceRegistry {
        &self.device_registry
    }
}

fn format_pci_location(d: &DeviceInfo) -> String {
    match d.pci_location() {
        Some((bus, dev, func)) => alloc::format!("{:02x}:{:02x}.{}", bus, dev, func),
        None => alloc::string::String::from("??:??.?"),
    }
}
