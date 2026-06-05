use crate::println;
use crate::serial_println;
use crate::shell::shell::DosShell;
use crate::fs::vfs::MODE_DIR;
use crate::nem::{self, NemDriverType};
use crate::drivers::driver_runtime::{self, DriverState};

const NEM_BUF_SIZE: usize = 16384;

fn driver_type_char(dt: NemDriverType) -> u8 {
    match dt {
        NemDriverType::Null => b'N',
        NemDriverType::Echo => b'E',
        NemDriverType::Lifecycle => b'L',
        NemDriverType::Mutation => b'M',
        NemDriverType::Fault => b'F',
        NemDriverType::Burst => b'B',
    }
}

fn pipeline_indicator(state: DriverState) -> &'static str {
    // Show pipeline progress as a 5-char visual: L I R B A
    // Each position is filled if the driver has reached that stage
    let loaded = '*';
    let init = if state as u8 >= DriverState::Initialized as u8 { '*' } else { '-' };
    let reg = if state as u8 >= DriverState::Registered as u8 { '*' } else { '-' };
    let bound = if state as u8 >= DriverState::Bound as u8 { '*' } else { '-' };
    let active = if state as u8 >= DriverState::Active as u8 { '*' } else { '-' };
    // Return as &str using a static buffer trick
    match (loaded, init, reg, bound, active) {
        ('*', '*', '*', '*', '*') => "█████",
        ('*', '*', '*', '*', '-') => "████ ",
        ('*', '*', '*', '-', '-') => "███  ",
        ('*', '*', '-', '-', '-') => "██   ",
        ('*', '-', '-', '-', '-') => "█    ",
        _ => "?????",
    }
}

impl DosShell {
    pub fn cmd_ndreg(&mut self, args: &[&str]) {
        let subcommand = args.first().copied().unwrap_or("list").to_ascii_uppercase();
        let path = args.get(1).copied().unwrap_or("");
        match subcommand.as_str() {
            "LIST" => self.ndreg_list(args.get(1..).unwrap_or(&[])),
            "SHOW" => self.ndreg_show(path),
            "QUERY" => self.ndreg_query(args.get(1..).unwrap_or(&[])),
            "RUNTIME" => self.ndreg_runtime(),
            "HEALTH" => self.ndreg_health(),
            "DEBUG" => self.ndreg_debug(path),
            "LOAD" => {
                if path.is_empty() {
                    println!("Usage: NDREG LOAD <path>");
                    return;
                }
                let full_path = self.resolve_absolute_path(path);
                match crate::drivers::nem::load_nem_driver(&full_path) {
                    Ok(id) => println!("Driver loaded successfully with ID {}", id),
                    Err(e) => println!("Failed to load driver: {:?}", e),
                }
            }
            "UNLOAD" => {
                if path.is_empty() {
                    println!("Usage: NDREG UNLOAD <driver_name> [/F]");
                    println!("  Unload a driver gracefully. /F forces unload without waiting for ACK.");
                    return;
                }
                let force = args.iter().any(|a| a.eq_ignore_ascii_case("/F"));
                match crate::drivers::hotreload::unload_driver(path, force) {
                    Ok(msg) => println!("{}", msg),
                    Err(e) => println!("Failed to unload driver: {}", e),
                }
            }
            "RELOAD" => {
                if path.is_empty() {
                    println!("Usage: NDREG RELOAD <path>");
                    println!("  Reload a driver from disk. The driver is unloaded and re-loaded");
                    println!("  through the certification pipeline with ABI version check.");
                    return;
                }
                let full_path = self.resolve_absolute_path(path);
                match crate::drivers::hotreload::reload_driver(&full_path) {
                    Ok(msg) => println!("{}", msg),
                    Err(e) => println!("Failed to reload driver: {}", e),
                }
            }
            _ => {
                println!("NDREG — NeoDOS Driver Registry v2 (W2 Hot Reload)");
                println!();
                println!("Pipeline: Loaded → Initialized → Registered → Bound → Active → Unloading → Unloaded → Loaded");
                println!();
                println!("Subcommands:");
                println!("  NDREG LIST [path]     List drivers with metadata + state");
                println!("  NDREG SHOW <name>     Show full driver details + errors");
                println!("  NDREG QUERY [filters] Driver count by state");
                println!("  NDREG RUNTIME         Runtime state snapshot");
                println!("  NDREG HEALTH          Validate driver metadata");
                println!("  NDREG DEBUG <name>    Diagnose why driver is NOT active");
                println!("  NDREG LOAD <path>     Load a driver via certification pipeline");
                println!("  NDREG UNLOAD <name> [/F]  Gracefully unload a driver");
                println!("  NDREG RELOAD <path>   Reload a driver (unload + load with ABI check)");
            }
        }
    }

