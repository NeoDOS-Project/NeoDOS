// ELF64 loader — parses ELF headers and loads PT_LOAD segments into memory.
// Supports ET_EXEC and ET_DYN (PIE) for x86-64.
//
// #762 — Every `PT_LOAD` segment must fit in the caller's [`LoadRegion`]; the
// image is rejected before a single byte is copied. `addr_space` is optional
// extra validation (protected regions + user window) for process images, but it
// is no longer what makes the copy safe.
//
// A4.3 — Range validation: segments must stay within user window, null vaddr
// rejected, no overlap with protected regions or other segments.
//
// ASLR v0.44 — PIE support: `load_offset` shifts all segment addresses and
// applies R_X86_64_RELATIVE relocations from `.rela.dyn`.

use core::ptr::copy_nonoverlapping;
use core::mem::size_of;
use alloc::vec::Vec;
use crate::memory::address_space::{AddressSpace, SegmentInfo};
use crate::log::LogSubsys;

// ── ELF64 constants ──

const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const EM_X86_64: u16 = 62;
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;

// Dynamic section tags
const DT_NULL: i64 = 0;
const DT_RELA: i64 = 7;
const DT_RELASZ: i64 = 8;
const DT_RELAENT: i64 = 9;

// Relocation types
const R_X86_64_RELATIVE: u32 = 8;

