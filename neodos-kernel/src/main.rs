#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]
#![allow(static_mut_refs)]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

extern crate alloc;
use core::panic::PanicInfo;

#[macro_use]
pub mod log;

mod allocator;
mod slab;
mod arch;
mod hal;
mod console;
mod cpu;
pub mod scheduler;
mod drivers;
mod buffer;
mod fs;
mod vfs;
mod input;
mod graphics;
mod font;
mod nem;
mod elf;
mod handle;
mod eventbus;
mod work_queue;
mod dpc;

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
mod watchdog;  // A3.3 Watchdog subsystem
mod crash;
mod security;
mod exception;  // A3.4 SEH + Exception Dispatcher
mod urn;
pub mod power;
mod object;
mod kwait;
mod net;
mod cm;
mod services;
mod virtio;
mod kbd;
mod abi_freeze;

use drivers::fat32::Fat32Driver;
use drivers::gpt;
use fs::neodos_v2::NeoDosFsV2;
use graphics::FramebufferInfo;
use vfs::partition::{PartitionInfo, PART_TYPE_NEODOS, PART_TYPE_ESP};
use vfs::io::{IoStack, PageCacheLevel};
use crate::log::LogSubsys;

pub const KERNEL_VERSION: &str = concat!("NeoDOS Kernel v", env!("CARGO_PKG_VERSION"), " - The Rusty DOS Revival");