    pub fn cmd_loadnem(&mut self, args: &[&str]) {
        let path = args.first().copied().unwrap_or("");
        if path.is_empty() {
            println!("LOADNEM <path>");
            println!("  Load a .nem driver file from NeoFS.");
            return;
        }
        crate::drivers::driver_loader::cmd_loadnem(path);
    }

    pub fn cmd_nemlist(&mut self) {
        crate::drivers::driver_loader::cmd_nemlist();
    }

    fn ndreg_list(&mut self, args: &[&str]) {
        let has_path = !args.is_empty();
        let search_dirs: &[&str] = if has_path {
            &[args[0]]
        } else {
            &["C:\\SYSTEM\\DRIVERS\\BOOT", "C:\\SYSTEM\\DRIVERS\\SYSTEM"]
        };

        for dir in search_dirs {
            let full_path = self.resolve_absolute_path(dir);

            println!(" Driver Registry: {}", full_path);
            println!();
            println!(" {:<18} {:>6} {:>8} {:>4} {:>5} {:>7} {:>5} {:>6} {:>8}", 
                "NAME", "TYPE", "CATEGORY", "ABI", "FLAGS", "STATE", "ERR", "SIZE", "PIPELINE");
            println!(" {} {} {} {} {} {} {} {} {}",
                str::repeat("-", 18),
                str::repeat("-", 6),
                str::repeat("-", 8),
                str::repeat("-", 4),
                str::repeat("-", 5),
                str::repeat("-", 7),
                str::repeat("-", 5),
                str::repeat("-", 6),
                str::repeat("-", 8));

            let mut nem_files: alloc::vec::Vec<alloc::string::String> = alloc::vec::Vec::new();

            crate::globals::with_vfs(|vfs| {
                match vfs.resolve_path(&full_path) {
                    Ok((drive_idx, node)) => {
                        if (node.mode & MODE_DIR) == 0 {
                            if !has_path { return; }
                            println!("  Not a directory");
                            return;
                        }

                        let mut i = 0;
                        loop {
                            match vfs.readdir(drive_idx, node.inode, i) {
                            Ok(Some(entry)) => {
                                let name = entry.name.to_ascii_uppercase();
                                if !name.ends_with(".NEM") {
                                    i += 1;
                                    continue;
                                }

                                if (entry.node.mode & MODE_DIR) != 0 {
                                    i += 1;
                                    continue;
                                }

                                let full_file = alloc::format!("{}\\{}", full_path.trim_end_matches('\\'), name);
                                let lc_file = full_file.to_ascii_lowercase();
                                nem_files.push(lc_file);
                                    i += 1;
                                }
                                Ok(None) => break,
                                Err(_) => break,
                            }
                        }
                    }
                    Err(_) => {
                        if !has_path {
                            return;
                        }
                        println!("  Path not found: {}", full_path);
                    }
                }
            });

            for f in &nem_files {
                self.ndreg_print_entry(f);
            }

            if nem_files.is_empty() && !has_path {
                continue;
            }
            if !has_path {
                println!();
            }
        }
    }

