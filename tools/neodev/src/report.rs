use crate::build::BuildReport;
use colored::*;
use std::time::Duration;

pub fn fmt_size(size: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut s = size as f64;
    for unit in UNITS {
        if s < 1024.0 {
            return format!("{:.1} {}", s, unit);
        }
        s /= 1024.0;
    }
    format!("{:.2} GB", s)
}

fn fmt_duration(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs > 60.0 {
        format!("{:.0}m {:.0}s", secs / 60.0, secs % 60.0)
    } else {
        format!("{:.1}s", secs)
    }
}

pub fn print_build_report(report: &BuildReport) {
    println!();
    println!("{}", "=".repeat(60));
    println!("{}", "BUILD REPORT".bold());
    println!("{}", "=".repeat(60));

    if let Some(ok) = report.kernel {
        println!(
            "  {:<30} {}",
            "Kernel:",
            if ok {
                "[OK]".bold().green()
            } else {
                "[FAIL]".bold().red()
            }
        );
    }

    if let Some(ok) = report.bootloader {
        println!(
            "  {:<30} {}",
            "Bootloader:",
            if ok {
                "[OK]".bold().green()
            } else {
                "[FAIL]".bold().red()
            }
        );
    }

    print_component_results("User Binaries:", &report.user_bins);
    print_component_results("NXL Libraries:", &report.nxl_libs);
    print_component_results("NEM Drivers:", &report.nem_drivers);

    let total_ok = report.kernel.unwrap_or(true)
        && report.bootloader.unwrap_or(true)
        && report.user_bins.iter().all(|(_, ok)| *ok)
        && report.nxl_libs.iter().all(|(_, ok)| *ok)
        && report.nem_drivers.iter().all(|(_, ok)| *ok);

    println!();
    if total_ok {
        println!("{} BUILD SUCCESSFUL", "[✓]".bold().green());
    } else {
        println!("{} BUILD FAILED", "[✗]".bold().red());
    }
    println!("  Total duration: {}", fmt_duration(report.duration));
    println!("{}", "=".repeat(60));
}

fn print_component_results(label: &str, items: &[(String, bool)]) {
    if items.is_empty() {
        return;
    }
    println!("  {} ({} items)", label, items.len());
    for (name, ok) in items {
        println!(
            "    {:25} {}",
            name,
            if *ok {
                "[OK]".bold().green()
            } else {
                "[FAIL]".bold().red()
            }
        );
    }
}

pub fn print_discovery_report(cfg: &crate::config::Config) -> Result<(), anyhow::Error> {
    let disc = crate::discovery::discover(&cfg.project_root)?;

    println!();
    println!("{}", "=".repeat(60));
    println!("{}", "NeoDOS Project Discovery".bold());
    println!("{}", "=".repeat(60));

    if let Some(k) = &disc.kernel {
        println!("  Kernel:            {} ({})", k.name.bold(), k.path.display());
    }
    if let Some(b) = &disc.bootloader {
        println!("  Bootloader:        {} ({})", b.name.bold(), b.path.display());
    }

    println!(
        "  User Binaries:     {} found",
        disc.user_bins.len().to_string().bold()
    );
    for p in &disc.user_bins {
        println!("    - {}", p.name);
    }

    println!(
        "  NXL Libraries:     {} found",
        disc.nxl_libs.len().to_string().bold()
    );
    for p in &disc.nxl_libs {
        println!("    - {}", p.name);
    }

    println!(
        "  NEM Drivers:       {} found",
        disc.nem_drivers.len().to_string().bold()
    );
    for p in &disc.nem_drivers {
        println!("    - {}", p.name);
    }

    println!(
        "  Tools:             {} found",
        disc.tools.len().to_string().bold()
    );
    for p in &disc.tools {
        println!("    - {}", p.name);
    }

    println!("{}", "=".repeat(60));
    Ok(())
}