const BOOTINFO_MAGIC: u32 = 0x4E444F53; // "NDOS" in ASCII
const KERNEL_VERSION_CODE: u32 = (10) << 8 | 5; // v0.10.5

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
/// # Safety
///
/// This function is called directly by the bootloader after exiting UEFI boot services.
/// It must only be called once, with a valid `BootInfo` pointer provided by the bootloader.
/// The caller must ensure that the boot info structure is correctly initialized and that
/// the system is in a state suitable for kernel initialization (long mode enabled, page
/// tables set up, etc.).
pub unsafe extern "sysv64" fn rust_start(boot_info: &BootInfo) -> ! {
    // 0. Verify boot info magic and version
    if boot_info.magic != BOOTINFO_MAGIC {
        // Too early for serial — signal via keyboard LEDs
        drivers::ps2::set_leds(0b001); // Scroll Lock ON = boot magic mismatch
        crate::hal::halt();
    }

    // 1. Initialize Graphics Renderer
    graphics::init(boot_info.fb_info);
    drivers::ps2::set_leds(0b100); // Caps Lock ON = kernel entry

    // 1b. Set up RAM disk from bootloader-loaded FS image
    drivers::block::set_ram_disk(boot_info.fs_image_addr, boot_info.fs_image_size);

    // 2. Setup Serial for output
    arch::x64::init_serial();
    log::init();

    // ── Boot Benchmark: calibrate TSC and mark kernel entry ──
    boot_benchmark::init();
    boot_benchmark::mark(boot_benchmark::BootStage::KernelEntry);
    boot_benchmark::watchdog_arm();

    // Check bootloader version compatibility
    let bootloader_version = boot_info.version;
    if bootloader_version != KERNEL_VERSION_CODE {
        kwarn!(LogSubsys::Kernel, "Version mismatch: bootloader v{:x}, kernel v{:x}",
            bootloader_version, KERNEL_VERSION_CODE);
    } else {
        kinfo!(LogSubsys::Kernel, "Bootloader version: v0.10.1 (compatible)");
    }

    kinfo!(LogSubsys::Kernel, "Graphics initialized: {}x{}", boot_info.fb_info.width, boot_info.fb_info.height);

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
    // ACPI power state (FADT, S5, reset register)
    power::acpi::init();
    if power::acpi::is_available() {
        println!("[+] ACPI power management initialized");
    } else {
        println!("[!] ACPI power management not available");
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


    // ============================================
    // PHASE 2.5: Physical memory map / allocator
    // (must complete before enabling interrupts —
    //  timer IRQ0 can fire immediately after STI)
    // ============================================
    memory::init(boot_info);

    // Initialize crash dump area (reserve 16 MB @ 0x0F000000)
    crash::init_crash_dump_area();
    memory::reserve_range(crash::CRASH_DUMP_AREA_BASE, crash::CRASH_DUMP_AREA_SIZE);

    // A3.3: Initialize watchdog subsystem (requires HPET + crash dump area)
    watchdog::init_watchdog();

    // ============================================
    // PHASE 2.75: Heap allocator (uses identity map)
    // ============================================
    allocator::init();

    // ============================================
    // PHASE 2.759: Object Manager (Ob) — new base module
    // Replaces KOBJ progressively. Must init before namespace.
    // ============================================
    object::init_object_manager();

    // ============================================
    // PHASE 2.7595: Timer Manager (Ob Timer Object)
    // ============================================
    object::timer::init_timer_manager();

    // ============================================
    // PHASE 2.76: Object Manager (Ob) namespace
    // Create root \ and standard directories.
    // ============================================
    println!("[+] Initializing Object Manager namespace...");
    object::namespace::init_object_namespace();

    // Create virtual info objects in Ob namespace (Memory, Interrupts)
    {
        use crate::object::{self, ObType};
        let _ = object::namespace::ob_create_directory("\\Global\\Info");
        if let Ok(mem_id) = object::ob_create_object(ObType::Key, "Memory", 1, 0, None) {
            let _ = object::namespace::ob_insert_object("\\Global\\Info\\Memory", mem_id);
        }
        if let Ok(int_id) = object::ob_create_object(ObType::Key, "Interrupts", 2, 0, None) {
            let _ = object::namespace::ob_insert_object("\\Global\\Info\\Interrupts", int_id);
        }
        if let Ok(cpu_id) = object::ob_create_object(ObType::Key, "CpuInfo", 3, 0, None) {
            let _ = object::namespace::ob_insert_object("\\Global\\Info\\CpuInfo", cpu_id);
        }
        if let Ok(ver_id) = object::ob_create_object(ObType::Key, "Version", 4, 0, None) {
            let _ = object::namespace::ob_insert_object("\\Global\\Info\\Version", ver_id);
        }
        if let Ok(dt_id) = object::ob_create_object(ObType::Key, "DateTime", 5, 0, None) {
            let _ = object::namespace::ob_insert_object("\\Global\\Info\\DateTime", dt_id);
        }
        if let Ok(drv_id) = object::ob_create_object(ObType::Key, "Drives", 6, 0, None) {
            let _ = object::namespace::ob_insert_object("\\Global\\Info\\Drives", drv_id);
        }
        if let Ok(drv_id) = object::ob_create_object(ObType::Key, "Drivers", 7, 0, None) {
            let _ = object::namespace::ob_insert_object("\\Global\\Info\\Drivers", drv_id);
        }
        if let Ok(cwd_id) = object::ob_create_object(ObType::Key, "Cwd", 8, 0, None) {
            let _ = object::namespace::ob_insert_object("\\Global\\Info\\Cwd", cwd_id);
        }
        if let Ok(kbd_id) = object::ob_create_object(ObType::Key, "Keyboard", 9, 0, None) {
            let _ = object::namespace::ob_insert_object("\\Global\\Info\\Keyboard", kbd_id);
        }
        if let Ok(vt_id) = object::ob_create_object(ObType::Key, "VtInfo", 11, 0, None) {
            let _ = object::namespace::ob_insert_object("\\Global\\Info\\VtInfo", vt_id);
        }
        if let Ok(net_id) = object::ob_create_object(ObType::Key, "Network", 10, 0, None) {
            let _ = object::namespace::ob_insert_object("\\Global\\Info\\Network", net_id);
        }
        if let Ok(proc_id) = object::ob_create_object(ObType::Key, "Process", 12, 0, None) {
            let _ = object::namespace::ob_insert_object("\\Global\\Info\\Process", proc_id);
        }
    }

    // ============================================
    // PHASE 2.765: Power Manager initialization
    // Registers \System\PowerManager as a persistent Ob object.
    // ============================================
    println!("[+] Initializing Power Manager...");
    object::power::init_power_manager();

    // ============================================
    // PHASE 2.77: Security subsystem initialization
    // Creates default admin/user tokens for process identity.
    // ============================================
    println!("[+] Initializing Security subsystem...");
    security::init_security();

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

    // ============================================
    // PHASE 2.91: I/O APIC initialization
    // Detect from MADT, disable legacy PIC, route ISA IRQs.
    // ============================================
    if interrupts::ioapic::init() {
        println!("[+] I/O APIC active, legacy PIC disabled");
    } else {
        println!("[!] I/O APIC not found, using legacy PIC");
    }

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
    // PHASE 6.1: TEB page mapping (A3.4 SEH)
    // Split first 2MB huge page and map TEB at 0x7000
    // as USER_ACCESSIBLE for user-mode exception handling.
    // Requires custom page tables to be active.
    // ============================================
    exception::init_teb_paging();

    // ============================================
    // PHASE 2.3 (after custom page tables): PCIe ECAM
    // Read MCFG from ACPI, map ECAM MMIO region as UC-.
    // ============================================
    drivers::pci::init_ecam();

    // ============================================
    // PHASE 3 (after custom page tables): Storage stack
    // ============================================
    boot_benchmark::mark(boot_benchmark::BootStage::StorageInit);
    boot_benchmark::watchdog_enter_stage(boot_benchmark::BootStage::StorageInit);
    if boot_benchmark::watchdog_check() {
        kerror!(LogSubsys::Watchdog, "Timeout before storage init!");
    }
    drivers::storage_manager::init_storage();
    boot_benchmark::mark(boot_benchmark::BootStage::StorageReady);
    boot_benchmark::watchdog_enter_stage(boot_benchmark::BootStage::StorageReady);
    let primary_idx = 0;

    // ── A5.1: Create IoStacks from GPT ──
    println!("[+] Initializing unified Page Cache (128 × 4 KB = 512 KB, hash + LRU)...");
    // VFS-5.1: Unified cache initialized eagerly via globals::PAGE_CACHE (const fn).

    // ── Boot tolerance: attempt storage init; warn on failure but continue ──
    println!("[+] Scanning GPT for partitions...");
    let (neodos_io, esp_io) = {
        let mut bdevs = globals::BLOCK_DEVICES.lock();
        let dev = match bdevs.get(primary_idx) {
            Some(dev) => dev,
            None => {
                kerror!(LogSubsys::Kernel, "Primary block device not found at index {}", primary_idx);
                kerror!(LogSubsys::Kernel, "Cannot continue without storage. Halting.");
                crate::hal::halt();
            }
        };

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
        kerror!(LogSubsys::Watchdog, "Timeout before first read!");
    }
    let sb_data = match neodos_io.read_sector(0) {
        Ok(data) => {
            boot_benchmark::mark(boot_benchmark::BootStage::FirstRead);
            boot_benchmark::watchdog_enter_stage(boot_benchmark::BootStage::FirstRead);
            data
        },
        Err(e) => {
            kerror!(LogSubsys::Kernel, "Failed to read superblock: {:?}", e);
            kerror!(LogSubsys::Kernel, "Storage device may be unresponsive or corrupt.");
            kerror!(LogSubsys::Kernel, "Cannot continue without filesystem. Halting.");
            crate::hal::halt();
        }
    };

    println!("[+] Mounting NeoDOS FS...");
    if boot_benchmark::watchdog_check() {
        kerror!(LogSubsys::Watchdog, "Timeout before FS mount!");
    }

    // Detectar formato: NE2 (NeoFS v2) o desconocido
    let magic = u32::from_le_bytes(sb_data[0..4].try_into().unwrap());
    let mut fs_type_label = "?";
    let mount_result: Result<(), &'static str> = if magic == 0x0032454E {
        // NE2 — NeoFS v2
        match NeoDosFsV2::new(neodos_io) {
            Ok(fs) => {
                fs_type_label = "NE2";
                let boxed = alloc::boxed::Box::new(fs);
                vfs::mount::vfs_mount_filesystem(
                    "\\Device\\NeoDosVolume0",
                    'C',
                    boxed,
                    vfs::mount::FilesystemType::NeoDosFs,
                ).map(|_| ())
            }
            Err(e) => {
                kwarn!(LogSubsys::Kernel, "NE2 mount failed: {:?}", e);
                Err("NE2 mount failed")
            }
        }
    } else if magic == 0x4F444F4E {
        // NEOD — NeoFS v1 (obsolete)
        kwarn!(LogSubsys::Kernel, "NeoFS v1 (NEOD) is obsolete and no longer supported");
        Err("NeoFS v1 is obsolete")
    } else {
        kwarn!(LogSubsys::Kernel, "Unknown filesystem magic: 0x{:08X}", magic);
        Err("unsupported filesystem format")
    };

    match mount_result {
        Ok(()) => {
            boot_benchmark::mark(boot_benchmark::BootStage::FsMounted);
            boot_benchmark::watchdog_enter_stage(boot_benchmark::BootStage::FsMounted);
            println!("[+] {} filesystem mounted on C:", fs_type_label);
        }
        Err(e) => {
            kerror!(LogSubsys::Kernel, "Failed to mount filesystem: {}", e);
            kerror!(LogSubsys::Kernel, "Filesystem may be corrupt or in an unsupported format.");
            kerror!(LogSubsys::Kernel, "Cannot continue. Halting.");
            crate::hal::halt();
        }
    }



    // ============================================
    // FAT32: via IoStack
    // ============================================
    println!("[+] Initializing FAT32 driver...");
    let fat32_mounted = if let Ok(fat32) = Fat32Driver::new(esp_io) {
        vfs::mount::vfs_mount_filesystem(
            "\\Device\\EspVolume0",
            'A',
            alloc::boxed::Box::new(fat32),
            vfs::mount::FilesystemType::Fat32,
        ).is_ok()
    } else {
        false
    };
    if fat32_mounted {
        println!("[+] FAT32 ESP mounted on A:");
    }

    // ============================================
    // NT5.6: Mount K:\ virtual kernel object drive
    // ============================================

    drivers::ps2::set_leds(0b111); // All ON = storage ready

    // A4.4: Initialize Input Manager (VT subsystem)
    input::init();

    // ============================================
    // PHASE 3.875: Keyboard Manager (NeoKBD)
    // Initializes ObType::KeyboardDevice at \Device\Keyboard,
    // loads layouts from C:\System\Keyboard\*.kbd,
    // reads config from Registry, and registers
    // event handler for keyboard input.
    // ============================================
    kbd::kbd_init();

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
    // PHASE 3.88: Networking subsystem initialization
    // Creates \Device\Tcp and \Device\Udp namespace objects.
    // NICs are registered by NEM drivers (e.g. e1000.nem).
    // ============================================
    println!("[+] Initializing networking subsystem...");
    net::init_networking();

    // ============================================
    // PHASE 3.881: Cm (Configuration Manager) — Registry
    // Initializes the \Registry namespace tree and mounts
    // the SYSTEM hive for persistent configuration storage.
    // ============================================
    println!("[+] Initializing Configuration Manager (Registry)...");
    cm::init_cm();

    // ============================================
    // PHASE 3.881b: Default registry values for boot
    // Creates CurrentControlSet\Services\NeoInit\DefaultShell,
    // Network\Interfaces\0\DHCPEnabled, Control\WaitForNetwork, etc.
    // Only sets values if they don't already exist.
    // ============================================
    // Default registry values are pre-generated by gen_system_hiv.py
    // Fallback: inject critical boot defaults into empty/corrupted hive
    cm::init::ensure_boot_defaults();
    // ============================================
    cm::init::ensure_language_default();
    // ============================================
    // PHASE 3.882: Service Manager (Sm) init
    // Loads configured services from Registry,
    // creates \Service\ namespace, resolves deps.
    // ============================================
    println!("[+] Initializing Service Manager...");
    services::sm_init();

    // ============================================
    // PHASE 3.883: Power Manager runtime init
    // Initializes PowerManager singleton, loads
    // active plan and policies from registry
    // (defaults pre-populated by gen_system_hiv.py).
    // ============================================
    println!("[+] Initializing Power Manager runtime...");
    power::init_power_manager();

    // ============================================
    // PHASE 3.9: Validate syscall ABI + ABI freeze
    // ============================================
    println!("[+] Validating syscall ABI...");
    syscall::validate_abi();
    println!("[+] Validating frozen ABIs (v0.42)...");
    if let Err(e) = abi_freeze::verify_all_frozen_abis() {
        panic!("ABI freeze violation: {}", e);
    }
    println!("[+] All frozen ABIs validated");

    // ============================================
    // PHASE 4: Start DOS Shell
    // ============================================

    testing::register_tests();

    // ── Run kernel self-tests at boot (for auto_test.py) ──
    {
        let (passed, failed) = testing::run_all();
        if failed == 0 {
            println!("All {} kernel tests passed.", passed);
        } else {
            println!("{} kernel tests passed, {} failed.", passed, failed);
        }
        println!("ALL_TESTS_COMPLETE");
    }

    // Spawn network kernel thread — drives net_tick() independently
    // of Ring 3 process activity.
    // Read the real function address from the static.
    // Direct fn→pointer→integer casts produce thunk addresses.
    if let Some(tid) = net::spawn_net_kthread(unsafe {
        core::ptr::read(&raw const net::NETD_PTR) as u64
    }) {
        println!("[+] netd kernel thread spawned (TID {})", tid);
    }

    // ── Boot Benchmark: shell ready ──
    boot_benchmark::mark(boot_benchmark::BootStage::ShellReady);

    // Load benchmark configuration from BOOT.CFG (now that VFS is mounted)
    boot_benchmark::load_config();

    // Detect which storage driver was selected
    let driver_name: &'static str = {
        let bdevs = globals::BLOCK_DEVICES.lock();
        // storage_manager priority: NVMe > VirtIO > AHCI > ATA
        if bdevs.count() > 0 {
            if boot_benchmark::VIRTIO_COMMANDS.load(core::sync::atomic::Ordering::Relaxed) > 0 {
                "VIRTIO.BLK"
            } else if boot_benchmark::AHCI_COMMANDS.load(core::sync::atomic::Ordering::Relaxed) > 0 {
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
    println!("[+] Loading NeoInit (PID 1, Ring 3)...");

    // Allocate random slot first (ASLR v0.44)
    let slot = match arch::x64::paging::alloc_user_slot() {
        Some(s) => s,
        None => {
            kerror!(LogSubsys::Kernel, "No free user slots for NeoInit.");
            kerror!(LogSubsys::Kernel, "All {} user slots are exhausted.", arch::x64::paging::USER_SLOT_COUNT);
            kerror!(LogSubsys::Kernel, "Cannot continue without Ring 3 process space. Halting.");
            crate::hal::halt();
        }
    };
    kinfo!(LogSubsys::Init, "allocated slot {} at code_base=0x{:x}",
        slot.slot_idx, slot.code_base);

    let mut addr_space = scheduler::address_space::AddressSpace::new();
    let (entry, loaded) = {
        let try_load = |path: &str, addr: &mut scheduler::address_space::AddressSpace| -> Option<u64> {
            let mut bin_buf = alloc::vec![0u8; 65536];
            let mut result = None;
            crate::globals::with_vfs(|vfs| {
                if let Ok((drive_idx, node)) = vfs.resolve_path(path) {
                    if (node.mode & fs::vfs::MODE_FILE) == 0 { return; }
                    let size = vfs.read(drive_idx, node.inode, 0, &mut bin_buf).unwrap_or_default();
                    kinfo!(
                        LogSubsys::Init, "resolved '{}': inode={} size={} mode=0x{:04x} read={} bytes",
                        path, node.inode, node.size, node.mode, size
                    );
                    if size < 4 { return; }
                    let data = &bin_buf[..size];
                    if let Ok(r) = elf::load_elf(data, Some(addr), slot.code_base) {
                        kinfo!(LogSubsys::Init, "ELF load OK: entry=0x{:x}", r.entry);
                        result = Some(r.entry);
                    }
                } else {
                    kinfo!(LogSubsys::Init, "path not found: {}", path);
                }
            });
            result
        };

        // Primary: NeoInit.nxe. Fallback: try Neoshell.nxe directly.
        let mut addr = scheduler::address_space::AddressSpace::new();
        let entry = try_load("C:\\Programs\\neoinit.nxe", &mut addr)
            .or_else(|| {
                kinfo!(LogSubsys::Init, "NeoInit not found, trying NEOSHELL.NXE as fallback...");
                try_load("C:\\Programs\\neoshell.nxe", &mut addr)
            })
            .unwrap_or(0);
        let loaded = entry != 0;
        if loaded { addr_space = addr; }
        (entry, loaded)
    };

    if !loaded {
        kerror!(LogSubsys::Kernel, "Neither NEOINIT.NXE nor NEOSHELL.NXE found or valid.");
        kerror!(LogSubsys::Kernel, "Ring 3 shell required to boot. Halting.");
        crate::hal::halt();
    }

    let pid = match usermode::spawn_usermode(
        entry, slot.stack_top, slot.slot_idx, 2, "\\", 0,
    ) {
        Ok(pid) => pid,
        Err(e) => {
            kerror!(LogSubsys::Kernel, "Failed to spawn NeoInit: {:?}", e);
            kerror!(LogSubsys::Kernel, "Ring 3 init process required. Halting.");
            crate::hal::halt();
        }
    };

    hal::without_interrupts(|| {
        if let Some(eproc) = scheduler::current_scheduler().lock().find_eprocess_mut(pid) {
            eproc.address_space = addr_space;
        }
    });

        println!("[+] NeoInit PID {} entered at entry=0x{:x}", pid, entry);
        kinfo!(LogSubsys::Init, "[THREAD] NeoInit started (PID={})", pid);

    // Mark NeoInit as Running so sm_start_auto_services doesn't spawn a duplicate
    services::sm_mark_neoinit_running(pid);

    // Start auto-start services (System/Auto start types in dependency order)
    services::sm_start_auto_services();

    crate::object::namespace::ob_namespace_debug();

    // Enter NeoInit (blocks until NeoInit exits, which it shouldn't)
    usermode::wait_for_process(pid);

    // If we get here, NeoInit exited (shouldn't happen)
    kerror!(LogSubsys::Kernel, "NeoInit PID {} exited unexpectedly!", pid);
    kerror!(LogSubsys::Kernel, "Ring 3 init process must not exit. Halting.");
    crate::hal::halt();
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
