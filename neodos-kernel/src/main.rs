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
mod module_abi;

use drivers::ata::{AtaChannel, AtaDriver};
use drivers::block::BlockDevice;
use drivers::fat32::Fat32Driver;
use drivers::gpt;
use drivers::pci;
use buffer::block_cache::BlockCache;
use fs::neodos_fs::NeoDosFs;
use graphics::FramebufferInfo;

const KERNEL_VERSION: &str = concat!("NeoDOS Kernel v", env!("CARGO_PKG_VERSION"), " - The Rusty DOS Revival");

const BOOTINFO_MAGIC: u32 = 0x4E444F53; // "NDOS" in ASCII
const KERNEL_VERSION_CODE: u32 = ((0 * 256) + 10) << 8 | 5; // v0.10.5

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
    drivers::block::set_ram_disk(boot_info.fs_image_addr, boot_info.fs_image_size);

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

    // ============================================
    // PHASE 2.5: Physical memory map / allocator
    // (must complete before enabling interrupts —
    //  timer IRQ0 can fire immediately after STI)
    // ============================================
    memory::init(boot_info);

    // ============================================
    // PHASE 2.75: Heap allocator (uses identity map)
    // ============================================
    allocator::init();

    println!("[+] Enabling interrupts...");
    arch::x64::enable_interrupts();

    // Initialize kernel service table for Ring-0 modules
    module_abi::init_kernel_service_table();
    println!("[+] Kernel service table @ 0x{:x}", module_abi::KERNEL_SERVICE_TABLE_ADDR);

    // ============================================
    // PHASE 3: Storage stack
    // ============================================
    println!("[+] Initializing ATA drivers...");
    *globals::ATA_DRIVER.lock() = Some(AtaDriver::new(AtaChannel::Primary));
    *globals::ATA_DRIVER_SECONDARY.lock() = Some(AtaDriver::new(AtaChannel::Secondary));

    println!("[+] Scanning PCI for IDE bus-master DMA...");
    if let Some(ide) = pci::find_ide_controller() {
        pci::enable_bus_master(&ide);
        globals::ATA_DRIVER.lock().as_mut().unwrap().init_dma(ide.bus_master_base);
        globals::ATA_DRIVER_SECONDARY.lock().as_mut().unwrap().init_dma(ide.bus_master_base + 8);
        println!("[+] ATA bus-master DMA enabled at BMBA 0x{:04X}", ide.bus_master_base);
    } else {
        println!("[!] No IDE bus-master controller found, using PIO");
    }

    println!("[+] Probing for AHCI controller...");
    let mut ahci_results = drivers::ahci::AhciDriver::probe_all();
    let ahci_port_count = ahci_results[0].as_ref().map(|a| a.port_count).unwrap_or(0);
    if let Some(ahci) = ahci_results[0].take() {
        *globals::AHCI_DRIVER.lock() = Some(ahci);
        println!("[+] AHCI: {} ports — available for BlockDevice fallback", ahci_port_count);
    } else {
        println!("[-] No AHCI controller found");
    }

    let mut ata_lock = globals::ATA_DRIVER.lock();
    let mut ata2_lock = globals::ATA_DRIVER_SECONDARY.lock();
    let ata = ata_lock.as_mut().unwrap();
    let ata2 = ata2_lock.as_mut().unwrap();
    let dev: &mut dyn BlockDevice = ata;
    let _dev2: &mut dyn BlockDevice = ata2;

    // Scan GPT on both physical disks
    println!("[+] Scanning GPT for NeoDOS partitions (disk 0)...");
    let disk0_parts = gpt::find_all_neodos_partitions(dev);
    println!("[+] Scanning GPT for NeoDOS partitions (disk 1)...");
    let _disk1_parts = gpt::find_all_neodos_partitions(_dev2);

    if let Some(part) = &disk0_parts[0] {
        println!("[+] Primary NeoDOS partition: LBA {}..{}",
            part.start_lba, part.end_lba);
        dev.set_base_lba(part.start_lba);
    } else {
        println!("[!] No GPT/NeoDOS partition found; assuming LBA 0");
    }

    println!("[+] Initializing Block Cache...");
    *globals::BLOCK_CACHE.lock() = Some(BlockCache::new());
    let mut cache_lock = globals::BLOCK_CACHE.lock();
    let cache = cache_lock.as_mut().unwrap();

    println!("[+] Reading Superblock...");
    let sb_data = match dev.read_sector(0) {
        Ok(data) => data,
        Err(_) => panic!("Failed to read superblock"),
    };

    println!("[+] Mounting NeoDOS FS...");
    match NeoDosFs::new(&sb_data) {
        Ok(mut fs) => {
            let _ = fs.rebuild_bitmap(cache, dev);
            crate::globals::with_vfs(|vfs| {
                vfs.mount('C', alloc::boxed::Box::new(fs)).unwrap();
            });
            println!("[+] NeoDOS FS mounted on C:");
        },
        Err(_) => panic!("Failed to mount filesystem"),
    }

    // Restore ATA base_lba to primary partition on primary disk
    if let Some(part) = &disk0_parts[0] {
        dev.set_base_lba(part.start_lba);
    }

    // Explicitly drop locks before FAT32 and Shell
    core::mem::drop(cache_lock);
    core::mem::drop(ata_lock);
    core::mem::drop(ata2_lock);

    // ============================================
    // FAT32: Read boot partition
    // ============================================
    println!("[+] Initializing FAT32 driver...");
    if let Some(mut ata_lock) = globals::ATA_DRIVER.try_lock() {
        if let Some(ata) = ata_lock.as_mut() {
            let dev: &mut dyn BlockDevice = ata;
            if let Ok(fat32) = Fat32Driver::new(dev) {
                crate::globals::with_vfs(|vfs| {
                    let _ = vfs.mount('A', alloc::boxed::Box::new(fat32));
                });
                println!("[+] FAT32 ESP mounted on A:");
            }
        }
    }

    // ============================================
    // PHASE 6 / PHASE 3: Custom Page Tables & User Memory
    // ============================================
    unsafe {
        arch::x64::paging::init_custom_page_tables();
    }

    // Split heap region huge pages into 4 KB PTs for demand paging
    arch::x64::paging::init_heap_demand_paging();

    drivers::keyboard::set_leds(0b111); // All ON = storage ready

    // ============================================
    // PHASE 4: Start DOS Shell
    // ============================================
    testing::register_tests();
    
    let mut shell = shell::DosShell::new();
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