    fn ndreg_print_entry(&self, full_path: &str) {
        static mut BUF: [u8; NEM_BUF_SIZE] = [0u8; NEM_BUF_SIZE];

        crate::globals::with_vfs(|vfs| {
            match vfs.resolve_path(full_path) {
                Ok((drive_idx, node)) => {
                    unsafe {
                        match vfs.read(drive_idx, node.inode, 0, &mut *core::ptr::addr_of_mut!(BUF)) {
                            Ok(n) if n > 0 => {
                                let data = {
                                    let ptr = core::ptr::addr_of!(BUF) as *const u8;
                                    core::slice::from_raw_parts(ptr, n)
                                };
                                match nem::parse_nem_v3(data) {
                                    Some(parsed) => {
                                        let dt_char = driver_type_char(parsed.driver_type) as char;
                                        
                                        // Look up runtime state + error from registry
                                        let mut state = DriverState::Unloaded;
                                        let mut last_error = driver_runtime::ERR_NONE;
                                        if let Some(drv) = crate::drivers::driver_runtime::get_driver_by_name(parsed.name) {
                                            state = drv.state;
                                            last_error = drv.last_error;
                                        }

                                        let mode_str = if parsed.header.flags & 1 != 0 { "RWD" } else { "R--" };
                                        let err_str = driver_runtime::err_to_str(last_error);
                                        let pipeline = pipeline_indicator(state);
                                        let abi_str = alloc::format!("{}.{}.{}",
                                            parsed.header.abi_min, parsed.header.abi_target, parsed.header.abi_max);
                                        println!(" {:<18} {:>6} {:>8} {:>4} {:>5} {:>7} {:>5} {:>6} {:>8}",
                                            parsed.name, dt_char,
                                            parsed.category.to_str(),
                                            &abi_str, mode_str,
                                            state.to_str(),
                                            if last_error == 0 { "." } else { err_str },
                                            parsed.text.len(),
                                            pipeline);
                                    }
                                    None => {
                                        println!(" {:<18} {:>6} {:>8} {:>4} {:>5} {:>7} {:>5} {:>6} {:>8}",
                                            "?", "?", "?", "?", "?", "INVALID", "?", 0, "?????");
                                    }
                                }
                            }
                            Ok(_) => {}
                            Err(_) => {}
                        }
                    }
                }
                Err(_) => {}
            }
        });
    }

    fn ndreg_show(&mut self, name: &str) {
        if name.is_empty() {
            println!("Usage: NDREG SHOW <driver_name>");
            return;
        }

        let lc_name = name.to_ascii_lowercase();
        let search_paths = [
            alloc::format!("C:\\SYSTEM\\DRIVERS\\BOOT\\{}", lc_name),
            alloc::format!("C:\\SYSTEM\\DRIVERS\\SYSTEM\\{}", lc_name),
        ];

        for full_path in &search_paths {
            if self.ndreg_show_file(full_path.as_str()) {
                return;
            }
        }

        println!("Driver '{}' not found in SYSTEM\\DRIVERS\\", name);
    }

