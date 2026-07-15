use crate::config::Config;
use crate::vmm::{HypervisorBackend, NetworkMode, StorageMode, VmConfig, VmInstance, VmStatus};
use anyhow::{Context, Result};
use colored::*;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct QemuBackend;

struct QemuInstance {
    child: Child,
    serial_file: Option<std::path::PathBuf>,
}

impl VmInstance for QemuInstance {
    fn serial_path(&self) -> Option<&Path> {
        self.serial_file.as_deref()
    }

    fn wait_timeout(&mut self, timeout: Duration) -> Result<Option<i32>> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match self.child.try_wait()? {
                Some(status) => return Ok(status.code()),
                None => std::thread::sleep(Duration::from_millis(100)),
            }
        }
        Ok(None)
    }

    fn kill(&mut self) -> Result<()> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }

    fn pid(&self) -> Option<u32> {
        Some(self.child.id())
    }
}

impl HypervisorBackend for QemuBackend {
    fn name(&self) -> &str {
        "qemu"
    }

    fn check_prerequisites(&self, cfg: &Config) -> Result<()> {
        let qemu = which("qemu-system-x86_64");
        if qemu.is_none() {
            anyhow::bail!("qemu-system-x86_64 not found. Install QEMU (qemu-kvm / qemu-system-x86)");
        }
        if !cfg.ovmf_code.exists() {
            anyhow::bail!("OVMF_CODE not found at {}. Install OVMF (edk2-ovmf / ovmf)", cfg.ovmf_code.display());
        }
        if !cfg.ovmf_vars_template.exists() {
            anyhow::bail!("OVMF_VARS template not found at {}. Install OVMF", cfg.ovmf_vars_template.display());
        }
        Ok(())
    }

    fn ensure_vm(&self, _cfg: &Config, vmcfg: &VmConfig) -> Result<()> {
        let disk = &vmcfg.disk_image;
        if !disk.exists() {
            anyhow::bail!(
                "Disk image not found: {}\nRun 'neodev build --image' first",
                disk.display()
            );
        }
        Ok(())
    }

    fn delete_vm(&self, _cfg: &Config, _vmcfg: &VmConfig) -> Result<()> {
        // QEMU does not maintain persistent VM state; nothing to delete
        Ok(())
    }

