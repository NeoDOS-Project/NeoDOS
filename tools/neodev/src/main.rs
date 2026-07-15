mod build;
mod clean;
mod config;
mod discovery;
mod image;
mod report;
mod run;
mod test_;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::*;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "neodev", about = "NeoDOS Development Tool", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build NeoDOS components
    Build {
        /// Build kernel
        #[arg(long)]
        kernel: bool,
        /// Build bootloader
        #[arg(long)]
        bootloader: bool,
        /// Build user binaries (NXE)
        #[arg(long)]
        userbin: bool,
        /// Build NXL shared libraries
        #[arg(long)]
        nxl: bool,
        /// Build NEM drivers
        #[arg(long)]
        nem: bool,
        /// Build everything (default)
        #[arg(long)]
        all: bool,
        /// Build kernel + bootloader only (quick build)
        #[arg(long)]
        quick: bool,
        /// Generate image after build
        #[arg(long)]
        image: bool,
        /// NeoDOS FS size in MB (default: 100)
        #[arg(long, default_value_t = 100)]
        neodos_size: u64,
        /// NE2 filesystem blocks (default: 25600 = 100 MB at 4 KB/block)
        #[arg(long, default_value_t = 25600)]
        neodos_blocks: u64,
    },
    /// Create disk images (NE2, ESP, GPT)
    Image {
        /// Output disk image path
        #[arg(long, default_value = "disk_image.img")]
        output: PathBuf,
        /// ESP size in MB
        #[arg(long, default_value_t = 100)]
        esp_size: u64,
        /// NeoDOS FS size in MB
        #[arg(long, default_value_t = 100)]
        neodos_size: u64,
        /// NE2 filesystem blocks (default: 25600 = 100 MB at 4 KB/block)
        #[arg(long, default_value_t = 25600)]
        blocks: u64,
        /// Label for the NE2 volume
        #[arg(long, default_value = "NEODOS")]
        label: String,
        /// Skip building — use existing artifacts
        #[arg(long)]
        no_build: bool,
    },
    /// Run NeoDOS in QEMU
    Run {
        /// Storage controller: ahci, ata, nvme, virtio
        #[arg(long, default_value = "ahci")]
        storage: String,
        /// Network mode: user, tap, bridge
        #[arg(long, default_value = "user")]
        net: String,
        /// Enable KVM acceleration
        #[arg(long)]
        kvm: bool,
        /// Enable GDB server on :1234
        #[arg(long)]
        gdb: bool,
        /// BDM mode (persistent OVMF vars)
        #[arg(long)]
        bdm: bool,
        /// Headless mode (no display)
        #[arg(long)]
        headless: bool,
        /// Serial output to file instead of stdio
        #[arg(long)]
        serial: Option<String>,
    },
    /// Run tests (NeoTest integration)
    Test {
        /// Storage controller: ahci, ata, virtio
        #[arg(long, default_value = "ahci")]
        storage: String,
        /// Enable KVM acceleration
        #[arg(long)]
        kvm: bool,
        /// Number of iterations
        #[arg(long, default_value_t = 1)]
        iterations: u32,
        /// Test timeout in seconds
        #[arg(long, default_value_t = 180)]
        timeout: u64,
    },
    /// Clean build artifacts
    Clean {
        /// Clean everything
        #[arg(long)]
        all: bool,
        /// Clean kernel only
        #[arg(long)]
        kernel: bool,
        /// Clean bootloader only
        #[arg(long)]
        bootloader: bool,
        /// Clean user binaries only
        #[arg(long)]
        userbin: bool,
        /// Clean NXL libraries only
        #[arg(long)]
        nxl: bool,
        /// Clean NEM drivers only
        #[arg(long)]
        nem: bool,
        /// Clean images only
        #[arg(long)]
        images: bool,
    },
    /// Show project configuration
    Config,
    /// List discovered projects
    List,
    /// Build NXP packages
    Nxp {
        /// Build NXP for all discovered user binaries
        #[arg(long)]
        all: bool,
        /// Build NXP for specific user binary
        name: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let project_root = find_project_root()?;
    let cfg = config::Config::load(&project_root)?;
    let disc = discovery::discover(&project_root)?;

    match &cli.command {
        Commands::Build {
            kernel,
            bootloader,
            userbin,
            nxl,
            nem,
            all,
            quick,
            image,
            neodos_size,
            neodos_blocks,
        } => cmd_build(&cfg, &disc, *kernel, *bootloader, *userbin, *nxl, *nem, *all, *quick, *image, *neodos_size, *neodos_blocks),
        Commands::Image {
            output,
            esp_size,
            neodos_size,
            blocks,
            label,
            no_build,
        } => cmd_image(&cfg, &disc, output, *esp_size, *neodos_size, *blocks, label, *no_build),
        Commands::Run {
            storage,
            net,
            kvm,
            gdb,
            bdm,
            headless,
            serial,
        } => cmd_run(&cfg, storage, net, *kvm, *gdb, *bdm, *headless, serial.as_deref()),
        Commands::Test {
            storage,
            kvm,
            iterations,
            timeout,
        } => cmd_test(&cfg, storage, *kvm, *iterations, *timeout),
        Commands::Clean {
            all,
            kernel,
            bootloader,
            userbin,
            nxl,
            nem,
            images,
        } => cmd_clean(&cfg, &disc, *all, *kernel, *bootloader, *userbin, *nxl, *nem, *images),
        Commands::Config => cmd_config(&cfg),
        Commands::List => cmd_list(&cfg),
        Commands::Nxp { all, name } => cmd_nxp(&cfg, &disc, *all, name.as_deref()),
    }
}

