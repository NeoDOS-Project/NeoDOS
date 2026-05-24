// src/drivers/nem/loader.rs
// Driver Certification Pipeline v1 — strict lifecycle: Loaded → Initialized → Registered → Bound → Active
//
// A driver loaded here follows all pipeline stages synchronously. If any step
// fails, the driver is left at its current state with a last_error code.
// NDREG can then inspect the state to determine why the driver is not ACTIVE.
//
// Supports NEM v1 (basic drivers) and NEM v2 (ABI-validated, categorized drivers).

use alloc::vec::Vec;
use crate::nem;
use crate::drivers::nem::policy;
use crate::drivers::driver_runtime::{self, DriverId, DriverState, PipelineStep};
use crate::eventbus::{self, EVENT_DRIVER_LOADED, SOURCE_KERNEL};
use log::info;

/// Load a .nem driver from a NeoFS path through the full certification pipeline.
/// Returns the driver id on success.
pub fn load_nem(path: &str) -> Result<DriverId, &'static str> {
    // ── STAGE 1: LOAD ──
    // Parse the .nem binary, validate ABI, check policy
    let data = read_file(path).map_err(|_| "IO error")?;
    let parsed = nem::parse_nem(&data).ok_or("Invalid NEM header")?;

    // Policy validation (driver type + v2 requirement)
    policy::validate_driver(&parsed)?;

    // ABI validation (v2 drivers must have compatible ABI)
    policy::validate_abi(&parsed)?;

    // Register runtime entry — sets state to Loaded (with category + ABI fields for v2)
    let id = if parsed.is_v2 {
        driver_runtime::register_driver_ext(
            parsed.name,
            parsed.driver_type,
            nem::NEM_API_VERSION,
            parsed.compat_flags,
            parsed.abi_min,
            parsed.abi_target,
            parsed.abi_max,
            parsed.category,
        )
    } else {
        driver_runtime::register_driver(
            parsed.name,
            parsed.driver_type,
            nem::NEM_API_VERSION,
            parsed.compat_flags,
        )
    }.map_err(|_| "Failed to register driver")?;

    // ── STAGE 2: INITIALIZE ──
    // Allocate user slot and copy driver code into memory
    let slot = match crate::arch::x64::paging::alloc_user_slot() {
        Some(s) => s,
        None => {
            driver_runtime::DRIVER_RUNTIME.lock()
                .set_error(id, driver_runtime::ERR_OUT_OF_MEMORY, true);
            return Err("Out of memory");
        }
    };

    if slot.stack_top > crate::arch::x64::paging::USER_LIMIT {
        crate::arch::x64::paging::free_user_slot(slot.slot_idx);
        driver_runtime::DRIVER_RUNTIME.lock()
            .set_error(id, driver_runtime::ERR_INIT_FAILED, true);
        return Err("Slot out of bounds");
    }

    let code_len = parsed.code.len();
    unsafe {
        let src = parsed.code.as_ptr();
        let dst = slot.code_base as *mut u8;
        core::ptr::copy_nonoverlapping(src, dst, code_len);
    }

    // Spawn driver task in scheduler — driver is now Initialized
    let entry_point = slot.code_base + parsed.entry_offset as u64 - 32;
    let _pid = crate::usermode::spawn_usermode(
        entry_point,
        slot.stack_top,
        slot.slot_idx,
        2,
        "\\SYSTEM\\DRIVERS",
    );

    // Transition to Initialized
    if driver_runtime::DRIVER_RUNTIME.lock()
        .try_transition(id, DriverState::Initialized).is_err()
    {
        driver_runtime::DRIVER_RUNTIME.lock()
            .set_error(id, driver_runtime::ERR_INIT_FAILED, true);
        return Err("Failed to transition to Initialized");
    }

    // ── STAGE 3: REGISTER ──
    // Push DRIVER_LOADED event and advance to Registered
    let _ = eventbus::push_event(
        EVENT_DRIVER_LOADED,
        SOURCE_KERNEL,
        0,
        id as u64,
        0,
        0,
    );

    if driver_runtime::DRIVER_RUNTIME.lock()
        .try_transition(id, DriverState::Registered).is_err()
    {
        driver_runtime::DRIVER_RUNTIME.lock()
            .set_error(id, driver_runtime::ERR_REGISTRATION_FAILED, true);
        return Err("Failed to transition to Registered");
    }

    // ── STAGE 4: BIND ──
    if driver_runtime::DRIVER_RUNTIME.lock()
        .try_transition(id, DriverState::Bound).is_err()
    {
        driver_runtime::DRIVER_RUNTIME.lock()
            .set_error(id, driver_runtime::ERR_BIND_FAILED, true);
        return Err("Failed to transition to Bound");
    }

    // ── STAGE 5: CERTIFY & ACTIVATE ──
    // Verify all conditions and mark driver ACTIVE
    match driver_runtime::DRIVER_RUNTIME.lock().certify_and_activate(id) {
        Ok(()) => {
            info!("[NEM] Loaded & Certified: {} (type={:?}, id={}, pid={})",
                path, parsed.driver_type, id, _pid);
            Ok(id)
        }
        Err(e) => {
            driver_runtime::DRIVER_RUNTIME.lock()
                .set_certification_step(id, PipelineStep::Certification);
            info!("[NEM] Loaded but NOT Active: {} (type={:?}, id={}) — {}",
                path, parsed.driver_type, id, e);
            Err(e)
        }
    }
}

/// Read a .nem file from NeoFS into a Vec<u8>.
pub fn read_file(path: &str) -> Result<Vec<u8>, ()> {
    crate::globals::with_vfs(|vfs| {
        let (drive_idx, node) = vfs.resolve_path(path).map_err(|_| ())?;
        if node.mode & crate::fs::vfs::MODE_FILE == 0 {
            return Err(());
        }
        let size = node.size as usize;
        if size == 0 || size > 65536 {
            return Err(());
        }
        let mut buf = alloc::vec::Vec::with_capacity(size);
        buf.resize(size, 0);
        let read = vfs.read(drive_idx, node.inode, 0, &mut buf).map_err(|_| ())?;
        buf.truncate(read);
        Ok(buf)
    })
}
