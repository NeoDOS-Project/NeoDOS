// src/interrupts/mod.rs
//! Kernel interrupt management subsystem.
//!
//! This module groups all interrupt-related infrastructure:
//!   - `msi` — MSI/MSI-X vector allocation and PCI configuration helpers.
//!   - `ioapic` — I/O APIC interrupt controller (replaces legacy PIC).
//!
//! The legacy PIC (8259A) initialisation lives in `arch::x64::pic`. The IDT
//! itself is in `arch::x64::idt`. This subsystem builds on top of those and
//! provides higher-level abstractions for modern MSI-based interrupt delivery.

pub mod msi;
pub mod ioapic;
