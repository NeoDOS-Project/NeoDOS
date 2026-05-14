#![no_std]
#![no_main]

extern crate alloc;

use uefi::prelude::*;
use uefi::proto::loaded_image::LoadedImage;
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

const BOOTINFO_MAGIC: u32 = 0x4E444F53; // "NDOS" in ASCII
const BOOT_VERSION: u32 = ((0 * 256) + 10) << 8 | 3; // major.minor.patch -> 0x000A03 = v0.10.3

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
}

#[uefi::entry]
fn efi_main() -> Status {
    uefi::helpers::init().expect("Failed to initialize UEFI services");

    log::info!("========================================");
    log::info!("NeoDOS Bootloader v{}", env!("CARGO_PKG_VERSION"));
    log::info!("========================================");

    // 1. Get GOP Framebuffer
    log::info!("[+] Initializing GOP...");
    let fb_info = match init_gop() {
        Some(fb) => {
            log::info!("[✓] Graphics: {}x{} @ 0x{:x}", fb.width, fb.height, fb.base_address);
            fb
        }
        None => {
            log::warn!("[!] GOP unavailable, continuing without framebuffer");
            FramebufferInfo { base_address: 0, size: 0, width: 800, height: 600, stride: 800 }
        }
    };

    // 2. Load kernel ELF
    log::info!("[+] Loading kernel ELF...");
    let kernel_data = load_esp_file(uefi::cstr16!("EFI\\NeoDOS\\kernel.elf"))
        .expect("Failed to load kernel ELF");
    log::info!("[+] Kernel ELF loaded: {} bytes, magic={:02X}{:02X}{:02X}{:02X}",
        kernel_data.len(),
        kernel_data[0], kernel_data[1], kernel_data[2], kernel_data[3]);

    // 3. Parse and Load ELF segments
    log::info!("[+] Parsing ELF program headers...");
    let entry_point = load_elf(&kernel_data).expect("Failed to load ELF segments");
    log::info!("[✓] Kernel loaded. Entry: 0x{:x}", entry_point);

    // 3.5. Load NeoDOS FS image from ESP into allocated pages
    log::info!("[+] Loading NeoDOS FS image...");
    let fs_image_data = load_esp_file(uefi::cstr16!("EFI\\NeoDOS\\neodos.fs"))
        .unwrap_or_else(|_| {
            log::warn!("[!] neodos.fs not found, continuing without embedded FS");
            alloc::vec::Vec::new()
        });

    let (fs_image_addr, fs_image_size) = if !fs_image_data.is_empty() {
        let fs_page_count = (fs_image_data.len() + 0xFFF) / 0x1000;
        match uefi::boot::allocate_pages(
            AllocateType::AnyPages,
            MemoryType::LOADER_DATA,
            fs_page_count,
        ) {
            Ok(fs_buf) => {
                let addr = fs_buf.as_ptr() as u64;
                let size = fs_image_data.len() as u64;
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        fs_image_data.as_ptr(),
                        fs_buf.as_ptr(),
                        fs_image_data.len(),
                    );
                }
                core::mem::forget(fs_image_data);
                log::info!("[✓] FS image: {} bytes at 0x{:x}", size, addr);
                (addr, size)
            }
            Err(e) => {
                log::error!("[!] Failed to allocate pages for FS image: {:?}", e.status());
                (0, 0)
            }
        }
    } else {
        (0, 0)
    };

    // 4/5. Exit boot services and capture the final UEFI memory map.
    //
    // The map buffer is allocated from UEFI pool (typically LOADER_DATA). After
    // ExitBootServices the pool allocator is unavailable, so we *leak* the map
    // (via `forget`) and pass its raw pointer + metadata to the kernel.
    log::info!("[+] Stall 2s...");
    uefi::boot::stall(core::time::Duration::from_secs(2));
    log::info!("[+] Exiting boot services...");
    let mmap = unsafe { uefi::boot::exit_boot_services(None) };
    let meta = mmap.meta();
    let mmap_buf = mmap.buffer();

    let boot_info = BootInfo {
        magic: BOOTINFO_MAGIC,
        version: BOOT_VERSION,
        fb_info,
        memory_map_addr: mmap_buf.as_ptr() as u64,
        memory_map_size: meta.map_size as u64,
        memory_map_desc_size: meta.desc_size as u64,
        memory_map_desc_version: meta.desc_version,
        fs_image_addr,
        fs_image_size,
    };
    core::mem::forget(mmap);

    // 6. Jump to kernel
    unsafe {
        core::arch::asm!("cli");
        
        let entry_fn: extern "sysv64" fn(&BootInfo) -> ! = core::mem::transmute(entry_point);
        entry_fn(&boot_info);
    }
}

