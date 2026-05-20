#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![allow(static_mut_refs)]

extern crate alloc;
use core::panic::PanicInfo;

mod allocator;
mod arch;
mod hal;
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
mod nem;
mod devices;
mod memory;
mod globals;
pub mod usermode;
pub mod syscall;
mod testing;
pub mod trace;
pub mod invariants;
pub mod panic_classification;

use drivers::fat32::Fat32Driver;
use drivers::gpt;
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
    hal::enable_interrupts();

    // ============================================
    // PHASE 3: Storage stack
    // ============================================
    drivers::storage_manager::init_storage();
    let primary_idx = 0;

    // ── NeoDOS FS via BlockDeviceManager ──
    println!("[+] Initializing Block Cache...");
    *globals::BLOCK_CACHE.lock() = Some(BlockCache::new());

    {
        let mut bdevs = globals::BLOCK_DEVICES.lock();
        let dev = bdevs.get(primary_idx)
            .expect("Primary block device vanished");

        // Scan GPT on primary disk
        println!("[+] Scanning GPT for NeoDOS partitions (disk 0)...");
        let disk0_parts = gpt::find_all_neodos_partitions(dev);

        if let Some(part) = &disk0_parts[0] {
            println!("[+] Primary NeoDOS partition: LBA {}..{}",
                part.start_lba, part.end_lba);
            dev.set_base_lba(part.start_lba);
        } else {
            println!("[!] No GPT/NeoDOS partition found; assuming LBA 0");
        }

        let mut cache_lock = globals::BLOCK_CACHE.lock();
        let cache = cache_lock.as_mut().expect("BlockCache not initialized");

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
                    if let Err(e) = vfs.mount('C', alloc::boxed::Box::new(fs)) {
                        panic!("Failed to mount C: {:?}", e);
                    }
                });
                println!("[+] NeoDOS FS mounted on C:");
            },
            Err(_) => panic!("Failed to mount filesystem"),
        }

        // Restore base_lba to primary partition
        if let Some(part) = &disk0_parts[0] {
            dev.set_base_lba(part.start_lba);
        }

        core::mem::drop(cache_lock);
    }

    // ============================================
    // FAT32: via BlockDeviceManager (with ATA fallback)
    // ============================================
    println!("[+] Initializing FAT32 driver...");
    let fat32_mounted = if let Ok(fat32) = Fat32Driver::new() {
        crate::globals::with_vfs(|vfs| {
            let _ = vfs.mount('A', alloc::boxed::Box::new(fat32));
        });
        true
    } else {
        false
    };
    if fat32_mounted {
        println!("[+] FAT32 ESP mounted on A:");
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
    // PHASE 3.5: Device Model + HAL Binding Layer
    // ============================================
    devices::register_boot_devices();

    // ============================================
    // PHASE 4: Start DOS Shell
    // ============================================
    testing::register_tests();
    
    let mut shell = shell::DosShell::new();
    shell.run();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    hal::disable_interrupts();

    let class = crate::panic_classification::current_panic_class();
    println!("\r\n!!! KERNEL PANIC (CLASS: {}) !!!", class.to_str());
    if let Some(location) = info.location() {
        println!("Location: {}:{}", location.file(), location.line());
    }
    println!("Message: {}", info.message());

    // Dump forensic info to serial (println may fail if framebuffer is corrupt)
    crate::panic_classification::dump_forensic_info();

    hal::halt();
}
