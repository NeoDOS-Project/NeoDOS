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

pub fn compile_nlt_files(cfg: &Config) -> Result<()> {
    let nltc_path = cfg.project_root.join("tools").join("nltc");
    let nltc_bin = nltc_path.join("target").join("debug").join("nltc");

    // Build nltc first if binary doesn't exist
    if !nltc_bin.exists() {
        println!("  {} Building nltc (NLT compiler)...", "[*]".bold().cyan());
        let status = Command::new("cargo")
            .args(["build"])
            .current_dir(&nltc_path)
            .status()
            .context("Failed to build nltc")?;
        if !status.success() {
            anyhow::bail!("nltc build failed");
        }
    }

    // Compile all TOML files in data/locale/
    let locale_dir = cfg.project_root.join("data").join("locale");
    if !locale_dir.exists() {
        println!("  {} No locale directory found at {}", "[!]".bold().yellow(), locale_dir.display());
        return Ok(());
    }

    for entry in std::fs::read_dir(&locale_dir)? {
        let entry = entry?;
        let lang_dir = entry.path();
        if !lang_dir.is_dir() { continue; }

        let mut compiled = 0;
        for file_entry in std::fs::read_dir(&lang_dir)? {
            let file_entry = file_entry?;
            let path = file_entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") { continue; }

            let input = &path;
            let mut output = path.to_path_buf();
            output.set_extension("nlt");

            let status = Command::new(&nltc_bin)
                .args([input.to_str().unwrap(), output.to_str().unwrap()])
                .status()
                .with_context(|| format!("Failed to compile NLT: {}", input.display()))?;

            if status.success() {
                compiled += 1;
            } else {
                anyhow::bail!("NLT compilation failed for: {}", input.display());
            }
        }
        if compiled > 0 {
            println!("  {} NLT files compiled for {}", "[✓]".bold().green(), lang_dir.file_name().unwrap().to_string_lossy());
        }
    }
    Ok(())
}

pub fn build_all(cfg: &Config, disc: &Discovery) -> Result<BuildReport> {
    let overall_start = Instant::now();

    // Ensure required Rust targets are installed
    ensure_targets(cfg)?;

    // Compile NLT files
    println!("{} Compiling NLT translation files...", "[*]".bold().cyan());
    compile_nlt_files(cfg)?;

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

/// Discover NXP packages (neopkg.toml files in userbin/ or standalone dirs)
pub struct NxpProject {
    pub name: String,
    pub dir: std::path::PathBuf,
}

pub fn discover_nxp_projects(root: &std::path::Path) -> Vec<NxpProject> {
    let mut projects = Vec::new();

    // Scan userbin/ for neopkg.toml files
    let userbin_dir = root.join("userbin");
    if userbin_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&userbin_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() { continue; }
                let toml_path = path.join("neopkg.toml");
                if toml_path.exists() {
                    let name = path.file_name().unwrap().to_string_lossy().to_string();
                    projects.push(NxpProject { name, dir: path });
                }
            }
        }
    }

    // Also scan root-level packages/ directory
    let pkg_dir = root.join("packages");
    if pkg_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&pkg_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() { continue; }
                let toml_path = path.join("neopkg.toml");
                if toml_path.exists() {
                    let name = path.file_name().unwrap().to_string_lossy().to_string();
                    // Avoid duplicates from userbin/
                    if !projects.iter().any(|p| p.name == name) {
                        projects.push(NxpProject { name, dir: path });
                    }
                }
            }
        }
    }

    projects
}

pub fn build_nxp_packages(
    _cfg: &Config,
    _disc: &Discovery,
    all: bool,
    name: Option<&str>,
) -> Result<()> {
    let projects = discover_nxp_projects(&_cfg.project_root);
    if projects.is_empty() {
        println!("  {} No NXP projects found (no neopkg.toml files)", "[!]".bold().yellow());
        println!("  {} To create one: place a neopkg.toml in userbin/<name>/", "   ".bold().cyan());
        return Ok(());
    }

    let mut built = 0;
    for project in &projects {
        if let Some(n) = name {
            if project.name != n { continue; }
        }

        let nxp_output = _cfg.project_root.join("packages").join(format!("{}.nxp", project.name));
        std::fs::create_dir_all(nxp_output.parent().unwrap())?;

        print!("  {:20} ", project.name);
        std::io::Write::flush(&mut std::io::stdout())?;

        // Check if the NXE binary exists for this project
        let nxe_path = _cfg.project_root.join("userbin").join(format!("{}.nxe", project.name));
        if !nxe_path.exists() && !all {
            println!("{} [SKIP - no .nxe]", "[!]".bold().yellow());
            continue;
        }

        // Look for the nxpkg tool
        let nxpkg_path = _cfg.project_root.join("tools").join("nxpkg")
            .join("target").join("debug").join("nxpkg");
        let nxpkg_exists = nxpkg_path.exists();

        if !nxpkg_exists {
            println!("{} [SKIP - nxpkg not built]", "[!]".bold().yellow());
            continue;
        }

        let status = std::process::Command::new(&nxpkg_path)
            .args(["create", project.dir.to_str().unwrap(), nxp_output.to_str().unwrap()])
            .status()
            .with_context(|| format!("Failed to create NXP for {}", project.name))?;

        if status.success() {
            println!("{} [OK]", "[✓]".bold().green());
            built += 1;
        } else {
            println!("{} [FAIL]", "[✗]".bold().red());
        }
    }

    if built > 0 {
        println!("  {} NXP packages built", "[✓]".bold().green());
    }
    Ok(())
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
