// ELF64 loader — parses ELF headers and loads PT_LOAD segments into memory.
// Supports ET_EXEC and ET_DYN (PIE) for x86-64.

use core::ptr::copy_nonoverlapping;
use core::mem::size_of;
use alloc::vec::Vec;

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

// ── Result ──

#[derive(Debug, PartialEq)]
pub struct ElfLoadResult {
    pub entry: u64,
}

/// Parse and load an ELF64 binary.
///
/// Validates the ELF header, then for each `PT_LOAD` segment:
/// - Copies `p_filesz` bytes from file offset `p_offset` to virtual address `p_vaddr`
/// - Zero-fills `p_memsz - p_filesz` bytes (`.bss`)
///
/// Returns the entry point on success.
///
/// # Safety
///
/// This function writes to absolute virtual addresses (`p_vaddr`) using raw pointers.
/// The caller must ensure:
/// - `data` contains the entire ELF file
/// - The target addresses are mapped and writable in the current page tables
/// - No segment overlaps other sensitive memory
pub fn load_elf(data: &[u8]) -> Option<ElfLoadResult> {
    if data.len() < size_of::<Elf64Hdr>() {
        return None;
    }

    // SAFETY: we've checked data.len() >= 64 (size_of::<Elf64Hdr>()).
    let hdr: &Elf64Hdr = unsafe { &*(data.as_ptr() as *const Elf64Hdr) };

    // ── Validate ELF identity ──
    if hdr.e_ident[..4] != ELF_MAGIC {
        return None;
    }
    if hdr.e_ident[4] != ELFCLASS64 {
        return None;
    }
    if hdr.e_ident[5] != ELFDATA2LSB {
        return None;
    }
    if hdr.e_machine != EM_X86_64 {
        return None;
    }
    if hdr.e_type != ET_EXEC && hdr.e_type != ET_DYN {
        return None;
    }

    // ── Validate program header table ──
    let phoff = hdr.e_phoff as usize;
    let phentsize = hdr.e_phentsize as usize;
    let phnum = hdr.e_phnum as usize;

    if phentsize != size_of::<Elf64Phdr>() {
        return None;
    }
    let ph_table_end = phoff.checked_add(phnum * phentsize)?;
    if ph_table_end > data.len() {
        return None;
    }

    // SAFETY: bounds checked above.
    let phdrs: &[Elf64Phdr] = unsafe {
        core::slice::from_raw_parts(
            data.as_ptr().add(phoff) as *const Elf64Phdr,
            phnum,
        )
    };

    // ── Load each PT_LOAD segment ──
    for ph in phdrs {
        if ph.p_type != PT_LOAD {
            continue;
        }

        let vaddr = ph.p_vaddr;
        let offset = ph.p_offset as usize;
        let filesz = ph.p_filesz as usize;
        let memsz = ph.p_memsz as usize;

        // Validate bounds
        let src_end = offset.checked_add(filesz)?;
        if src_end > data.len() {
            return None;
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

    Some(ElfLoadResult {
        entry: hdr.e_entry,
    })
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

/// Register ELF loader tests with the kernel test framework.
pub fn register_elf_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_ne;

    test_case!("elf_parse_valid_header", {
        let code = [0x90u8; 16];
        let raw = build_valid_elf(0x400000, 0x400000, &code);
        let result = load_elf(&raw);
        test_ne!(result, None);
        test_eq!(result.unwrap().entry, 0x400000);
    });

    test_case!("elf_parse_invalid_magic", {
        let code = [0x90u8; 16];
        let mut raw = build_valid_elf(0x400000, 0x400000, &code);
        raw[0..4].copy_from_slice(b"BAD\x00");
        test_eq!(load_elf(&raw), None);
    });

    test_case!("elf_parse_invalid_class", {
        let code = [0x90u8; 16];
        let mut raw = build_valid_elf(0x400000, 0x400000, &code);
        raw[4] = 1; // change from ELFCLASS64 to ELFCLASS32
        test_eq!(load_elf(&raw), None);
    });

    test_case!("elf_parse_invalid_machine", {
        let code = [0x90u8; 16];
        let mut raw = build_valid_elf(0x400000, 0x400000, &code);
        // e_machine at offset 18
        raw[18..20].copy_from_slice(&3u16.to_le_bytes()); // EM_I386 instead of EM_X86_64
        test_eq!(load_elf(&raw), None);
    });

    test_case!("elf_parse_truncated_header", {
        let data = [0u8; 4];
        test_eq!(load_elf(&data), None);
    });

    test_case!("elf_parse_load_segment", {
        let code = [0xb8, 0x00, 0x00, 0x00, 0x00,  // mov eax, 0
                    0xcd, 0x80];                     // int 0x80
        let mut test_buf = [0u8; 32];
        let load_addr = test_buf.as_mut_ptr() as u64;
        let raw = build_valid_elf(load_addr, load_addr, &code);
        let result = load_elf(&raw);
        test_ne!(result, None);
        test_eq!(result.unwrap().entry, load_addr);
        // Verify code was loaded into our buffer
        test_eq!(test_buf[0], 0xb8);
        test_eq!(test_buf[5], 0xcd);
    });

    test_case!("elf_parse_bad_phentsize", {
        let code = [0x90u8; 8];
        let mut raw = build_valid_elf(0x400000, 0x400000, &code);
        // e_phentsize at offset 54..56
        raw[54..56].copy_from_slice(&99u16.to_le_bytes());
        test_eq!(load_elf(&raw), None);
    });
}

