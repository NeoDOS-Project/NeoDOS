use crate::arch::x64::paging;
use crate::log::LogSubsys;
use crate::globals;
use crate::fs::vfs::{VfsNode, MODE_DIR, MODE_FILE};
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;
use lazy_static::lazy_static;
use alloc::vec::Vec;

/// Export-table address of the relocatable (PIE) `math.nxl`, for the NXL test.
pub static MATH_EXPORT_BASE: AtomicU64 = AtomicU64::new(0);

const NXL_REGION_BASE: u64 = 0x1e00_0000;
const NXL_REGION_SIZE: u64 = 0x20_0000;
const NXL_SLOT_SIZE: u64 = 0x4_0000;
const NXL_SLOT_COUNT: usize = 8;
const NXL_MAX_SIZE: usize = 64 * 1024;

#[derive(Clone, Copy)]
struct NxlSlot {
    loaded: bool,
    base: u64,
    size: usize,
    name: [u8; 24],
}

/// ELF64 section header (64 bytes). Only the fields we need are read.
#[repr(C)]
struct Elf64Shdr {
    sh_name: u32,
    sh_type: u32,
    sh_flags: u64,
    sh_addr: u64,
    sh_offset: u64,
    sh_size: u64,
    sh_link: u32,
    sh_info: u32,
    sh_addralign: u64,
    sh_entsize: u64,
}

/// ELF64 symbol table entry (24 bytes).
#[repr(C)]
struct Elf64Sym {
    st_name: u32,
    st_info: u8,
    st_other: u8,
    st_shndx: u16,
    st_value: u64,
    st_size: u64,
}

lazy_static! {
    /// name → runtime address for symbols exported by loaded PIE NXLs (B2).
    static ref NXL_SYMBOLS: Mutex<Vec<(Vec<u8>, u64)>> = Mutex::new(Vec::new());
}

lazy_static! {
    static ref NXL_REGISTRY: Mutex<[NxlSlot; NXL_SLOT_COUNT]> = {
        const SLOT: NxlSlot = NxlSlot { loaded: false, base: 0, size: 0, name: [0u8; 24] };
        Mutex::new([
            NxlSlot { loaded: false, base: 0x1e00_0000, size: 0, name: [0u8; 24] },
            NxlSlot { loaded: false, base: 0x1e04_0000, size: 0, name: [0u8; 24] },
            NxlSlot { loaded: false, base: 0x1e08_0000, size: 0, name: [0u8; 24] },
            NxlSlot { loaded: false, base: 0x1e0c_0000, size: 0, name: [0u8; 24] },
            NxlSlot { loaded: false, base: 0x1e10_0000, size: 0, name: [0u8; 24] },
            NxlSlot { loaded: false, base: 0x1e14_0000, size: 0, name: [0u8; 24] },
            NxlSlot { loaded: false, base: 0x1e18_0000, size: 0, name: [0u8; 24] },
            NxlSlot { loaded: false, base: 0x1e1c_0000, size: 0, name: [0u8; 24] },
        ])
    };
}

pub fn init_nxl_region() -> bool {
    kinfo!(LogSubsys::Nxl, "Initializing shared library region 0x{:x}..0x{:x}",
        NXL_REGION_BASE, NXL_REGION_BASE + NXL_REGION_SIZE);

    if paging::split_2mb_page(NXL_REGION_BASE).is_err() {
        kerror!(LogSubsys::Nxl, "FAILED to split 2MB page");
        return false;
    }

    if paging::set_pd_user_accessible(NXL_REGION_BASE, true).is_err() {
        kerror!(LogSubsys::Nxl, "FAILED to set PD USER_ACCESSIBLE");
        return false;
    }

    kinfo!(LogSubsys::Nxl, "Region ready: {} x {} KB slots",
        NXL_SLOT_COUNT, NXL_SLOT_SIZE / 1024);
    true
}

pub fn load_nxl() -> bool {
    let ok = match nxl_load("C:\\System\\Libraries\\fs.nxl") {
        Some(base) => {
            kinfo!(LogSubsys::Nxl, "libneodos NXL loaded at 0x{:x}", base);
            dump_abi_table(base);
            true
        }
        None => {
            kwarn!(LogSubsys::Nxl, "libneodos.nxl not found");
            false
        }
    };

    // B1 spike: load the relocatable (PIE) math library at boot so the
    // R_X86_64_RELATIVE path is exercised and available to the NXL tests.
    match nxl_load("C:\\System\\Libraries\\math.nxl") {
        Some(base) => {
            MATH_EXPORT_BASE.store(base, Ordering::Relaxed);
            kinfo!(LogSubsys::Nxl, "PIE math.nxl export table at 0x{:x}", base);
        }
        None => kwarn!(LogSubsys::Nxl, "math.nxl not found"),
    }

    ok
}