fn find_project_root() -> Result<PathBuf> {
    let mut dir = std::env::current_dir()?;
    loop {
        if dir.join("neodos-kernel").join("Cargo.toml").exists()
            || dir.join("bootloader.efi").exists()
        {
            return Ok(dir);
        }
        if !dir.pop() {
            anyhow::bail!(
                "Could not find NeoDOS project root. Run neodev from within the project directory."
            );
        }
    }
}

fn cmd_build(
    cfg: &config::Config,
    disc: &discovery::Discovery,
    kernel: bool,
    bootloader: bool,
    userbin: bool,
    nxl: bool,
    nem: bool,
    all: bool,
    quick: bool,
    image: bool,
    neodos_size: u64,
    neodos_blocks: u64,
) -> Result<()> {
    println!("{} NeoDOS Build", "[*]".bold().cyan());
    println!();

    if all || (!kernel && !bootloader && !userbin && !nxl && !nem && !quick) {
        // Default: build all
        let report = build::build_all(cfg, disc)?;
        report::print_build_report(&report);
        if image && report.bootloader.unwrap_or(false) {
            cmd_image(cfg, disc, &cfg.project_root.join("disk_image.img"), cfg.esp_size_mb, neodos_size, neodos_blocks, "NEODOS", true)?;
        } else if image {
            println!("  {} Skipping image generation (build had failures)", "[!]".bold().yellow());
        }
        return Ok(());
    }

    // Individual builds
    if quick {
        build::ensure_targets(cfg)?;
        build::build_kernel(cfg, disc)?;
        build::build_bootloader(cfg, disc)?;
        if image {
            cmd_image(cfg, disc, &cfg.project_root.join("disk_image.img"), cfg.esp_size_mb, neodos_size, neodos_blocks, "NEODOS", true)?;
        }
        return Ok(());
    }

    build::ensure_targets(cfg)?;
    let mut kernel_ok = true;
    let mut bl_ok = true;

    if kernel {
        kernel_ok = build::build_kernel(cfg, disc).unwrap_or(false);
    }
    if bootloader {
        bl_ok = build::build_bootloader(cfg, disc).unwrap_or(false);
    }
    if userbin {
        let _ = build::build_user_bins(disc)?;
    }
    if nxl {
        let _ = build::build_nxl_libs(disc)?;
    }
    if nem {
        let _ = build::build_nem_drivers(disc)?;
    }

    if image && kernel_ok && bl_ok {
        cmd_image(cfg, disc, &cfg.project_root.join("disk_image.img"), cfg.esp_size_mb, cfg.neodos_size_mb, 2560, "NEODOS", true)?;
    }

    Ok(())
}

