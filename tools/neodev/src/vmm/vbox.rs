use crate::config::Config;
use crate::vmm::{HypervisorBackend, NetworkMode, VmConfig, VmInstance, VmStatus};
use anyhow::{Context, Result};
use colored::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub struct VirtualBoxBackend;

struct VBoxInstance {
    serial_file: Option<PathBuf>,
    vm_name: String,
}

impl VmInstance for VBoxInstance {
    fn serial_path(&self) -> Option<&Path> {
        self.serial_file.as_deref()
    }

    fn wait_timeout(&mut self, timeout: Duration) -> Result<Option<i32>> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let status = vm_status(&self.vm_name)?;
            match status {
                VmStatus::Running | VmStatus::Paused => {
                    std::thread::sleep(Duration::from_millis(500));
                }
                VmStatus::Stopped => {
                    return Ok(Some(0));
                }
                VmStatus::NotFound => {
                    return Ok(None);
                }
            }
        }
        Ok(None)
    }

    fn kill(&mut self) -> Result<()> {
        vm_poweroff(&self.vm_name)
    }

    fn pid(&self) -> Option<u32> {
        None
    }
}

impl HypervisorBackend for VirtualBoxBackend {
    fn name(&self) -> &str {
        "virtualbox"
    }

    fn check_prerequisites(&self, _cfg: &Config) -> Result<()> {
        let vboxmanage = which("VBoxManage");
        if vboxmanage.is_none() {
            anyhow::bail!(
                "VBoxManage not found. Install VirtualBox and ensure VBoxManage is in PATH.\n\
                 Download: https://www.virtualbox.org/wiki/Linux_Downloads"
            );
        }

        // Check version
        let output = Command::new("VBoxManage")
            .args(["--version"])
            .output()
            .context("Failed to run VBoxManage --version")?;
        if output.status.success() {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            println!("  VirtualBox version: {}", version);
        } else {
            anyhow::bail!("VBoxManage reported an error. Is VirtualBox properly installed?");
        }

        Ok(())
    }

    fn ensure_vm(&self, _cfg: &Config, vmcfg: &VmConfig) -> Result<()> {
        let name = &vmcfg.name;

        // Ensure VM is not locked from a previous session
        let _ = vm_poweroff(name);
        std::thread::sleep(Duration::from_millis(500));

        // Check if disk image exists
        if !vmcfg.disk_image.exists() {
            anyhow::bail!(
                "Disk image not found: {}\nRun 'neodev build --image' first",
                vmcfg.disk_image.display()
            );
        }

        // Convert/refresh VDI
        let vdi_path = &vmcfg.disk_vdi;
        if vmcfg.disk_image.exists() {
            refresh_vdi(vmcfg)?;
        }

        if vm_exists(name) {
            println!("  VM '{}' already exists, reconfiguring", name);
            // Always reapply configuration and re-attach storage (handles VDI UUID changes)
            modify_vm(name, vmcfg)?;
            attach_storage(name, vdi_path)?;
            return Ok(());
        }

        // Create VM
        println!("  Creating VirtualBox VM '{}'...", name);
        let status = Command::new("VBoxManage")
            .args(["createvm", "--name", name, "--ostype", "Linux_64", "--register"])
            .status()
            .context("Failed to create VirtualBox VM")?;
        if !status.success() {
            anyhow::bail!("VBoxManage createvm failed");
        }

        // Configure and attach storage
        modify_vm(name, vmcfg)?;
        attach_storage(name, vdi_path)?;

        println!("  VM '{}' created successfully", name);
        Ok(())
    }

    fn delete_vm(&self, _cfg: &Config, vmcfg: &VmConfig) -> Result<()> {
        let name = &vmcfg.name;
        if !vm_exists(name) {
            println!("  VM '{}' does not exist", name);
            return Ok(());
        }
        println!("  Deleting VM '{}'...", name);
        let status = Command::new("VBoxManage")
            .args(["unregistervm", name, "--delete"])
            .status()
            .context("Failed to delete VM")?;
        if !status.success() {
            anyhow::bail!("VBoxManage unregistervm failed");
        }
        println!("  VM '{}' deleted", name);
        Ok(())
    }