/// Dump key entries from the NXL AbiTable at `base`.
/// The table mirrors libneodos/src/export.rs AbiTable layout (v7).
fn dump_abi_table(base: u64) {
    let tbl = base as *const u64;
    // Offsets map to AbiTable fields in order (each 8 bytes until version: u32)
    let names = [
        "sys_exit", "sys_write", "sys_read", "sys_getpid", "sys_yield",
        "sys_close", "sys_brk", "sys_mmap", "sys_munmap",
        "stdout_write", "stderr_write", "stdin_read", "dll_print", "dll_eprint",
        "file_open", "file_read", "file_write",
        "brk", "sbrk", "mmap", "munmap",
        "err_einval", "err_enonet", "err_enomem", "err_eacces", "err_ebadf",
        "err_efault", "err_enosys", "err_eagain", "err_epipe", "err_eenoent",
        "err_enotdir", "err_eisdir", "err_eio", "err_enodev", "err_ebusy",
        "sys_loadlib",
        "sys_ob_open", "sys_ob_create", "sys_ob_query_info",
        "sys_ob_set_info", "sys_ob_enum", "sys_ob_wait",
    ];
    // version is at offset 42*8
    let version = unsafe { core::ptr::read_volatile(tbl.add(42) as *const u32) };
    crate::serial_println!("[NXL] AbiTable at 0x{:x} version={}", base, version);
    for (i, name) in names.iter().enumerate() {
        let val = unsafe { core::ptr::read_volatile(tbl.add(i)) };
        let status = if val == 0 { "NULL" } else if val < 0x4000000 || val > 0x4400000 { "OUT_OF_RANGE" } else { "ok" };
        if status != "ok" {
            crate::serial_println!("[NXL]   {} [{:2}] = 0x{:016x} {}", name, i, val, status);
        }
    }
}

pub fn nxl_load(path: &str) -> Option<u64> {
    let mut buf_vec: Vec<u8> = alloc::vec![0u8; NXL_MAX_SIZE];
    let buf = buf_vec.as_mut_slice();

    let image_size = {
        let mut size = 0usize;
        let result = globals::with_vfs(|vfs| {
            let resolved = match vfs.resolve_path(path) {
                Ok(result) => Some(result),
                Err(e) => {
                    kerror!(LogSubsys::Nxl, "resolve '{}' failed: {:?}", path, e);
                    None
                }
            }.or_else(|| resolve_nxl_fallback(vfs, path));

            match resolved {
                Some((drive_idx, node)) => {
                    match vfs.read(drive_idx, node.inode, 0, buf) {
                        Ok(n) => { size = n; Ok(()) }
                        Err(e) => {
                            kerror!(LogSubsys::Nxl, "read error: {:?}", e);
                            Err(())
                        }
                    }
                }
                None => Err(()),
            }
        });
        if result.is_err() || size == 0 { return None; }
        size
    };

    let data = &buf[..image_size];

    // PIE (ET_DYN) NXLs are relocatable: place them in any free slot and let
    // the ELF loader apply R_X86_64_RELATIVE relocations at the chosen base.
    // Legacy NXLs are ET_EXEC linked at a fixed slot base and take the path
    // below (no relocation).
    if elf_is_pie(data) {
        return nxl_load_pie(data, image_size, path);
    }

    // Parse ELF to find the compiled vaddr base (first PT_LOAD vaddr aligned to slot boundary)
    let compiled_base = match elf_compiled_base(data) {
        Some(b) => b,
        None => {
            kerror!(LogSubsys::Nxl, "Cannot determine compiled base");
            return None;
        }
    };

    // Find a free slot under lock (atomic find + reserve, CB3)
    let (slot_idx, base) = {
        let mut registry = NXL_REGISTRY.lock();

        // Check if already loaded at this base, reuse
        for slot in registry.iter() {
            if slot.loaded && slot.base == compiled_base {
                kdebug!(LogSubsys::Nxl, "'{}' already loaded at 0x{:x}, reusing", path, slot.base);
                return Some(slot.base);
            }
        }

        // Find a free slot whose base matches the compiled base
        let idx = match registry.iter().position(|s| s.base == compiled_base && !s.loaded) {
            Some(i) => i,
            None => {
                kerror!(LogSubsys::Nxl, "No free slot for compiled base 0x{:x}", compiled_base);
                return None;
            }
        };

        let slot_base = registry[idx].base;
        // Mark as taken immediately — prevents TOCTOU race (CB3)
        registry[idx].loaded = true;

        (idx, slot_base)
    };

    kinfo!(LogSubsys::Nxl, "Loading '{}' @ slot {} => 0x{:x} (compiled 0x{:x})", path, slot_idx, base, compiled_base);

    let result = match crate::elf::load_elf(data, None, 0) {
        Ok(r) => r,
        Err(e) => {
            kerror!(LogSubsys::Nxl, "ELF load failed: {:?}", e);
            // Release the reserved slot on failure
            let mut registry = NXL_REGISTRY.lock();
            registry[slot_idx].loaded = false;
            return None;
        }
    };
    kdebug!(LogSubsys::Nxl, "ELF entry=0x{:x}", result.entry);

    // Mark each segment with appropriate page permissions based on ELF p_flags
    for seg in &result.segments {
        mark_segment_user_accessible(seg.vaddr, seg.memsz, seg.flags);
    }

    // Update slot metadata under lock
    {
        let mut registry = NXL_REGISTRY.lock();
        registry[slot_idx] = NxlSlot {
            loaded: true,
            base,
            size: image_size,
            name: {
                let mut n = [0u8; 24];
                let b = path.as_bytes();
                let l = core::cmp::min(b.len(), 23);
                n[..l].copy_from_slice(&b[..l]);
                n
            },
        };
    }

    kinfo!(LogSubsys::Nxl, "'{}' => 0x{:x} ({} bytes)", path, base, image_size);
    Some(base)
}