fn cmd_image(
    cfg: &config::Config,
    disc: &discovery::Discovery,
    output: &PathBuf,
    _esp_size: u64,
    _neodos_size: u64,
    blocks: u64,
    label: &str,
    no_build: bool,
) -> Result<()> {
    println!("{} NeoDOS Image Generation", "[*]".bold().cyan());
    println!();

    if !no_build {
        // Compile NLT files before image generation
        println!("  Compiling NLT translation files...");
        let _ = build::compile_nlt_files(cfg);

        // Quick build of kernel + bootloader if needed
        if !cfg.project_root.join("kernel.elf").exists()
            || !cfg.project_root.join("bootloader.efi").exists()
        {
            println!("  Building kernel and bootloader first...");
            let _ = build::build_kernel(cfg, disc);
            let _ = build::build_bootloader(cfg, disc);
        }
    }

    // 1. Generate registry hive
    image::generate_registry_hive(cfg)?;

    // 2. Build NE2 filesystem image
    let fs_image = cfg.project_root.join("scripts").join("neodos_image.img");
    image::build_ne2_image(cfg, disc, &fs_image, label, blocks)?;

    // 3. Create ESP partition
    let esp_image = image::create_esp_image(cfg)?;

    // 4. Create unified GPT disk image
    image::create_gpt_image(cfg, &esp_image, &fs_image, output)?;

    // Cleanup temp files
    if esp_image.exists() {
        let _ = std::fs::remove_file(&esp_image);
    }

    println!();
    println!(
        "{} Disk image ready: {}",
        "[✓]".bold().green(),
        output.display()
    );
    println!("  Next: neodev run");

    Ok(())
}

fn cmd_run(
    cfg: &config::Config,
    storage_str: &str,
    net_str: &str,
    kvm: bool,
    gdb: bool,
    bdm: bool,
    headless: bool,
    serial: Option<&str>,
) -> Result<()> {
    let storage = match storage_str {
        "ahci" => run::StorageMode::Ahci,
        "ata" => run::StorageMode::Ata,
        "nvme" => run::StorageMode::Nvme,
        "virtio" => run::StorageMode::Virtio,
        _ => anyhow::bail!("Unknown storage mode: {}. Use: ahci, ata, nvme, virtio", storage_str),
    };

    let net = match net_str {
        "user" => run::NetMode::User,
        "tap" => run::NetMode::Tap,
        "bridge" => run::NetMode::Bridge,
        _ => anyhow::bail!("Unknown net mode: {}. Use: user, tap, bridge", net_str),
    };

    let opts = run::RunOptions {
        storage,
        net,
        kvm,
        gdb,
        headless,
        bdm,
        serial_file: serial.map(|s| s.to_string()),
        ..Default::default()
    };

    run::run_qemu(cfg, &opts)
}

fn cmd_test(
    cfg: &config::Config,
    storage_str: &str,
    kvm: bool,
    iterations: u32,
    timeout: u64,
) -> Result<()> {
    use test_::TestOptions;

    let storage = match storage_str {
        "ahci" => run::StorageMode::Ahci,
        "ata" => run::StorageMode::Ata,
        "virtio" => run::StorageMode::Virtio,
        _ => anyhow::bail!("Unknown storage mode: {}. Use: ahci, ata, virtio", storage_str),
    };

    let opts = TestOptions {
        storage,
        kvm,
        iterations,
        timeout,
    };

    test_::run_tests(cfg, &opts)?;
    Ok(())
}

fn cmd_clean(
    cfg: &config::Config,
    disc: &discovery::Discovery,
    all: bool,
    kernel: bool,
    bootloader: bool,
    userbin: bool,
    nxl: bool,
    nem: bool,
    images: bool,
) -> Result<()> {
    let opts = clean::CleanOptions {
        all,
        kernel,
        bootloader,
        userbin,
        nxl,
        nem,
        images,
    };
    clean::clean(cfg, disc, &opts)
}

fn cmd_config(cfg: &config::Config) -> Result<()> {
    println!("{} NeoDOS Configuration", "[*]".bold().cyan());
    println!("  Project root:     {}", cfg.project_root.display());
    println!("  Kernel target:    {}", cfg.kernel_target);
    println!("  Bootloader target: {}", cfg.bootloader_target);
    println!("  ESP size:         {} MB", cfg.esp_size_mb);
    println!("  NeoDOS FS size:   {} MB", cfg.neodos_size_mb);
    println!("  QEMU memory:      {}", cfg.qemu_memory);
    println!("  OVMF code:        {}", cfg.ovmf_code.display());
    println!("  OVMF vars:        {}", cfg.ovmf_vars_template.display());

    report::print_discovery_report(cfg)?;
    Ok(())
}

fn cmd_list(cfg: &config::Config) -> Result<()> {
    report::print_discovery_report(cfg)
}

fn cmd_nxp(cfg: &config::Config, disc: &discovery::Discovery, all: bool, name: Option<&str>) -> Result<()> {
    build::build_nxp_packages(cfg, disc, all, name)
}
