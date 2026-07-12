use crate::config::Config;
use crate::discovery::Discovery;
use crate::report;
use anyhow::{Context, Result};
use colored::*;
use std::process::Command;
use std::time::Instant;

pub struct BuildReport {
    pub kernel: Option<bool>,
    pub bootloader: Option<bool>,
    pub user_bins: Vec<(String, bool)>,
    pub nxl_libs: Vec<(String, bool)>,
    pub nem_drivers: Vec<(String, bool)>,
    pub duration: std::time::Duration,
}

pub fn build_kernel(cfg: &Config, disc: &Discovery) -> Result<bool> {
    let kernel = disc
        .kernel
        .as_ref()
        .context("Kernel project not found")?;
    let start = Instant::now();
    println!("{} Building kernel...", "[*]".bold().cyan());

    let status = Command::new("cargo")
        .args(["+nightly", "build", "--target", &cfg.kernel_target, "--release"])
        .current_dir(&kernel.path)
        .status()
        .context("Failed to run cargo build for kernel")?;

    let ok = status.success();
    if ok {
        let src = kernel
            .path
            .join("target")
            .join(&cfg.kernel_target)
            .join("release")
            .join("neodos_kernel");
        let dst = cfg.project_root.join("kernel.elf");
        if src.exists() {
            std::fs::copy(&src, &dst)
                .context("Failed to copy kernel ELF to project root")?;
            println!(
                "{} Kernel ELF: {} ({})",
                "[✓]".bold().green(),
                dst.display(),
                report::fmt_size(std::fs::metadata(&dst)?.len())
            );
        }
    }
    let elapsed = start.elapsed();
    println!(
        "{} Kernel build took {:.1}s",
        if ok { "[✓]".bold().green() } else { "[✗]".bold().red() },
        elapsed.as_secs_f64()
    );
    Ok(ok)
}

pub fn build_bootloader(cfg: &Config, disc: &Discovery) -> Result<bool> {
    let bl = disc
        .bootloader
        .as_ref()
        .context("Bootloader project not found")?;
    let start = Instant::now();
    println!("{} Building bootloader...", "[*]".bold().cyan());

    let status = Command::new("cargo")
        .args([
            "build",
            "--target",
            &cfg.bootloader_target,
            "--release",
        ])
        .current_dir(&bl.path)
        .status()
        .context("Failed to run cargo build for bootloader")?;

    let ok = status.success();
    if ok {
        let src = bl
            .path
            .join("target")
            .join(&cfg.bootloader_target)
            .join("release")
            .join("neodos_bootloader.efi");
        let dst = cfg.project_root.join("bootloader.efi");
        if src.exists() {
            std::fs::copy(&src, &dst)
                .context("Failed to copy bootloader EFI to project root")?;
            println!(
                "{} Bootloader: {} ({})",
                "[✓]".bold().green(),
                dst.display(),
                report::fmt_size(std::fs::metadata(&dst)?.len())
            );
        }
    }
    let elapsed = start.elapsed();
    println!(
        "{} Bootloader build took {:.1}s",
        if ok { "[✓]".bold().green() } else { "[✗]".bold().red() },
        elapsed.as_secs_f64()
    );
    Ok(ok)
}

pub fn build_user_bins(disc: &Discovery) -> Result<Vec<(String, bool)>> {
    let start = Instant::now();
    println!("{} Building user-mode binaries (NXE)...", "[*]".bold().cyan());

    let mut results = vec![];
    for project in &disc.user_bins {
        print!("  {:20} ", project.name);
        std::io::Write::flush(&mut std::io::stdout())?;

        let status = Command::new("cargo")
            .args(["build", "--release"])
            .current_dir(&project.path)
            .status()
            .with_context(|| format!("Failed to build {}", project.name))?;

        let ok = status.success();
        if ok {
            let nxe_name = format!("{}.nxe", project.name);
            let src = project
                .path
                .join("target")
                .join("x86_64-unknown-none")
                .join("release")
                .join(&project.name);
            let dst = project.path.parent().unwrap().join(&nxe_name);
            if src.exists() {
                std::fs::copy(&src, &dst)?;
            }
            println!("{}", "[OK]".bold().green());
        } else {
            println!("{}", "[FAIL]".bold().red());
        }
        results.push((project.name.clone(), ok));
    }

    println!(
        "{} User binaries built in {:.1}s",
        "[✓]".bold().green(),
        start.elapsed().as_secs_f64()
    );
    Ok(results)
}