/// True when `data` is a position-independent ELF (`ET_DYN`).
fn elf_is_pie(data: &[u8]) -> bool {
    if data.len() < core::mem::size_of::<crate::elf::Elf64Hdr>() {
        return false;
    }
    let hdr: &crate::elf::Elf64Hdr =
        unsafe { &*(data.as_ptr() as *const crate::elf::Elf64Hdr) };
    hdr.e_ident[..4] == [0x7f, b'E', b'L', b'F'] && hdr.e_type == 3 // ET_DYN
}

/// Lowest PT_LOAD virtual address (the library's link base; 0 for `. = 0` PIE).
fn first_load_vaddr(data: &[u8]) -> Option<u64> {
    use core::mem::size_of;

    if data.len() < size_of::<crate::elf::Elf64Hdr>() {
        return None;
    }
    let hdr: &crate::elf::Elf64Hdr =
        unsafe { &*(data.as_ptr() as *const crate::elf::Elf64Hdr) };
    if hdr.e_ident[..4] != [0x7f, b'E', b'L', b'F'] {
        return None;
    }

    let phoff = hdr.e_phoff as usize;
    let phentsize = hdr.e_phentsize as usize;
    let phnum = hdr.e_phnum as usize;
    if phentsize != size_of::<crate::elf::Elf64Phdr>() {
        return None;
    }
    if phoff + phnum * phentsize > data.len() {
        return None;
    }

    let mut min_vaddr: Option<u64> = None;
    for i in 0..phnum {
        let off = phoff + i * phentsize;
        let phdr: &crate::elf::Elf64Phdr =
            unsafe { &*(data.as_ptr().add(off) as *const crate::elf::Elf64Phdr) };
        if phdr.p_type == 1 {
            min_vaddr = Some(match min_vaddr {
                Some(m) => m.min(phdr.p_vaddr),
                None => phdr.p_vaddr,
            });
        }
    }
    min_vaddr
}

/// 24-byte slot name from an NXL path.
fn nxl_slot_name(path: &str) -> [u8; 24] {
    let mut n = [0u8; 24];
    let b = path.as_bytes();
    let l = core::cmp::min(b.len(), 23);
    n[..l].copy_from_slice(&b[..l]);
    n
}

/// True when a slot's stored name matches `path` (prefix compare, NUL-padded).
fn slot_name_matches(slot_name: &[u8; 24], path: &str) -> bool {
    let want = nxl_slot_name(path);
    slot_name == &want
}

