// src/interrupts/msi.rs
//! MSI and MSI-X interrupt support infrastructure.
//!
//! # Architecture
//! This module has two operational modes:
//!
//! **Direct mode** (default, no NEM PCI driver):
//!   The kernel writes MSI registers directly through the port-IO primitives in
//!   `drivers::pci`. This is the fast path used during early boot before the NEM
//!   driver stack is initialised.
//!
//! **Delegated mode** (when `pci.nem` is ACTIVE):
//!   The kernel pushes an `EVENT_MSI_CONFIGURE` event onto the event bus.
//!   `pci.nem` receives the event, performs the PCI config-space writes using its
//!   own `hst_outl`/`hst_inl` HST calls (which go through the capability/isolation
//!   layer), and fires back `EVENT_MSI_CONFIGURED`. This keeps all PCI access
//!   within the NEM driver's security boundary.
//!
//! # Vector allocator
//! A 256-entry boolean bitmap tracks which interrupt vectors are in use.
//! Vectors 0-47 are pre-marked as reserved:
//!   - 0-31  : CPU exceptions
//!   - 32-47 : legacy PIC IRQs (IRQ0-15 remapped)
//!   - 0x80  : syscall gate (INT 0x80)
//!
//! # Dynamic IDT dispatch
//! Because the IDT is a `lazy_static!` it cannot be rebuilt at runtime.
//! Instead, every MSI vector (48-255) uses a shared dispatch path through
//! `arch::x64::idt::msi_dispatch` which consults a per-vector handler table.

use lazy_static::lazy_static;
use spin::Mutex;

// ─── Vector allocator ────────────────────────────────────────────────────────

const MAX_VECTOR: usize = 256;
const FIRST_MSI_VECTOR: usize = 48;

lazy_static! {
    /// `true` = vector is in use / reserved.
    static ref VECTOR_BITMAP: Mutex<[bool; MAX_VECTOR]> = {
        let mut bm = [false; MAX_VECTOR];
        for i in 0..FIRST_MSI_VECTOR { bm[i] = true; }
        bm[0x80] = true;   // syscall gate
        Mutex::new(bm)
    };
}

/// Allocate a free interrupt vector in the MSI range (48-255).
pub fn msi_alloc_vector() -> Option<u8> {
    let mut bitmap = VECTOR_BITMAP.lock();
    for i in FIRST_MSI_VECTOR..MAX_VECTOR {
        if !bitmap[i] {
            bitmap[i] = true;
            return Some(i as u8);
        }
    }
    None
}

/// Release a previously-allocated MSI vector.
pub fn msi_free_vector(vector: u8) {
    let idx = vector as usize;
    if idx < FIRST_MSI_VECTOR || idx >= MAX_VECTOR { return; }
    VECTOR_BITMAP.lock()[idx] = false;
}

// ─── PCI NEM driver detection ─────────────────────────────────────────────────

/// Returns `true` when the `pci.nem` NEM driver is loaded and active.
/// In that case, MSI configuration should be delegated via the event bus.
fn pci_nem_is_active() -> bool {
    use crate::drivers::driver_runtime::{DRIVER_RUNTIME, DriverState};
    let rt = DRIVER_RUNTIME.lock();
    rt.get_by_name("pci")
        .map(|d| d.state == DriverState::Active)
        .unwrap_or(false)
}

// ─── Helper: pack a BDF + offset into a single u32 ───────────────────────────

#[inline]
fn pack_bdf(bus: u8, dev: u8, func: u8) -> u32 {
    ((bus as u32) << 16) | ((dev as u32) << 11) | ((func as u32) << 8)
}

// ─── Direct MSI configuration (kernel → PCI config space) ────────────────────

/// Write MSI registers directly. Used when `pci.nem` is not active.
fn configure_msi_direct(bus: u8, dev: u8, func: u8, cap: u8, vector: u8) {
    use crate::drivers::pci::{
        pci_config_read_word, pci_config_write_dword, pci_config_write_word,
    };

    let ctrl = pci_config_read_word(bus, dev, func, cap + 2);
    let is_64bit = (ctrl & (1 << 7)) != 0;

    // Message Address: Local APIC bus address for CPU 0
    pci_config_write_dword(bus, dev, func, cap + 4, 0xFEE0_0000);
    if is_64bit {
        pci_config_write_dword(bus, dev, func, cap + 8, 0);
    }

    // Message Data: fixed delivery, edge-triggered, vector[7:0]
    let data_off = if is_64bit { cap + 12 } else { cap + 8 };
    pci_config_write_dword(bus, dev, func, data_off, (vector as u32) & 0xFF);

    // Enable MSI (bit 0), clear MME[6:4]
    let new_ctrl = (ctrl & !0x0070) | 0x0001;
    pci_config_write_word(bus, dev, func, cap + 2, new_ctrl);
}

