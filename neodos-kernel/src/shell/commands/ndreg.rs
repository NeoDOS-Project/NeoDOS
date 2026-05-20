use crate::println;
use crate::serial_println;
use crate::shell::shell::DosShell;
use crate::fs::vfs::MODE_DIR;
use crate::nem::{self, NemDriverType};

const NEM_BUF_SIZE: usize = 4096;

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

impl DosShell {
    pub fn cmd_ndreg(&mut self, args: &[&str]) {
        let subcommand = args.first().copied().unwrap_or("list");
        match subcommand.to_ascii_lowercase().as_str() {
            "list" => self.ndreg_list(args.get(1..).unwrap_or(&[])),
            "show" => self.ndreg_show(args.get(1).copied().unwrap_or("")),
            "query" => self.ndreg_query(args.get(1..).unwrap_or(&[])),
            "runtime" => self.ndreg_runtime(),
            "health" => self.ndreg_health(),
            _ => {
                println!("NDREG — NeoDOS Driver Registry");
                println!();
                println!("Subcommands:");
                println!("  NDREG LIST [path]     List drivers with metadata");
                println!("  NDREG SHOW <name>     Show full driver details");
                println!("  NDREG QUERY [filters] Filter drivers");
                println!("  NDREG RUNTIME         Show runtime state snapshot");
                println!("  NDREG HEALTH          Validate driver metadata");
            }
        }
    }

    fn ndreg_list(&mut self, args: &[&str]) {
        let has_path = !args.is_empty();
        let search_dirs: &[&str] = if has_path {
            &[args[0]]
        } else {
            &["C:\\SYSTEM\\DRIVERS\\TEST", "C:\\SYSTEM\\DRIVERS"]
        };

        for dir in search_dirs {
            let full_path = self.resolve_absolute_path(dir);

            println!(" Driver Registry: {}", full_path);
            println!();
            println!(" {:<18} {:>6} {:>4} {:>5} {:>5} {:>6}", "NAME", "TYPE", "ABI", "FLAGS", "STATE", "SIZE");
            println!(" {} {} {} {} {} {}",
                str::repeat("-", 18),
                str::repeat("-", 6),
                str::repeat("-", 4),
                str::repeat("-", 5),
                str::repeat("-", 5),
                str::repeat("-", 6));

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
                                    nem_files.push(full_file);
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
                                match nem::parse_nem(data) {
                                    Some(parsed) => {
                                        let dt_char = driver_type_char(parsed.driver_type) as char;
                                        let state = "UNLOADED";
                                        let mode_str = if parsed.compat_flags & 1 != 0 { "RWD" } else { "R--" };
                                        println!(" {:<18} {:>6} {:>4} {:>5} {:>5} {:>6}",
                                            parsed.name, dt_char,
                                            "v0.3", mode_str, state,
                                            parsed.code.len());
                                    }
                                    None => {
                                        println!(" {:<18} {:>6} {:>4} {:>5} {:>5} {:>6}",
                                            "?", "?", "?", "?", "INVALID", 0);
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

        let search_paths = [
            alloc::format!("C:\\SYSTEM\\DRIVERS\\TEST\\{}", name),
            alloc::format!("C:\\SYSTEM\\DRIVERS\\{}", name),
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
                                match nem::parse_nem(data) {
                                    Some(parsed) => {
                                        found = true;
                                        println!("========================================");
                                        println!("  NeoDOS Driver Registry");
                                        println!("========================================");
                                        println!("  Driver Name:     {}", parsed.name);
                                        println!("  Path:            {}", full_path);
                                        println!("  Driver Type:     {} ({})", 
                                            parsed.driver_type.to_str(), parsed.driver_type as u8);
                                        println!("  NEM Format:      v1");
                                        println!("  ABI Version:     v0.3");
                                        println!("  File Size:       {} bytes", node.size);
                                        println!("  Code Size:       {} bytes", parsed.code.len());
                                        println!("  Entry Offset:    0x{:04X}", parsed.entry_offset);
                                        println!("  Compat Flags:    0x{:04X}", parsed.compat_flags);
                                        println!("  Runtime State:   UNLOADED");
                                        println!("  Driver Category: TEST");
                                        println!("  Permissions:     R--");
                                        println!();
                                        serial_println!("[NDREG] Show '{}': type={}, code={}B, flags=0x{:04X}",
                                            parsed.name, parsed.driver_type.to_str(),
                                            parsed.code.len(), parsed.compat_flags);
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
            "C:\\SYSTEM\\DRIVERS\\TEST",
            "C:\\SYSTEM\\DRIVERS",
        ];

        let mut total = 0u32;
        let mut invalid = 0u32;

        println!(" Driver Registry Query");
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
                                                if nem::parse_nem(data).is_some() {
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

        println!("  Total drivers:    {}", total);
        println!("  Loaded:           0");
        println!("  Invalid/Unknown:  {}", invalid);
        println!("  Quarantined:      0");
    }

    fn ndreg_runtime(&mut self) {
        println!("========================================");
        println!("  NeoDOS Runtime Driver State");
        println!("========================================");
        println!("  Driver Loader:    NOT ACTIVE");
        println!("  Loaded Drivers:   0");
        println!("  Active Drivers:   0");
        println!("  Failed Loads:     0");
        println!("  Quarantined:      0");
        println!("  Last Error:       None");
        println!();
        println!("  (Runtime loader not yet implemented)");
    }

    fn ndreg_health(&mut self) {
        let search_paths = [
            "C:\\SYSTEM\\DRIVERS\\TEST",
            "C:\\SYSTEM\\DRIVERS",
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
                                                if let Some(parsed) = nem::parse_nem(data) {
                                                    let mut status = "OK";
                                                    let mut reason = "";
                                                    if parsed.code.is_empty() {
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
