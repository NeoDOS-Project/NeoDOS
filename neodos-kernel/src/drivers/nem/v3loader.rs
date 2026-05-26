// src/drivers/nem/v3loader.rs
// NEM v3 — Standalone binary driver loader
//
// Loads a NEM v3 standalone binary (.nem file), allocates memory from the
// kernel heap, applies relocations, resolves undefined symbols against the
// kernel export table, and returns function pointers for entry points.

use alloc::alloc::{alloc, dealloc, Layout};
use alloc::vec::Vec;
use crate::nem::{self, NemReloc, NemSymbol, ParsedNemV3};
use crate::nem::{
    R_NEM_64, R_NEM_PC32, R_NEM_32, R_NEM_32S, R_NEM_PLT32,
    NEM_SECT_TEXT, NEM_SECT_RODATA, NEM_SECT_DATA, NEM_SECT_UNDEF,
};

// ── Kernel Export Table ──

pub struct KernelExport {
    pub name: &'static str,
    pub addr: *const (),
}

// Safety: KernelExport is read-only after init, only mutated at compile-time
unsafe impl Sync for KernelExport {}

macro_rules! export_entry {
    ($fn:ident) => {
        KernelExport { name: stringify!($fn), addr: $fn as *const () }
    };
}

pub fn kernel_exports() -> &'static [KernelExport] {
    &KERNEL_EXPORTS
}

pub fn resolve_export(name: &str) -> Option<*const ()> {
    KERNEL_EXPORTS.iter().find(|e| e.name == name).map(|e| e.addr)
}

// HAL functions exported to NEM drivers
unsafe extern "C" fn hst_push_input_byte(byte: u8) {
    crate::input::push_byte(byte);
    crate::syscall::wake_blocked_readers();
}

unsafe extern "C" fn hst_log(level: u32, msg: *const u8, len: usize) {
    let s = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(msg, len)) };
    match level {
        0 => log::error!("[DRV] {}", s),
        1 => log::warn!("[DRV] {}", s),
        _ => log::info!("[DRV] {}", s),
    }
}

unsafe extern "C" fn hst_get_ticks() -> u64 { crate::hal::get_ticks() }
unsafe extern "C" fn hst_ack_irq(vector: u8) { crate::hal::ack_irq(vector); }

unsafe extern "C" fn hst_push_event(et: u32, src: u32, dev: u32, d0: u64, d1: u64, fl: u32) -> i64 {
    match crate::eventbus::push_event(et, src, dev, d0, d1, fl) {
        Ok(id) => id as i64,
        Err(_) => -1,
    }
}

unsafe extern "C" fn hst_inb(port: u16) -> u8 { crate::hal::inb(port) }
unsafe extern "C" fn hst_outb(port: u16, val: u8) { crate::hal::outb(port, val) }
unsafe extern "C" fn hst_inw(port: u16) -> u16 { crate::hal::inw(port) }
unsafe extern "C" fn hst_outw(port: u16, val: u16) { crate::hal::outw(port, val) }
unsafe extern "C" fn hst_inl(port: u16) -> u32 { crate::hal::inl(port) }
unsafe extern "C" fn hst_outl(port: u16, val: u32) { crate::hal::outl(port, val) }

static KERNEL_EXPORTS: &[KernelExport] = &[
    export_entry!(hst_push_input_byte),
    export_entry!(hst_log),
    export_entry!(hst_get_ticks),
    export_entry!(hst_ack_irq),
    export_entry!(hst_push_event),
    export_entry!(hst_inb),
    export_entry!(hst_outb),
    export_entry!(hst_inw),
    export_entry!(hst_outw),
    export_entry!(hst_inl),
    export_entry!(hst_outl),
];

// ── Memory allocation ──

const MAX_DRIVER_SIZE: usize = 1024 * 1024; // 1 MB per driver

fn alloc_driver_memory(size: usize) -> Option<*mut u8> {
    if size == 0 || size > MAX_DRIVER_SIZE {
        return None;
    }
    // Align to 16 bytes (minimum for heap allocator)
    let layout = Layout::from_size_align(size, 16).ok()?;
    let ptr = unsafe { alloc(layout) };
    if ptr.is_null() {
        return None;
    }
    unsafe { core::ptr::write_bytes(ptr, 0, size); }
    Some(ptr)
}

unsafe fn free_driver_memory(ptr: *mut u8, size: usize) {
    if let Ok(layout) = Layout::from_size_align(size, 16) {
        dealloc(ptr, layout);
    }
}

// ── Load result ──

