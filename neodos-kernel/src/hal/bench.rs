//! Benchmark hooks.
//!
//! Drivers must not depend on the boot benchmark subsystem (INV-1). They call
//! these no-op-able hooks; `boot_benchmark` registers its counters at boot.
//! Without a registered hook the calls are free.

use core::sync::atomic::{AtomicUsize, Ordering};

pub struct BenchHooks {
    pub ahci_cmd_start: fn(),
    pub ahci_cmd_polled: fn(u64),
    pub ahci_cmd_done: fn(u64),
    pub ahci_cmd_timeout: fn(),
    pub ahci_dma_failure: fn(),
    pub virtio_cmd: fn(),
}

static AHCI_START: AtomicUsize = AtomicUsize::new(0);
static AHCI_POLLED: AtomicUsize = AtomicUsize::new(0);
static AHCI_DONE: AtomicUsize = AtomicUsize::new(0);
static AHCI_TIMEOUT: AtomicUsize = AtomicUsize::new(0);
static AHCI_DMA_FAIL: AtomicUsize = AtomicUsize::new(0);
static VIRTIO_CMD: AtomicUsize = AtomicUsize::new(0);

pub fn set_hooks(h: &BenchHooks) {
    AHCI_START.store(h.ahci_cmd_start as usize, Ordering::Release);
    AHCI_POLLED.store(h.ahci_cmd_polled as usize, Ordering::Release);
    AHCI_DONE.store(h.ahci_cmd_done as usize, Ordering::Release);
    AHCI_TIMEOUT.store(h.ahci_cmd_timeout as usize, Ordering::Release);
    AHCI_DMA_FAIL.store(h.ahci_dma_failure as usize, Ordering::Release);
    VIRTIO_CMD.store(h.virtio_cmd as usize, Ordering::Release);
}

#[inline]
fn call0(slot: &AtomicUsize) {
    let raw = slot.load(Ordering::Acquire);
    if raw != 0 {
        let f: fn() = unsafe { core::mem::transmute(raw) };
        f();
    }
}

#[inline]
fn call1(slot: &AtomicUsize, v: u64) {
    let raw = slot.load(Ordering::Acquire);
    if raw != 0 {
        let f: fn(u64) = unsafe { core::mem::transmute(raw) };
        f(v);
    }
}

pub fn ahci_cmd_start() { call0(&AHCI_START); }
pub fn ahci_cmd_polled(loops: u64) { call1(&AHCI_POLLED, loops); }
pub fn ahci_cmd_done(wait_ms: u64) { call1(&AHCI_DONE, wait_ms); }
pub fn ahci_cmd_timeout() { call0(&AHCI_TIMEOUT); }
pub fn ahci_dma_failure() { call0(&AHCI_DMA_FAIL); }
pub fn virtio_cmd() { call0(&VIRTIO_CMD); }