/// Helper: read framebuffer info from an opened GOP protocol.
fn extract_fb_from_gop(gop: &mut GraphicsOutput) -> Option<FramebufferInfo> {
    let mode = gop.current_mode_info();
    let (width, height) = mode.resolution();
    let mut fb = gop.frame_buffer();
    if fb.size() == 0 || width == 0 || height == 0 {
        return None;
    }
    Some(FramebufferInfo {
        base_address: fb.as_mut_ptr() as u64,
        size: fb.size(),
        width,
        height,
        stride: mode.stride(),
    })
}

/// Try to initialise the UEFI Graphics Output Protocol.
///
/// Uses ONLY `GetProtocol` (shared open) — exclusive open is avoided
/// because some firmware (e.g. ThinkPad X270) crashes when trying to
/// open GOP exclusively while ConOut already holds it.
///
/// Also tries `find_handles` to enumerate ALL GOP handles.
/// Returns `None` when no usable GOP could be obtained.
fn init_gop() -> Option<FramebufferInfo> {
    let image_handle = uefi::boot::image_handle();
    let gop_handle = uefi::boot::get_handle_for_protocol::<GraphicsOutput>().ok()?;

    // Helper: try a single handle with GetProtocol (safe, shared open).
    let try_get_protocol = |handle| -> Option<FramebufferInfo> {
        if let Ok(mut s) = unsafe {
            uefi::boot::open_protocol::<GraphicsOutput>(
                uefi::boot::OpenProtocolParams { handle, agent: image_handle, controller: None },
                uefi::boot::OpenProtocolAttributes::GetProtocol,
            )
        } {
            let fb = extract_fb_from_gop(&mut *s);
            core::mem::forget(s);
            return fb;
        }
        None
    };

    // Try the first handle.
    if let Some(fb) = try_get_protocol(gop_handle) {
        return Some(fb);
    }

    // Fallback: enumerate ALL GOP handles via find_handles.
    log::warn!("[!] GetProtocol on first handle failed, trying all handles...");
    if let Ok(handles) = uefi::boot::find_handles::<GraphicsOutput>() {
        for handle in &handles {
            if let Some(fb) = try_get_protocol(*handle) {
                return Some(fb);
            }
        }
    }

    log::warn!("[!] GOP not available");
    None
}

