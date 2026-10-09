// src/virtio/transport.rs — VirtIO PCI transport abstraction
// Supports legacy I/O BAR (PCI 0.95 / 1.0 transitional) and modern MMIO BAR (VirtIO 1.0+).

use core::sync::atomic::{fence, Ordering};
use crate::drivers::pci::pci_config_read_word;
use crate::drivers::pci::pci_config_read_dword;
use crate::drivers::pci::pci_config_write_word;
use crate::log::LogSubsys;


// ── Legacy I/O register offsets ──────────────────────────────────────
const LEGACY_HOST_FEATURES: u16 = 0x00;
const LEGACY_GUEST_FEATURES: u16 = 0x04;
const LEGACY_QUEUE_PFN: u16 = 0x08;
const LEGACY_QUEUE_NUM: u16 = 0x0C;
const LEGACY_QUEUE_SEL: u16 = 0x0E;
const LEGACY_QUEUE_NOTIFY: u16 = 0x10;
const LEGACY_STATUS: u16 = 0x12;
const LEGACY_DEVICE_CFG: u16 = 0x14;

// ── Modern MMIO register offsets (BAR0 — common config) ─────────────
const MOD_DEVICE_FEATURES_SEL: u64 = 0x0c;
const MOD_DEVICE_FEATURES: u64 = 0x10;
const MOD_DRIVER_FEATURES_SEL: u64 = 0x1c;
const MOD_DRIVER_FEATURES: u64 = 0x20;
const MOD_QUEUE_SEL: u64 = 0x2c;
const MOD_QUEUE_SIZE: u64 = 0x34;
const MOD_QUEUE_READY: u64 = 0x38;
const MOD_QUEUE_DESC: u64 = 0x48;  // 52 bits valid, 2x32-bit
const MOD_QUEUE_AVAIL: u64 = 0x50; // 2x32-bit
const MOD_QUEUE_USED: u64 = 0x58;  // 2x32-bit
const MOD_DEVICE_STATUS: u64 = 0x5c;

// ── Status bits ─────────────────────────────────────────────────────
const STATUS_ACK: u8 = 0x01;
const STATUS_DRIVER: u8 = 0x02;
const STATUS_DRIVER_OK: u8 = 0x04;
const STATUS_FEATURES_OK: u8 = 0x08;
const STATUS_FAILED: u8 = 0x80;

// ── PCI vendor ──────────────────────────────────────────────────────
pub const VIRTIO_VENDOR: u16 = 0x1AF4;

pub enum VirtioTransport {
    Legacy { io_base: u16 },
    Modern { common: u64, isr: u64, device_cfg: u64, notify: u64, notify_off_multiplier: u32 },
}

impl VirtioTransport {
    pub fn probe_for(vendor: u16, device: u16) -> Option<(Self, u8, u8, u8)> {
        for bus in 0..=2 {
            for dev in 0..32 {
                for func in 0..8 {
                    let v = pci_config_read_word(bus, dev, func, 0);
                    if v == 0xFFFF || v == 0 { if func == 0 { break; } continue; }
                    let did = pci_config_read_word(bus, dev, func, 2);
                    if v == vendor && did == device {
                        if let Some(transport) = Self::probe_at(bus, dev, func) {
                            return Some((transport, bus, dev, func));
                        }
                        continue;
                    }
                }
            }
        }
        None
    }

    /// Probe for ANY VirtIO device (vendor 0x1AF4) — returns (transport, bus, dev, func, device_id).
    pub fn probe_any<F>(mut accept: F) -> Option<(Self, u8, u8, u8, u16)>
    where F: FnMut(u16) -> bool {
        for bus in 0..=0 {
            for dev in 0..32 {
                for func in 0..8 {
                    let v = pci_config_read_word(bus, dev, func, 0);
                    if v == 0xFFFF || v == 0 { if func == 0 { break; } continue; }
                    if v != VIRTIO_VENDOR { continue; }
                    let did = pci_config_read_word(bus, dev, func, 2);
                    if !accept(did) { continue; }
                    if let Some(transport) = Self::probe_at(bus, dev, func) {
                        return Some((transport, bus, dev, func, did));
                    }
                }
            }
        }
        None
    }

