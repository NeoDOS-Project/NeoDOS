// src/devices/mod.rs
// NeoDOS Device Model + HAL Binding Layer v0.3
//
// All hardware access MUST be mediated through this layer.
// Drivers NEVER touch hardware directly.

use crate::println;
use crate::serial_println;

const MAX_DEVICES: usize = 32;
const MAX_BINDINGS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum DeviceClass {
    Input = 0,
    Storage = 1,
    Timer = 2,
    Communication = 3,
    Virtual = 4,
    Unknown = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum DeviceType {
    Keyboard = 0,
    Disk = 1,
    Timer = 2,
    Serial = 3,
    Framebuffer = 4,
    PciController = 5,
    AhciController = 6,
    IdeController = 7,
    UsbController = 8,
    Generic = 9,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum DeviceState {
    Offline = 0,
    Online = 1,
    Error = 2,
}

pub const CAP_READ: u32 = 1 << 0;
pub const CAP_WRITE: u32 = 1 << 1;
pub const CAP_IRQ: u32 = 1 << 2;
pub const CAP_DMA: u32 = 1 << 3;
pub const CAP_MMIO: u32 = 1 << 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum IoRegionKind {
    Port = 0,
    MemoryMapped = 1,
}

#[derive(Debug, Clone, Copy)]
pub struct IoRegion {
    pub kind: IoRegionKind,
    pub base: u64,
    pub len: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct Device {
    pub id: u32,
    pub device_type: DeviceType,
    pub class: DeviceClass,
    pub state: DeviceState,
    pub capabilities: u32,
    pub irq_vector: Option<u8>,
    pub name: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct DeviceHandle {
    pub device_id: u32,
    pub capabilities: u32,
}

#[derive(Debug, Clone, Copy)]
struct Binding {
    driver_name: &'static str,
    device_id: u32,
}

pub struct DeviceRegistry {
    devices: [Option<Device>; MAX_DEVICES],
    bindings: [Option<Binding>; MAX_BINDINGS],
    count: usize,
    binding_count: usize,
    next_id: u32,
}

impl DeviceRegistry {
    pub const fn new() -> Self {
        const NONE_DEV: Option<Device> = None;
        const NONE_BIND: Option<Binding> = None;
        DeviceRegistry {
            devices: [NONE_DEV; MAX_DEVICES],
            bindings: [NONE_BIND; MAX_BINDINGS],
            count: 0,
            binding_count: 0,
            next_id: 1,
        }
    }

    pub fn register(&mut self, device_type: DeviceType, class: DeviceClass,
                     name: &'static str, description: &'static str,
                     capabilities: u32, irq_vector: Option<u8>) -> Option<u32> {
        if self.count >= MAX_DEVICES {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        let idx = self.count;
        self.count += 1;
        self.devices[idx] = Some(Device {
            id,
            device_type,
            class,
            state: DeviceState::Online,
            capabilities,
            irq_vector,
            name,
            description,
        });
        serial_println!("[DEV] {}: {} ({}) id={} irq={:?} caps=0x{:x}",
            name, description, class as u32, id, irq_vector, capabilities);
        Some(id)
    }

    pub fn find_by_id(&self, id: u32) -> Option<&Device> {
        for i in 0..self.count {
            if let Some(dev) = &self.devices[i] {
                if dev.id == id {
                    return Some(dev);
                }
            }
        }
        None
    }

    pub fn find_by_name(&self, name: &str) -> Option<&Device> {
        for i in 0..self.count {
            if let Some(dev) = &self.devices[i] {
                if dev.name == name {
                    return Some(dev);
                }
            }
        }
        None
    }

    pub fn set_state(&mut self, id: u32, state: DeviceState) {
        for i in 0..self.count {
            if let Some(dev) = &mut self.devices[i] {
                if dev.id == id {
                    dev.state = state;
                    return;
                }
            }
        }
    }

    pub fn set_error(&mut self, id: u32) {
        self.set_state(id, DeviceState::Error);
    }

    pub fn iter(&self) -> DeviceIter<'_> {
        DeviceIter { registry: self, idx: 0 }
    }

    pub fn bind(&mut self, driver_name: &'static str, device_id: u32) -> Option<DeviceHandle> {
        if self.binding_count >= MAX_BINDINGS {
            return None;
        }
        // Check device exists and is online without holding a borrow
        let caps = {
            let dev = self.find_by_id(device_id)?;
            if dev.state != DeviceState::Online {
                return None;
            }
            dev.capabilities
        };
        let idx = self.binding_count;
        self.binding_count += 1;
        self.bindings[idx] = Some(Binding { driver_name, device_id });
        Some(DeviceHandle { device_id, capabilities: caps })
    }

    pub fn unbind(&mut self, driver_name: &'static str, device_id: u32) {
        for i in 0..self.binding_count {
            if let Some(b) = self.bindings[i] {
                if b.driver_name == driver_name && b.device_id == device_id {
                    self.bindings[i] = None;
                    return;
                }
            }
        }
    }

    pub fn is_bound(&self, device_id: u32) -> bool {
        for i in 0..self.binding_count {
            if let Some(b) = self.bindings[i] {
                if b.device_id == device_id {
                    return true;
                }
            }
        }
        false
    }

    pub fn device_count(&self) -> usize { self.count }
    pub fn binding_count(&self) -> usize { self.binding_count }
}

pub struct DeviceIter<'a> {
    registry: &'a DeviceRegistry,
    idx: usize,
}

impl<'a> Iterator for DeviceIter<'a> {
    type Item = &'a Device;
    fn next(&mut self) -> Option<Self::Item> {
        while self.idx < self.registry.count {
            let i = self.idx;
            self.idx += 1;
            if let Some(dev) = &self.registry.devices[i] {
                return Some(dev);
            }
        }
        None
    }
}

// Global device registry

pub fn init() {
    let mut reg = crate::globals::DEVICE_REGISTRY.lock();
    serial_println!("[DEV] Device model initialized");
    let _ = &mut *reg;
}

pub fn register_device(device_type: DeviceType, class: DeviceClass,
                        name: &'static str, description: &'static str,
                        capabilities: u32, irq_vector: Option<u8>) -> Option<u32> {
    crate::globals::DEVICE_REGISTRY.lock().register(device_type, class, name, description, capabilities, irq_vector)
}

pub fn find_device(id: u32) -> Option<Device> {
    crate::globals::DEVICE_REGISTRY.lock().find_by_id(id).copied()
}

pub fn find_device_by_name(name: &str) -> Option<Device> {
    crate::globals::DEVICE_REGISTRY.lock().find_by_name(name).copied()
}

pub fn set_device_state(id: u32, state: DeviceState) {
    crate::globals::DEVICE_REGISTRY.lock().set_state(id, state);
}

pub fn bind_driver(driver_name: &'static str, device_id: u32) -> Option<DeviceHandle> {
    let handle = crate::globals::DEVICE_REGISTRY.lock().bind(driver_name, device_id)?;
    serial_println!("[DEV] Bound '{}' to device {}", driver_name, device_id);
    Some(handle)
}

pub fn unbind_driver(driver_name: &'static str, device_id: u32) {
    crate::globals::DEVICE_REGISTRY.lock().unbind(driver_name, device_id);
    serial_println!("[DEV] Unbound '{}' from device {}", driver_name, device_id);
}

pub fn for_each_device<F: FnMut(&Device)>(mut f: F) {
    let reg = crate::globals::DEVICE_REGISTRY.lock();
    for dev in reg.iter() {
        f(dev);
    }
}

// ============================================
// HAL Binding Layer — mediated hardware access
// ============================================

#[derive(Debug, Clone, Copy)]
pub enum DeviceError {
    NotFound,
    NotBound,
    Offline,
    InvalidHandle,
    IoError,
}

pub fn device_read(_handle: &DeviceHandle, _offset: u64, _buf: &mut [u8]) -> Result<usize, DeviceError> {
    Err(DeviceError::IoError)
}

pub fn device_write(_handle: &DeviceHandle, _offset: u64, _buf: &[u8]) -> Result<usize, DeviceError> {
    Err(DeviceError::IoError)
}

pub fn device_query_status(handle: &DeviceHandle) -> DeviceState {
    let reg = crate::globals::DEVICE_REGISTRY.lock();
    match reg.find_by_id(handle.device_id) {
        Some(dev) => dev.state,
        None => DeviceState::Offline,
    }
}

pub fn device_register_irq(_handle: &DeviceHandle, _callback: fn()) -> Result<(), DeviceError> {
    Err(DeviceError::IoError)
}

pub fn device_ack_irq(_handle: &DeviceHandle) -> Result<(), DeviceError> {
    Err(DeviceError::IoError)
}

fn caps_to_str(caps: u32) -> [u8; 5] {
    let mut p = [b'-'; 5];
    if caps & CAP_READ != 0 { p[0] = b'R'; }
    if caps & CAP_WRITE != 0 { p[1] = b'W'; }
    if caps & CAP_IRQ != 0 { p[2] = b'I'; }
    if caps & CAP_DMA != 0 { p[3] = b'D'; }
    if caps & CAP_MMIO != 0 { p[4] = b'M'; }
    p
}

fn state_to_str(s: DeviceState) -> &'static str {
    match s {
        DeviceState::Online => "ONLINE",
        DeviceState::Offline => "OFFLINE",
        DeviceState::Error => "ERROR",
    }
}

fn class_to_str(c: DeviceClass) -> &'static str {
    match c {
        DeviceClass::Input => "INPUT",
        DeviceClass::Storage => "STORAGE",
        DeviceClass::Timer => "TIMER",
        DeviceClass::Communication => "COMM",
        DeviceClass::Virtual => "VIRT",
        DeviceClass::Unknown => "???",
    }
}

fn type_to_str(t: DeviceType) -> &'static str {
    match t {
        DeviceType::Keyboard => "KBD",
        DeviceType::Disk => "DISK",
        DeviceType::Timer => "TIMR",
        DeviceType::Serial => "SER",
        DeviceType::Framebuffer => "FB",
        DeviceType::PciController => "PCI",
        DeviceType::AhciController => "AHCI",
        DeviceType::IdeController => "IDE",
        DeviceType::UsbController => "USB",
        DeviceType::Generic => "GEN",
    }
}