    fn run(&self, cfg: &Config, vmcfg: &VmConfig) -> Result<()> {
        let start = Instant::now();
        println!("{} NeoDOS QEMU Session", "[*]".bold().cyan());
        println!();

        let mut cmd = Command::new("qemu-system-x86_64");

        // Machine and accelerator
        let kvm = cfg.qemu_kvm;
        let accel = if kvm && Path::new("/dev/kvm").exists() {
            "kvm"
        } else {
            if kvm {
                eprintln!("  {} KVM requested but /dev/kvm not available; using TCG", "[!]".bold().yellow());
            }
            "tcg"
        };
        println!("  QEMU accelerator: {}", accel);
        cmd.args(["-machine", &format!("q35,accel={}", accel)]);

        // Monitor
        cmd.args(["-monitor", "telnet:127.0.0.1:4444,server,nowait"]);
        println!("  QEMU Monitor: localhost:4444");

        // GDB
        if vmcfg.gdb {
            cmd.args(["-gdb", &format!("tcp::{}", vmcfg.gdb_port)]);
            println!("  GDB:          localhost:{} (use 'gdb -x .gdbinit')", vmcfg.gdb_port);
        }

        // Display
        if vmcfg.headless {
            cmd.args(["-display", "none"]);
        }

        // OVMF
        let ovmf_vars = if cfg.qemu_bdm {
            let persistent = cfg.project_root.join("OVMF_VARS.fd");
            if !persistent.exists() {
                std::fs::copy(&cfg.ovmf_vars_template, &persistent)?;
                println!("  Created persistent OVMF_VARS: {}", persistent.display());
            }
            println!("  BDM mode: preserving OVMF_VARS");
            persistent
        } else {
            let tmp = std::path::PathBuf::from(format!("/tmp/OVMF_VARS_{}.fd", std::process::id()));
            std::fs::copy(&cfg.ovmf_vars_template, &tmp)?;
            tmp
        };

        cmd.args([
            "-drive",
            &format!("if=pflash,format=raw,readonly=on,file={}", cfg.ovmf_code.display()),
            "-drive",
            &format!("if=pflash,format=raw,file={}", ovmf_vars.display()),
        ]);

        // Storage
        let disk_image = &vmcfg.disk_image;
        match vmcfg.storage_mode {
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
        match vmcfg.network {
            NetworkMode::User => {
                cmd.args([
                    "-netdev", "user,id=net0,net=10.0.1.0/24,dhcpstart=10.0.1.80,host=10.0.1.1",
                    "-device", "e1000,netdev=net0",
                ]);
                println!("  Network: user-mode (SLiRP)");
            }
            NetworkMode::Bridged => {
                let bridge_name = std::env::var("NEODOS_BRIDGE").unwrap_or_else(|_| "neodos0".into());
                if Path::new("/dev/net/tun").exists() {
                    cmd.args([
                        "-netdev", "tap,id=net0,ifname=tap0,script=no",
                        "-device", "e1000,netdev=net0",
                    ]);
                    println!("  Network: TAP (tap0)");
                } else {
                    cmd.args([
                        "-netdev", &format!("bridge,id=net0,br={}", bridge_name),
                        "-device", "e1000,netdev=net0",
                    ]);
                    println!("  Network: bridge ({})", bridge_name);
                }
            }
            NetworkMode::None => {
                println!("  Network: disabled");
            }
        }

        // Memory
        cmd.args(["-m", &format!("{}M", vmcfg.memory_mb)]);

        // Serial
        if let Some(ref serial_file) = vmcfg.serial_file {
            cmd.args(["-serial", &format!("file:{}", serial_file.display())]);
        } else {
            cmd.arg("-serial").arg("stdio");
        }

        println!();
        println!("{}", "==========================================".bold());
        println!("{}", "Launching QEMU...".bold());
        if !vmcfg.headless {
            println!("{}", "Close the QEMU window to exit".bold());
        }
        println!("{}", "==========================================".bold());
        println!();

        let output_log = cfg.project_root.join("qemu_output.log");
        let log_file = std::fs::File::create(&output_log)?;

        cmd.stdout(Stdio::from(log_file.try_clone()?));
        cmd.stderr(Stdio::from(log_file));
        cmd.stdin(Stdio::inherit());

        let mut child = cmd.spawn().context("Failed to launch QEMU")?;
        let exit_status = child.wait()?;

        if !cfg.qemu_bdm {
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

    fn start_headless(&self, cfg: &Config, vmcfg: &VmConfig) -> Result<Box<dyn VmInstance>> {
        let mut cmd = Command::new("qemu-system-x86_64");

        let accel = if cfg.qemu_kvm && Path::new("/dev/kvm").exists() {
            "kvm"
        } else {
            "tcg"
        };

        cmd.args([
            "-machine", &format!("q35,accel={}", accel),
            "-monitor", "telnet:127.0.0.1:4446,server,nowait",
            "-display", "none",
            "-no-reboot",
        ]);

        let ovmf_vars = format!("/tmp/OVMF_VARS_test_{}.fd", std::process::id());
        std::fs::copy(&cfg.ovmf_vars_template, &ovmf_vars)?;

        cmd.args([
            "-drive", &format!("if=pflash,format=raw,readonly=on,file={}", cfg.ovmf_code.display()),
            "-drive", &format!("if=pflash,format=raw,file={}", ovmf_vars),
        ]);

        let disk_image = &vmcfg.disk_image;
        match vmcfg.storage_mode {
            StorageMode::Ahci => {
                cmd.args([
                    "-device", "ahci,id=ahci",
                    "-drive", &format!("if=none,format=raw,file={},id=mydisk", disk_image.display()),
                    "-device", "ide-hd,drive=mydisk,bus=ahci.0",
                ]);
            }
            StorageMode::Virtio => {
                cmd.args([
                    "-drive", &format!("if=none,format=raw,file={},id=virtioblk", disk_image.display()),
                    "-device", "virtio-blk-pci,disable-modern=on,drive=virtioblk",
                ]);
            }
            _ => {
                cmd.args([
                    "-drive", &format!("format=raw,file={},index=0,media=disk", disk_image.display()),
                ]);
            }
        }

        cmd.args([
            "-netdev", "user,id=net0,net=10.0.1.0/24,dhcpstart=10.0.1.80,host=10.0.1.1",
            "-device", "e1000,netdev=net0",
            "-m", &format!("{}M", vmcfg.memory_mb),
        ]);

        if let Some(ref serial_file) = vmcfg.serial_file {
            cmd.args(["-serial", &format!("file:{}", serial_file.display())]);
        }

        let child = cmd
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to start QEMU")?;

        Ok(Box::new(QemuInstance {
            child,
            serial_file: vmcfg.serial_file.clone(),
        }))
    }

    fn stop(&self, _cfg: &Config, _vmcfg: &VmConfig) -> Result<()> {
        // For QEMU, we kill the process. This is a best-effort via pkill.
        let _ = Command::new("pkill")
            .args(["-f", "qemu-system-x86_64.*NeoDOS"])
            .status();
        // Also try to use the monitor
        if let Ok(mut stream) = std::net::TcpStream::connect_timeout(
            &"127.0.0.1:4444".parse().unwrap(),
            Duration::from_secs(2),
        ) {
            use std::io::Write;
            let _ = stream.write_all(b"quit\n");
        }
        let _ = Command::new("pkill")
            .args(["-f", "qemu-system-x86_64"])
            .status();
        Ok(())
    }

    fn reset(&self, _cfg: &Config, _vmcfg: &VmConfig) -> Result<()> {
        // Try monitor reset via telnet
        if let Ok(mut stream) = std::net::TcpStream::connect_timeout(
            &"127.0.0.1:4444".parse().unwrap(),
            Duration::from_secs(2),
        ) {
            use std::io::Write;
            let _ = stream.write_all(b"system_reset\n");
            println!("  Reset command sent via QEMU monitor");
        } else {
            anyhow::bail!("Cannot reset: QEMU monitor not reachable on :4444");
        }
        Ok(())
    }

    fn status(&self, _cfg: &Config, _vmcfg: &VmConfig) -> Result<VmStatus> {
        // Check if any qemu-system-x86_64 process is running
        let output = Command::new("pgrep")
            .args(["-f", "qemu-system-x86_64"])
            .output()
            .ok();
        match output {
            Some(o) if o.status.success() => Ok(VmStatus::Running),
            _ => Ok(VmStatus::Stopped),
        }
    }
}

fn which(cmd: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        for dir in std::env::split_paths(&paths) {
            let full = dir.join(cmd);
            if full.is_file() {
                return Some(full);
            }
        }
        None
    })
}
