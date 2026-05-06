#![no_std]
#![no_main]

extern crate alloc;

use uefi::prelude::*;
use uefi::proto::media::file::{File, FileAttribute, FileMode};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::proto::console::gop::GraphicsOutput;
use uefi::boot::{MemoryType, AllocateType};
use uefi::mem::memory_map::MemoryMap;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FramebufferInfo {
    pub base_address: u64,
    pub size: usize,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
}

#[repr(C)]
pub struct BootInfo {
    pub fb_info: FramebufferInfo,
    pub memory_map_addr: u64,
    pub memory_map_size: u64,
    pub memory_map_desc_size: u64,
    pub memory_map_desc_version: u32,
}

#[uefi::entry]
fn efi_main() -> Status {
    uefi::helpers::init().expect("Failed to initialize UEFI services");

    log::info!("========================================");
    log::info!("NeoDOS Bootloader v0.6");
    log::info!("========================================");

    // 1. Get GOP Framebuffer
    log::info!("[+] Initializing GOP...");
    let gop_handle = uefi::boot::get_handle_for_protocol::<GraphicsOutput>().expect("GOP not found");
    let mut gop = uefi::boot::open_protocol_exclusive::<GraphicsOutput>(gop_handle).expect("Failed to open GOP");
    
    let mode = gop.current_mode_info();
    let (width, height) = mode.resolution();
    let fb_info = FramebufferInfo {
        base_address: gop.frame_buffer().as_mut_ptr() as u64,
        size: gop.frame_buffer().size(),
        width,
        height,
        stride: mode.stride(),
    };
    log::info!("[✓] Graphics: {}x{} @ 0x{:x}", width, height, fb_info.base_address);

    // 2. Load kernel ELF
    log::info!("[+] Loading kernel ELF...");
    let kernel_data = load_file().expect("Failed to load kernel ELF");
    
    // 3. Parse and Load ELF segments
    let entry_point = load_elf(&kernel_data).expect("Failed to load ELF segments");
    log::info!("[✓] Kernel loaded. Entry: 0x{:x}", entry_point);

    // 4. Prepare BootInfo
    // We'll place BootInfo at a fixed address for simplicity, 
    // or just pass it to the kernel entry point.
    // 5. Exit boot services
    log::info!("[+] Exiting boot services...");
    let mmap = unsafe { uefi::boot::exit_boot_services(None) };
    let meta = mmap.meta();
    let mmap_buf = mmap.buffer();

    let boot_info = BootInfo {
        fb_info,
        memory_map_addr: mmap_buf.as_ptr() as u64,
        memory_map_size: meta.map_size as u64,
        memory_map_desc_size: meta.desc_size as u64,
        memory_map_desc_version: meta.desc_version,
    };
    core::mem::forget(mmap);

    // 6. Jump to kernel
    unsafe {
        core::arch::asm!("cli");
        
        let entry_fn: extern "sysv64" fn(&BootInfo) -> ! = core::mem::transmute(entry_point);
        entry_fn(&boot_info);
    }
}

fn load_file() -> Result<alloc::vec::Vec<u8>, uefi::Error> {
    let fs_handle = uefi::boot::get_handle_for_protocol::<SimpleFileSystem>()?;
    let mut fs = uefi::boot::open_protocol_exclusive::<SimpleFileSystem>(fs_handle)?;
    let mut root = fs.open_volume()?;
    let mut file = root
        .open(uefi::cstr16!("EFI\\NeoDOS\\kernel.elf"), FileMode::Read, FileAttribute::empty())?
        .into_regular_file()
        .expect("Kernel is not a regular file");
    
    let mut buffer = alloc::vec::Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let bytes_read = file.read(&mut chunk)?;
        if bytes_read == 0 { break; }
        buffer.extend_from_slice(&chunk[..bytes_read]);
    }
    Ok(buffer)
}

fn load_elf(data: &[u8]) -> Result<u64, ()> {
    if data.len() < 64 || &data[0..4] != b"\x7fELF" { return Err(()); }
    
    let entry_point = u64::from_le_bytes(data[24..32].try_into().unwrap());
    let ph_offset = u64::from_le_bytes(data[32..40].try_into().unwrap());
    let ph_count = u16::from_le_bytes(data[56..58].try_into().unwrap());
    let ph_size = u16::from_le_bytes(data[54..56].try_into().unwrap());

    for i in 0..ph_count {
        let offset = ph_offset as usize + i as usize * ph_size as usize;
        let ph = &data[offset..offset + ph_size as usize];
        
        let p_type = u32::from_le_bytes(ph[0..4].try_into().unwrap());
        if p_type == 1 { // PT_LOAD
            let p_offset = u64::from_le_bytes(ph[8..16].try_into().unwrap());
            let p_vaddr = u64::from_le_bytes(ph[16..24].try_into().unwrap());
            let p_filesz = u64::from_le_bytes(ph[32..40].try_into().unwrap());
            let p_memsz = u64::from_le_bytes(ph[40..48].try_into().unwrap());
            
            unsafe {
                core::ptr::write_bytes(p_vaddr as *mut u8, 0, p_memsz as usize);
                core::ptr::copy_nonoverlapping(
                    data.as_ptr().add(p_offset as usize),
                    p_vaddr as *mut u8,
                    p_filesz as usize,
                );
            }
        }
    }
    
    Ok(entry_point)
}