// ── ELF64 header (64 bytes) ──

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct Elf64Hdr {
    pub e_ident: [u8; 16],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

// ── ELF64 program header (56 bytes) ──

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct Elf64Phdr {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

// ── ELF64 dynamic section entry (16 bytes) ──

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct Elf64Dyn {
    d_tag: i64,
    d_val: u64,
}

// ── ELF64 RELA entry (24 bytes) ──

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct Elf64Rela {
    pub r_offset: u64,
    pub r_info: u64,
    pub r_addend: i64,
}

// ── Load region ──

/// Address range a loaded image is allowed to occupy.
///
/// Every `PT_LOAD` segment (after `load_offset`) must lie entirely inside it,
/// and no two segments may overlap. The check runs on the parsed segment map
/// *before* any byte is copied, so an image that violates it never touches
/// memory — not even in the `addr_space = None` path used by NXL loading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadRegion {
    pub base: u64,
    /// Exclusive end of the region.
    pub limit: u64,
}

impl LoadRegion {
    /// Region `[base, base + size)`. A zero-sized or overflowing region accepts
    /// no segment (see [`LoadRegion::contains`]).
    pub const fn new(base: u64, size: u64) -> Self {
        LoadRegion { base, limit: base.saturating_add(size) }
    }

    pub const fn from_bounds(base: u64, limit: u64) -> Self {
        LoadRegion { base, limit }
    }

    /// True when `[vaddr, vaddr + memsz)` fits entirely in the region.
    pub fn contains(&self, vaddr: u64, memsz: u64) -> bool {
        if vaddr < self.base || vaddr >= self.limit || self.limit <= self.base {
            return false;
        }
        match vaddr.checked_add(memsz) {
            Some(end) => end <= self.limit,
            None => false,
        }
    }
}

// ── Error type ──

#[derive(Debug, PartialEq)]
pub enum ElfLoadError {
    InvalidHeader,
    InvalidMagic,
    InvalidClass,
    InvalidEndian,
    InvalidMachine,
    InvalidType,
    InvalidPhdrSize,
    PhdrTableOutOfBounds,
    SegmentDataOutOfBounds,
    /// A `PT_LOAD` segment is empty, overflows the address space or does not fit
    /// in the [`LoadRegion`] passed to the loader.
    SegmentOutOfRegion,
    /// Two `PT_LOAD` segments overlap.
    SegmentOverlap,
    /// The image declares no `PT_LOAD` segment at all.
    NoLoadSegments,
    AddressSpaceViolation(i64),
    EntryNotInSegment,
    RelocationError(&'static str),
}

// ── Result ──

#[derive(Debug, PartialEq)]
pub struct ElfLoadResult {
    pub entry: u64,
    pub segments: Vec<SegmentInfo>,
}

/// Convert a virtual address to a file offset using the program headers.
/// Returns `None` if no PT_LOAD segment covers the address.
fn vaddr_to_offset(phdrs: &[Elf64Phdr], vaddr: u64) -> Option<u64> {
    for ph in phdrs {
        if ph.p_type != PT_LOAD {
            continue;
        }
        if vaddr >= ph.p_vaddr && vaddr < ph.p_vaddr + ph.p_filesz {
            let offset = vaddr - ph.p_vaddr;
            return Some(ph.p_offset + offset);
        }
    }
    None
}

/// Parse the `.dynamic` section to find `.rela.dyn` offset and size.
/// Returns `(rela_offset, rela_size)` as file offsets/bytes, or `None` if no
/// PT_DYNAMIC or no DT_RELA/DT_RELASZ entries.
fn find_rela_dyn(data: &[u8], phdrs: &[Elf64Phdr]) -> Option<(usize, usize)> {
    // Find PT_DYNAMIC program header
    let dynamic_vaddr = {
        let mut addr = 0u64;
        let mut found = false;
        for ph in phdrs {
            if ph.p_type == PT_DYNAMIC {
                addr = ph.p_vaddr;
                found = true;
                break;
            }
        }
        if !found { return None; }
        addr
    };

    // Convert dynamic vaddr to file offset
    let dynamic_offset = vaddr_to_offset(phdrs, dynamic_vaddr)?;
    let dyn_entry_size = size_of::<Elf64Dyn>();
    let data_len = data.len();

    // Parse the .dynamic section for DT_RELA, DT_RELASZ
    let mut rela_addr = 0u64;
    let mut rela_size = 0u64;
    let mut found_rela = false;
    let mut found_relasz = false;

    let mut offset = dynamic_offset as usize;
    loop {
        if offset + dyn_entry_size > data_len {
            break;
        }
        // SAFETY: bounds checked above
        let entry: &Elf64Dyn = unsafe { &*(data.as_ptr().add(offset) as *const Elf64Dyn) };
        if entry.d_tag == DT_NULL {
            break;
        }
        if entry.d_tag == DT_RELA {
            rela_addr = entry.d_val;
            found_rela = true;
        }
        if entry.d_tag == DT_RELASZ {
            rela_size = entry.d_val;
            found_relasz = true;
        }
        if found_rela && found_relasz {
            break;
        }
        offset += dyn_entry_size;
    }

    if !found_rela || !found_relasz || rela_size == 0 {
        return None;
    }

    // Convert the RELA table vaddr to file offset
    let rela_offset = vaddr_to_offset(phdrs, rela_addr)?;
    Some((rela_offset as usize, rela_size as usize))
}

/// Apply R_X86_64_RELATIVE relocations to the loaded binary.
/// Every RELA entry tells us to write `load_base + r_addend` at address
/// `load_base + r_offset`.
///
/// #762: the target must fall inside one of the already-validated segments, so
/// a crafted `.rela.dyn` cannot write outside the image (this used to be a
/// second, unchecked write primitive on the same `addr_space = None` path).
fn apply_rela_relocations(
    data: &[u8],
    phdrs: &[Elf64Phdr],
    segments: &[SegmentInfo],
    load_base: u64,
) -> Result<(), ElfLoadError> {
    let (rela_offset, rela_size) = match find_rela_dyn(data, phdrs) {
        Some(v) => v,
        None => return Ok(()), // No relocations → nothing to do
    };

    let rela_entry_size = size_of::<Elf64Rela>();
    let count = rela_size / rela_entry_size;

    for i in 0..count {
        let entry_offset = match i.checked_mul(rela_entry_size).and_then(|o| rela_offset.checked_add(o)) {
            Some(o) => o,
            None => return Err(ElfLoadError::RelocationError("RELA offset overflow")),
        };
        if entry_offset.checked_add(rela_entry_size).map_or(true, |end| end > data.len()) {
            return Err(ElfLoadError::RelocationError("RELA entry out of bounds"));
        }

        // SAFETY: bounds checked above
        let rela: &Elf64Rela = unsafe { &*(data.as_ptr().add(entry_offset) as *const Elf64Rela) };

        let r_type = (rela.r_info & 0xFFFF_FFFF) as u32;

        match r_type {
            R_X86_64_RELATIVE => {
                // *(load_base + r_offset) = load_base + r_addend
                let target_addr = load_base.wrapping_add(rela.r_offset);
                let value = load_base.wrapping_add(rela.r_addend as u64);
                let width = size_of::<u64>() as u64;

                let in_segment = segments.iter().any(|seg| {
                    target_addr >= seg.vaddr
                        && match target_addr.checked_add(width) {
                            Some(end) => end <= seg.vaddr.saturating_add(seg.memsz),
                            None => false,
                        }
                });
                if !in_segment {
                    return Err(ElfLoadError::RelocationError(
                        "relocation target outside the image segments",
                    ));
                }

                // SAFETY: validated segment map; target lies in the image.
                unsafe {
                    let ptr = target_addr as *mut u64;
                    core::ptr::write(ptr, value);
                }
            }
            other => {
                // For v1, warn about unknown relocations but don't fail.
                // R_X86_64_64 and R_X86_64_GLOB_DAT may occur in PIE binaries
                // with external references.
                kwarn!(LogSubsys::Elf,
                    "unhandled relocation type {} at offset 0x{:x}",
                    other, rela.r_offset
                );
            }
        }
    }

    Ok(())
}

/// Parse and load an ELF64 binary.
///
/// `region` is mandatory and bounds every `PT_LOAD` segment: a segment that is
/// empty (`vaddr == 0`), overflows, overlaps another segment or leaves the
/// region makes the whole image fail with [`ElfLoadError::SegmentOutOfRegion`]
/// / [`ElfLoadError::SegmentOverlap`] **before any byte is copied**. Callers must
/// pass the slot the image is going to live in (process slot, NXL slot, or the
/// test scratch buffer).
///
/// `addr_space` adds the A4.3 checks (protected regions and the 4–36 MB user
/// window) on top of the region check, and turns the entry-point check into a
/// hard error. It is `None` for images outside the user window (NXL slots).
///
/// `load_offset` is added to all segment virtual addresses and the entry point.
/// For PIE binaries (ET_DYN), `load_offset` should be non-zero.
/// For non-PIE (ET_EXEC), use `load_offset = 0`.
pub fn load_elf(
    data: &[u8],
    region: LoadRegion,
    mut addr_space: Option<&mut AddressSpace>,
    load_offset: u64,
) -> Result<ElfLoadResult, ElfLoadError> {
    if data.len() < size_of::<Elf64Hdr>() {
        return Err(ElfLoadError::InvalidHeader);
    }

    // SAFETY: we've checked data.len() >= 64 (size_of::<Elf64Hdr>()).
    let hdr: &Elf64Hdr = unsafe { &*(data.as_ptr() as *const Elf64Hdr) };

    // ── Validate ELF identity ──
    if hdr.e_ident[..4] != ELF_MAGIC {
        return Err(ElfLoadError::InvalidMagic);
    }
    if hdr.e_ident[4] != ELFCLASS64 {
        return Err(ElfLoadError::InvalidClass);
    }
    if hdr.e_ident[5] != ELFDATA2LSB {
        return Err(ElfLoadError::InvalidEndian);
    }
    if hdr.e_machine != EM_X86_64 {
        return Err(ElfLoadError::InvalidMachine);
    }
    if hdr.e_type != ET_EXEC && hdr.e_type != ET_DYN {
        return Err(ElfLoadError::InvalidType);
    }

    // ── Validate program header table ──
    let phoff = hdr.e_phoff as usize;
    let phentsize = hdr.e_phentsize as usize;
    let phnum = hdr.e_phnum as usize;

    if phentsize != size_of::<Elf64Phdr>() {
        return Err(ElfLoadError::InvalidPhdrSize);
    }
    let ph_table_end = phoff.checked_add(phnum * phentsize)
        .ok_or(ElfLoadError::PhdrTableOutOfBounds)?;
    if ph_table_end > data.len() {
        return Err(ElfLoadError::PhdrTableOutOfBounds);
    }

    // SAFETY: bounds checked above.
    let phdrs: &[Elf64Phdr] = unsafe {
        core::slice::from_raw_parts(
            data.as_ptr().add(phoff) as *const Elf64Phdr,
            phnum,
        )
    };

    // ── Collect PT_LOAD segments, validating the whole map before any copy ──
    let mut segments = Vec::new();
    let entry = hdr.e_entry
        .checked_add(load_offset)
        .ok_or(ElfLoadError::SegmentOutOfRegion)?;
    let mut entry_in_segment = false;

    for ph in phdrs {
        if ph.p_type != PT_LOAD {
            continue;
        }

        let vaddr = ph
            .p_vaddr
            .checked_add(load_offset)
            .ok_or(ElfLoadError::SegmentOutOfRegion)?;
        let memsz = ph.p_memsz;
        let flags = ph.p_flags;

        // #762: the region check is what makes the copy safe. Without it the
        // loader used to write to any address the image asked for whenever the
        // caller passed `addr_space = None` (production NXL and process paths).
        if vaddr == 0 || !region.contains(vaddr, memsz) {
            return Err(ElfLoadError::SegmentOutOfRegion);
        }

        // Source bounds and `filesz <= memsz` are checked here too, so a bad
        // segment is rejected before the load loop starts touching memory.
        let src_end = ph
            .p_offset
            .checked_add(ph.p_filesz)
            .ok_or(ElfLoadError::SegmentDataOutOfBounds)?;
        if src_end > data.len() as u64 || ph.p_filesz > memsz {
            return Err(ElfLoadError::SegmentDataOutOfBounds);
        }

        // No overlap with previously collected segments (independent of
        // `addr_space`, which is None for NXL slots).
        let new_seg = SegmentInfo { vaddr, memsz, flags };
        if segments.iter().any(|existing| AddressSpace::segments_overlap(&new_seg, existing)) {
            return Err(ElfLoadError::SegmentOverlap);
        }

        // Check entry point containment (check 5 — log warning, don't fail)
        if entry >= vaddr && entry < vaddr.saturating_add(memsz) {
            entry_in_segment = true;
        }

        // Validate segment range and register with address space (if provided)
        if let Some(ref mut space) = addr_space {
            space.add_segment(vaddr, memsz, flags)
                .map_err(ElfLoadError::AddressSpaceViolation)?;
        }

        segments.push(new_seg);
    }

    if segments.is_empty() {
        return Err(ElfLoadError::NoLoadSegments);
    }

    // Check 5: entry point must be in a PT_LOAD segment
    if !entry_in_segment {
        if addr_space.is_some() {
            return Err(ElfLoadError::EntryNotInSegment);
        }
        kwarn!(LogSubsys::Elf, "entry 0x{:x} not contained in any PT_LOAD segment", entry);
    }

    // ── Load each PT_LOAD segment ──
    for ph in phdrs {
        if ph.p_type != PT_LOAD {
            continue;
        }

        let vaddr = ph.p_vaddr + load_offset;
        let offset = ph.p_offset as usize;
        let filesz = ph.p_filesz as usize;
        let memsz = ph.p_memsz as usize;

        // Validate source bounds
        let src_end = offset.checked_add(filesz)
            .ok_or(ElfLoadError::SegmentDataOutOfBounds)?;
        if src_end > data.len() {
            return Err(ElfLoadError::SegmentDataOutOfBounds);
        }

        // Copy file data to virtual address
        if filesz > 0 {
            // SAFETY: offsets validated above; caller guarantees vaddr is mapped+writable.
            unsafe {
                let src = data.as_ptr().add(offset);
                let dst = vaddr as *mut u8;
                copy_nonoverlapping(src, dst, filesz);
            }
        }

        // Zero-fill .bss (memsz > filesz)
        if memsz > filesz {
            // SAFETY: caller guarantees vaddr..vaddr+memsz is mapped+writable.
            unsafe {
                let zero_start = (vaddr + filesz as u64) as *mut u8;
                core::ptr::write_bytes(zero_start, 0u8, memsz - filesz);
            }
        }
    }

    // ── Apply RELA relocations (for PIE/DYN binaries) ──
    if load_offset != 0 {
        apply_rela_relocations(data, phdrs, &segments, load_offset)?;
    }

    Ok(ElfLoadResult { entry, segments })
}

// ── Tests ──

/// Build a minimal valid ELF64 binary with one PT_LOAD segment.
///
/// `pub(crate)` so cross-module regression tests (process creation) can feed the
/// loader a crafted image; the loader never executes it.
pub(crate) fn build_valid_elf(entry: u64, vaddr: u64, code: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();

    // ELF header (64 bytes)
    buf.extend_from_slice(b"\x7fELF");
    buf.push(2);  // class = ELFCLASS64
    buf.push(1);  // data = ELFDATA2LSB
    buf.push(1);  // version
    buf.push(0);  // osabi
    buf.push(0);  // abiversion
    buf.extend_from_slice(&[0u8; 7]);  // padding
    buf.extend_from_slice(&ET_EXEC.to_le_bytes());  // e_type
    buf.extend_from_slice(&EM_X86_64.to_le_bytes());  // e_machine
    buf.extend_from_slice(&1u32.to_le_bytes());  // e_version
    buf.extend_from_slice(&entry.to_le_bytes());  // e_entry
    buf.extend_from_slice(&64u64.to_le_bytes());  // e_phoff
    buf.extend_from_slice(&0u64.to_le_bytes());  // e_shoff
    buf.extend_from_slice(&0u32.to_le_bytes());  // e_flags
    buf.extend_from_slice(&64u16.to_le_bytes());  // e_ehsize
    buf.extend_from_slice(&56u16.to_le_bytes());  // e_phentsize
    buf.extend_from_slice(&1u16.to_le_bytes());  // e_phnum
    buf.extend_from_slice(&0u16.to_le_bytes());  // e_shentsize
    buf.extend_from_slice(&0u16.to_le_bytes());  // e_shnum
    buf.extend_from_slice(&0u16.to_le_bytes());  // e_shstrndx

    // Program header (56 bytes): PT_LOAD
    let code_offset = 64 + 56;  // right after headers
    let filesz = code.len();
    buf.extend_from_slice(&PT_LOAD.to_le_bytes());  // p_type
    buf.extend_from_slice(&7u32.to_le_bytes());  // p_flags (R+W+X)
    buf.extend_from_slice(&(code_offset as u64).to_le_bytes());  // p_offset
    buf.extend_from_slice(&vaddr.to_le_bytes());  // p_vaddr
    buf.extend_from_slice(&vaddr.to_le_bytes());  // p_paddr
    buf.extend_from_slice(&(filesz as u64).to_le_bytes());  // p_filesz
    buf.extend_from_slice(&(filesz as u64).to_le_bytes());  // p_memsz
    buf.extend_from_slice(&1u64.to_le_bytes());  // p_align

    // Code
    buf.extend_from_slice(code);

    buf
}

/// Build a minimal valid ELF64 binary with two PT_LOAD segments.
fn build_elf_two_segments(
    entry: u64,
    vaddr1: u64, code1: &[u8],
    vaddr2: u64, code2: &[u8],
) -> Vec<u8> {
    let mut buf = Vec::new();

    // ELF header (64 bytes)
    buf.extend_from_slice(b"\x7fELF");
    buf.push(2);  // ELFCLASS64
    buf.push(1);  // ELFDATA2LSB
    buf.push(1);  // version
    buf.push(0);  // osabi
    buf.push(0);  // abiversion
    buf.extend_from_slice(&[0u8; 7]);  // padding
    buf.extend_from_slice(&ET_EXEC.to_le_bytes());
    buf.extend_from_slice(&EM_X86_64.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&entry.to_le_bytes());
    buf.extend_from_slice(&64u64.to_le_bytes());  // e_phoff
    buf.extend_from_slice(&0u64.to_le_bytes());  // e_shoff
    buf.extend_from_slice(&0u32.to_le_bytes());  // e_flags
    buf.extend_from_slice(&64u16.to_le_bytes());  // e_ehsize
    buf.extend_from_slice(&56u16.to_le_bytes());  // e_phentsize
    buf.extend_from_slice(&2u16.to_le_bytes());   // e_phnum = 2
    buf.extend_from_slice(&0u16.to_le_bytes());  // e_shentsize
    buf.extend_from_slice(&0u16.to_le_bytes());  // e_shnum
    buf.extend_from_slice(&0u16.to_le_bytes());  // e_shstrndx

    // Program headers start at offset 64
    let phdr_end = 64 + 2 * 56;
    let code1_offset = phdr_end as u64;
    let code2_offset = code1_offset + code1.len() as u64;

    // PHDR 1: PT_LOAD
    buf.extend_from_slice(&PT_LOAD.to_le_bytes());
    buf.extend_from_slice(&7u32.to_le_bytes());
    buf.extend_from_slice(&code1_offset.to_le_bytes());
    buf.extend_from_slice(&vaddr1.to_le_bytes());
    buf.extend_from_slice(&vaddr1.to_le_bytes());
    buf.extend_from_slice(&(code1.len() as u64).to_le_bytes());
    buf.extend_from_slice(&(code1.len() as u64).to_le_bytes());
    buf.extend_from_slice(&1u64.to_le_bytes());

    // PHDR 2: PT_LOAD
    buf.extend_from_slice(&PT_LOAD.to_le_bytes());
    buf.extend_from_slice(&7u32.to_le_bytes());
    buf.extend_from_slice(&code2_offset.to_le_bytes());
    buf.extend_from_slice(&vaddr2.to_le_bytes());
    buf.extend_from_slice(&vaddr2.to_le_bytes());
    buf.extend_from_slice(&(code2.len() as u64).to_le_bytes());
    buf.extend_from_slice(&(code2.len() as u64).to_le_bytes());
    buf.extend_from_slice(&1u64.to_le_bytes());

    // Code sections
    buf.extend_from_slice(code1);
    buf.extend_from_slice(code2);

    buf
}

/// Region for images built at a fixed synthetic address, wide enough for the
/// small test segments (`from_bounds` keeps a zero-vaddr region invalid).
fn region_around(addr: u64) -> LoadRegion {
    LoadRegion::new(addr, 0x1000)
}

/// Region covering a writable scratch buffer, so the loader's copy lands in
/// real memory instead of an arbitrary virtual address.
fn region_of(buf: &[u8]) -> LoadRegion {
    let base = buf.as_ptr() as u64;
    LoadRegion::from_bounds(base, base + buf.len() as u64)
}

/// Minimal ET_DYN image with `.dynamic` + one `R_X86_64_RELATIVE` entry, linked
/// at virtual 0 so the loader must apply `load_offset` and the relocation.
/// `r_offset` is relative to the load base (the relocation target).
fn build_elf_with_rela(r_offset: u64, addend: i64) -> Vec<u8> {
    const HDR: usize = 64;
    const PH: usize = 56;
    const CODE_OFF: usize = HDR + 3 * PH;      // 232
    const DYN_OFF: usize = CODE_OFF + 16;      // 248
    const RELA_OFF: usize = DYN_OFF + 48;      // 296
    const DYN_VADDR: u64 = 0x100;

    let mut buf = Vec::new();

    // ELF header (64 bytes), ET_DYN, entry 0 (inside the code segment once the
    // load offset is applied).
    buf.extend_from_slice(b"\x7fELF");
    buf.push(2);  // class = ELFCLASS64
    buf.push(1);  // data = ELFDATA2LSB
    buf.push(1);  // version
    buf.push(0);  // osabi
    buf.push(0);  // abiversion
    buf.extend_from_slice(&[0u8; 7]);  // padding
    buf.extend_from_slice(&ET_DYN.to_le_bytes());  // e_type
    buf.extend_from_slice(&EM_X86_64.to_le_bytes());  // e_machine
    buf.extend_from_slice(&1u32.to_le_bytes());  // e_version
    buf.extend_from_slice(&0u64.to_le_bytes());  // e_entry
    buf.extend_from_slice(&(HDR as u64).to_le_bytes());  // e_phoff
    buf.extend_from_slice(&0u64.to_le_bytes());  // e_shoff
    buf.extend_from_slice(&0u32.to_le_bytes());  // e_flags
    buf.extend_from_slice(&64u16.to_le_bytes());  // e_ehsize
    buf.extend_from_slice(&(PH as u16).to_le_bytes());  // e_phentsize
    buf.extend_from_slice(&3u16.to_le_bytes());  // e_phnum
    buf.extend_from_slice(&0u16.to_le_bytes());  // e_shentsize
    buf.extend_from_slice(&0u16.to_le_bytes());  // e_shnum
    buf.extend_from_slice(&0u16.to_le_bytes());  // e_shstrndx

    // PT_LOAD #1: code at link vaddr 0.
    buf.extend_from_slice(&PT_LOAD.to_le_bytes());
    buf.extend_from_slice(&7u32.to_le_bytes());  // R+W+X
    buf.extend_from_slice(&(CODE_OFF as u64).to_le_bytes());
    buf.extend_from_slice(&0u64.to_le_bytes());  // p_vaddr
    buf.extend_from_slice(&0u64.to_le_bytes());  // p_paddr
    buf.extend_from_slice(&16u64.to_le_bytes());  // p_filesz
    buf.extend_from_slice(&16u64.to_le_bytes());  // p_memsz
    buf.extend_from_slice(&1u64.to_le_bytes());

    // PT_LOAD #2: .dynamic + .rela.dyn at link vaddr 0x100.
    buf.extend_from_slice(&PT_LOAD.to_le_bytes());
    buf.extend_from_slice(&6u32.to_le_bytes());  // R+W
    buf.extend_from_slice(&(DYN_OFF as u64).to_le_bytes());
    buf.extend_from_slice(&DYN_VADDR.to_le_bytes());
    buf.extend_from_slice(&DYN_VADDR.to_le_bytes());
    buf.extend_from_slice(&72u64.to_le_bytes());  // 3 dyn entries + 1 rela
    buf.extend_from_slice(&72u64.to_le_bytes());
    buf.extend_from_slice(&1u64.to_le_bytes());

    // PT_DYNAMIC pointing at the dynamic entries.
    buf.extend_from_slice(&PT_DYNAMIC.to_le_bytes());
    buf.extend_from_slice(&6u32.to_le_bytes());
    buf.extend_from_slice(&(DYN_OFF as u64).to_le_bytes());
    buf.extend_from_slice(&DYN_VADDR.to_le_bytes());
    buf.extend_from_slice(&DYN_VADDR.to_le_bytes());
    buf.extend_from_slice(&48u64.to_le_bytes());
    buf.extend_from_slice(&48u64.to_le_bytes());
    buf.extend_from_slice(&1u64.to_le_bytes());

    // Code.
    buf.extend_from_slice(&[0x90u8; 16]);

    // .dynamic: DT_RELA, DT_RELASZ, DT_NULL.
    buf.extend_from_slice(&DT_RELA.to_le_bytes());
    buf.extend_from_slice(&(DYN_VADDR + 48).to_le_bytes());  // rela vaddr
    buf.extend_from_slice(&DT_RELASZ.to_le_bytes());
    buf.extend_from_slice(&24u64.to_le_bytes());
    buf.extend_from_slice(&DT_NULL.to_le_bytes());
    buf.extend_from_slice(&0u64.to_le_bytes());

    // .rela.dyn: one R_X86_64_RELATIVE.
    buf.extend_from_slice(&r_offset.to_le_bytes());
    buf.extend_from_slice(&(R_X86_64_RELATIVE as u64).to_le_bytes());  // r_info
    buf.extend_from_slice(&addend.to_le_bytes());

    debug_assert_eq!(buf.len(), RELA_OFF + 24);
    buf
}

/// Register ELF loader tests with the kernel test framework.
pub fn register_elf_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;

    // Region that must not be the reason for rejection in the A4.3 tests: they
    // check the address-space collisions, so the region bound is left unbounded.
    const ANY_REGION: LoadRegion = LoadRegion::from_bounds(1, u64::MAX);

    // ── Original parser tests (updated for Result return) ──

    test_case!("elf_parse_valid_header", {
        let code = [0x90u8; 16];
        let raw = build_valid_elf(0x400000, 0x400000, &code);
        let result = load_elf(&raw, region_around(0x400000), None, 0);
        test_true!(result.is_ok());
        test_eq!(result.unwrap().entry, 0x400000);
    });

    test_case!("elf_parse_invalid_magic", {
        let code = [0x90u8; 16];
        let mut raw = build_valid_elf(0x400000, 0x400000, &code);
        raw[0..4].copy_from_slice(b"BAD\x00");
        test_eq!(load_elf(&raw, region_around(0x400000), None, 0), Err(ElfLoadError::InvalidMagic));
    });

    test_case!("elf_parse_invalid_class", {
        let code = [0x90u8; 16];
        let mut raw = build_valid_elf(0x400000, 0x400000, &code);
        raw[4] = 1;
        test_eq!(load_elf(&raw, region_around(0x400000), None, 0), Err(ElfLoadError::InvalidClass));
    });

    test_case!("elf_parse_invalid_machine", {
        let code = [0x90u8; 16];
        let mut raw = build_valid_elf(0x400000, 0x400000, &code);
        raw[18..20].copy_from_slice(&3u16.to_le_bytes());
        test_eq!(load_elf(&raw, region_around(0x400000), None, 0), Err(ElfLoadError::InvalidMachine));
    });

    test_case!("elf_parse_truncated_header", {
        let data = [0u8; 4];
        test_eq!(load_elf(&data, region_around(0x400000), None, 0), Err(ElfLoadError::InvalidHeader));
    });

    test_case!("elf_parse_load_segment", {
        let code = [0xb8, 0x00, 0x00, 0x00, 0x00,
                    0xcd, 0x80];
        let mut test_buf = [0u8; 32];
        let load_addr = test_buf.as_mut_ptr() as u64;
        let raw = build_valid_elf(load_addr, load_addr, &code);
        let result = load_elf(&raw, region_of(&test_buf), None, 0);
        test_true!(result.is_ok());
        let r = result.unwrap();
        test_eq!(r.entry, load_addr);
        test_eq!(test_buf[0], 0xb8);
        test_eq!(test_buf[5], 0xcd);
    });

    test_case!("elf_parse_bad_phentsize", {
        let code = [0x90u8; 8];
        let mut raw = build_valid_elf(0x400000, 0x400000, &code);
        raw[54..56].copy_from_slice(&99u16.to_le_bytes());
        test_eq!(load_elf(&raw, region_around(0x400000), None, 0), Err(ElfLoadError::InvalidPhdrSize));
    });

    // ── A4.3 Range validation tests ──

    test_case!("elf_validation_valid_range", {
        let code = [0x90u8; 32];
        let raw = build_valid_elf(0x400000, 0x400000, &code);
        let mut addr_space = AddressSpace::new();
        let result = load_elf(&raw, ANY_REGION, Some(&mut addr_space), 0);
        test_true!(result.is_ok());
        let r = result.unwrap();
        test_eq!(r.segments.len(), 1);
        test_eq!(r.segments[0].vaddr, 0x400000);
        test_eq!(r.segments[0].memsz, 32);
    });

    test_case!("elf_reject_zero_vaddr", {
        let code = [0x90u8; 16];
        let raw = build_valid_elf(0, 0, &code);
        let mut addr_space = AddressSpace::new();
        // #762: the region check now owns null vaddr (`vaddr == 0` never fits a
        // real load region, so it is rejected before the copy loop).
        let result = load_elf(&raw, ANY_REGION, Some(&mut addr_space), 0);
        test_eq!(result, Err(ElfLoadError::SegmentOutOfRegion));
    });

    test_case!("elf_reject_kernel_collision", {
        // Segment at 0x4000000 (kernel image base, v0.40) should be rejected
        let code = [0x90u8; 16];
        let raw = build_valid_elf(0x4000000, 0x4000000, &code);
        let mut addr_space = AddressSpace::new();
        let result = load_elf(&raw, ANY_REGION, Some(&mut addr_space), 0);
        test_true!(result.is_err());
        match result {
            Err(ElfLoadError::AddressSpaceViolation(e)) => {
                test_eq!(e, crate::memory::address_space::ELF_ERR_KERNEL_COLLISION);
            }
            _ => test_true!(false),
        }
    });

    test_case!("elf_reject_heap_collision", {
        // Segment at 0x2400000 (kernel heap base, v0.40) should be rejected
        let code = [0x90u8; 16];
        let raw = build_valid_elf(0x2400000, 0x2400000, &code);
        let mut addr_space = AddressSpace::new();
        let result = load_elf(&raw, ANY_REGION, Some(&mut addr_space), 0);
        test_true!(result.is_err());
        match result {
            Err(ElfLoadError::AddressSpaceViolation(e)) => {
                test_eq!(e, crate::memory::address_space::ELF_ERR_KERNEL_COLLISION);
            }
            _ => test_true!(false),
        }
    });

    test_case!("elf_reject_mmap_collision", {
        // Segment at 0x20000000 (mmap region base) should be rejected
        let code = [0x90u8; 16];
        let raw = build_valid_elf(0x20000000, 0x20000000, &code);
        let mut addr_space = AddressSpace::new();
        let result = load_elf(&raw, ANY_REGION, Some(&mut addr_space), 0);
        test_true!(result.is_err());
        match result {
            Err(ElfLoadError::AddressSpaceViolation(e)) => {
                test_eq!(e, crate::memory::address_space::ELF_ERR_MMAP_COLLISION);
            }
            _ => test_true!(false),
        }
    });

    test_case!("elf_malicious_no_triple_fault", {
        // Try loading at multiple invalid addresses — should all fail gracefully
        let code = [0x90u8; 16];
        let mut addr_space = AddressSpace::new();

        let targets: &[u64] = &[
            0x0,            // null
            0x4000000,      // kernel image (v0.40)
            0x2400000,      // kernel heap (v0.40)
            0x10000000,     // user heap
            0x20000000,     // mmap region
            0x30000000,     // driver isolation
        ];

        for &vaddr in targets {
            let raw = build_valid_elf(vaddr, vaddr, &code);
            let result = load_elf(&raw, ANY_REGION, Some(&mut addr_space), 0);
            test_true!(result.is_err());
        }
    });

    test_case!("elf_overlap_segments", {
        let code = [0x90u8; 16];
        // Two overlapping segments at same vaddr. #762: the loader detects the
        // overlap itself (it no longer depends on `addr_space` being provided).
        let raw = build_elf_two_segments(0x400000, 0x400000, &code, 0x400008, &code);
        let mut addr_space = AddressSpace::new();
        let result = load_elf(&raw, ANY_REGION, Some(&mut addr_space), 0);
        test_eq!(result, Err(ElfLoadError::SegmentOverlap));
    });

    test_case!("elf_user_heap_collision", {
        // Segment in user heap range (0x10000000) should be rejected
        let code = [0x90u8; 16];
        let raw = build_valid_elf(0x10000000, 0x10000000, &code);
        let mut addr_space = AddressSpace::new();
        let result = load_elf(&raw, ANY_REGION, Some(&mut addr_space), 0);
        test_true!(result.is_err());
        match result {
            Err(ElfLoadError::AddressSpaceViolation(e)) => {
                test_eq!(e, crate::memory::address_space::ELF_ERR_HEAP_COLLISION);
            }
            _ => test_true!(false),
        }
    });

    // ── ASLR v0.44: PIE + RELA tests ──

    test_case!("elf_pie_load_with_offset", {
        // Test loading a PIE-style ELF (segments at vaddr=0) with a load_offset
        let code = [0x90u8; 32];
        let raw = build_valid_elf(0, 0, &code);
        let offset = 0x460000;
        // Without load_offset the image maps at 0: rejected (#762), the loader
        // no longer copies to address zero.
        test_eq!(
            load_elf(&raw, region_around(offset), None, 0),
            Err(ElfLoadError::SegmentOutOfRegion)
        );
        // With load_offset, vaddr and entry are shifted
        let result_off = load_elf(&raw, region_around(offset), None, offset);
        test_true!(result_off.is_ok());
        let r = result_off.unwrap();
        test_eq!(r.entry, offset);
        test_eq!(r.segments.len(), 1);
        test_eq!(r.segments[0].vaddr, offset);
    });

    test_case!("elf_pie_accept_zero_vaddr_with_offset", {
        // With load_offset != 0, zero vaddr should be accepted for AddressSpace
        // because the actual loaded address is vaddr + load_offset
        let code = [0x90u8; 32];
        let raw = build_valid_elf(0, 0, &code);
        let offset = 0x420000; // slot 1
        let mut addr_space = AddressSpace::new();
        let result = load_elf(&raw, ANY_REGION, Some(&mut addr_space), offset);
        test_true!(result.is_ok());
        let r = result.unwrap();
        test_eq!(r.entry, offset);
        test_eq!(r.segments[0].vaddr, offset);
    });

    test_case!("elf_pie_offset_out_of_user_window", {
        // load_offset that pushes segments outside user window should be rejected.
        // 0x30000000 is the driver isolation region → KERNEL_COLLISION
        let code = [0x90u8; 32];
        let offset = 0x30000000; // driver isolation region
        let raw = build_valid_elf(0, 0, &code);
        let mut addr_space = AddressSpace::new();
        let result = load_elf(&raw, ANY_REGION, Some(&mut addr_space), offset);
        test_true!(result.is_err());
        match result {
            Err(ElfLoadError::AddressSpaceViolation(e)) => {
                test_eq!(e, crate::memory::address_space::ELF_ERR_KERNEL_COLLISION);
            }
            _ => test_true!(false),
        }
    });

    test_case!("elf_pie_offset_above_user_limit", {
        // load_offset that pushes segments above USER_LIMIT should get
        // VADDR_OUT_OF_RANGE
        let code = [0x90u8; 32];
        let offset = 0x300000; // USER_LIMIT is 0x2400000, so 0x300000 is above it
        let raw = build_valid_elf(0, 0, &code); // vaddr+offset = 0x300000 > 0x2400000
        let mut addr_space = AddressSpace::new();
        let result = load_elf(&raw, ANY_REGION, Some(&mut addr_space), offset);
        test_true!(result.is_err());
        match result {
            Err(ElfLoadError::AddressSpaceViolation(e)) => {
                test_eq!(e, crate::memory::address_space::ELF_ERR_VADDR_OUT_OF_RANGE);
            }
            _ => test_true!(false),
        }
    });

    test_case!("elf_pie_overlapping_segments_with_offset", {
        // Overlap detection should work with load_offset
        let code = [0x90u8; 16];
        let offset = 0x400000;
        let raw = build_elf_two_segments(0x400000, 0, &code, 8, &code);
        let mut addr_space = AddressSpace::new();
        let result = load_elf(&raw, ANY_REGION, Some(&mut addr_space), offset);
        test_true!(result.is_err());
    });

    // ── AUDIT-36: userbin_link_smoke — entry point validation ──

    test_case!("userbin_link_smoke", {
        let code = [0xccu8; 16];
        let raw = build_valid_elf(0x400000, 0x400000, &code);
        let mut addr_space = AddressSpace::new();
        let result = load_elf(&raw, ANY_REGION, Some(&mut addr_space), 0);
        test_true!(result.is_ok());
        let r = result.unwrap();
        test_eq!(r.entry, 0x400000);
        test_eq!(r.segments.len(), 1);
        test_true!(r.segments[0].vaddr <= r.entry);
        test_true!(r.entry < r.segments[0].vaddr + r.segments[0].memsz);
    });

    test_case!("elf_entry_not_in_segment_returns_error", {
        let code = [0xccu8; 16];
        let raw = build_valid_elf(0x500000, 0x400000, &code);
        let mut addr_space = AddressSpace::new();
        let result = load_elf(&raw, ANY_REGION, Some(&mut addr_space), 0);
        test_eq!(result, Err(ElfLoadError::EntryNotInSegment));
    });

    test_case!("elf_entry_outside_segment_warns_without_addrspace", {
        let code = [0xccu8; 16];
        let raw = build_valid_elf(0x500000, 0x400000, &code);
        let result = load_elf(&raw, region_around(0x400000), None, 0);
        test_true!(result.is_ok());
    });

    // ── #762: the region check replaces "trust the caller" on the None path ──

    test_case!("elf_load_region_bounds", {
        let r = LoadRegion::from_bounds(0x1000, 0x2000);
        test_true!(r.contains(0x1000, 0x1000));
        test_true!(!r.contains(0x1000, 0x1001));
        test_true!(!r.contains(0x0FFF, 1));
        // The limit itself is outside the region, even for an empty range.
        test_true!(!r.contains(0x2000, 0));
        test_true!(!r.contains(u64::MAX, 1));
        // A zero-sized or inverted region accepts nothing.
        test_true!(!LoadRegion::new(0x1000, 0).contains(0x1000, 0));
        test_true!(!LoadRegion::from_bounds(0x2000, 0x1000).contains(0x2000, 1));
    });

    test_case!("elf_reject_segment_outside_process_slot", {
        // A segment that starts in the code slot but ends in the stack region
        // of the same process must be rejected (#762 acceptance).
        use crate::arch::x64::paging::{MAX_BIN_SIZE, USER_BASE};
        let code = [0x90u8; 16];
        let vaddr = USER_BASE + MAX_BIN_SIZE - 8;
        let raw = build_valid_elf(vaddr, vaddr, &code);
        let region = LoadRegion::new(USER_BASE, MAX_BIN_SIZE);
        test_eq!(
            load_elf(&raw, region, None, 0),
            Err(ElfLoadError::SegmentOutOfRegion)
        );
    });

    test_case!("elf_reject_kernel_write_without_addrspace", {
        // The image asks to be loaded on top of kernel memory. Rejected before
        // the copy, so the sentinel is untouched.
        let mut sentinel = [0xA5u8; 64];
        let target = sentinel.as_mut_ptr() as u64;
        let raw = build_valid_elf(target, target, &[0u8; 16]);
        let region = LoadRegion::new(
            crate::arch::x64::paging::USER_BASE,
            crate::arch::x64::paging::MAX_BIN_SIZE,
        );
        test_eq!(
            load_elf(&raw, region, None, 0),
            Err(ElfLoadError::SegmentOutOfRegion)
        );
        test_true!(sentinel.iter().all(|b| *b == 0xA5));
    });

    test_case!("elf_reject_segment_above_user_limit", {
        use crate::arch::x64::paging::{MAX_BIN_SIZE, USER_BASE, USER_LIMIT};
        let code = [0x90u8; 16];
        let vaddr = USER_LIMIT - 8;  // would cross into kernel heap
        let raw = build_valid_elf(vaddr, vaddr, &code);
        let region = LoadRegion::new(USER_BASE, MAX_BIN_SIZE);
        test_eq!(
            load_elf(&raw, region, None, 0),
            Err(ElfLoadError::SegmentOutOfRegion)
        );
    });

    test_case!("elf_reject_overlapping_segments_without_addrspace", {
        let code = [0x90u8; 16];
        let raw = build_elf_two_segments(0x400000, 0x400000, &code, 0x400008, &code);
        let region = LoadRegion::from_bounds(0x400000, 0x401000);
        test_eq!(
            load_elf(&raw, region, None, 0),
            Err(ElfLoadError::SegmentOverlap)
        );
    });

    test_case!("elf_reject_no_load_segments", {
        let code = [0x90u8; 16];
        let mut raw = build_valid_elf(0x400000, 0x400000, &code);
        // Turn the only PT_LOAD into PT_PHDR (6).
        raw[64..68].copy_from_slice(&6u32.to_le_bytes());
        test_eq!(
            load_elf(&raw, region_around(0x400000), None, 0),
            Err(ElfLoadError::NoLoadSegments)
        );
    });

    test_case!("elf_reject_relocation_target_outside_segments", {
        // #762: a crafted .rela.dyn must not write outside the loaded image.
        #[repr(align(8))]
        struct Scratch([u8; 512]);
        let mut scratch = Scratch([0u8; 512]);
        let base = scratch.0.as_mut_ptr() as u64;
        let raw = build_elf_with_rela(0x1000, 0);  // base + 4 KB: outside
        let region = LoadRegion::from_bounds(base, base + 512);
        test_eq!(
            load_elf(&raw, region, None, base),
            Err(ElfLoadError::RelocationError(
                "relocation target outside the image segments"
            ))
        );
        // Nothing was written past the image (last segment ends at 0x148).
        test_true!(scratch.0[0x148..].iter().all(|b| *b == 0));
    });

    test_case!("elf_applies_relocation_inside_segment", {
        #[repr(align(8))]
        struct Scratch([u8; 512]);
        let mut scratch = Scratch([0u8; 512]);
        let base = scratch.0.as_mut_ptr() as u64;
        let raw = build_elf_with_rela(0x8, 0x1234);
        let region = LoadRegion::from_bounds(base, base + 512);
        test_true!(load_elf(&raw, region, None, base).is_ok());
        let value = u64::from_le_bytes([
            scratch.0[8], scratch.0[9], scratch.0[10], scratch.0[11],
            scratch.0[12], scratch.0[13], scratch.0[14], scratch.0[15],
        ]);
        test_eq!(value, base.wrapping_add(0x1234));
    });
}
