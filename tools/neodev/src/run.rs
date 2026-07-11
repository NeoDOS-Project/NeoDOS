use crate::config::Config;
use anyhow::{Context, Result};
use colored::*;
use std::process::{Command, Stdio};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StorageMode {
    Ahci,
    Ata,
    Nvme,
    Virtio,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NetMode {
    User,
    Tap,
    Bridge,
}

pub struct RunOptions {
    pub storage: StorageMode,
    pub net: NetMode,
    pub kvm: bool,
    pub gdb: bool,
    pub headless: bool,
    pub bdm: bool,
    pub serial_file: Option<String>,
    pub monitor_port: Option<u16>,
    pub gdb_port: Option<u16>,
    pub extra_args: Vec<String>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            storage: StorageMode::Ahci,
            net: NetMode::User,
            kvm: false,
            gdb: false,
            headless: false,
            bdm: false,
            serial_file: None,
            monitor_port: Some(4444),
            gdb_port: Some(1234),
            extra_args: vec![],
        }
    }
}

pub fn run_qemu(cfg: &Config, opts: &RunOptions) -> Result<()> {
    let start = Instant::now();
    println!("{} NeoDOS QEMU Session", "[*]".bold().cyan());
    println!();

    let disk_image = cfg.project_root.join("disk_image.img");
    if !disk_image.exists() {
        anyhow::bail!("Disk image not found: {}\nRun 'neodev build --image' first", disk_image.display());
    }

    // Check OVMF
    let ovmf_code = &cfg.ovmf_code;
    if !ovmf_code.exists() {
        anyhow::bail!("OVMF_CODE not found at {}", ovmf_code.display());
    }
    let ovmf_vars_template = &cfg.ovmf_vars_template;
    if !ovmf_vars_template.exists() {
        anyhow::bail!("OVMF_VARS template not found at {}", ovmf_vars_template.display());
    }

    // OVMF VARS (permanent for BDM, ephemeral otherwise)
    let ovmf_vars = if opts.bdm {
        let persistent = cfg.project_root.join("OVMF_VARS.fd");
        if !persistent.exists() {
            std::fs::copy(ovmf_vars_template, &persistent)?;
            println!("  Created persistent OVMF_VARS: {}", persistent.display());
        }
        println!("  BDM mode: preserving OVMF_VARS");
        persistent
    } else {
        let tmp = std::path::PathBuf::from(format!("/tmp/OVMF_VARS_{}.fd", std::process::id()));
        std::fs::copy(ovmf_vars_template, &tmp)?;
        tmp
    };

    // QEMU command building
    let mut cmd = Command::new("qemu-system-x86_64");

    // Machine and accelerator
    let accel = if opts.kvm && std::path::Path::new("/dev/kvm").exists() {
        "kvm"
    } else {
        if opts.kvm {
            eprintln!("  {} KVM requested but /dev/kvm not available; using TCG", "[!]".bold().yellow());
        }
        "tcg"
    };
    println!("  QEMU accelerator: {}", accel);
    cmd.args(["-machine", &format!("q35,accel={}", accel)]);

    // Monitor
    let mon_port = opts.monitor_port.unwrap_or(4444);
    cmd.args([
        "-monitor",
        &format!("telnet:127.0.0.1:{},server,nowait", mon_port),
    ]);
    println!("  QEMU Monitor: localhost:{}", mon_port);

    // GDB
    if opts.gdb {
        let gdb_port = opts.gdb_port.unwrap_or(1234);
        cmd.args(["-gdb", &format!("tcp::{}", gdb_port)]);
        println!("  GDB:          localhost:{} (use 'gdb -x .gdbinit')", gdb_port);
    }

    // Display
    if opts.headless {
        cmd.args(["-display", "none"]);
    }

    // OVMF
    cmd.args([
        "-drive",
        &format!("if=pflash,format=raw,readonly=on,file={}", ovmf_code.display()),
        "-drive",
        &format!("if=pflash,format=raw,file={}", ovmf_vars.display()),
    ]);

    // Storage
    match opts.storage {
        StorageMode::Ahci => {
            cmd.args([
                "-device", "ahci,id=ahci",
                "-drive", &format!("if=none,format=raw,file={},id=mydisk", disk_image.display()),
                "-device", "ide-hd,drive=mydisk,bus=ahci.0",
            ]);
            println!("  Storage: AHCI Mode");
        }
        StorageMode::Ata => {
            cmd.args([
                "-drive", &format!("format=raw,file={},index=0,media=disk", disk_image.display()),
            ]);
            println!("  Storage: ATA/IDE Mode");
        }
        StorageMode::Nvme => {
            cmd.args([
                "-drive", &format!("if=none,format=raw,file={},id=nvm", disk_image.display()),
                "-device", "nvme,serial=deadbeef,drive=nvm",
            ]);
            println!("  Storage: NVMe Mode");
        }
        StorageMode::Virtio => {
            cmd.args([
                "-drive", &format!("if=none,format=raw,file={},id=virtioblk", disk_image.display()),
                "-device", "virtio-blk-pci,disable-legacy=on,drive=virtioblk",
            ]);
            println!("  Storage: VirtIO Block Mode");
        }
    }

    // Network
    match opts.net {
        NetMode::User => {
            cmd.args([
                "-netdev", "user,id=net0,net=10.0.1.0/24,dhcpstart=10.0.1.80,host=10.0.1.1",
                "-device", "e1000,netdev=net0",
            ]);
            println!("  Network: user-mode (SLiRP)");
        }
        NetMode::Tap => {
            if !std::path::Path::new("/dev/net/tun").exists() {
                anyhow::bail!("TAP mode requires /dev/net/tun. Fall back to user mode.");
            }
            cmd.args([
                "-netdev", "tap,id=net0,ifname=tap0,script=no",
                "-device", "e1000,netdev=net0",
            ]);
            println!("  Network: TAP (tap0)");
        }
        NetMode::Bridge => {
            let bridge_name = std::env::var("NEODOS_BRIDGE").unwrap_or_else(|_| "neodos0".into());
            cmd.args([
                "-netdev", &format!("bridge,id=net0,br={}", bridge_name),
                "-device", "e1000,netdev=net0",
            ]);
            println!("  Network: bridge ({})", bridge_name);
        }
    }

    // Memory
    cmd.args(["-m", &cfg.qemu_memory]);

    // Serial
    if let Some(serial_file) = &opts.serial_file {
        cmd.args(["-serial", &format!("file:{}", serial_file)]);
    } else {
        cmd.arg("-serial").arg("stdio");
    }

    // Extra arguments
    for arg in &opts.extra_args {
        cmd.arg(arg);
    }

    println!();
    println!("{}", "==========================================".bold());
    println!("{}", "Launching QEMU...".bold());
    if !opts.headless {
        println!("{}", "Close the QEMU window to exit".bold());
    }
    println!("{}", "==========================================".bold());
    println!();

    // Log output
    let output_log = cfg.project_root.join("qemu_output.log");
    let log_file = std::fs::File::create(&output_log)?;

    cmd.stdout(Stdio::from(log_file.try_clone()?));
    cmd.stderr(Stdio::from(log_file));
    cmd.stdin(Stdio::inherit());

    let mut child = cmd.spawn().context("Failed to launch QEMU")?;
    let exit_status = child.wait()?;

    // Cleanup
    if !opts.bdm {
        let _ = std::fs::remove_file(&ovmf_vars);
    }

    println!();
    println!("{} QEMU stopped (exit code: {})", "[*]".bold().cyan(), exit_status);
    println!("  Duration: {:.1}s", start.elapsed().as_secs_f64());
    println!("  Output saved to: {}", output_log.display());

    if !exit_status.success() {
        eprintln!("  {} QEMU exited with non-zero status", "[!]".bold().yellow());
    }

    Ok(())
}