    fn ndreg_show_file(&self, full_path: &str) -> bool {
        static mut BUF: [u8; NEM_BUF_SIZE] = [0u8; NEM_BUF_SIZE];
        let mut found = false;

        crate::globals::with_vfs(|vfs| {
            match vfs.resolve_path(full_path) {
                Ok((drive_idx, node)) => {
                    unsafe {
                        match vfs.read(drive_idx, node.inode, 0, &mut *core::ptr::addr_of_mut!(BUF)) {
                            Ok(n) if n > 0 => {
                                let data = {
                                    let ptr = core::ptr::addr_of!(BUF) as *const u8;
                                    core::slice::from_raw_parts(ptr, n)
                                };
                                match nem::parse_nem_v3(data) {
                                    Some(parsed) => {
                                        found = true;

                                        // Look up runtime state from registry
                                        let mut drv_state = DriverState::Unloaded;
                                        let mut last_error = driver_runtime::ERR_NONE;
                                        let mut cert_step = 0u8;
                                        if let Some(drv) = crate::drivers::driver_runtime::get_driver_by_name(parsed.name) {
                                            drv_state = drv.state;
                                            last_error = drv.last_error;
                                            cert_step = drv.certification_step;
                                        }

                                        println!("========================================");
                                        println!("  NeoDOS Driver Registry v1");
                                        println!("  Certification Pipeline");
                                        println!("========================================");
                                        println!("  Driver Name:     {}", parsed.name);
                                        println!("  Path:            {}", full_path);
                                        println!("  Driver Type:     {} ({})", 
                                            parsed.driver_type.to_str(), parsed.driver_type as u8);
                                        println!("  NEM Format:      {}", "v3");
                                        println!("  Driver Category: {}", parsed.category.to_str());
                                        println!("  ABI Range:       {} ≤ {} ≤ {}",
                                            parsed.header.abi_min, parsed.header.abi_target, parsed.header.abi_max);
                                        println!("  File Size:       {} bytes", node.size);
                                        println!("  Code Size:       {} bytes", parsed.header.text_size);
                                        println!("  Entry Offset:    0x{:04X}", parsed.header.entry_init);
                                        println!("  Compat Flags:    0x{:04X}", parsed.header.flags);
                                        // Show capabilities from runtime (or category default)
                                        let caps_bits = match crate::drivers::driver_runtime::get_driver_by_name(parsed.name) {
                                            Some(d) => d.caps,
                                            None => crate::drivers::caps::capability_for_category(parsed.category).bits,
                                        };
                                        let caps_str = crate::drivers::caps::CapabilitySet::new(caps_bits).format();
                                        println!("  Capabilities:    0x{:016X}", caps_bits);
                                        println!("                   {}", caps_str);
                                        println!("  Permissions:     R--");
                                        // X4: Show isolation info
                                        if let Some(drv) = crate::drivers::driver_runtime::get_driver_by_name(parsed.name) {
                                            let iso_mode = drv.isolation_mode;
                                            let iso_mode_str = crate::drivers::isolation::isolation_mode_str(
                                                match iso_mode {
                                                    0 => crate::drivers::isolation::IsolationMode::None,
                                                    1 => crate::drivers::isolation::IsolationMode::Basic,
                                                    2 => crate::drivers::isolation::IsolationMode::Sandbox,
                                                    _ => crate::drivers::isolation::IsolationMode::None,
                                                }
                                            );
                                            println!("  Isolation:       {} (mode={})", iso_mode_str, iso_mode);
                                            if drv.isolated_base != 0 {
                                                println!("  ISO Region:      0x{:x}..0x{:x} ({} KB)",
                                                    drv.isolated_base,
                                                    drv.isolated_base + drv.isolated_size,
                                                    drv.isolated_size / 1024);
                                            }
                                        } else {
                                            println!("  Isolation:       NONE (not loaded)");
                                        }
                                        println!();
                                        println!(" ── Lifecycle State ──");
                                        println!("  Runtime State:   {} ({})", drv_state.to_str(), drv_state as u8);
                                        println!("  Pipeline:        {} L-I-R-B-A", pipeline_indicator(drv_state));
                                        println!("  Last Error:      {} ({})", 
                                            driver_runtime::err_to_str(last_error), last_error);
                                        println!("  Pipeline Step:   {} ({})",
                                            if cert_step == 0 { "NONE" } else {
                                                match cert_step {
                                                    1 => "LOAD",
                                                    2 => "INIT",
                                                    3 => "REGISTER",
                                                    4 => "BIND",
                                                    5 => "CERTIFY",
                                                    _ => "UNKNOWN",
                                                }
                                            }, cert_step);
                                        println!();
                                        println!(" ── Certification Check ──");
                                        if drv_state == DriverState::Active {
                                            println!("  ✓ FULLY CERTIFIED — Driver is ACTIVE");
                                        } else {
            println!("  ✗ NOT ACTIVE — {}", match drv_state {
                DriverState::Loaded => "Stuck at LOADED (not initialized)",
                DriverState::Initialized => "Stuck at INIT (not registered)",
                DriverState::Registered => "Stuck at REGISTERED (not bound)",
                DriverState::Bound => "Stuck at BOUND (certification pending)",
                DriverState::Faulted => "FAULTED — see error code",
                DriverState::Unloaded => "Terminated",
                DriverState::Unloading => "UNLOADING — graceful drain in progress",
                _ => "Unknown state",
            });
                                            if last_error != 0 {
                                                println!("  Cause: {} ({})", driver_runtime::err_to_str(last_error), last_error);
                                            }
                                            if cert_step != 0 {
                                                println!("  Failed at pipeline step: {}", match cert_step {
                                                    1 => "LOAD", 2 => "INIT", 3 => "REGISTER", 
                                                    4 => "BIND", 5 => "CERTIFY", _ => "?",
                                                });
                                            }
                                        }
                                        println!();
                                        serial_println!("[NDREG] Show '{}': type={}, code={}B, flags=0x{:04X}, state={:?}",
                                            parsed.name, parsed.driver_type.to_str(),
                                            parsed.text.len(), parsed.header.flags, drv_state);
                                    }
                                    None => {
                                        println!("  Invalid NEM driver: {}", full_path);
                                    }
                                }
                            }
                            Ok(_) => {
                                println!("  Empty file: {}", full_path);
                            }
                            Err(e) => {
                                println!("  Error reading '{}': {:?}", full_path, e);
                            }
                        }
                    }
                }
                Err(_) => {}
            }
        });

        found
    }

