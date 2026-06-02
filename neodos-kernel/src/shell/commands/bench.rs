use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_bench(&mut self, args: &[&str]) {
        if args.is_empty() {
            // Show current benchmark configuration
            let report_enabled = crate::boot_benchmark::ENABLE_BOOT_BENCHMARK_REPORT
                .load(core::sync::atomic::Ordering::Relaxed);
            let ahci_enabled = crate::boot_benchmark::ENABLE_AHCI_DEBUG_OUTPUT
                .load(core::sync::atomic::Ordering::Relaxed);
            
            println!("Boot Benchmark Configuration:");
            println!("  BENCHMARK_REPORT: {}", if report_enabled { "ON" } else { "OFF" });
            println!("  AHCI_DEBUG:       {}", if ahci_enabled { "ON" } else { "OFF" });
            println!();
            println!("Usage:");
            println!("  BENCH REPORT <ON|OFF>     - Enable/disable boot benchmark report");
            println!("  BENCH AHCI <ON|OFF>       - Enable/disable AHCI debug output");
            return;
        }

        match args.get(0).map(|s| s.to_uppercase()).as_deref() {
            Some("REPORT") => {
                if let Some(flag) = args.get(1) {
                    match flag.to_uppercase().as_str() {
                        "ON" | "1" | "YES" | "TRUE" => {
                            crate::boot_benchmark::set_benchmark_report_enabled(true);
                            println!("Boot benchmark report: ON");
                        }
                        "OFF" | "0" | "NO" | "FALSE" => {
                            crate::boot_benchmark::set_benchmark_report_enabled(false);
                            println!("Boot benchmark report: OFF");
                        }
                        _ => println!("Invalid value. Use: ON or OFF"),
                    }
                } else {
                    println!("Usage: BENCH REPORT <ON|OFF>");
                }
            }
            Some("AHCI") => {
                if let Some(flag) = args.get(1) {
                    match flag.to_uppercase().as_str() {
                        "ON" | "1" | "YES" | "TRUE" => {
                            crate::boot_benchmark::set_ahci_debug_enabled(true);
                            println!("AHCI debug output: ON");
                        }
                        "OFF" | "0" | "NO" | "FALSE" => {
                            crate::boot_benchmark::set_ahci_debug_enabled(false);
                            println!("AHCI debug output: OFF");
                        }
                        _ => println!("Invalid value. Use: ON or OFF"),
                    }
                } else {
                    println!("Usage: BENCH AHCI <ON|OFF>");
                }
            }
            _ => {
                println!("Unknown subcommand. Available:");
                println!("  BENCH REPORT <ON|OFF>  - Control boot benchmark report");
                println!("  BENCH AHCI <ON|OFF>    - Control AHCI debug output");
            }
        }
    }
}