/// Locate a section by name: `(sh_addr, file offset, size, entry size)`.
fn find_section(data: &[u8], want: &[u8]) -> Option<(u64, usize, usize, usize)> {
    use core::mem::size_of;

    if data.len() < size_of::<crate::elf::Elf64Hdr>() {
        return None;
    }
    let hdr: &crate::elf::Elf64Hdr =
        unsafe { &*(data.as_ptr() as *const crate::elf::Elf64Hdr) };
    if hdr.e_ident[..4] != [0x7f, b'E', b'L', b'F'] {
        return None;
    }

    let shoff = hdr.e_shoff as usize;
    let shentsize = hdr.e_shentsize as usize;
    let shnum = hdr.e_shnum as usize;
    let shstrndx = hdr.e_shstrndx as usize;
    if shentsize < size_of::<Elf64Shdr>() || shnum == 0 || shstrndx >= shnum {
        return None;
    }
    if shoff.checked_add(shnum.checked_mul(shentsize)?)? > data.len() {
        return None;
    }

    let shdr_at = |i: usize| -> Option<&Elf64Shdr> {
        let off = shoff + i * shentsize;
        if off + size_of::<Elf64Shdr>() > data.len() {
            return None;
        }
        Some(unsafe { &*(data.as_ptr().add(off) as *const Elf64Shdr) })
    };

    let shstr = shdr_at(shstrndx)?;
    let st_off = shstr.sh_offset as usize;
    let st_end = st_off.checked_add(shstr.sh_size as usize)?;
    let strtab = data.get(st_off..st_end)?;

    for i in 0..shnum {
        if let Some(sh) = shdr_at(i) {
            let no = sh.sh_name as usize;
            if no < strtab.len() {
                let end = strtab[no..]
                    .iter()
                    .position(|&b| b == 0)
                    .map(|p| no + p)
                    .unwrap_or(strtab.len());
                if &strtab[no..end] == want {
                    return Some((
                        sh.sh_addr,
                        sh.sh_offset as usize,
                        sh.sh_size as usize,
                        sh.sh_entsize as usize,
                    ));
                }
            }
        }
    }
    None
}

/// NUL-terminated name at `off` in a string table.
fn strtab_name(strtab: &[u8], off: usize) -> Option<&[u8]> {
    if off >= strtab.len() {
        return None;
    }
    let end = strtab[off..]
        .iter()
        .position(|&b| b == 0)
        .map(|p| off + p)
        .unwrap_or(strtab.len());
    let name = &strtab[off..end];
    if name.is_empty() { None } else { Some(name) }
}

/// Find the virtual address (link-relative) of the `.export_table` section.
///
/// Legacy NXLs place the export table at offset 0; PIE NXLs let lld map the ELF
/// header into the first LOAD segment, so the table moves. Consumers still read
/// `returned_base + 0`, so the loader must report the table's real address.
fn nxl_export_table_offset(data: &[u8]) -> Option<u64> {
    find_section(data, b".export_table").map(|(addr, _, _, _)| addr)
}

/// Register every defined symbol exported by a PIE NXL under `load_offset`.
fn register_nxl_symbols(data: &[u8], load_offset: u64) {
    use core::mem::size_of;

    let (_, dynsym_off, dynsym_size, dynsym_ent) =
        match find_section(data, b".dynsym") { Some(s) => s, None => return };
    let (_, dynstr_off, dynstr_size, _) =
        match find_section(data, b".dynstr") { Some(s) => s, None => return };
    if dynsym_ent < size_of::<Elf64Sym>() {
        return;
    }
    let strtab = match data.get(dynstr_off..dynstr_off.saturating_add(dynstr_size)) {
        Some(s) => s,
        None => return,
    };
    let count = dynsym_size / dynsym_ent;

    let mut reg = NXL_SYMBOLS.lock();
    for i in 1..count {
        let off = dynsym_off + i * dynsym_ent;
        if off + size_of::<Elf64Sym>() > data.len() {
            break;
        }
        let sym: &Elf64Sym = unsafe { &*(data.as_ptr().add(off) as *const Elf64Sym) };
        if sym.st_shndx == 0 {
            continue; // SHN_UNDEF
        }
        let name = match strtab_name(strtab, sym.st_name as usize) {
            Some(n) => n,
            None => continue,
        };
        let addr = load_offset.wrapping_add(sym.st_value);
        if let Some(entry) = reg.iter_mut().find(|(n, _)| n.as_slice() == name) {
            entry.1 = addr;
        } else {
            reg.push((name.to_vec(), addr));
        }
    }
}

/// Look up an exported NXL symbol by name.
pub fn nxl_lookup_symbol(name: &str) -> Option<u64> {
    nxl_lookup_symbol_bytes(name.as_bytes())
}

fn nxl_lookup_symbol_bytes(name: &[u8]) -> Option<u64> {
    let reg = NXL_SYMBOLS.lock();
    reg.iter().find(|(n, _)| n.as_slice() == name).map(|(_, a)| *a)
}