fn load_esp_file(path: &uefi::CStr16) -> Result<alloc::vec::Vec<u8>, uefi::Error> {
    let image_handle = uefi::boot::image_handle();
    let loaded_image = uefi::boot::open_protocol_exclusive::<LoadedImage>(image_handle)?;
    let device_handle = loaded_image.device()
        .ok_or(uefi::Status::NOT_FOUND)?;
    drop(loaded_image);
    let mut fs = uefi::boot::open_protocol_exclusive::<SimpleFileSystem>(device_handle)?;
    let mut root = fs.open_volume()?;
    let mut file = root
        .open(path, FileMode::Read, FileAttribute::empty())?
        .into_regular_file()
        .expect("File is not a regular file");
    
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
    if data.len() < 64 {
        log::error!("[!] ELF too short: {} bytes", data.len());
        return Err(());
    }
    if &data[0..4] != b"\x7fELF" {
        log::error!("[!] Bad ELF magic: {:02X} {:02X} {:02X} {:02X}",
            data[0], data[1], data[2], data[3]);
        return Err(());
    }

    let entry_point = u64::from_le_bytes(data[24..32].try_into().unwrap());
    let ph_offset = u64::from_le_bytes(data[32..40].try_into().unwrap());
    let ph_count = u16::from_le_bytes(data[56..58].try_into().unwrap());
    let ph_size = u16::from_le_bytes(data[54..56].try_into().unwrap());

    log::info!("[+] ELF entry=0x{:x} PH offset=0x{:x} count={} size={}",
        entry_point, ph_offset, ph_count, ph_size);

    // First pass: find required address range for all LOAD segments
    let mut min_vaddr = u64::MAX;
    let mut max_end = 0u64;
    for i in 0..ph_count {
        let offset = ph_offset as usize + i as usize * ph_size as usize;
        if offset + ph_size as usize > data.len() {
            log::error!("[!] PH {} extends past file (offset 0x{:x}, size {})", i, offset, ph_size);
            return Err(());
        }
        let ph = &data[offset..offset + ph_size as usize];
        let p_type = u32::from_le_bytes(ph[0..4].try_into().unwrap());
        if p_type == 1 {
            let p_vaddr = u64::from_le_bytes(ph[16..24].try_into().unwrap());
            let p_memsz = u64::from_le_bytes(ph[40..48].try_into().unwrap());
            let end = p_vaddr.checked_add(p_memsz).ok_or(())?;
            if p_vaddr < min_vaddr { min_vaddr = p_vaddr; }
            if end > max_end { max_end = end; }
        }
    }

    if min_vaddr == u64::MAX {
        log::error!("[!] No PT_LOAD segments in ELF");
        return Err(());
    }

    // Allocate pages covering the entire kernel range via UEFI boot services.
    // This guarantees the pages are mapped in UEFI page tables and won't fault.
    let alloc_base = min_vaddr & !0xFFF;      // page-align down
    let alloc_end  = (max_end + 0xFFF) & !0xFFF;  // page-align up
    let page_count = ((alloc_end - alloc_base) >> 12) as usize;

    log::info!("[+] Allocating {} pages @ 0x{:x} (range 0x{:x}-0x{:x})",
        page_count, alloc_base, alloc_base, alloc_end);

    match uefi::boot::allocate_pages(
        AllocateType::Address(alloc_base),
        MemoryType::LOADER_DATA,
        page_count,
    ) {
        Ok(addr) => log::info!("[✓] Allocated at 0x{:x}", addr.as_ptr() as u64),
        Err(e) => {
            log::error!("[!] Failed to allocate pages at 0x{:x}: {:?}", alloc_base, e.status());
            log::error!("[!] Falling back to AnyPages...");
            let addr = uefi::boot::allocate_pages(
                AllocateType::AnyPages,
                MemoryType::LOADER_DATA,
                page_count,
            ).map_err(|e2| {
                log::error!("[!] AnyPages also failed: {:?}", e2.status());
            })?;
            let addr_u64 = addr.as_ptr() as u64;
            log::warn!("[!] Kernel loaded at 0x{:x} instead of 0x{:x} (will likely crash!)", addr_u64, alloc_base);
        }
    }

    // Second pass: copy segment data
    for i in 0..ph_count {
        let offset = ph_offset as usize + i as usize * ph_size as usize;
        let ph = &data[offset..offset + ph_size as usize];

        let p_type = u32::from_le_bytes(ph[0..4].try_into().unwrap());
        let p_flags = u32::from_le_bytes(ph[4..8].try_into().unwrap());
        if p_type == 1 { // PT_LOAD
            let p_offset = u64::from_le_bytes(ph[8..16].try_into().unwrap());
            let p_vaddr  = u64::from_le_bytes(ph[16..24].try_into().unwrap());
            let p_paddr  = u64::from_le_bytes(ph[24..32].try_into().unwrap());
            let p_filesz = u64::from_le_bytes(ph[32..40].try_into().unwrap());
            let p_memsz  = u64::from_le_bytes(ph[40..48].try_into().unwrap());

            log::info!("[+]  PH[{}] LOAD vaddr=0x{:x} paddr=0x{:x} filesz={} memsz={} flags=0x{:x}",
                i, p_vaddr, p_paddr, p_filesz, p_memsz, p_flags);

            if p_filesz > 0 {
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
    }

    Ok(entry_point)
}