    fn ndreg_query(&mut self, _args: &[&str]) {
        let search_paths = [
            "C:\\SYSTEM\\DRIVERS\\BOOT",
            "C:\\SYSTEM\\DRIVERS\\SYSTEM",
        ];

        let mut total = 0u32;
        let mut invalid = 0u32;

        println!(" Driver Registry Query — Certification Pipeline v1");
        println!();

        static mut BUF: [u8; NEM_BUF_SIZE] = [0u8; NEM_BUF_SIZE];

        for base_path in &search_paths {
            crate::globals::with_vfs(|vfs| {
                match vfs.resolve_path(base_path) {
                    Ok((drive_idx, node)) => {
                        if (node.mode & MODE_DIR) == 0 {
                            return;
                        }

                        let mut i = 0;
                        loop {
                            match vfs.readdir(drive_idx, node.inode, i) {
                                Ok(Some(entry)) => {
                                    let name = entry.name.to_ascii_uppercase();
                                    if !name.ends_with(".NEM") || (entry.node.mode & MODE_DIR) != 0 {
                                        i += 1;
                                        continue;
                                    }

                                    unsafe {
                                        match vfs.read(drive_idx, entry.node.inode, 0, &mut *core::ptr::addr_of_mut!(BUF)) {
                                            Ok(n) if n > 0 => {
                                                let data = {
                                    let ptr = core::ptr::addr_of!(BUF) as *const u8;
                                    core::slice::from_raw_parts(ptr, n)
                                };
                                                if nem::parse_nem_v3(data).is_some() {
                                                    total += 1;
                                                } else {
                                                    invalid += 1;
                                                }
                                            }
                                            _ => { invalid += 1; }
                                        }
                                    }
                                    i += 1;
                                }
                                Ok(None) => break,
                                Err(_) => break,
                            }
                        }
                    }
                    Err(_) => {}
                }
            });
        }

        // Query runtime state breakdown
        let runtime = crate::drivers::driver_runtime::DRIVER_RUNTIME.lock();
        let state_breakdown = runtime.state_counts();
        let total_loaded = runtime.count();
        let active = runtime.active_count();
        let loaded_non_active = runtime.loaded_count();
        let faulted = runtime.faulted_count();
        drop(runtime);

        println!("  File system NEM binaries:");
        println!("    Total drivers:    {}", total);
        println!("    Invalid/Unknown:  {}", invalid);
        println!("    Quarantined:      0");
        println!();
        println!("  Runtime state (registry):");
        println!("    Total in registry: {}", total_loaded);
        println!("    ACTIVE:            {}", active);
        println!("    NOT active:        {}", loaded_non_active);
        println!("    FAULTED:           {}", faulted);
        if !state_breakdown.is_empty() {
            println!();
            println!("  Per-state breakdown:");
            for (state, count) in &state_breakdown {
                println!("    {:12} {}", state.to_str(), count);
            }
        }
    }