/// Resolve `R_X86_64_64/GLOB_DAT/JUMP_SLOT` imports of a PIE NXL against the
/// registry of already-loaded NXEs, patching the target slots in place.
fn resolve_nxl_imports(data: &[u8], load_offset: u64) {
    use core::mem::size_of;

    let (_, dynstr_off, dynstr_size, _) =
        match find_section(data, b".dynstr") { Some(s) => s, None => return };
    let (_, dynsym_off, dynsym_size, dynsym_ent) =
        match find_section(data, b".dynsym") { Some(s) => s, None => return };
    if dynsym_ent < size_of::<Elf64Sym>() {
        return;
    }
    let strtab = match data.get(dynstr_off..dynstr_off.saturating_add(dynstr_size)) {
        Some(s) => s,
        None => return,
    };
    let sym_count = dynsym_size / dynsym_ent;

    for rela_name in [b".rela.dyn".as_slice(), b".rela.plt".as_slice()] {
        let (_, rela_off, rela_size, rela_ent) =
            match find_section(data, rela_name) { Some(s) => s, None => continue };
        if rela_ent < size_of::<crate::elf::Elf64Rela>() {
            continue;
        }
        let count = rela_size / rela_ent;
        for i in 0..count {
            let off = rela_off + i * rela_ent;
            if off + size_of::<crate::elf::Elf64Rela>() > data.len() {
                break;
            }
            let rela: &crate::elf::Elf64Rela =
                unsafe { &*(data.as_ptr().add(off) as *const crate::elf::Elf64Rela) };
            let r_type = (rela.r_info & 0xFFFF_FFFF) as u32;
            // R_X86_64_64 = 1, R_X86_64_GLOB_DAT = 6, R_X86_64_JUMP_SLOT = 7
            if !matches!(r_type, 1 | 6 | 7) {
                continue;
            }
            let sym_idx = (rela.r_info >> 32) as usize;
            if sym_idx >= sym_count {
                continue;
            }
            let sym_off = dynsym_off + sym_idx * dynsym_ent;
            if sym_off + size_of::<Elf64Sym>() > data.len() {
                continue;
            }
            let sym: &Elf64Sym = unsafe { &*(data.as_ptr().add(sym_off) as *const Elf64Sym) };
            let name = match strtab_name(strtab, sym.st_name as usize) {
                Some(n) => n,
                None => continue,
            };
            match nxl_lookup_symbol_bytes(name) {
                Some(target) => {
                    // S + A for R_X86_64_64; A is normally 0 for GLOB_DAT/JUMP_SLOT.
                    let value = target.wrapping_add(rela.r_addend as u64);
                    let slot = load_offset.wrapping_add(rela.r_offset);
                    unsafe { core::ptr::write_volatile(slot as *mut u64, value) };
                    kdebug!(LogSubsys::Nxl, "resolved NXL import {:?} -> 0x{:x}", name, value);
                }
                None => {
                    kwarn!(LogSubsys::Nxl, "unresolved NXL import {:?}", name);
                }
            }
        }
    }
}

/// Load a position-independent (PIE/ET_DYN) NXL into any free slot.
///
/// The library is linked at virtual 0 and `load_elf` is given a non-zero
/// `load_offset`, which makes it apply the `R_X86_64_RELATIVE` relocations from
/// `.rela.dyn`. The result is identical in shape to the legacy fixed-base path
/// (base returned is the export-table address).
fn nxl_load_pie(data: &[u8], image_size: usize, path: &str) -> Option<u64> {
    // Reuse an already-loaded PIE by file name (base is not fixed any more).
    {
        let registry = NXL_REGISTRY.lock();
        for slot in registry.iter() {
            if slot.loaded && slot_name_matches(&slot.name, path) {
                kdebug!(LogSubsys::Nxl, "PIE '{}' already loaded at 0x{:x}, reusing", path, slot.base);
                return Some(slot.base);
            }
        }
    }

    // Reserve any free slot: a PIE does not care which base it lands on.
    let (slot_idx, base) = {
        let mut registry = NXL_REGISTRY.lock();
        match registry.iter().position(|s| !s.loaded) {
            Some(i) => {
                // Mark taken immediately (same TOCTOU guard as the legacy path).
                registry[i].loaded = true;
                (i, registry[i].base)
            }
            None => {
                kerror!(LogSubsys::Nxl, "No free NXL slot for PIE '{}'", path);
                return None;
            }
        }
    };

    let min_vaddr = first_load_vaddr(data).unwrap_or(0);
    let load_offset = base.wrapping_sub(min_vaddr);

    kinfo!(LogSubsys::Nxl, "Loading PIE '{}' @ slot {} => 0x{:x} (link 0x{:x}, offset 0x{:x})",
        path, slot_idx, base, min_vaddr, load_offset);

    let result = match crate::elf::load_elf(data, None, load_offset) {
        Ok(r) => r,
        Err(e) => {
            kerror!(LogSubsys::Nxl, "PIE ELF load failed for '{}': {:?}", path, e);
            let mut registry = NXL_REGISTRY.lock();
            registry[slot_idx].loaded = false;
            return None;
        }
    };
    kdebug!(LogSubsys::Nxl, "PIE entry=0x{:x}", result.entry);

    // B2: publish exports and resolve imports *before* finalizing page
    // permissions. GOT/RELRO segments are writable at load time and must be
    // patched before `mark_segment_user_accessible` applies PF_W (otherwise a
    // read-only GOT page faults on the write).
    register_nxl_symbols(data, load_offset);
    resolve_nxl_imports(data, load_offset);

    for seg in &result.segments {
        mark_segment_user_accessible(seg.vaddr, seg.memsz, seg.flags);
    }

    // Consumers read `returned_base + 0` as the export table. Find the real
    // section address (PIE moves it past the mapped ELF header) and report
    // that absolute address, matching the legacy fixed-base contract.
    let export_off = nxl_export_table_offset(data).unwrap_or(0);
    let export_addr = load_offset.wrapping_add(export_off);
    if export_off != 0 {
        kdebug!(LogSubsys::Nxl, "PIE '{}' export table at load+0x{:x}", path, export_off);
    }

    {
        let mut registry = NXL_REGISTRY.lock();
        registry[slot_idx] = NxlSlot {
            loaded: true,
            base: export_addr,
            size: image_size,
            name: nxl_slot_name(path),
        };
    }

    kinfo!(LogSubsys::Nxl, "PIE '{}' => export table 0x{:x} (load 0x{:x}, {} bytes)",
        path, export_addr, base, image_size);
    Some(export_addr)
}

