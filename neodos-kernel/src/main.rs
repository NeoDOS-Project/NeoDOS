#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![allow(static_mut_refs)]

extern crate alloc;

use core::panic::PanicInfo;

mod allocator;
mod arch;
mod console;
mod cpu;
mod scheduler;
mod processes;
mod drivers;
mod buffer;
mod fs;
mod input;
mod shell;
mod graphics;
mod font;
mod tsr;
mod memory;
mod globals;
pub mod usermode;
pub mod syscall;
mod testing;

use drivers::ata::AtaDriver;
use drivers::fat32::Fat32Driver;
use drivers::gpt;
use drivers::pci;
use buffer::block_cache::BlockCache;
use fs::neodos_fs::NeoDosFs;
use graphics::FramebufferInfo;

const KERNEL_VERSION: &str = concat!("NeoDOS Kernel v", env!("CARGO_PKG_VERSION"), " - The Rusty DOS Revival");

#[repr(C)]
pub struct BootInfo {
    pub fb_info: FramebufferInfo,
    pub memory_map_addr: u64,
    pub memory_map_size: u64,
    pub memory_map_desc_size: u64,
    pub memory_map_desc_version: u32,
}

#[no_mangle]
#[link_section = ".text.entry"]
pub unsafe extern "sysv64" fn _start(boot_info: &BootInfo) -> ! {
    // 1. Initialize Graphics Renderer
    graphics::init(boot_info.fb_info.clone());
    
    // 2. Setup Serial for output
    arch::x64::init_serial();

    // 3. Print kernel banner to serial
    serial_println!("========================================");
    serial_println!("{}", KERNEL_VERSION);
    serial_println!("========================================");
    serial_println!("[+] Graphics initialized: {}x{}", boot_info.fb_info.width, boot_info.fb_info.height);

    // 4. Initialize legacy VGA as backup (might not work, but keeps code compatible)
    console::init();

    // ============================================
    // PHASE 2: Initialize CPU structures
    // ============================================
    serial_println!("[+] Initializing GDT...");
    arch::x64::init_gdt();
    
    serial_println!("[+] Initializing IDT...");
    arch::x64::init_idt();
    
    serial_println!("[+] Initializing PIC...");
    arch::x64::init_pic();

    serial_println!("[+] Initializing PS/2 controller...");
    drivers::keyboard::init_ps2();

    serial_println!("[+] Enabling interrupts...");
    arch::x64::enable_interrupts();

    // ============================================
    // PHASE 2.5: Physical memory map / allocator
    // ============================================
    memory::init(boot_info);

    // ============================================
    // PHASE 2.75: Heap allocator (uses identity map)
    // ============================================
    allocator::init();

    // ============================================
    // PHASE 3: Storage stack
    // ============================================
    serial_println!("[+] Initializing ATA driver...");
    globals::ATA_DRIVER = Some(AtaDriver::new());

    serial_println!("[+] Scanning PCI for IDE bus-master DMA...");
    if let Some(ide) = pci::find_ide_controller() {
        pci::enable_bus_master(&ide);
        globals::ATA_DRIVER.as_mut().unwrap().init_dma(ide.bus_master_base);
        serial_println!("[+] ATA bus-master DMA enabled at BMBA 0x{:04X}", ide.bus_master_base);
    } else {
        serial_println!("[!] No IDE bus-master controller found, using PIO");
    }

    let ata = globals::ATA_DRIVER.as_mut().unwrap();

    serial_println!("[+] Parsing GPT for NeoDOS partition...");
    if let Some(part) = gpt::find_neodos_partition(ata) {
        serial_println!("[+] NeoDOS partition found: LBA {}..{}",
            part.start_lba, part.end_lba);
        ata.set_base_lba(part.start_lba as u32);
    } else {
        serial_println!("[!] No GPT/NeoDOS partition found; assuming LBA 0");
    }

    serial_println!("[+] Initializing Block Cache...");
    globals::BLOCK_CACHE = Some(BlockCache::new());
    let cache = globals::BLOCK_CACHE.as_mut().unwrap();

    serial_println!("[+] Reading Superblock...");
    let sb_data = match ata.read_sector(0) {
        Ok(data) => data,
        Err(_) => panic!("Failed to read superblock"),
    };

    serial_println!("[+] Mounting NeoDOS FS...");
    match NeoDosFs::new(&sb_data) {
        Ok(fs) => {
            globals::NEODOS_FS = Some(fs);
            serial_println!("[+] NeoDOS FS mounted");
    let _ = globals::NEODOS_FS.as_mut().unwrap().rebuild_bitmap(cache, ata);
    serial_println!("[+] Block bitmap rebuilt");
        },
        Err(_) => panic!("Failed to mount filesystem"),
    }

    // ============================================
    // FAT32: Read boot partition
    // ============================================
    serial_println!("[+] Initializing FAT32 driver...");
    globals::FAT32_DRIVER = Fat32Driver::new(ata).ok();

    // ============================================
    // PHASE 6 / PHASE 3: Custom Page Tables & User Memory
    // ============================================
    unsafe {
        arch::x64::paging::init_custom_page_tables();
    }


    // ============================================
    // PHASE 4: Start DOS Shell
    // ============================================
    serial_println!("[+] Starting NeoDOS Shell...");

    testing::register_tests();
    
    let mut shell = shell::DosShell::new(
        globals::NEODOS_FS.as_mut().unwrap(),
        globals::BLOCK_CACHE.as_mut().unwrap(),
        globals::ATA_DRIVER.as_mut().unwrap(),
        globals::FAT32_DRIVER.take()
    );
    
    shell.run();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    arch::disable_interrupts();
    serial_println!("\r\n!!! KERNEL PANIC !!!");
    if let Some(location) = info.location() {
        serial_println!("Location: {}:{}", location.file(), location.line());
    }

    arch::halt();
}