    fn run(&self, cfg: &Config, vmcfg: &VmConfig) -> Result<()> {
        let name = &vmcfg.name;
        println!("{} NeoDOS VirtualBox Session", "[*]".bold().cyan());
        println!();

        // Ensure VM exists
        self.ensure_vm(cfg, vmcfg)?;

        // Start VM
        println!("  Starting VM '{}'...", name);
        let start_type = if vmcfg.headless { "headless" } else { "gui" };
        let mut cmd = Command::new("VBoxManage");
        cmd.args(["startvm", name, "--type", start_type]);

        println!("  VM type: {}", start_type);
        println!();
        println!("{}", "=".repeat(50));
        println!("  VM Name:     {}", name);
        println!("  Memory:      {} MB", vmcfg.memory_mb);
        println!("  CPUs:        {}", vmcfg.cpus);
        println!("  EFI:         {}", if vmcfg.efi { "enabled" } else { "disabled" });
        println!("  Disk:        {}", vmcfg.disk_vdi.display());
        println!("  Network:     {:?}", vmcfg.network);
        println!("  Serial:      {}", vmcfg.serial_file.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "stdout (GUI)".into()));
        println!("{}", "=".repeat(50));
        println!();

        let start = Instant::now();
        let output = cmd
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .context("Failed to start VirtualBox VM")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to start VM: {}", stderr);
        }

        // VBoxManage startvm exits immediately; the VM runs in background.
        // Wait for the VM to stop (poll status)
        println!("  VM started. Waiting for shutdown... (Ctrl+C to detach)");
        loop {
            std::thread::sleep(Duration::from_secs(2));
            match vm_status(name)? {
                VmStatus::Running | VmStatus::Paused => {
                    // Still running
                }
                VmStatus::Stopped | VmStatus::NotFound => {
                    println!();
                    println!("{} VM stopped", "[*]".bold().cyan());
                    println!("  Duration: {:.1}s", start.elapsed().as_secs_f64());
                    break;
                }
            }
        }

        Ok(())
    }

    fn start_headless(&self, _cfg: &Config, vmcfg: &VmConfig) -> Result<Box<dyn VmInstance>> {
        // NOT calling ensure_vm here — caller already does it.
        // Calling it again would poweroff the running VM via vm_poweroff().
        let name = &vmcfg.name;

        if !vm_exists(name) {
            anyhow::bail!("VM '{}' does not exist. Run 'neodev vm create' first.", name);
        }

        // Start VM in headless mode
        let output = Command::new("VBoxManage")
            .args(["startvm", name, "--type", "headless"])
            .output()
            .context("Failed to start VM in headless mode")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to start VM headless: {}", stderr);
        }

        // Give the VM time to initialize
        std::thread::sleep(Duration::from_secs(3));

        Ok(Box::new(VBoxInstance {
            serial_file: vmcfg.serial_file.clone(),
            vm_name: name.clone(),
        }))
    }

    fn stop(&self, _cfg: &Config, vmcfg: &VmConfig) -> Result<()> {
        let name = &vmcfg.name;
        println!("  Stopping VM '{}'...", name);

        // Try ACPI power button first
        let status = Command::new("VBoxManage")
            .args(["controlvm", name, "acpipowerbutton"])
            .status()
            .context("Failed to send ACPI poweroff")?;

        if status.success() {
            // Wait up to 10 seconds for graceful shutdown
            for _ in 0..10 {
                std::thread::sleep(Duration::from_secs(1));
                match vm_status(name)? {
                    VmStatus::Stopped | VmStatus::NotFound => {
                        println!("  VM stopped gracefully");
                        return Ok(());
                    }
                    _ => {}
                }
            }
            println!("  ACPI timeout, forcing poweroff...");
        }

        // Force poweroff
        let status = Command::new("VBoxManage")
            .args(["controlvm", name, "poweroff"])
            .status()
            .context("Failed to force poweroff VM")?;
        if !status.success() {
            // VM might already be stopped
        }
        println!("  VM powered off");
        Ok(())
    }

    fn reset(&self, _cfg: &Config, vmcfg: &VmConfig) -> Result<()> {
        let name = &vmcfg.name;
        println!("  Resetting VM '{}'...", name);
        let status = Command::new("VBoxManage")
            .args(["controlvm", name, "reset"])
            .status()
            .context("Failed to reset VM")?;
        if !status.success() {
            anyhow::bail!("VBoxManage controlvm reset failed");
        }
        Ok(())
    }

    fn status(&self, _cfg: &Config, vmcfg: &VmConfig) -> Result<VmStatus> {
        vm_status(&vmcfg.name)
    }
}

// ─── Helper functions ───