    fn ndreg_runtime(&mut self) {
        println!("========================================");
        println!("  NeoDOS Driver Runtime State v1");
        println!("  Certification Pipeline Snapshot");
        println!("========================================");

        let runtime = crate::drivers::driver_runtime::DRIVER_RUNTIME.lock();
        let count = runtime.count();
        let active = runtime.active_count();
        let loaded_non_active = runtime.loaded_count();
        let faulted = runtime.faulted_count();
        let next_id = runtime.next_driver_id();
        let timer_tick = crate::hal::get_ticks();
        drop(runtime);

        println!("  Timer Tick:       {}", timer_tick);
        println!("  Next Driver ID:   {}", next_id);
        println!("  Total in Registry: {}", count);
        println!("  ACTIVE Drivers:   {}", active);
        println!("  NOT Active:       {}", loaded_non_active);
        println!("  FAULTED:          {}", faulted);
        println!("  Event Bus Queue:  {} avail", crate::eventbus::EVENT_BUS.queue_available());
        println!("  Event Bus Hdlrs:  {}", crate::eventbus::EVENT_BUS.handler_count());
        println!("  Next Event ID:    {}", crate::eventbus::EVENT_BUS.next_event_id());
        println!();

        let names = crate::drivers::driver_runtime::driver_names();
        if names.is_empty() {
            println!("  No drivers loaded.");
        } else {
            println!("  ID  NAME                  CATEGORY  STATE       ERR    EVENTS  TICKS  ISO     PIPELINE");
            println!("  --- --------------------  --------  ----------  -----  ------  -----  ------  --------");
            let r = crate::drivers::driver_runtime::DRIVER_RUNTIME.lock();
            for (name, id, state) in &names {
                if let Some(drv) = r.get(*id) {
                    let err_str = driver_runtime::err_to_str(drv.last_error);
                    let pipe = pipeline_indicator(drv.state);
                    let iso_str = match drv.isolation_mode {
                        0 => "NONE",
                        1 => "BASIC",
                        2 => "SBOX",
                        _ => "?",
                    };
                    println!(
                        "  {:>3}  {:20}  {:>8}  {:10}  {:>5}  {:>6}  {:>5}  {:>6}  {:>8}",
                        id, name, drv.category.to_str(), state.to_str(),
                        if drv.last_error == 0 { "." } else { err_str },
                        drv.events_received, drv.tick_count,
                        iso_str,
                        pipe
                    );
                }
            }
        }
    }

