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

use drivers::ata::{AtaChannel, AtaDriver};
use drivers::fat32::Fat32Driver;
use drivers::gpt;
use drivers::pci;
use buffer::block_cache::BlockCache;
use fs::neodos_fs::NeoDosFs;
use fs::volume::Volume;
use graphics::FramebufferInfo;

const KERNEL_VERSION: &str = concat!("NeoDOS Kernel v", env!("CARGO_PKG_VERSION"), " - The Rusty DOS Revival");

const BOOTINFO_MAGIC: u32 = 0x4E444F53; // "NDOS" in ASCII
const KERNEL_VERSION_CODE: u32 = ((0 * 256) + 10) << 8 | 3; // v0.10.3

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

#[no_mangle]
#[link_section = ".text.entry"]
pub unsafe extern "sysv64" fn rust_start(boot_info: &BootInfo) -> ! {
    // 0. Verify boot info magic and version
    if boot_info.magic != BOOTINFO_MAGIC {
        // Can't use println yet, serial not initialized
        loop {}
    }

    // 1. Initialize Graphics Renderer
    graphics::init(boot_info.fb_info.clone());
    drivers::keyboard::set_leds(0b100); // Caps Lock ON = kernel entry

    // 1b. Set up RAM disk from bootloader-loaded FS image
    globals::RAM_DISK_BASE = boot_info.fs_image_addr;
    globals::RAM_DISK_SIZE = boot_info.fs_image_size;

    // 2. Setup Serial for output
    arch::x64::init_serial();

    // Check bootloader version compatibility
    let bootloader_version = boot_info.version;
    if bootloader_version != KERNEL_VERSION_CODE {
        serial_println!("[!] Version mismatch: bootloader v{:x}, kernel v{:x}", bootloader_version, KERNEL_VERSION_CODE);
    } else {
        serial_println!("[+] Bootloader version: v0.10.1 (compatible)");
    }

    serial_println!("[+] Graphics initialized: {}x{}", boot_info.fb_info.width, boot_info.fb_info.height);

    // 4. Initialize legacy VGA as backup (might not work, but keeps code compatible)
    console::init();
    drivers::keyboard::set_leds(0b110); // Caps Lock + Num Lock ON = console ready

    println!("========================================");
    println!("{}", KERNEL_VERSION);
    println!("========================================");

    // ============================================
    // PHASE 2: Initialize CPU structures
    // ============================================
    println!("[+] Initializing GDT...");
    arch::x64::init_gdt();
    
    println!("[+] Initializing IDT...");
    arch::x64::init_idt();
    
    println!("[+] Initializing PIC...");
    arch::x64::init_pic();

    println!("[+] Initializing PS/2 controller...");
    drivers::keyboard::init_ps2();

    println!("[+] Scanning for USB HID keyboards...");
    drivers::usb_hid::init_usb_keyboard();

    println!("[+] Enabling interrupts...");
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
    println!("[+] Initializing ATA drivers...");
    globals::ATA_DRIVER = Some(AtaDriver::new(AtaChannel::Primary));
    globals::ATA_DRIVER_SECONDARY = Some(AtaDriver::new(AtaChannel::Secondary));

    println!("[+] Scanning PCI for IDE bus-master DMA...");
    if let Some(ide) = pci::find_ide_controller() {
        pci::enable_bus_master(&ide);
        globals::ATA_DRIVER.as_mut().unwrap().init_dma(ide.bus_master_base);
        globals::ATA_DRIVER_SECONDARY.as_mut().unwrap().init_dma(ide.bus_master_base + 8);
        println!("[+] ATA bus-master DMA enabled at BMBA 0x{:04X}", ide.bus_master_base);
    } else {
        println!("[!] No IDE bus-master controller found, using PIO");
    }

    println!("[+] Probing for AHCI controller...");
    let mut ahci_results = drivers::ahci::AhciDriver::probe_all();
    if let Some(ahci) = ahci_results[0].take() {
        let port_count = ahci.port_count;
        globals::AHCI_DRIVER = Some(ahci);
        globals::ATA_DRIVER.as_mut().unwrap().ahci_fallback = true;
        if port_count >= 2 {
            globals::ATA_DRIVER_SECONDARY.as_mut().unwrap().ahci_fallback = true;
            println!("[+] AHCI: {} ports — fallback enabled for both ATA channels", port_count);
        } else {
            println!("[+] AHCI: 1 port — fallback enabled for primary ATA only");
        }
    } else {
        println!("[-] No AHCI controller found");
    }

    let ata = globals::ATA_DRIVER.as_mut().unwrap();
    let ata2 = globals::ATA_DRIVER_SECONDARY.as_mut().unwrap();

    // Scan GPT on both physical disks
    println!("[+] Scanning GPT for NeoDOS partitions (disk 0)...");
    let disk0_parts = gpt::find_all_neodos_partitions(ata);
    println!("[+] Scanning GPT for NeoDOS partitions (disk 1)...");
    let disk1_parts = gpt::find_all_neodos_partitions(ata2);

    if let Some(part) = &disk0_parts[0] {
        println!("[+] Primary NeoDOS partition: LBA {}..{}",
            part.start_lba, part.end_lba);
        ata.set_base_lba(part.start_lba as u32);
    } else {
        println!("[!] No GPT/NeoDOS partition found; assuming LBA 0");
    }

    println!("[+] Initializing Block Cache...");
    globals::BLOCK_CACHE = Some(BlockCache::new());
    let cache = globals::BLOCK_CACHE.as_mut().unwrap();

    println!("[+] Reading Superblock...");
    let sb_data = match ata.read_sector(0) {
        Ok(data) => data,
        Err(_) => panic!("Failed to read superblock"),
    };

    println!("[+] Mounting NeoDOS FS...");
    match NeoDosFs::new(&sb_data) {
        Ok(fs) => {
            globals::NEODOS_FS = Some(fs);
            println!("[+] NeoDOS FS mounted");
    let _ = globals::NEODOS_FS.as_mut().unwrap().rebuild_bitmap(cache, ata);
    println!("[+] Block bitmap rebuilt");
        },
        Err(_) => panic!("Failed to mount filesystem"),
    }

    // Collect extra volumes: extra disk0 partitions + all disk1 partitions
    let mut extra_volumes: [Option<Volume>; 3] = [None, None, None];
    let mut vol_idx = 0;

    for i in 1..gpt::MAX_NEODOS_PARTITIONS {
        if vol_idx >= 3 { break; }
        if let Some(part) = &disk0_parts[i] {
            println!("[+] Extra disk0 volume at LBA {}..{}",
                part.start_lba, part.end_lba);
            if let Ok(vol) = Volume::from_partition(ata, part.start_lba as u32) {
                extra_volumes[vol_idx] = Some(vol);
                println!("[+] Extra volume {} mounted", vol_idx);
                vol_idx += 1;
            }
        }
    }

    for i in 0..gpt::MAX_NEODOS_PARTITIONS {
        if vol_idx >= 3 { break; }
        if let Some(part) = &disk1_parts[i] {
            println!("[+] Disk1 volume at LBA {}..{}",
                part.start_lba, part.end_lba);
            if let Ok(vol) = Volume::from_partition(ata2, part.start_lba as u32) {
                extra_volumes[vol_idx] = Some(vol);
                println!("[+] Extra volume {} mounted (disk 1)", vol_idx);
                vol_idx += 1;
            }
        }
    }

    // Restore ATA base_lba to primary partition on primary disk
    if let Some(part) = &disk0_parts[0] {
        ata.set_base_lba(part.start_lba as u32);
    }

    // ============================================
    // FAT32: Read boot partition
    // ============================================
    println!("[+] Initializing FAT32 driver...");
    globals::FAT32_DRIVER = Fat32Driver::new(ata).ok();

    // ============================================
    // PHASE 6 / PHASE 3: Custom Page Tables & User Memory
    // ============================================
    unsafe {
        arch::x64::paging::init_custom_page_tables();
    }


    drivers::keyboard::set_leds(0b111); // All ON = storage ready

    // ============================================
    // PHASE 4: Start DOS Shell
    // ============================================
    println!("[+] Starting NeoDOS Shell...");

    testing::register_tests();
    
    let mut shell = shell::DosShell::new(
        globals::NEODOS_FS.as_mut().unwrap(),
        globals::BLOCK_CACHE.as_mut().unwrap(),
        globals::ATA_DRIVER.as_mut().unwrap(),
        globals::ATA_DRIVER_SECONDARY.as_mut().unwrap(),
        globals::FAT32_DRIVER.take(),
        extra_volumes
    );
    
    shell.run();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    arch::disable_interrupts();
    println!("\r\n!!! KERNEL PANIC !!!");
    if let Some(location) = info.location() {
        println!("Location: {}:{}", location.file(), location.line());
    }
    println!("Message: {}", info.message());

    arch::halt();
}