/// Peek at the ELF header to find the first PT_LOAD virtual address, aligned to slot size.
fn elf_compiled_base(data: &[u8]) -> Option<u64> {
    use core::mem::size_of;

    if data.len() < size_of::<crate::elf::Elf64Hdr>() {
        return None;
    }

    let hdr: &crate::elf::Elf64Hdr = unsafe { &*(data.as_ptr() as *const crate::elf::Elf64Hdr) };
    if hdr.e_ident[..4] != [0x7f, b'E', b'L', b'F'] {
        return None;
    }

    let phoff = hdr.e_phoff as usize;
    let phentsize = hdr.e_phentsize as usize;
    let phnum = hdr.e_phnum as usize;

    if phentsize != size_of::<crate::elf::Elf64Phdr>() {
        return None;
    }
    if phoff + phnum * phentsize > data.len() {
        return None;
    }

    for i in 0..phnum {
        let off = phoff + i * phentsize;
        let phdr: &crate::elf::Elf64Phdr = unsafe { &*(data.as_ptr().add(off) as *const crate::elf::Elf64Phdr) };
        if phdr.p_type == 1 {
            // Align base to slot boundary
            return Some(phdr.p_vaddr & !(NXL_SLOT_SIZE - 1));
        }
    }

    None
}

fn resolve_nxl_fallback(vfs: &mut crate::fs::vfs::Vfs, path: &str) -> Option<(usize, VfsNode)> {
    let file_name = path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(path);

    for drive_idx in 0..vfs.drives.len() {
        if vfs.drives[drive_idx].is_none() {
            continue;
        }

        if let Some(found) = search_directory(vfs, drive_idx, 0, file_name, 0) {
            return Some(found);
        }
    }

    None
}

fn search_directory(
    vfs: &mut crate::fs::vfs::Vfs,
    drive_idx: usize,
    inode: u32,
    file_name: &str,
    depth: usize,
) -> Option<(usize, VfsNode)> {
    if depth > 16 {
        return None;
    }

    let mut index = 0usize;
    loop {
        match vfs.readdir(drive_idx, inode, index) {
            Ok(Some(entry)) => {
                if entry.name.eq_ignore_ascii_case(file_name) && (entry.node.mode & MODE_FILE) != 0 {
                    return Some((drive_idx, entry.node));
                }

                if (entry.node.mode & MODE_DIR) != 0 {
                    if let Some(found) = search_directory(vfs, drive_idx, entry.node.inode, file_name, depth + 1) {
                        return Some(found);
                    }
                }

                index += 1;
            }
            Ok(None) => break,
            Err(_) => {
                index += 1;
            }
        }
    }

    None
}