    fn probe_at(bus: u8, dev: u8, func: u8) -> Option<Self> {
        // Enable I/O + Memory + Bus Master
        let cmd_old = pci_config_read_word(bus, dev, func, 0x04);
        pci_config_write_word(bus, dev, func, 0x04, cmd_old | 0x07);
        let bar0 = pci_config_read_dword(bus, dev, func, 0x10);
        if bar0 & 1 != 0 {
            let io_base = (bar0 & 0xFFFC) as u16;
            if io_base == 0 { return None; }
            kinfo!(LogSubsys::Virtio, "Legacy I/O at 0x{:04x}", io_base);
            return Some(VirtioTransport::Legacy { io_base });
        }
        // MMIO BAR not supported
        None
    }

    // ── I/O port helpers ───────────────────────────────────────────
    fn inb(&self, reg: u16) -> u8 {
        if let VirtioTransport::Legacy { io_base } = self {
            unsafe { crate::hal::raw::raw_inb(io_base + reg) }
        } else { 0 }
    }
    fn inw(&self, reg: u16) -> u16 {
        if let VirtioTransport::Legacy { io_base } = self {
            unsafe { crate::hal::raw::raw_inw(io_base + reg) }
        } else { 0 }
    }
    fn inl(&self, reg: u16) -> u32 {
        if let VirtioTransport::Legacy { io_base } = self {
            unsafe { crate::hal::raw::raw_inl(io_base + reg) }
        } else { 0 }
    }
    fn outb(&self, reg: u16, val: u8) {
        if let VirtioTransport::Legacy { io_base } = self {
            unsafe { crate::hal::raw::raw_outb(io_base + reg, val) }
        }
    }
    fn outw(&self, reg: u16, val: u16) {
        if let VirtioTransport::Legacy { io_base } = self {
            unsafe { crate::hal::raw::raw_outw(io_base + reg, val) }
        }
    }
    fn outl(&self, reg: u16, val: u32) {
        if let VirtioTransport::Legacy { io_base } = self {
            unsafe { crate::hal::raw::raw_outl(io_base + reg, val) }
        }
    }

    // ── MMIO helpers ───────────────────────────────────────────────
    fn mmio_read32(&self, base: u64, offset: u64) -> u32 {
        unsafe { (base as *const u32).add((offset / 4) as usize).read_volatile() }
    }
    fn mmio_write32(&self, base: u64, offset: u64, val: u32) {
        unsafe { (base as *mut u32).add((offset / 4) as usize).write_volatile(val) }
    }
    fn mmio_write64(&self, base: u64, offset: u64, val: u64) {
        unsafe {
            let p = (base + offset) as *mut u64;
            p.write_volatile(val);
        }
    }
    // ── High-level operations ──────────────────────────────────────

    /// Reset device.
    pub fn reset(&self) {
        match self {
            VirtioTransport::Legacy { .. } => {
                self.outb(LEGACY_STATUS, 0);
                fence(Ordering::SeqCst);
            }
            VirtioTransport::Modern { common, .. } => {
                self.mmio_write32(*common, MOD_DEVICE_STATUS, 0);
                fence(Ordering::SeqCst);
            }
        }
    }

    /// Write device status.
    pub fn write_status(&self, status: u8) {
        match self {
            VirtioTransport::Legacy { .. } => self.outb(LEGACY_STATUS, status),
            VirtioTransport::Modern { common, .. } => self.mmio_write32(*common, MOD_DEVICE_STATUS, status as u32),
        }
        fence(Ordering::SeqCst);
    }

    /// Read device status.
    pub fn read_status(&self) -> u8 {
        match self {
            VirtioTransport::Legacy { .. } => self.inb(LEGACY_STATUS),
            VirtioTransport::Modern { common, .. } => self.mmio_read32(*common, MOD_DEVICE_STATUS) as u8,
        }
    }

    /// Read host features (32-bit).
    pub fn read_host_features(&self) -> u32 {
        match self {
            VirtioTransport::Legacy { .. } => self.inl(LEGACY_HOST_FEATURES),
            VirtioTransport::Modern { common, .. } => {
                self.mmio_write32(*common, MOD_DEVICE_FEATURES_SEL, 0);
                self.mmio_read32(*common, MOD_DEVICE_FEATURES)
            }
        }
    }

    /// Write guest features (32-bit).
    pub fn write_guest_features(&self, features: u32) {
        match self {
            VirtioTransport::Legacy { .. } => self.outl(LEGACY_GUEST_FEATURES, features),
            VirtioTransport::Modern { common, .. } => {
                self.mmio_write32(*common, MOD_DRIVER_FEATURES_SEL, 0);
                self.mmio_write32(*common, MOD_DRIVER_FEATURES, features);
            }
        }
        fence(Ordering::SeqCst);
    }