/// Delegate MSI configuration to `pci.nem` via the event bus.
/// Returns immediately — `pci.nem` acks asynchronously with EVENT_MSI_CONFIGURED.
fn configure_msi_via_eventbus(bus: u8, dev: u8, func: u8, cap: u8, vector: u8) {
    // data0: [63:32] = vector, [31:0] = packed BDF
    // data1: [7:0]   = cap_offset
    let packed_bdf = pack_bdf(bus, dev, func);
    let data0: u64 = ((vector as u64) << 32) | (packed_bdf as u64);
    let data1: u64 = cap as u64;

    let _ = crate::eventbus::EVENT_BUS.push_event(
        crate::eventbus::EVENT_MSI_CONFIGURE,
        crate::eventbus::SOURCE_KERNEL,
        0,          // device_id not used here
        data0,
        data1,
        crate::eventbus::EVENT_FLAG_URGENT,
    );
}

// ─── MSI-X enable stub ────────────────────────────────────────────────────────

/// Minimal MSI-X enable: sets the MSI-X Enable bit and Function Mask so the
/// device stops asserting its legacy INTx# line. Per-entry table writes require
/// BAR mapping, which is not yet available — this is clearly documented as a
/// TODO for when a generic PCI BAR mapping utility is added.
pub fn configure_msix_enable_only(
    bus: u8, dev: u8, func: u8, cap: u8,
) -> Result<(), &'static str> {
    use crate::drivers::pci::{pci_config_read_word, pci_config_write_word};

    let ctrl = pci_config_read_word(bus, dev, func, cap + 2);
    // Set Enable (bit 15) + Function Mask (bit 14) — masks all entries globally
    // until the driver has programmed each table entry.
    let new_ctrl = ctrl | (1 << 15) | (1 << 14);
    pci_config_write_word(bus, dev, func, cap + 2, new_ctrl);

    // TODO: map BAR[BIR], iterate table, write per-entry address/data,
    //       then clear Function Mask (bit 14).
    Err("MSI-X per-entry table write requires BAR mapping (not yet implemented)")
}

// ─── EVENT_MSI_CONFIGURED kernel-side listener ────────────────────────────────

/// Event handler registered in the kernel event bus.
/// Called when `pci.nem` completes an MSI configuration request.
fn on_msi_configured(ev: &crate::eventbus::Event) {
    let packed_bdf = ev.data0 as u32;
    let bus  = ((packed_bdf >> 16) & 0xFF) as u8;
    let dev  = ((packed_bdf >> 11) & 0x1F) as u8;
    let func = ((packed_bdf >>  8) & 0x07) as u8;
    crate::serial_println!(
        "[MSI] pci.nem confirmed MSI on {:02x}:{:02x}.{}",
        bus, dev, func
    );
}

/// Register the `EVENT_MSI_CONFIGURED` listener.  Call once during kernel init
/// (after the event bus is available but before any driver tries MSI).
pub fn init() {
    let _ = crate::eventbus::EVENT_BUS.register_handler(
        crate::eventbus::EVENT_MSI_CONFIGURED,
        on_msi_configured,
        "msi_configured_ack",
    );
}

// ─── High-level facade ────────────────────────────────────────────────────────

/// Allocate a vector, configure the PCI MSI capability (directly or via
/// `pci.nem`), and register an IDT handler — all in one call.
///
/// Returns the allocated vector number on success.
pub fn msi_request(
    bus: u8, dev: u8, func: u8,
    handler: fn(vector: u8),
) -> Result<u8, &'static str> {
    // 1. Find MSI capability (cap ID 0x05) — kernel always reads this itself.
    let cap = crate::drivers::pci::find_capability(bus, dev, func, 0x05)
        .ok_or("Device has no MSI capability")?;

    // 2. Allocate a free IDT vector.
    let vector = msi_alloc_vector().ok_or("No free MSI vectors available")?;

    // 3. Configure PCI registers — delegate to pci.nem if active, else direct.
    if pci_nem_is_active() {
        configure_msi_via_eventbus(bus, dev, func, cap, vector);
        // pci.nem will fire back EVENT_MSI_CONFIGURED asynchronously.
        crate::serial_println!(
            "[MSI] Delegated {:02x}:{:02x}.{} → vector {:#04x} to pci.nem",
            bus, dev, func, vector
        );
    } else {
        configure_msi_direct(bus, dev, func, cap, vector);
        crate::serial_println!(
            "[MSI] Direct config {:02x}:{:02x}.{} → vector {:#04x}",
            bus, dev, func, vector
        );
    }

    // 4. Register the interrupt handler in the IDT dispatch table.
    crate::arch::x64::idt::msi_register_handler(vector, handler);

    Ok(vector)
}

/// Release an MSI vector: disable MSI on the device, unregister the handler,
/// and return the vector to the free pool.
pub fn msi_release(bus: u8, dev: u8, func: u8, vector: u8) {
    // Disable MSI Enable bit via direct access (always safe to write directly).
    if let Some(cap) = crate::drivers::pci::find_capability(bus, dev, func, 0x05) {
        let ctrl = crate::drivers::pci::pci_config_read_word(bus, dev, func, cap + 2);
        crate::drivers::pci::pci_config_write_word(bus, dev, func, cap + 2, ctrl & !0x0001);
    }
    crate::arch::x64::idt::msi_unregister_handler(vector);
    msi_free_vector(vector);
    crate::serial_println!(
        "[MSI] Released {:02x}:{:02x}.{} vector {:#04x}",
        bus, dev, func, vector
    );
}