    /// DEBUG subcommand: diagnose why a driver is NOT active.
    /// Checks all 5 pipeline stages and reports which one is blocking activation.
    fn ndreg_debug(&mut self, name: &str) {
        if name.is_empty() {
            println!("Usage: NDREG DEBUG <driver_name>");
            println!();
            println!("Diagnoses why a loaded driver is NOT showing as ACTIVE.");
            println!("Checks: registry, init, event bus binding, sandbox, certification.");
            return;
        }

        let lc_name = name.to_ascii_lowercase();
        let search_paths = [
            alloc::format!("C:\\SYSTEM\\DRIVERS\\BOOT\\{}", lc_name),
            alloc::format!("C:\\SYSTEM\\DRIVERS\\SYSTEM\\{}", lc_name),
        ];

        // Find the driver in the runtime registry
        let drv_instance = crate::drivers::driver_runtime::get_driver_by_name(name);
        let drv = match drv_instance {
            Some(d) => d,
            None => {
                // Check if the .nem file exists even if not loaded
                let mut file_exists = false;
                for p in &search_paths {
                    crate::globals::with_vfs(|vfs| {
                        if vfs.resolve_path(p).is_ok() {
                            file_exists = true;
                        }
                    });
                }
                if file_exists {
                    println!("========================================");
                    println!("  NDREG DEBUG: {}", name);
                    println!("========================================");
                    println!();
                    println!("  ✗ Driver binary found but NOT loaded in registry.");
                    println!();
                    println!("  Possible causes:");
                    println!("    1. Driver was never loaded via NDREG LOAD or LOADNEM.");
                    println!("    2. Driver was loaded but later removed/unloaded.");
                    println!("    3. Driver name mismatch between file and registry.");
                    println!();
                    println!("  To load: NDREG LOAD C:\\SYSTEM\\DRIVERS\\TEST\\{}.nem", lc_name);
                } else {
                    println!("Driver '{}' not found in registry or filesystem.", name);
                }
                return;
            }
        };

        println!("========================================");
        println!("  NDREG DEBUG: {}", name);
        println!("  Driver ID:    {}", drv.id);
        println!("========================================");
        println!();
        println!(" ── Current State ──");
        println!("  State:        {} ({})", drv.state.to_str(), drv.state as u8);
        println!("  Last Error:   {} ({})", driver_runtime::err_to_str(drv.last_error), drv.last_error);
        println!("  Pipeline Step: {}", if drv.certification_step == 0 { "NONE (no failure)" } else {
            match drv.certification_step {
                1 => "LOAD", 2 => "INIT", 3 => "REGISTER", 4 => "BIND", 5 => "CERTIFY", _ => "?",
            }
        });
        println!("  Pipeline:     {} L-I-R-B-A", pipeline_indicator(drv.state));
        println!();

        // ── Diagnostic checklist ──
        println!(" ── Certification Pipeline Diagnostic ──");
        println!();

        // Check 1: Is state at least Loaded?
        println!("  [1/5] LOAD:   Binary loaded and parsed?");
        if drv.state as u8 >= DriverState::Loaded as u8 {
            println!("    ✓ PASS — Driver is in registry (state >= LOADED)");
        } else {
            println!("    ✗ FAIL — Driver not in Loaded state");
            println!("    → Load the driver first: NDREG LOAD <path>");
            println!();
            return;
        }

        // Check 2: Is state at least Initialized?
        println!("  [2/5] INIT:   driver_init() executed?");
        if drv.state as u8 >= DriverState::Initialized as u8 {
            println!("    ✓ PASS — Driver is initialized (state >= INIT)");
        } else {
            println!("    ✗ FAIL — Driver not initialized");
            println!("    → Legacy loader used (stays in LOADED). Use NDREG LOAD instead of LOADNEM.");
            if drv.last_error == driver_runtime::ERR_INIT_FAILED {
                println!("    → Init failed: OUT_OF_MEMORY or SLOT_OUT_OF_BOUNDS");
            }
            println!();
            return;
        }

        // Check 3: Is state at least Registered?
        println!("  [3/5] REG:    Registry + Event Bus updated?");
        if drv.state as u8 >= DriverState::Registered as u8 {
            println!("    ✓ PASS — Driver is registered (state >= REGISTERED)");
        } else {
            println!("    ✗ FAIL — Registry commit missing");
            println!("    → The driver was initialized but the registry was never updated.");
            println!("    → Check if driver_init() returned success and called back.");
            if drv.last_error == driver_runtime::ERR_REGISTRATION_FAILED {
                println!("    → Registration specifically failed.");
            }
            println!();
            return;
        }

        // Check 4: Is state at least Bound?
        println!("  [4/5] BIND:   Event Bus binding completed?");
        if drv.state as u8 >= DriverState::Bound as u8 {
            println!("    ✓ PASS — Driver is bound (state >= BOUND)");
        } else {
            println!("    ✗ FAIL — Event Bus binding missing");
            println!("    → Driver registered but never bound to Event Bus.");
            println!("    → Check if driver registered event handlers via register_event().");
            if drv.last_error == driver_runtime::ERR_BIND_FAILED {
                println!("    → Binding specifically failed.");
            }
            println!();
            return;
        }

        // Check 5: Is state Active?
        println!("  [5/5] ACTIVE: Certification complete?");
        if drv.state == DriverState::Active {
            println!("    ✓ FULLY CERTIFIED — Driver is ACTIVE and operational!");
        } else if drv.state == DriverState::Bound {
            println!("    ✗ NOT ACTIVE — Stuck at BOUND (certification pending)");
            println!("    → All pipeline stages passed but certification step failed.");
            if drv.last_error == driver_runtime::ERR_CERTIFICATION_FAILED {
                println!("    → Certification check failed:");
                println!("      - Check sandbox validation");
                println!("      - Check no fault flags set");
                println!("      - Check scheduler activation task ran");
            } else {
                println!("    → Last error: {} ({})", 
                    driver_runtime::err_to_str(drv.last_error), drv.last_error);
            }
        } else if drv.state == DriverState::Faulted {
            println!("    ✗ FAULTED — Driver faulted during operation");
            println!("    → Error code: {} ({})", 
                driver_runtime::err_to_str(drv.last_error), drv.last_error);
        }
        println!();

        // Summary
        println!(" ── Summary ──");
        if drv.state == DriverState::Active {
            println!("  Driver is fully certified and ACTIVE.");
        } else {
            println!("  Driver is in state {} — NOT ACTIVE.", drv.state.to_str());
            println!("  {}", drv.inactive_reason());
        }
    }

