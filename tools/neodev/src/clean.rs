use crate::config::Config;
use crate::discovery::Discovery;
use anyhow::Result;
use colored::*;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct CleanOptions {
    pub all: bool,
    pub kernel: bool,
    pub bootloader: bool,
    pub userbin: bool,
    pub nxl: bool,
    pub nem: bool,
    pub images: bool,
}

impl Default for CleanOptions {
    fn default() -> Self {
        Self {
            all: true,
            kernel: false,
            bootloader: false,
            userbin: false,
            nxl: false,
            nem: false,
            images: false,
        }
    }
}

impl CleanOptions {
    fn should_clean(&self, flag: bool) -> bool {
        self.all || flag
    }
}

pub fn clean(cfg: &Config, disc: &Discovery, opts: &CleanOptions) -> Result<()> {
    let start = Instant::now();
    println!("{} Cleaning NeoDOS build artifacts...", "[*]".bold().cyan());

    if opts.should_clean(opts.kernel) {
        clean_cargo_target(disc.kernel.as_ref().map(|p| &p.path));
    }

    if opts.should_clean(opts.bootloader) {
        clean_cargo_target(disc.bootloader.as_ref().map(|p| &p.path));
    }

    if opts.should_clean(opts.userbin) {
        for project in &disc.user_bins {
            clean_cargo_target(Some(&project.path));
        }
    }

    if opts.should_clean(opts.nxl) {
        for project in &disc.nxl_libs {
            clean_cargo_target(Some(&project.path));
        }
    }

    if opts.should_clean(opts.images) {
        let root = &cfg.project_root;
        remove_if_exists(&root.join("tmp_esp.img"));
        remove_if_exists(&root.join("disk_image.img"));
        remove_if_exists(&root.join("disk_image.vdi"));
        remove_if_exists(&root.join("scripts/neodos_image.img"));
        remove_if_exists(&root.join("scripts/neodos_image2.img"));
        remove_if_exists(&root.join("scripts/system.hiv"));
    }

    if opts.should_clean(opts.nem) || opts.all {
        let nem_dir = format!("/tmp/nem_drivers_{}", std::process::id());
        let _ = std::fs::remove_dir_all(&nem_dir);
    }

    // Root-level artifacts
    if opts.all {
        let root = &cfg.project_root;
        remove_if_exists(&root.join("bootloader.efi"));
        remove_if_exists(&root.join("kernel.elf"));
        for entry in walkdir::WalkDir::new(root.join("userbin"))
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".nxe") || name.ends_with(".elf") {
                remove_if_exists(entry.path());
            }
        }
        // NXL files at root
        for entry in walkdir::WalkDir::new(root)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".nxl") {
                remove_if_exists(entry.path());
            }
        }
        // Logs
        for entry in walkdir::WalkDir::new(root)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".log") {
                remove_if_exists(entry.path());
            }
        }
    }

    println!(
        "{} Clean completed in {:.1}s",
        "[✓]".bold().green(),
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

fn clean_cargo_target(path: Option<&PathBuf>) {
    if let Some(path) = path {
        let target_dir = path.join("target");
        if target_dir.exists() {
            match std::fs::remove_dir_all(&target_dir) {
                Ok(_) => println!("  Removed: {}", target_dir.display()),
                Err(e) => eprintln!("  Error removing {}: {}", target_dir.display(), e),
            }
        }
    }
}

fn remove_if_exists(path: &Path) {
    if path.is_dir() {
        if let Err(e) = std::fs::remove_dir_all(path) {
            eprintln!("  Error removing {}: {}", path.display(), e);
        }
    } else if path.exists() {
        if let Err(e) = std::fs::remove_file(path) {
            eprintln!("  Error removing {}: {}", path.display(), e);
        }
    }
}