#[derive(Debug)]
pub struct NemV3LoadResult {
    pub base: *mut u8,
    pub total_size: usize,
    pub text_base: *mut u8,
    pub rodata_base: *mut u8,
    pub data_base: *mut u8,
    pub entry_init: Option<unsafe extern "C" fn() -> i32>,
    pub entry_event: Option<unsafe extern "C" fn(*const crate::eventbus::Event) -> i32>,
    pub entry_activate: Option<unsafe extern "C" fn() -> i32>,
    pub entry_fini: Option<unsafe extern "C" fn() -> i32>,
    pub name: Vec<u8>,
}

/// Load a NEM v3 standalone binary into kernel heap memory.
///
/// 1. Parses the .nem v3 format
/// 2. Validates ABI
/// 3. Allocates memory, copies sections
/// 4. Applies relocations (resolves kernel exports)
/// 5. Finds entry points
pub fn load_nem_v3(data: &[u8]) -> Result<NemV3LoadResult, &'static str> {
    let parsed = nem::parse_nem_v3(data).ok_or("Invalid NEM v3 header")?;
    validate_v3_abi(&parsed)?;

    let total = parsed.header.total_mem_size as usize;
    if total == 0 || total > MAX_DRIVER_SIZE {
        return Err("Invalid driver size");
    }

    let base = alloc_driver_memory(total).ok_or("Out of memory for driver")?;

    let text_off = 0usize;
    let rodata_off = parsed.header.text_size as usize;
    let data_off = rodata_off + parsed.header.rodata_size as usize;

    unsafe {
        core::ptr::copy_nonoverlapping(parsed.text.as_ptr(), base.add(text_off), parsed.text.len());
        core::ptr::copy_nonoverlapping(parsed.rodata.as_ptr(), base.add(rodata_off), parsed.rodata.len());
        core::ptr::copy_nonoverlapping(parsed.data.as_ptr(), base.add(data_off), parsed.data.len());
    }

    let text_base = base;
    let rodata_base = unsafe { base.add(rodata_off) };
    let data_base = unsafe { base.add(data_off) };
    let bss_base = unsafe { data_base.add(parsed.header.data_size as usize) };

    // Apply relocations
    for reloc in parsed.relocs {
        apply_relocation(
            reloc,
            text_base,
            rodata_base,
            data_base,
            bss_base,
            parsed.symbols,
            parsed.strtab,
        )?;
    }

    // Find entry points (first by symbol name, fallback to header offset)
    let entry_init = find_entry_fn(parsed.symbols, parsed.strtab, "driver_init", text_base, parsed.header.entry_init);
    let entry_event = find_entry_event(parsed.symbols, parsed.strtab, "driver_on_event", text_base, parsed.header.entry_event);
    let entry_activate = find_entry_fn(parsed.symbols, parsed.strtab, "driver_activate", text_base, 0);
    let entry_fini = find_entry_fn(parsed.symbols, parsed.strtab, "driver_fini", text_base, parsed.header.entry_fini);

    Ok(NemV3LoadResult {
        base,
        total_size: total,
        text_base,
        rodata_base,
        data_base,
        entry_init,
        entry_event,
        entry_activate,
        entry_fini,
        name: parsed.name.as_bytes().to_vec(),
    })
}

/// Unload a driver, freeing its memory.
pub unsafe fn unload_nem_v3(result: &NemV3LoadResult) {
    free_driver_memory(result.base, result.total_size);
}

// ── ABI validation ──

fn validate_v3_abi(parsed: &ParsedNemV3) -> Result<(), &'static str> {
    let result = crate::drivers::abi::negotiate_default(
        parsed.header.abi_min,
        parsed.header.abi_target,
        parsed.header.abi_max,
    );
    if result.is_compatible() {
        Ok(())
    } else {
        Err(result.to_str())
    }
}

// ── Entry point resolution ──

fn get_sym_name<'a>(sym: &NemSymbol, strtab: &'a [u8]) -> Option<&'a str> {
    let off = sym.name_off as usize;
    if off >= strtab.len() { return None; }
    let end = strtab[off..].iter().position(|&b| b == 0)?;
    core::str::from_utf8(&strtab[off..off + end]).ok()
}

fn find_entry_fn(
    symbols: &[NemSymbol], strtab: &[u8], name: &str,
    text_base: *mut u8, fallback: u32,
) -> Option<unsafe extern "C" fn() -> i32> {
    for sym in symbols {
        if sym.section == NEM_SECT_UNDEF { continue; }
        if get_sym_name(sym, strtab) == Some(name) {
            let addr = unsafe { text_base.add(sym.value as usize) };
            return Some(unsafe { core::mem::transmute(addr) });
        }
    }
    if fallback != 0 && fallback != 0xFFFFFFFF {
        let addr = unsafe { text_base.add(fallback as usize) };
        return Some(unsafe { core::mem::transmute(addr) });
    }
    None
}

