#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![allow(static_mut_refs)]
#![allow(dead_code)]

extern crate alloc;
use core::panic::PanicInfo;

mod allocator;
mod slab;
mod arch;
mod hal;
mod console;
mod cpu;
pub mod scheduler;
mod processes;
mod drivers;
mod buffer;
mod fs;
mod vfs;
mod input;
mod shell;
mod graphics;
mod font;
mod nem;
mod elf;
mod handle;
mod pipe;
mod eventbus;
mod work_queue;
mod dpc;
mod kobj;
mod memory;
mod globals;
pub mod usermode;
pub mod syscall;
mod nxl;
mod apc;
mod irp;
mod interrupts;
mod timers;
mod testing;
pub mod trace;
pub mod invariants;
pub mod panic_classification;
pub mod boot_benchmark;
mod crash;

use drivers::fat32::Fat32Driver;
use drivers::gpt;
use buffer::block_cache::BlockCache;
use fs::neodos_fs::NeoDosFs;
use graphics::FramebufferInfo;
use vfs::partition::{PartitionInfo, PART_TYPE_NEODOS, PART_TYPE_ESP};
use vfs::io::{IoStack, PageCacheLevel};

pub const KERNEL_VERSION: &str = concat!("NeoDOS Kernel v", env!("CARGO_PKG_VERSION"), " - The Rusty DOS Revival");

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
    pub acpi_rsdp_addr: u64,  // ACPI RSDP physical address (0 if not found)
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
    drivers::ps2::set_leds(0b100); // Caps Lock ON = kernel entry

    // 1b. Set up RAM disk from bootloader-loaded FS image
    drivers::block::set_ram_disk(boot_info.fs_image_addr, boot_info.fs_image_size);

    // 2. Setup Serial for output
    arch::x64::init_serial();

    // ── Boot Benchmark: calibrate TSC and mark kernel entry ──
    boot_benchmark::init();
    boot_benchmark::mark(boot_benchmark::BootStage::KernelEntry);
    boot_benchmark::watchdog_arm();

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
    drivers::ps2::set_leds(0b110); // Caps Lock + Num Lock ON = console ready

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
    
    println!("[+] Initializing MSI subsystem...");
    interrupts::msi::init();
    
    println!("[+] Initializing PIC...");
    arch::x64::init_pic();

    println!("[+] Initializing HPET / timers...");
    unsafe {
        timers::BOOT_RSDP_ADDR = boot_info.acpi_rsdp_addr;
    }
    if boot_info.acpi_rsdp_addr != 0 {
        println!("[+] ACPI RSDP at 0x{:x}", boot_info.acpi_rsdp_addr);
    }
    timers::init();
    if timers::active() == timers::TimerSource::Hpet {
        println!("[+] Initializing APIC timer...");
        if timers::apic::init_apic_timer() {
            timers::set_active(timers::TimerSource::ApicTimer);
            println!("[+] APIC timer active ({} KHz bus)", timers::apic::apic_bus_khz());
        } else {
            println!("[+] APIC timer not available, using HPET");
        }
    }

    println!("[+] Initializing PS/2 controller...");
    drivers::ps2::init_ps2();

    println!("[+] Scanning for USB HID keyboards...");
    drivers::usb_hid::init_usb_keyboard();

    // ============================================
    // PHASE 2.5: Physical memory map / allocator
    // (must complete before enabling interrupts —
    //  timer IRQ0 can fire immediately after STI)
    // ============================================
    memory::init(boot_info);

    // Initialize crash dump area (reserve 16 MB @ 0x0F000000)
    crash::init_crash_dump_area();
    memory::reserve_range(crash::CRASH_DUMP_AREA_BASE, crash::CRASH_DUMP_AREA_SIZE);

    // ============================================
    // PHASE 2.75: Heap allocator (uses identity map)
    // ============================================
    allocator::init();

    // ============================================
    // PHASE 2.76: Object Manager (Ob) namespace
    // Create root \ and standard directories.
    // ============================================
    //println!("[+] Initializing Object Manager namespace...");
    //kobj::namespace::init_object_namespace();

    // ============================================
    // PHASE 2.8: SMP — Start Application Processors
    // ============================================
    println!("[+] Initializing SMP (per-CPU data structures)...");
    let cpu_count = arch::x64::smp::init_smp();
    println!("[+] {} CPU(s) online", cpu_count);

    // ============================================
    // PHASE 2.9: IPI infrastructure
    // ============================================
    arch::x64::ipi::init();

    println!("[+] Enabling interrupts...");
    hal::enable_interrupts();

    // ============================================
    // PHASE 6 / PHASE 3: Custom Page Tables & User Memory
    // ============================================
    unsafe {
        arch::x64::paging::init_custom_page_tables();
    }

    // Split heap region huge pages into 4 KB PTs for demand paging
    arch::x64::paging::init_heap_demand_paging();
    // Split mmap region huge pages for lazy file/anonymous mapping
    arch::x64::paging::init_mmap_demand_paging();

    // ============================================
    // PHASE 3 (after custom page tables): Storage stack
    // ============================================
    boot_benchmark::mark(boot_benchmark::BootStage::StorageInit);
    boot_benchmark::watchdog_enter_stage(boot_benchmark::BootStage::StorageInit);
    if boot_benchmark::watchdog_check() {
        serial_println!("[WATCHDOG] Timeout before storage init!");
    }
    drivers::storage_manager::init_storage();
    boot_benchmark::mark(boot_benchmark::BootStage::StorageReady);
    boot_benchmark::watchdog_enter_stage(boot_benchmark::BootStage::StorageReady);
    let primary_idx = 0;

    // ── A5.1: Create IoStacks from GPT ──
    println!("[+] Initializing Block Cache...");
    *globals::BLOCK_CACHE.lock() = Some(BlockCache::new());
    println!("[+] Initializing Page Cache (128 × 4 KB = 512 KB, hash + LRU)...");

    println!("[+] Scanning GPT for partitions...");
    let (neodos_io, esp_io) = {
        let mut bdevs = globals::BLOCK_DEVICES.lock();
        let dev = bdevs.get(primary_idx)
            .expect("Primary block device vanished");

        // Find both NeoDOS and ESP partitions
        let disk0_parts = gpt::find_all_neodos_partitions(dev);
        let esp_parts = gpt::find_all_esp_partitions(dev);

        let neodos_part = disk0_parts[0].map(|p| {
            PartitionInfo::new(p.start_lba, p.end_lba - p.start_lba, PART_TYPE_NEODOS)
        });
        let esp_part = esp_parts[0].map(|p| {
            PartitionInfo::new(p.start_lba, p.end_lba - p.start_lba, PART_TYPE_ESP)
        });

        if let Some(ref part) = neodos_part {
            println!("[+] NeoDOS partition: LBA {}..{} ({} sectors)",
                part.base_lba, part.base_lba + part.sector_count, part.sector_count);
        } else {
            println!("[!] No GPT/NeoDOS partition found; assuming LBA 0");
        }
        if let Some(ref part) = esp_part {
            println!("[+] ESP partition: LBA {}..{} ({} sectors)",
                part.base_lba, part.base_lba + part.sector_count, part.sector_count);
        } else {
            println!("[!] No ESP partition found");
        }

        let neodos_io = match neodos_part {
            Some(p) => IoStack::with_partition(primary_idx, p, PageCacheLevel::L1),
            None => IoStack::new(primary_idx),
        };
        let esp_io = match esp_part {
            Some(p) => IoStack::with_partition(primary_idx, p, PageCacheLevel::L1),
            None => IoStack::new(primary_idx),
        };

        (neodos_io, esp_io)
    };

    // Store partition base for shell commands (FSCK etc.)
    if let Some(ref part) = neodos_io.partition {
        globals::PRIMARY_PARTITION_BASE.store(part.base_lba, core::sync::atomic::Ordering::Relaxed);
    }

    // ── NeoDOS FS via IoStack ──
    println!("[+] Reading Superblock...");
    if boot_benchmark::watchdog_check() {
        serial_println!("[WATCHDOG] Timeout before first read!");
    }
    let sb_data = match neodos_io.read_sector(0) {
        Ok(data) => {
            boot_benchmark::mark(boot_benchmark::BootStage::FirstRead);
            boot_benchmark::watchdog_enter_stage(boot_benchmark::BootStage::FirstRead);
            data
        },
        Err(_) => panic!("Failed to read superblock"),
    };

    println!("[+] Mounting NeoDOS FS...");
    if boot_benchmark::watchdog_check() {
        serial_println!("[WATCHDOG] Timeout before FS mount!");
    }
    match NeoDosFs::new(&sb_data, neodos_io) {
        Ok(mut fs) => {
            let _ = fs.rebuild_bitmap_with_io();
            crate::globals::with_vfs(|vfs| {
                if let Err(e) = vfs.mount('C', alloc::boxed::Box::new(fs)) {
                    panic!("Failed to mount C: {:?}", e);
                }
            });
            boot_benchmark::mark(boot_benchmark::BootStage::FsMounted);
            boot_benchmark::watchdog_enter_stage(boot_benchmark::BootStage::FsMounted);
            println!("[+] NeoDOS FS mounted on C:");
        },
        Err(_) => panic!("Failed to mount filesystem"),
    }

    // ============================================
    // FAT32: via IoStack
    // ============================================
    println!("[+] Initializing FAT32 driver...");
    let fat32_mounted = if let Ok(fat32) = Fat32Driver::new(esp_io) {
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

    drivers::ps2::set_leds(0b111); // All ON = storage ready

    // ============================================
    // PHASE 3.75: Driver Runtime + Built-in Drivers
    // ============================================
    drivers::builtin_drivers::init();

    // ============================================
    // PHASE 3.80: X4 — Driver Isolation Layer
    // Initialize isolated driver region at 0x30000000
    // for loading NEM drivers with restricted permissions.
    // ============================================
    println!("[+] Initializing Driver Isolation Layer...");
    drivers::isolation::init_isolated_region();

    // ============================================
    // PHASE 3.85: Boot Driver Loader
    // Auto-scan and load BOOT .nem drivers first,
    // then SYSTEM .nem drivers.
    // ============================================
    println!("[+] Initializing Boot Driver Loader...");
    drivers::boot_loader::boot_load_all();

    // ============================================
    // PHASE 3.86: Reclaim AHCI port after NEM driver init
    // The NEM AHCI driver's port_init() overwrites PORT_CLB/PORT_FB
    // with its own buffer addresses, breaking BootAhci DMA.
    // ============================================
    crate::drivers::boot_ahci::BootAhci::reclaim_ahci_port();

    // ============================================
    // PHASE 3.87: Initialise NEM driver bridges + DLL loader
    // ============================================
    drivers::rtc_bridge::init();
    nxl::init_nxl_region();
    drivers::hotreload::init_hot_reload();
    nxl::load_nxl();

    // ============================================
    // PHASE 3.9: Validate syscall ABI
    // ============================================
    println!("[+] Validating syscall ABI...");
    syscall::validate_abi();

    // ============================================
    // PHASE 4: Start DOS Shell
    // ============================================
    testing::register_tests();

    // ── Boot Benchmark: shell ready ──
    boot_benchmark::mark(boot_benchmark::BootStage::ShellReady);

    // Load benchmark configuration from BOOT.CFG (now that VFS is mounted)
    boot_benchmark::load_config();

    // Detect which storage driver was selected
    let driver_name: &'static str = {
        let bdevs = globals::BLOCK_DEVICES.lock();
        // storage_manager priority: NVMe > AHCI > ATA
        if bdevs.count() > 0 {
            // Check AHCI debug counters to see if AHCI was used
            if boot_benchmark::AHCI_COMMANDS.load(core::sync::atomic::Ordering::Relaxed) > 0 {
                "AHCI.NEM"
            } else {
                "ATA.PIO"
            }
        } else {
            "UNKNOWN"
        }
    };
    boot_benchmark::print_report(driver_name);

    // ── PHASE 4: NeoInit (PID 1, Ring 3) ──
    // Loads NEOINIT.NXE as the init process. NeoInit spawns NEOSHELL.NXE
    // via sys_spawn (RAX=7). When the shell exits, sys_spawn restores
    // NeoInit's code and returns, and NeoInit respawns the shell.
    // Set NEOINIT=0 in C:\System\Kernel\boot.cfg to skip NeoInit for testing.
    if !boot_benchmark::NEOINIT_ENABLED.load(core::sync::atomic::Ordering::Relaxed) {
        println!("[+] NeoInit disabled (BOOT.CFG). Starting kernel shell.");
        let mut sh = shell::DosShell::new();
        sh.run();
    }
    println!("[+] Loading NeoInit (PID 1, Ring 3)...");

    let mut addr_space = scheduler::address_space::AddressSpace::new();
    let (entry, loaded) = {
        static mut BIN_BUF: [u8; 65536] = [0u8; 65536];
        let mut entry: u64 = 0;
        let mut loaded = false;
        crate::globals::with_vfs(|vfs| {
            if let Ok((drive_idx, node)) = vfs.resolve_path("C:\\Programs\\NeoInit.nxe") {
                if (node.mode & fs::vfs::MODE_FILE) == 0 { return; }
                let size = unsafe {
                    match vfs.read(drive_idx, node.inode, 0, &mut BIN_BUF) {
                        Ok(n) => n,
                        Err(_) => 0,
                    }
                };
                crate::serial_println!(
                    "[NEOINIT] resolved inode={} size={} mode=0x{:04x} read={} bytes",
                    node.inode,
                    node.size,
                    node.mode,
                    size
                );
                if size < 4 {
                    crate::serial_println!("[NEOINIT] file read too small");
                    return;
                }
                let data = unsafe { &BIN_BUF[..size] };
                match elf::load_elf(data, Some(&mut addr_space)) {
                    Ok(r) => {
                        entry = r.entry;
                        loaded = true;
                        crate::serial_println!("[NEOINIT] ELF load OK: entry=0x{:x}", entry);
                    }
                    Err(err) => {
                        crate::serial_println!("[NEOINIT] ELF load failed: {:?}", err);
                    }
                }
            } else {
                crate::serial_println!("[NEOINIT] path not found: C:\\Programs\\NeoInit.nxe");
            }
        });
        (entry, loaded)
    };

    if !loaded {
        println!("[!] NEOINIT.NXE not found or invalid. Starting kernel shell.");
        let mut sh = shell::DosShell::new();
        sh.run();
    }

    let slot = match arch::x64::paging::alloc_user_slot() {
        Some(s) => s,
        None => {
            println!("[!] No free user slots. Starting kernel shell.");
            let mut sh = shell::DosShell::new();
            sh.run();
        }
    };

    let pid = usermode::spawn_usermode(
        entry, slot.stack_top, slot.slot_idx, 2, "\\", 0,
    );

    hal::without_interrupts(|| {
        if let Some(eproc) = scheduler::current_scheduler().lock().find_eprocess_mut(pid) {
            eproc.address_space = addr_space;
        }
    });

    println!("[+] NeoInit PID {} entered at entry=0x{:x}", pid, entry);

    // Enter NeoInit (blocks until NeoInit exits, which it shouldn't)
    usermode::wait_for_process(pid);

    // If we get here, NeoInit exited (shouldn't happen)
    println!("[!] NeoInit PID {} exited! This should not occur.", pid);
    scheduler::cleanup_terminated_process(pid);
    let mut sh = shell::DosShell::new();
    sh.run();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    hal::disable_interrupts();

    let class = crate::panic_classification::current_panic_class();
    println!("\r\n!!! KERNEL PANIC (CLASS: {}) !!!", class.to_str());

    // Capture approximate RIP from return address on stack, and RSP
    let rsp: u64 = unsafe { crate::hal::raw::raw_read_rsp() };
    let rip: u64 = unsafe { (rsp as *const u64).read() };

    // Dump crash dump to serial + RAM buffer (must happen before any other output)
    crate::crash::dump_panic(rip, rsp);

    if let Some(location) = info.location() {
        println!("Location: {}:{}", location.file(), location.line());
    }
    println!("Message: {}", info.message());

    // Dump forensic info to serial (println may fail if framebuffer is corrupt)
    crate::panic_classification::dump_forensic_info();

    hal::halt();
}