fn which(cmd: &str) -> Option<PathBuf> {
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

fn vm_exists(name: &str) -> bool {
    Command::new("VBoxManage")
        .args(["showvminfo", name])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn vm_status(name: &str) -> Result<VmStatus> {
    if !vm_exists(name) {
        return Ok(VmStatus::NotFound);
    }
    let output = Command::new("VBoxManage")
        .args(["showvminfo", name, "--machinereadable"])
        .output()
        .context("Failed to get VM status")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.contains("VMState=\"running\"") {
        Ok(VmStatus::Running)
    } else if stdout.contains("VMState=\"paused\"") {
        Ok(VmStatus::Paused)
    } else {
        Ok(VmStatus::Stopped)
    }
}

fn vm_poweroff(name: &str) -> Result<()> {
    let status = Command::new("VBoxManage")
        .args(["controlvm", name, "poweroff"])
        .status()
        .context("Failed to poweroff VM")?;
    if !status.success() {
        // VM might already be stopped
    }
    Ok(())
}

fn convert_to_vdi(raw_path: &Path, vdi_path: &Path) -> Result<()> {
    println!("  Converting {} to VDI...", raw_path.display());
    if vdi_path.exists() {
        std::fs::remove_file(vdi_path)?;
    }
    let status = Command::new("VBoxManage")
        .args([
            "convertfromraw",
            raw_path.to_str().unwrap(),
            vdi_path.to_str().unwrap(),
        ])
        .status()
        .context("Failed to convert disk image to VDI")?;
    if !status.success() {
        anyhow::bail!("VBoxManage convertfromraw failed");
    }
    println!("  VDI created: {}", vdi_path.display());
    Ok(())
}

fn refresh_vdi(vmcfg: &VmConfig) -> Result<()> {
    let raw = &vmcfg.disk_image;
    let vdi = &vmcfg.disk_vdi;

    if !vdi.exists() {
        return convert_to_vdi(raw, vdi);
    }

    // Check if raw image is newer than VDI
    let raw_modified = std::fs::metadata(raw)
        .and_then(|m| m.modified())
        .ok();
    let vdi_modified = std::fs::metadata(vdi)
        .and_then(|m| m.modified())
        .ok();

    let needs_refresh = match (raw_modified, vdi_modified) {
        (Some(raw_mtime), Some(vdi_mtime)) => raw_mtime > vdi_mtime,
        (Some(_), None) => true,
        _ => false,
    };

    if needs_refresh {
        println!("  Disk image updated, re-converting VDI...");
        convert_to_vdi(raw, vdi)?;
    }

    Ok(())
}

fn modify_vm(name: &str, vmcfg: &VmConfig) -> Result<()> {
    // Memory
    Command::new("VBoxManage")
        .args(["modifyvm", name, "--memory", &vmcfg.memory_mb.to_string()])
        .status()
        .context("Failed to set memory")?;

    // CPUs
    Command::new("VBoxManage")
        .args(["modifyvm", name, "--cpus", &vmcfg.cpus.to_string()])
        .status()
        .context("Failed to set CPUs")?;

    // EFI
    if vmcfg.efi {
        Command::new("VBoxManage")
            .args(["modifyvm", name, "--firmware", "efi"])
            .status()
            .context("Failed to enable EFI")?;
    }

    // Chipset (ICH9 for better ACPI support)
    Command::new("VBoxManage")
        .args(["modifyvm", name, "--chipset", "ich9"])
        .status()
        .context("Failed to set chipset")?;

    // Serial port (always enable with absolute path)
    let serial_path = vmcfg.serial_file.as_deref()
        .map(|p| {
            if p.is_absolute() { p.to_path_buf() }
            else { std::env::current_dir().unwrap_or_default().join(p) }
        })
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default().join("vbox_serial.log"));
    // Ensure parent directory exists
    if let Some(parent) = serial_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    Command::new("VBoxManage")
        .args([
            "modifyvm", name,
            "--uart1", "0x3F8", "4",
            "--uartmode1", "file", serial_path.to_str().unwrap(),
        ])
        .status()
        .context("Failed to configure serial port")?;

    // Network
    match vmcfg.network {
        NetworkMode::User => {
            Command::new("VBoxManage")
                .args([
                    "modifyvm", name,
                    "--nic1", "nat",
                    "--nictype1", "82540EM",
                    "--cableconnected1", "on",
                ])
                .status()
                .context("Failed to configure NAT network")?;
        }
        NetworkMode::Bridged => {
            // Detect active interface for bridging
            let bridge_iface = detect_bridge_interface();
            Command::new("VBoxManage")
                .args([
                    "modifyvm", name,
                    "--nic1", "bridged",
                    "--bridgeadapter1", &bridge_iface,
                    "--nictype1", "82540EM",
                    "--cableconnected1", "on",
                    "--nicpromisc1", "allow-all",
                ])
                .status()
                .context("Failed to configure bridged network")?;
            println!("  Bridged network via: {}", bridge_iface);
        }
        NetworkMode::None => {
            Command::new("VBoxManage")
                .args(["modifyvm", name, "--nic1", "none"])
                .status()
                .context("Failed to disable network")?;
        }
    }

    Ok(())
}

fn attach_storage(name: &str, vdi_path: &Path) -> Result<()> {
    // Detach existing medium first (handles UUID changes on VDI re-conversion)
    let _ = Command::new("VBoxManage")
        .args([
            "storageattach", name,
            "--storagectl", "AHCI",
            "--port", "0",
            "--device", "0",
            "--type", "hdd",
            "--medium", "none",
        ])
        .status();

    // Remove old medium from registry (ignore errors)
    let _ = Command::new("VBoxManage")
        .args(["closemedium", "disk", vdi_path.to_str().unwrap()])
        .status();

    // Remove existing AHCI controller if present, then re-add
    let _ = Command::new("VBoxManage")
        .args(["storagectl", name, "--name", "AHCI", "--remove"])
        .status();

    // Add AHCI controller
    Command::new("VBoxManage")
        .args(["storagectl", name, "--name", "AHCI", "--add", "sata", "--controller", "IntelAhci"])
        .status()
        .context("Failed to add AHCI storage controller")?;

    // Attach VDI
    Command::new("VBoxManage")
        .args([
            "storageattach", name,
            "--storagectl", "AHCI",
            "--port", "0",
            "--device", "0",
            "--type", "hdd",
            "--medium", vdi_path.to_str().unwrap(),
        ])
        .status()
        .context("Failed to attach disk")?;

    Ok(())
}

fn detect_bridge_interface() -> String {
    // Detect active physical interfaces for VirtualBox bridge mode.
    // Priority: Ethernet > Wi-Fi > any up interface > first non-loopback.
    // Filters out: lo, tap*, docker*, vbox*, virbr*
    let mut ethernet_candidates: Vec<String> = Vec::new();
    let mut wifi_candidates: Vec<String> = Vec::new();
    let mut fallback: Option<String> = None;

    if let Ok(output) = Command::new("ip")
        .args(["-o", "link", "show"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() < 2 { continue; }
            let iface = parts[1].trim().to_string();

            // Skip virtual interfaces
            if iface == "lo"
                || iface.starts_with("tap")
                || iface.starts_with("docker")
                || iface.starts_with("vbox")
                || iface.starts_with("virbr")
                || iface.starts_with("br-")
            {
                continue;
            }

            // Check operstate
            let state_path = format!("/sys/class/net/{}/operstate", iface);
            let is_up = std::fs::read_to_string(&state_path)
                .map(|s| s.trim() == "up")
                .unwrap_or(false);

            if !is_up { continue; }

            // Check if the interface has a carrier (cable connected)
            let carrier_path = format!("/sys/class/net/{}/carrier", iface);
            let has_carrier = std::fs::read_to_string(&carrier_path)
                .map(|s| s.trim() == "1")
                .unwrap_or(false);

            if !has_carrier { continue; }

            // Classify by type
            let uevent_path = format!("/sys/class/net/{}/uevent", iface);
            let is_ethernet = std::fs::read_to_string(&uevent_path)
                .map(|s| s.contains("DEVTYPE=wlan") || s.contains("DEVTYPE=wifi"))
                .map(|is_wifi| !is_wifi)
                .unwrap_or(true);

            // Check if it has an IP address (likely connected to a network)
            let has_ip = has_ip_address(&iface);

            if is_ethernet {
                if has_ip {
                    ethernet_candidates.push(iface.clone());
                }
                if fallback.is_none() {
                    fallback = Some(iface.clone());
                }
            } else {
                wifi_candidates.push(iface.clone());
            }
        }
    }

    // Priority: Ethernet with IP > Wi-Fi with IP > any Ethernet > any Wi-Fi > fallback
    let selected = ethernet_candidates.first()
        .or_else(|| wifi_candidates.first())
        .or_else(|| fallback.as_ref())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "eth0".to_string());

    println!(
        "  Bridged interface candidates: {} Ethernet ({}), {} Wi-Fi ({})",
        ethernet_candidates.len(),
        ethernet_candidates.join(", "),
        wifi_candidates.len(),
        wifi_candidates.join(", "),
    );
    println!("  Selected: {} for bridged networking", selected);

    selected
}

fn has_ip_address(iface: &str) -> bool {
    if let Ok(output) = Command::new("ip")
        .args(["-4", "addr", "show", "dev", iface])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout.contains("inet ")
    } else {
        false
    }
}