fn find_entry_event(
    symbols: &[NemSymbol], strtab: &[u8], name: &str,
    text_base: *mut u8, fallback: u32,
) -> Option<unsafe extern "C" fn(*const crate::eventbus::Event) -> i32> {
    for sym in symbols {
        if sym.section == NEM_SECT_UNDEF { continue; }
        if get_sym_name(sym, strtab) == Some(name) {
            let addr = unsafe { text_base.add(sym.value as usize) };
            return Some(unsafe { core::mem::transmute(addr) });
        }
    }
    if fallback != 0 && fallback != 0xFFFFFFFF {
        let addr = unsafe { text_base.add(fallback as usize) };
        return Some(unsafe { core::mem::transmute(addr) });
    }
    None
}

// ── Relocation ──

fn apply_relocation(
    reloc: &NemReloc,
    text_base: *mut u8,
    rodata_base: *mut u8,
    data_base: *mut u8,
    bss_base: *mut u8,
    symbols: &[NemSymbol],
    strtab: &[u8],
) -> Result<(), &'static str> {
    let section_base = match reloc.section as u8 {
        NEM_SECT_TEXT => text_base,
        NEM_SECT_RODATA => rodata_base,
        NEM_SECT_DATA => data_base,
        crate::nem::NEM_SECT_BSS => bss_base,
        _ => return Err("Invalid relocation section"),
    };

    let place = unsafe { section_base.add(reloc.offset as usize) };

    let (sym_value, _is_undef) = if reloc.sym_idx == 0xFF {
        (text_base as u64, false)
    } else if (reloc.sym_idx as usize) < symbols.len() {
        let sym = &symbols[reloc.sym_idx as usize];
        if sym.section == NEM_SECT_UNDEF {
            let sym_name = get_sym_name(sym, strtab).ok_or("Symbol name not found")?;
            let addr = resolve_export(sym_name).ok_or("Unresolved kernel symbol")?;
            (addr as u64, true)
        } else {
            let sym_section_base = match sym.section as u8 {
                NEM_SECT_TEXT => text_base,
                NEM_SECT_RODATA => rodata_base,
                NEM_SECT_DATA => data_base,
                crate::nem::NEM_SECT_BSS => bss_base,
                _ => return Err("Invalid symbol section"),
            };
            (unsafe { sym_section_base.add(sym.value as usize) } as u64, false)
        }
    } else {
        return Err("Symbol index out of range");
    };

    let s = sym_value;
    let a = reloc.addend as i64;
    let p = place as u64;

    unsafe {
        match reloc.r_type {
            R_NEM_64 => {
                core::ptr::write(place as *mut u64, (s as u64).wrapping_add(a as u64));
            }
            R_NEM_PC32 | R_NEM_PLT32 => {
                core::ptr::write(place as *mut i32, (s as i64).wrapping_add(a).wrapping_sub(p as i64) as i32);
            }
            R_NEM_32 => {
                core::ptr::write(place as *mut u32, (s as u64).wrapping_add(a as u64) as u32);
            }
            R_NEM_32S => {
                core::ptr::write(place as *mut i32, (s as i64).wrapping_add(a) as i32);
            }
            _ => return Err("Unknown relocation type"),
        }
    }

    Ok(())
}

// ── Event Bus Bridge ──
// Bridges the v3 driver's driver_on_event(*const NeoEvent) -> i32 calling
// convention to the kernel event bus's fn(&Event) ABI.

const MAX_V3_HANDLERS: usize = 8;

#[derive(Copy, Clone)]
struct V3HandlerEntry {
    event_type: u32,
    fn_ptr: usize,
}

static V3_HANDLERS: spin::Mutex<[Option<V3HandlerEntry>; MAX_V3_HANDLERS]> =
    spin::Mutex::new([None; MAX_V3_HANDLERS]);

fn v3_event_bridge(event: &crate::eventbus::Event) {
    let table = V3_HANDLERS.lock();
    for entry in table.iter() {
        if let Some(e) = entry {
            if e.event_type == event.event_type {
                let f: unsafe extern "C" fn(*const crate::eventbus::Event) -> i32 =
                    unsafe { core::mem::transmute(e.fn_ptr) };
                let _ = unsafe { f(event as *const _) };
                return;
            }
        }
    }
}

/// Register a v3 driver's event handler with the kernel event bus.
/// Dispatches to the correct driver via a per-event-type lookup table.
pub fn register_v3_event_bus_handler(
    entry_event: Option<unsafe extern "C" fn(*const crate::eventbus::Event) -> i32>,
    event_type: u32,
) -> Result<(), ()> {
    let event_fn_ptr = match entry_event {
        Some(f) => f as usize,
        None => return Ok(()),
    };
    {
        let mut table = V3_HANDLERS.lock();
        for slot in table.iter_mut() {
            if slot.is_none() {
                *slot = Some(V3HandlerEntry {
                    event_type,
                    fn_ptr: event_fn_ptr,
                });
                break;
            }
        }
    }
    crate::eventbus::EVENT_BUS.register_handler(
        event_type,
        v3_event_bridge,
        "v3_driver_event",
    )
}

