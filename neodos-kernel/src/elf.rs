// ELF64 loader — parses ELF headers and loads PT_LOAD segments into memory.
// Supports ET_EXEC and ET_DYN (PIE) for x86-64.
//
// A4.3 — Range validation: segments must stay within user window, null vaddr
// rejected, no overlap with protected regions or other segments.

use core::ptr::copy_nonoverlapping;
use core::mem::size_of;
use alloc::vec::Vec;
use crate::scheduler::address_space::{AddressSpace, SegmentInfo};

// ── ELF64 constants ──

const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const EM_X86_64: u16 = 62;
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;
const PT_LOAD: u32 = 1;

// ── ELF64 header (64 bytes) ──

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct Elf64Hdr {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

// ── ELF64 program header (56 bytes) ──

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct Elf64Phdr {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
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
    AddressSpaceViolation(i64),
}

// ── Result ──

#[derive(Debug, PartialEq)]
pub struct ElfLoadResult {
    pub entry: u64,
    pub segments: Vec<SegmentInfo>,
}

/// Parse and load an ELF64 binary with optional address space validation.
///
/// When `addr_space` is `Some`, validates segment ranges against protected regions
/// and user window. When `None`, loads without address space checks (for tests
/// and early boot where validation is not needed).
///
/// Returns the entry point and loaded segment info on success.
pub fn load_elf(data: &[u8], mut addr_space: Option<&mut AddressSpace>) -> Result<ElfLoadResult, ElfLoadError> {
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

    // ── Collect PT_LOAD segments for validation ──
    let mut segments = Vec::new();
    let entry = hdr.e_entry;
    let mut entry_in_segment = false;

    for ph in phdrs {
        if ph.p_type != PT_LOAD {
            continue;
        }

        let vaddr = ph.p_vaddr;
        let memsz = ph.p_memsz;

        // Check entry point containment (check 5 — log warning, don't fail)
        if entry >= vaddr && entry < vaddr.saturating_add(memsz) {
            entry_in_segment = true;
        }

        // Validate segment range and register with address space (if provided)
        if let Some(ref mut space) = addr_space {
            space.add_segment(vaddr, memsz)
                .map_err(|e| ElfLoadError::AddressSpaceViolation(e))?;
        }

        segments.push(SegmentInfo { vaddr, memsz });
    }

    // Check 5: entry point not in any PT_LOAD — log warning
    if !entry_in_segment && !segments.is_empty() {
        crate::serial_println!("[ELF] WARNING: entry 0x{:x} not contained in any PT_LOAD segment", entry);
    }

    // ── Load each PT_LOAD segment ──
    for ph in phdrs {
        if ph.p_type != PT_LOAD {
            continue;
        }

        let vaddr = ph.p_vaddr;
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

    Ok(ElfLoadResult { entry, segments })
}

// ── Tests ──

/// Build a minimal valid ELF64 binary with one PT_LOAD segment.
fn build_valid_elf(entry: u64, vaddr: u64, code: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();

    // ELF header (64 bytes)
    buf.extend_from_slice(b"\x7fELF");
    buf.push(2);  // class = ELFCLASS64
    buf.push(1);  // data = ELFDATA2LSB
    buf.push(1);  // version
    buf.push(0);  // osabi
    buf.push(0);  // abiversion
    buf.extend_from_slice(&[0u8; 7]);  // padding
    buf.extend_from_slice(&(ET_EXEC as u16).to_le_bytes());  // e_type
    buf.extend_from_slice(&(EM_X86_64 as u16).to_le_bytes());  // e_machine
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
    buf.extend_from_slice(&(PT_LOAD as u32).to_le_bytes());  // p_type
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
    buf.extend_from_slice(&(ET_EXEC as u16).to_le_bytes());
    buf.extend_from_slice(&(EM_X86_64 as u16).to_le_bytes());
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
    buf.extend_from_slice(&(PT_LOAD as u32).to_le_bytes());
    buf.extend_from_slice(&7u32.to_le_bytes());
    buf.extend_from_slice(&code1_offset.to_le_bytes());
    buf.extend_from_slice(&vaddr1.to_le_bytes());
    buf.extend_from_slice(&vaddr1.to_le_bytes());
    buf.extend_from_slice(&(code1.len() as u64).to_le_bytes());
    buf.extend_from_slice(&(code1.len() as u64).to_le_bytes());
    buf.extend_from_slice(&1u64.to_le_bytes());

    // PHDR 2: PT_LOAD
    buf.extend_from_slice(&(PT_LOAD as u32).to_le_bytes());
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

/// Register ELF loader tests with the kernel test framework.
pub fn register_elf_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;

    // ── Original parser tests (updated for Result return) ──

    test_case!("elf_parse_valid_header", {
        let code = [0x90u8; 16];
        let raw = build_valid_elf(0x400000, 0x400000, &code);
        let result = load_elf(&raw, None);
        test_true!(result.is_ok());
        test_eq!(result.unwrap().entry, 0x400000);
    });

    test_case!("elf_parse_invalid_magic", {
        let code = [0x90u8; 16];
        let mut raw = build_valid_elf(0x400000, 0x400000, &code);
        raw[0..4].copy_from_slice(b"BAD\x00");
        test_eq!(load_elf(&raw, None), Err(ElfLoadError::InvalidMagic));
    });

    test_case!("elf_parse_invalid_class", {
        let code = [0x90u8; 16];
        let mut raw = build_valid_elf(0x400000, 0x400000, &code);
        raw[4] = 1;
        test_eq!(load_elf(&raw, None), Err(ElfLoadError::InvalidClass));
    });

    test_case!("elf_parse_invalid_machine", {
        let code = [0x90u8; 16];
        let mut raw = build_valid_elf(0x400000, 0x400000, &code);
        raw[18..20].copy_from_slice(&3u16.to_le_bytes());
        test_eq!(load_elf(&raw, None), Err(ElfLoadError::InvalidMachine));
    });

    test_case!("elf_parse_truncated_header", {
        let data = [0u8; 4];
        test_eq!(load_elf(&data, None), Err(ElfLoadError::InvalidHeader));
    });

    test_case!("elf_parse_load_segment", {
        let code = [0xb8, 0x00, 0x00, 0x00, 0x00,
                    0xcd, 0x80];
        let mut test_buf = [0u8; 32];
        let load_addr = test_buf.as_mut_ptr() as u64;
        let raw = build_valid_elf(load_addr, load_addr, &code);
        let result = load_elf(&raw, None);
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
        test_eq!(load_elf(&raw, None), Err(ElfLoadError::InvalidPhdrSize));
    });

    // ── A4.3 Range validation tests ──

    test_case!("elf_validation_valid_range", {
        let code = [0x90u8; 32];
        let raw = build_valid_elf(0x400000, 0x400000, &code);
        let mut addr_space = AddressSpace::new();
        let result = load_elf(&raw, Some(&mut addr_space));
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
        let result = load_elf(&raw, Some(&mut addr_space));
        test_true!(result.is_err());
        match result {
            Err(ElfLoadError::AddressSpaceViolation(e)) => {
                test_eq!(e, crate::scheduler::address_space::ELF_ERR_ZERO_VADDR);
            }
            _ => test_true!(false),
        }
    });

    test_case!("elf_reject_kernel_collision", {
        // Segment at 0x200000 (kernel image base) should be rejected
        let code = [0x90u8; 16];
        let raw = build_valid_elf(0x200000, 0x200000, &code);
        let mut addr_space = AddressSpace::new();
        let result = load_elf(&raw, Some(&mut addr_space));
        test_true!(result.is_err());
        match result {
            Err(ElfLoadError::AddressSpaceViolation(e)) => {
                test_eq!(e, crate::scheduler::address_space::ELF_ERR_KERNEL_COLLISION);
            }
            _ => test_true!(false),
        }
    });

    test_case!("elf_reject_heap_collision", {
        // Segment at 0x1000000 (kernel heap base) should be rejected
        let code = [0x90u8; 16];
        let raw = build_valid_elf(0x1000000, 0x1000000, &code);
        let mut addr_space = AddressSpace::new();
        let result = load_elf(&raw, Some(&mut addr_space));
        test_true!(result.is_err());
        match result {
            Err(ElfLoadError::AddressSpaceViolation(e)) => {
                test_eq!(e, crate::scheduler::address_space::ELF_ERR_KERNEL_COLLISION);
            }
            _ => test_true!(false),
        }
    });

    test_case!("elf_reject_mmap_collision", {
        // Segment at 0x20000000 (mmap region base) should be rejected
        let code = [0x90u8; 16];
        let raw = build_valid_elf(0x20000000, 0x20000000, &code);
        let mut addr_space = AddressSpace::new();
        let result = load_elf(&raw, Some(&mut addr_space));
        test_true!(result.is_err());
        match result {
            Err(ElfLoadError::AddressSpaceViolation(e)) => {
                test_eq!(e, crate::scheduler::address_space::ELF_ERR_MMAP_COLLISION);
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
            0x100000,       // kernel image
            0x1000000,      // kernel heap
            0x10000000,     // user heap
            0x20000000,     // mmap region
            0x30000000,     // driver isolation
        ];

        for &vaddr in targets {
            let raw = build_valid_elf(vaddr, vaddr, &code);
            let result = load_elf(&raw, Some(&mut addr_space));
            test_true!(result.is_err());
        }
    });

    test_case!("elf_overlap_segments", {
        let code = [0x90u8; 16];
        // Two overlapping segments at same vaddr
        let raw = build_elf_two_segments(0x400000, 0x400000, &code, 0x400008, &code);
        let mut addr_space = AddressSpace::new();
        let result = load_elf(&raw, Some(&mut addr_space));
        test_true!(result.is_err());
        match result {
            Err(ElfLoadError::AddressSpaceViolation(e)) => {
                test_eq!(e, crate::scheduler::address_space::ELF_ERR_SEGMENT_OVERLAP);
            }
            _ => test_true!(false),
        }
    });

    test_case!("elf_user_heap_collision", {
        // Segment in user heap range (0x10000000) should be rejected
        let code = [0x90u8; 16];
        let raw = build_valid_elf(0x10000000, 0x10000000, &code);
        let mut addr_space = AddressSpace::new();
        let result = load_elf(&raw, Some(&mut addr_space));
        test_true!(result.is_err());
        match result {
            Err(ElfLoadError::AddressSpaceViolation(e)) => {
                test_eq!(e, crate::scheduler::address_space::ELF_ERR_HEAP_COLLISION);
            }
            _ => test_true!(false),
        }
    });
}