    /// Read config space u32.
    pub fn read_config32(&self, offset: u16) -> u32 {
        match self {
            VirtioTransport::Legacy { .. } => self.inl(LEGACY_DEVICE_CFG + offset),
            VirtioTransport::Modern { device_cfg, .. } => self.mmio_read32(*device_cfg, offset as u64),
        }
    }

    /// Read config space u64 (two 32-bit reads).
    pub fn read_config64(&self, offset: u16) -> u64 {
        let lo = self.read_config32(offset);
        let hi = self.read_config32(offset + 4);
        (hi as u64) << 32 | lo as u64
    }

    /// Set up a virtqueue.
    pub fn setup_queue(&self, queue_idx: u16, desc_phys: u64, avail_phys: u64, used_phys: u64, size: u16) -> bool {
        match self {
            VirtioTransport::Legacy { .. } => {
                self.outw(LEGACY_QUEUE_SEL, queue_idx);
                fence(Ordering::SeqCst);
                let qsize = self.inw(LEGACY_QUEUE_NUM);
                if qsize == 0 { return false; }
                let pfn = (desc_phys >> 12) as u32;
                self.outl(LEGACY_QUEUE_PFN, pfn);
                fence(Ordering::SeqCst);
                true
            }
            VirtioTransport::Modern { common, .. } => {
                self.mmio_write32(*common, MOD_QUEUE_SEL, queue_idx as u32);
                fence(Ordering::SeqCst);
                self.mmio_write32(*common, MOD_QUEUE_SIZE, size as u32);
                fence(Ordering::SeqCst);
                self.mmio_write64(*common, MOD_QUEUE_DESC, desc_phys);
                self.mmio_write64(*common, MOD_QUEUE_AVAIL, avail_phys);
                self.mmio_write64(*common, MOD_QUEUE_USED, used_phys);
                fence(Ordering::SeqCst);
                self.mmio_write32(*common, MOD_QUEUE_READY, 1);
                fence(Ordering::SeqCst);
                self.mmio_read32(*common, MOD_QUEUE_READY) == 1
            }
        }
    }

    /// Notify device — ring doorbell for a queue.
    pub fn notify(&self, queue_idx: u16) {
        match self {
            VirtioTransport::Legacy { .. } => {
                self.outw(LEGACY_QUEUE_NOTIFY, queue_idx);
            }
            VirtioTransport::Modern { notify, notify_off_multiplier, .. } => {
                // In modern mode, notify by writing queue_idx to (notify_base + qidx * mult)
                let off = (queue_idx as u32) * *notify_off_multiplier;
                let addr = *notify + off as u64;
                unsafe {
                    (addr as *mut u32).write_volatile(queue_idx as u32);
                }
            }
        }
        fence(Ordering::SeqCst);
    }

    /// Negotiate features: read host, AND with accepted, write guest.
    pub fn negotiate_features(&self, accepted: u32) -> u32 {
        let host = self.read_host_features();
        let guest = host & accepted;
        self.write_guest_features(guest);
        guest
    }

    /// Standard init sequence (ACK → DRIVER → features → FEATURES_OK → DRIVER_OK).
    /// Returns false on failure.
    pub fn standard_init(&self, accepted_features: u32) -> Result<u32, ()> {
        self.reset();
        // Wait for reset
        for _ in 0..1000 {
            if self.read_status() == 0 { break; }
            core::hint::spin_loop();
        }

        // ACK | DRIVER
        self.write_status(STATUS_ACK);
        self.write_status(STATUS_ACK | STATUS_DRIVER);

        // Features
        let guest_features = self.negotiate_features(accepted_features);

        // FEATURES_OK
        self.write_status(STATUS_ACK | STATUS_DRIVER | STATUS_FEATURES_OK);
        let status = self.read_status();
        if status & STATUS_FAILED != 0 {
            return Err(());
        }

        // Read config (capacity, etc.) can be done here for legacy mode
        Ok(guest_features)
    }

    /// Finalize init: write DRIVER_OK.
    pub fn finalize_init(&self) {
        let s = self.read_status();
        self.write_status(s | STATUS_DRIVER_OK);
    }

    /// Try to probe as modern (device 0x1042) then legacy (0x1001).
    pub fn probe_block() -> Option<(Self, u8, u8, u8)> {
        // Try modern first
        if let Some(result) = Self::probe_for(VIRTIO_VENDOR, 0x1042) {
            return Some(result);
        }
        // Fall back to legacy
        Self::probe_for(VIRTIO_VENDOR, 0x1001)
    }

    /// Return true if using modern transport.
    pub fn is_modern(&self) -> bool {
        matches!(self, VirtioTransport::Modern { .. })
    }
}