// ── Tests ──

pub fn register_v3_loader_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_ne;
    use crate::test_true;

    test_case!("v3_kernel_exports_resolve", {
        let addr = resolve_export("hst_push_input_byte");
        test_ne!(addr, None);
        let addr2 = resolve_export("hst_log");
        test_ne!(addr2, None);
        let nonexistent = resolve_export("nonexistent_function_xyz");
        test_eq!(nonexistent, None);
    });

    test_case!("v3_kernel_export_table_size", {
        let exports = kernel_exports();
        test_true!(exports.len() >= 10);
        test_true!(exports.iter().any(|e| e.name == "hst_inb"));
        test_true!(exports.iter().any(|e| e.name == "hst_outb"));
    });

    test_case!("v3_parse_valid_nem", {
        use crate::nem::{build_valid_nem_v3, NemReloc, R_NEM_PC32};
        let code = [0x90u8; 64];
        let relocs = [
            NemReloc { offset: 0x10, section: 0, r_type: R_NEM_PC32, sym_idx: 0xFF, addend: -4 },
        ];
        let raw = build_valid_nem_v3("TESTSIMPLE", &code, &[], &[], 0, &relocs);
        let parsed = crate::nem::parse_nem_v3(&raw);
        test_ne!(parsed, None);
        let p = parsed.unwrap();
        test_eq!(p.name, "TESTSIMPLE");
        test_eq!(p.text.len(), 64);
        test_eq!(p.relocs.len(), 1);
    });

    test_case!("v3_validate_abi_passes", {
        let raw = crate::nem::build_valid_nem_v3("ABITEST", &[0x90u8; 16], &[], &[], 0, &[]);
        let parsed = crate::nem::parse_nem_v3(&raw).unwrap();
        test_true!(validate_v3_abi(&parsed).is_ok());
    });

    test_case!("v3_validate_abi_rejects_zero", {
        let mut raw = crate::nem::build_valid_nem_v3("BADABI", &[0x90u8; 16], &[], &[], 0, &[]);
        raw[16..18].copy_from_slice(&[0u8; 2]); // abi_min = 0
        let parsed = crate::nem::parse_nem_v3(&raw).unwrap();
        test_true!(validate_v3_abi(&parsed).is_err());
    });

    test_case!("v3_reloc_r64_patches_correctly", {
        use crate::nem::{build_valid_nem_v3, NemReloc, R_NEM_64};
        let code = [0x00u8; 32];
        let relocs = [
            NemReloc { offset: 0, section: 0, r_type: R_NEM_64, sym_idx: 0xFF, addend: 0 },
        ];
        let raw = build_valid_nem_v3("R64TEST", &code, &[], &[], 0, &relocs);
        let result = load_nem_v3(&raw);
        test_true!(result.is_ok());
        let r = result.unwrap();
        let written = unsafe { core::ptr::read(r.text_base as *const u64) };
        test_eq!(written, r.text_base as u64);
        unsafe { unload_nem_v3(&r); }
    });

    test_case!("v3_load_minimal_driver", {
        use crate::nem::build_valid_nem_v3;
        let code = [0xC3u8; 16]; // RET
        let raw = build_valid_nem_v3("MINLOAD", &code, &[], &[], 0, &[]);
        let result = load_nem_v3(&raw);
        test_true!(result.is_ok());
        let r = result.unwrap();
        test_eq!(r.name.as_slice(), b"MINLOAD");
        test_true!(r.entry_init.is_none()); // no symbols
        unsafe { unload_nem_v3(&r); }
    });

    test_case!("v3_load_with_sections", {
        use crate::nem::build_valid_nem_v3;
        let code = [0x90u8; 32];
        let rodata = b"HELLO_DRIVER";
        let data = [0x42u8; 8];
        let raw = build_valid_nem_v3("SECTEST", &code, rodata, &data, 16, &[]);
        let result = load_nem_v3(&raw);
        test_true!(result.is_ok());
        let r = result.unwrap();
        // Verify rodata was copied correctly
        let loaded_rodata = unsafe { core::slice::from_raw_parts(r.rodata_base, rodata.len()) };
        test_eq!(loaded_rodata, rodata);
        let loaded_data = unsafe { core::slice::from_raw_parts(r.data_base, data.len()) };
        test_eq!(loaded_data, &data[..]);
        unsafe { unload_nem_v3(&r); }
    });
}
