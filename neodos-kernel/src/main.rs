#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![feature(allocator_api)]
#![feature(strict_provenance)]
#![feature(ptr_fn_addr_eq)]
#![feature(unsigned_is_multiple_of)]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]
#![allow(static_mut_refs)]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

extern crate alloc;
use core::panic::PanicInfo;

#[macro_use]
pub mod log;

// Slab/heap allocator now live under `memory/`; keep `crate::{slab, allocator}`.
pub use memory::{allocator, slab};
mod boot;
mod arch;
mod hal;
mod console;
pub mod scheduler;
mod drivers;
mod buffer;
mod fs;
// VFS now lives under `fs/vfs`; keep the historical `crate::vfs` path.
pub use fs::vfs;
mod input;
mod graphics;
// Font now lives under `graphics/font`; keep the historical `crate::font`.
pub use graphics::font;
// NEM format parser now lives under `drivers/nem/format`; keep `crate::nem`.
pub use drivers::nem::format as nem;
mod eventbus;
mod dpc;
mod memory;
pub mod syscall;
mod apc;
mod irp;
mod interrupts;
mod timers;
mod testing;
mod watchdog;  // A3.3 Watchdog subsystem
mod crash;
mod security;
mod exception;  // A3.4 SEH + Exception Dispatcher
mod urn;
// Power Manager now lives under `services/power`; keep `crate::power`.
pub use services::power;
mod object;
mod kwait;
mod net;
mod cm;
mod services;
// Keyboard now lives under `input/kbd`; keep the historical `crate::kbd`.
pub use input::kbd;
mod stress_spawn; // #345 Phase 2 diagnostic spawn-storm harness
mod i18n_tests;
mod id_index; // NEODOS-05 (#635): O(1) id index (Obj table + scheduler)
// Cross-cutting infrastructure now lives under `infra/`.
mod infra;
pub use infra::{
    abi_freeze, boot_benchmark, cpu, elf, globals, handle, invariants, lock_order,
    nxl, panic_classification, trace, usermode, work_queue,
};

use graphics::FramebufferInfo;

pub const KERNEL_VERSION: &str = concat!(
    "NeoDOS Kernel v",
    env!("CARGO_PKG_VERSION"),
    " (git ",
    env!("NEODOS_GIT_REV"),
    ") - The Rusty DOS Revival"
);

const BOOTINFO_MAGIC: u32 = 0x4E444F53; // "NDOS" in ASCII
const KERNEL_VERSION_CODE: u32 = (10) << 8 | 5; // v0.10.5

#[repr(C)]
pub struct BootInfo {
    pub magic: u32,           // must be 0x4E444F53
    pub version: u32,         // bootloader version (0x00MMmmPP: major, minor, patch)
    pub fb_info: FramebufferInfo,
    pub memory_map_addr: u64,
    pub memory_map_size: u64,
    pub memory_map_desc_size: u64,
    pub memory_map_desc_version: u32,
    pub fs_image_addr: u64,
    pub fs_image_size: u64,
    pub acpi_rsdp_addr: u64,  // ACPI RSDP physical address (0 if not found)
}

#[no_mangle]
#[link_section = ".text.entry"]
/// # Safety
///
/// This function is called directly by the bootloader after exiting UEFI boot services.
/// It must only be called once, with a valid `BootInfo` pointer provided by the bootloader.
/// The caller must ensure that the boot info structure is correctly initialized and that
/// the system is in a state suitable for kernel initialization (long mode enabled, page
/// tables set up, etc.).
pub unsafe extern "sysv64" fn rust_start(boot_info: &BootInfo) -> ! {
    boot::init(boot_info)
}
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    hal::disable_interrupts();

    let class = crate::panic_classification::current_panic_class();
    // Lock-free first report: capture the panic even if the regular logger
    // deadlocks on the SERIAL1 spinlock.
    crate::raw_serial_println!(
        "[PANIC] class={} rsp={:#x} msg={}",
        class.to_str(),
        unsafe { crate::hal::raw::raw_read_rsp() },
        info.message(),
    );
    crate::scheduler::diag::dump_raw();
    crate::scheduler::diag::sys_dump_raw();
    crate::scheduler::diag::frame_dump_raw();
    crate::scheduler::diag::ctx_dump_raw();
    crate::scheduler::diag::rsp_dump_raw();
    crate::scheduler::diag::dr_dump_raw();
    crate::scheduler::diag::kcpu_dump_raw();
    crate::scheduler::diag::st_dump_raw(); // #345 Phase 2A stress trace
    crate::scheduler::diag::kstack::dump_raw(); // #476 H1 switch-out kstack tracking
    crate::slab::free_bad_dump(); // #476 FREE_BAD allocator ownership audit
    crate::scheduler::diag::iretq::dump_raw(); // #476 iretq frame audit
    crate::raw_serial_println!(
        "[VFS_STATE] owner_cpu={} owner_tid={} owner_pid={} owner_rip=0x{:x} owner_acq={} waiter_cpu={} waiter=0x{:x} waits={}",
        crate::scheduler::diag::vfs_owner_cpu(),
        crate::scheduler::diag::vfs_owner_tid(),
        crate::scheduler::diag::vfs_owner_pid(),
        crate::scheduler::diag::vfs_owner_rip(),
        crate::scheduler::diag::vfs_owner_acq(),
        crate::scheduler::diag::vfs_waiter_cpu(),
        crate::scheduler::diag::vfs_waiter_word(),
        crate::scheduler::diag::vfs_wait_count());
    crate::raw_serial_println!("[CORRELATION] last_DOUBLE_RUNNING_seq={}", crate::scheduler::diag::dr_last_seq());
    println!("\r\n!!! KERNEL PANIC (CLASS: {}) !!!", class.to_str());

    // Capture approximate RIP from return address on stack, and RSP
    let rsp: u64 = unsafe { crate::hal::raw::raw_read_rsp() };
    let rip: u64 = unsafe { (rsp as *const u64).read() };

    // Dump crash dump to serial + RAM buffer (must happen before any other output)
    crate::crash::dump_panic(rip, rsp);

    if let Some(location) = info.location() {
        println!("Location: {}:{}", location.file(), location.line());
    }
    println!("Message: {}", info.message());

    // Dump forensic info to serial (println may fail if framebuffer is corrupt)
    crate::panic_classification::dump_forensic_info();

    hal::halt();
}