/// Mark pages for an ELF segment with USER_ACCESSIBLE and WRITABLE (if PF_W).
/// ELF p_flags: PF_R=4, PF_W=2, PF_X=1
fn mark_segment_user_accessible(vaddr: u64, memsz: u64, p_flags: u32) {
    let start = vaddr & !(paging::PAGE_4K - 1);
    let end = (vaddr + memsz + paging::PAGE_4K - 1) & !(paging::PAGE_4K - 1);
    let writable = (p_flags & 2) != 0;

    let mut addr = start;
    while addr < end {
        if let Some(entry) = crate::hal::walk_ptes_4k(addr) {
            use x86_64::structures::paging::PageTableFlags;
            let phys = entry.addr();
            let mut flags = entry.flags();
            flags |= PageTableFlags::USER_ACCESSIBLE;
            if writable {
                flags |= PageTableFlags::WRITABLE;
            } else {
                flags.remove(PageTableFlags::WRITABLE);
            }
            entry.set_addr(phys, flags);
            crate::hal::flush_tlb(addr);
        }
        addr += paging::PAGE_4K;
    }

    kdebug!(LogSubsys::Nxl, "Marked 0x{:x}..0x{:x} USER_ACCESSIBLE{}",
        start, end, if writable { " + WRITABLE" } else { "" });
}