    fn ndreg_health(&mut self) {
        let search_paths = [
            "C:\\SYSTEM\\DRIVERS\\BOOT",
            "C:\\SYSTEM\\DRIVERS\\SYSTEM",
        ];

        println!("========================================");
        println!("  NeoDOS Driver Health Check");
        println!("========================================");
        println!();

        let mut ok_count = 0u32;
        let mut fail_count = 0u32;

        static mut BUF: [u8; NEM_BUF_SIZE] = [0u8; NEM_BUF_SIZE];

        for base_path in &search_paths {
            crate::globals::with_vfs(|vfs| {
                match vfs.resolve_path(base_path) {
                    Ok((drive_idx, node)) => {
                        if (node.mode & MODE_DIR) == 0 {
                            return;
                        }

                        let mut i = 0;
                        loop {
                            match vfs.readdir(drive_idx, node.inode, i) {
                                Ok(Some(entry)) => {
                                    let name = entry.name.to_ascii_uppercase();
                                    if !name.ends_with(".NEM") || (entry.node.mode & MODE_DIR) != 0 {
                                        i += 1;
                                        continue;
                                    }

                                    unsafe {
                                        match vfs.read(drive_idx, entry.node.inode, 0, &mut *core::ptr::addr_of_mut!(BUF)) {
                                            Ok(n) if n > 0 => {
                                                let data = {
                                    let ptr = core::ptr::addr_of!(BUF) as *const u8;
                                    core::slice::from_raw_parts(ptr, n)
                                };
                                                if let Some(parsed) = nem::parse_nem_v3(data) {
                                                    let mut status = "OK";
                                                    let mut reason = "";
                                                    if parsed.text.is_empty() {
                                                        status = "FAIL";
                                                        reason = "empty code section";
                                                    } else if data.len() < 32 {
                                                        status = "FAIL";
                                                        reason = "truncated";
                                                    }
                                                    if status == "OK" {
                                                        ok_count += 1;
                                                    } else {
                                                        fail_count += 1;
                                                    }
                                                    println!("  {:<20} [{}]", parsed.name, status);
                                                    if !reason.is_empty() {
                                                        println!("  {:>20}   -> {}", "", reason);
                                                    }
                                                } else {
                                                    fail_count += 1;
                                                    println!("  {:<20} [FAIL] -> invalid NEM header", name);
                                                }
                                            }
                                            Ok(_) => {
                                                fail_count += 1;
                                                println!("  {:<20} [FAIL] -> empty file", name);
                                            }
                                            Err(_) => {
                                                fail_count += 1;
                                                println!("  {:<20} [FAIL] -> read error", name);
                                            }
                                        }
                                    }
                                    i += 1;
                                }
                                Ok(None) => break,
                                Err(_) => break,
                            }
                        }
                    }
                    Err(e) => {
                        println!("  Path '{}' not found: {:?}", base_path, e);
                    }
                }
            });
        }

        println!();
        println!("  Summary:");
        println!("    OK:     {}", ok_count);
        println!("    FAIL:   {}", fail_count);
        println!("  Status: {}", if fail_count == 0 { "PASS" } else { "FAIL" });
    }
}