pub fn build_nxl_libs(disc: &Discovery) -> Result<Vec<(String, bool)>> {
    let start = Instant::now();
    println!("{} Building NXL shared libraries...", "[*]".bold().cyan());

    let mut results = vec![];
    for project in &disc.nxl_libs {
        print!("  {:20} ", project.name);
        std::io::Write::flush(&mut std::io::stdout())?;

        let status = Command::new("cargo")
            .args(["build", "--release"])
            .current_dir(&project.path)
            .status()
            .with_context(|| format!("Failed to build NXL {}", project.name))?;

        let ok = status.success();
        if ok {
            let bin_name = project
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let src = project
                .path
                .join("target")
                .join("x86_64-unknown-none")
                .join("release")
                .join(&bin_name);
            let nxl_name = if project.name == "libneodos" {
                "libneodos.nxl".to_string()
            } else if project.name == "libmath" {
                "libmath.nxl".to_string()
            } else if project.name == "libconsole" {
                "console.nxl".to_string()
            } else if project.name == "libnet" {
                "net.nxl".to_string()
            } else {
                format!("{}.nxl", project.name)
            };
            let dst = project.path.parent().unwrap().join(&nxl_name);
            if src.exists() {
                std::fs::copy(&src, &dst)?;
                println!(
                    "{} [OK] ({})",
                    "[✓]".bold().green(),
                    report::fmt_size(std::fs::metadata(&dst)?.len())
                );
            } else {
                println!("{} [FAIL - binary not found]", "[✗]".bold().red());
            }
        } else {
            println!("{}", "[FAIL]".bold().red());
        }
        results.push((project.name.clone(), ok));
    }

    let elapsed = start.elapsed().as_secs_f64();
    println!("{} NXL libraries built in {:.1}s", "[✓]".bold().green(), elapsed);
    Ok(results)
}

pub fn build_nem_drivers(disc: &Discovery) -> Result<Vec<(String, bool)>> {
    let start = Instant::now();
    println!("{} Building NEM drivers...", "[*]".bold().cyan());

    let mut results = vec![];
    for project in &disc.nem_drivers {
        print!("  {:20} ", project.name);
        std::io::Write::flush(&mut std::io::stdout())?;

        let nem_dir = format!("/tmp/nem_drivers_{}", std::process::id());
        let output_dir = match project.name.as_str() {
            "ps2kbd" | "ps2mouse" | "rtc" | "serial" => format!("{}/BOOT", nem_dir),
            _ => format!("{}/SYSTEM", nem_dir),
        };
        std::fs::create_dir_all(&output_dir)?;

        let build_script = project.path.join("build_nem.py");
        let status = Command::new("python3")
            .arg(&build_script)
            .arg(&output_dir)
            .current_dir(&project.path)
            .status()
            .with_context(|| format!("Failed to build NEM driver {}", project.name))?;

        let ok = status.success();
        if ok {
            let nem_file = format!("{}/{}.nem", output_dir, project.name);
            if std::path::Path::new(&nem_file).exists() {
                println!(
                    "{} [OK] ({})",
                    "[✓]".bold().green(),
                    report::fmt_size(std::fs::metadata(&nem_file)?.len())
                );
            } else {
                println!("{} [OK]", "[✓]".bold().green());
            }
        } else {
            println!("{}", "[FAIL]".bold().red());
        }
        results.push((project.name.clone(), ok));
    }

    let elapsed = start.elapsed().as_secs_f64();
    println!("{} NEM drivers built in {:.1}s", "[✓]".bold().green(), elapsed);
    Ok(results)
}

pub fn build_all(cfg: &Config, disc: &Discovery) -> Result<BuildReport> {
    let overall_start = Instant::now();

    // Ensure required Rust targets are installed
    ensure_targets(cfg)?;

    let kernel = build_kernel(cfg, disc).ok();
    let bootloader = build_bootloader(cfg, disc).ok();
    let user_bins = build_user_bins(disc)?;
    let nxl_libs = build_nxl_libs(disc)?;
    let nem_drivers = build_nem_drivers(disc)?;
    let duration = overall_start.elapsed();

    Ok(BuildReport {
        kernel,
        bootloader,
        user_bins,
        nxl_libs,
        nem_drivers,
        duration,
    })
}

pub fn ensure_targets(cfg: &Config) -> Result<()> {
    for target in [&cfg.bootloader_target, &cfg.kernel_target] {
        let status = Command::new("rustup")
            .args(["target", "add", target])
            .status()
            .context("Failed to run rustup target add")?;
        if !status.success() {
            eprintln!("  Warning: could not verify target {}", target);
        }
    }
    Ok(())
}