/// Build a minimal PIE ELF (in memory) with one undefined symbol referenced by
/// a `R_X86_64_GLOB_DAT` relocation at `r_offset = 0`. Used to exercise
/// `resolve_nxl_imports` deterministically without shipping a second NXL.
///
/// Sections: null, `.dynsym`, `.dynstr`, `.rela.dyn`, `.shstrtab`.
fn build_synthetic_import_elf(sym_name: &[u8]) -> alloc::vec::Vec<u8> {
    fn put16(b: &mut [u8], o: usize, v: u16) { b[o..o + 2].copy_from_slice(&v.to_le_bytes()); }
    fn put32(b: &mut [u8], o: usize, v: u32) { b[o..o + 4].copy_from_slice(&v.to_le_bytes()); }
    fn put64(b: &mut [u8], o: usize, v: u64) { b[o..o + 8].copy_from_slice(&v.to_le_bytes()); }

    const DYNSTR_OFF: usize = 64;
    let mut dynstr: alloc::vec::Vec<u8> = alloc::vec![0u8];
    dynstr.extend_from_slice(sym_name);
    dynstr.push(0);

    let dynsym_off = (DYNSTR_OFF + dynstr.len() + 7) & !7;
    const DYNSYM_SIZE: usize = 48; // 2 entries × 24
    let rela_off = dynsym_off + DYNSYM_SIZE;
    const RELA_SIZE: usize = 24;
    let shstr_off = rela_off + RELA_SIZE;

    let mut shstr: alloc::vec::Vec<u8> = alloc::vec![0u8];
    shstr.extend_from_slice(b".dynsym\0");   // name off 1
    shstr.extend_from_slice(b".dynstr\0");   // name off 9
    shstr.extend_from_slice(b".rela.dyn\0"); // name off 17
    shstr.extend_from_slice(b".shstrtab\0"); // name off 27

    let shoff = (shstr_off + shstr.len() + 7) & !7;
    const SHNUM: usize = 5;
    let mut b = alloc::vec![0u8; shoff + SHNUM * 64];

    // ELF header (ET_DYN, little-endian, 64-bit).
    b[0..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
    b[4] = 2; // ELFCLASS64
    b[5] = 1; // ELFDATA2LSB
    b[6] = 1; // EV_CURRENT
    put16(&mut b, 16, 3);  // e_type = ET_DYN
    put16(&mut b, 18, 62); // e_machine = EM_X86_64
    put32(&mut b, 20, 1);  // e_version
    put64(&mut b, 40, shoff as u64); // e_shoff
    put16(&mut b, 52, 64); // e_ehsize
    put16(&mut b, 54, 56); // e_phentsize
    put16(&mut b, 56, 0);  // e_phnum
    put16(&mut b, 58, 64); // e_shentsize
    put16(&mut b, 60, SHNUM as u16);
    put16(&mut b, 62, 4);  // e_shstrndx = .shstrtab

    // .dynstr
    b[DYNSTR_OFF..DYNSTR_OFF + dynstr.len()].copy_from_slice(&dynstr);

    // .dynsym[1]: undefined global "sym_name"
    let e1 = dynsym_off + 24;
    put32(&mut b, e1, 1);        // st_name -> dynstr offset 1
    b[e1 + 4] = 0x10;            // st_info = GLOBAL NOTYPE
    put16(&mut b, e1 + 6, 0);    // st_shndx = SHN_UNDEF

    // .rela.dyn[0]: GLOB_DAT on symbol 1, at r_offset 0
    put64(&mut b, rela_off, 0);                 // r_offset
    put64(&mut b, rela_off + 8, (1u64 << 32) | 6); // r_info = sym 1, type 6
    put64(&mut b, rela_off + 16, 0);            // r_addend

    // .shstrtab
    b[shstr_off..shstr_off + shstr.len()].copy_from_slice(&shstr);

    // Section headers.
    let mut wsh = |i: usize, name: u32, typ: u32, flags: u64, off: usize, size: u64,
                   link: u32, info: u32, align: u64, entsize: u64| {
        let o = shoff + i * 64;
        put32(&mut b, o, name);
        put32(&mut b, o + 4, typ);
        put64(&mut b, o + 8, flags);
        put64(&mut b, o + 24, off as u64);
        put64(&mut b, o + 32, size);
        put32(&mut b, o + 40, link);
        put32(&mut b, o + 44, info);
        put64(&mut b, o + 48, align);
        put64(&mut b, o + 56, entsize);
    };
    // index 0 = null (zeroed)
    wsh(1, 1, 11, 2, dynsym_off, DYNSYM_SIZE as u64, 2, 1, 8, 24); // .dynsym
    wsh(2, 9, 3, 2, DYNSTR_OFF, dynstr.len() as u64, 0, 0, 1, 0);  // .dynstr
    wsh(3, 17, 4, 2, rela_off, RELA_SIZE as u64, 1, 0, 8, 24);     // .rela.dyn
    wsh(4, 27, 3, 0, shstr_off, shstr.len() as u64, 0, 0, 1, 0);   // .shstrtab

    b
}

/// Register NXL loader tests with the kernel test framework.
pub fn register_nxl_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;

    // `math.nxl` is built as PIE and loaded at boot through the relocatable
    // path (R_X86_64_RELATIVE). Verify the relocated export table is callable.
    test_case!("nxl_pie_math_export_table", {
        let base = MATH_EXPORT_BASE.load(Ordering::Relaxed);
        test_true!(base != 0);

        // MathAbiTable: version: u32 @ 0, add: fn(i64,i64)->i64 @ 8.
        let add: extern "C" fn(i64, i64) -> i64 = unsafe {
            core::mem::transmute(*(base.wrapping_add(8) as *const u64))
        };
        test_eq!(add(2, 3), 5);
        test_eq!(add(-4, 1), -3);
    });

    // B2: named symbol resolution from the library's `.dynsym`.
    test_case!("nxl_symbol_lookup_math_add", {
        let addr = match nxl_lookup_symbol("math_add") {
            Some(a) => a,
            None => return Err("math_add not exported in .dynsym"),
        };
        let add: extern "C" fn(i64, i64) -> i64 = unsafe { core::mem::transmute(addr) };
        test_eq!(add(2, 3), 5);
        test_eq!(add(40, 2), 42);

        // The exported object symbol must match the export-table address.
        let exported = nxl_lookup_symbol("MATH_EXPORT_TABLE");
        test_eq!(exported, Some(MATH_EXPORT_BASE.load(Ordering::Relaxed)));
    });

    // B2: import resolution writes the registry address into the target slot.
    test_case!("nxl_resolve_import_glob_dat", {
        NXL_SYMBOLS.lock().push((b"fake_target".to_vec(), 0x1234_5678_9ABC_DEF0));
        let blob = build_synthetic_import_elf(b"fake_target");
        let mut slot: u64 = 0;
        let load_offset = &mut slot as *mut u64 as u64;
        resolve_nxl_imports(&blob, load_offset);
        test_eq!(slot, 0x1234_5678_9ABC_DEF0);
    });

    // B2: an unresolved import leaves the slot untouched (and only warns).
    test_case!("nxl_resolve_import_unresolved", {
        let blob = build_synthetic_import_elf(b"missing_symbol_xyz");
        let mut slot: u64 = 0xAAAA_AAAA_AAAA_AAAA;
        let load_offset = &mut slot as *mut u64 as u64;
        resolve_nxl_imports(&blob, load_offset);
        test_eq!(slot, 0xAAAA_AAAA_AAAA_AAAA);
    });

    // B2 end-to-end: load a real PIE NXL that imports `math_add` from
    // `math.nxl` and call it. The loader must resolve the cross-library
    // GLOB_DAT + the R_X86_64_64 export-table pointer from the registry.
    test_case!("nxl_cross_library_import_end_to_end", {
        static ARITH: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/testdata/arith_nxl_fixture.dat"
        ));
        let base = match nxl_load_pie(ARITH, ARITH.len(), "arith.test") {
            Some(b) => b,
            None => return Err("failed to load arith fixture"),
        };
        // ArithAbiTable: version: u32 @ 0, sum3: fn(i64,i64,i64)->i64 @ 8.
        let sum3: extern "C" fn(i64, i64, i64) -> i64 =
            unsafe { core::mem::transmute(*(base.wrapping_add(8) as *const u64)) };
        test_eq!(sum3(1, 2, 3), 6);
        test_eq!(sum3(10, -4, 1), 7);
    });
}