fn binding_state(reg: &DeviceRegistry, id: u32) -> &'static str {
    if reg.is_bound(id) { "BOUND" } else { "FREE" }
}

pub fn print_device_list() {
    let reg = crate::globals::DEVICE_REGISTRY.lock();
    println!(" ID  TYPE CLASS STATE    CAPS BIND   NAME");
    for dev in reg.iter() {
        let caps = caps_to_str(dev.capabilities);
        let caps_str = core::str::from_utf8(&caps).unwrap_or("-----");
        let bind = binding_state(&reg, dev.id);
        println!(" {:>3} {:>4} {:>6} {:>7} {} {:>5}  {}",
            dev.id, type_to_str(dev.device_type), class_to_str(dev.class),
            state_to_str(dev.state), caps_str, bind, dev.name);
    }
}

pub fn list() {
    let count = crate::globals::DEVICE_REGISTRY.lock().device_count();
    if count == 0 {
        println!("No devices registered.");
        return;
    }
    print_device_list();
    println!("  Total: {} device(s)", count);
}

// ============================================
// Boot-time device registration
// ============================================

pub fn register_boot_devices() {
    serial_println!("[DEV] Registering boot-time devices...");

    // Timer (PIT / HPET)
    register_device(DeviceType::Timer, DeviceClass::Timer, "pit",
        "Programmable Interval Timer", CAP_READ, Some(32));

    // Serial port (COM1)
    register_device(DeviceType::Serial, DeviceClass::Communication, "com1",
        "Serial Port COM1", CAP_READ | CAP_WRITE | CAP_IRQ, Some(36));

    // PS/2 Keyboard
    register_device(DeviceType::Keyboard, DeviceClass::Input, "ps2kbd",
        "PS/2 Keyboard", CAP_READ | CAP_IRQ, Some(33));

    // Framebuffer
    register_device(DeviceType::Framebuffer, DeviceClass::Virtual, "framebuffer",
        "UEFI GOP Framebuffer", CAP_READ | CAP_WRITE | CAP_MMIO, None);

    // PCI bus
    register_device(DeviceType::PciController, DeviceClass::Communication, "pci",
        "PCI Configuration Space", CAP_READ | CAP_WRITE, None);

    serial_println!("[DEV] Boot-time device registration complete");
}
