use crate::config::Config;
use anyhow::Result;
use colored::*;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VmStatus {
    Running,
    Paused,
    Stopped,
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NetworkMode {
    User,
    Bridged,
    None,
}

#[derive(Debug, Clone)]
pub struct VmConfig {
    pub name: String,
    pub memory_mb: u32,
    pub cpus: u32,
    pub efi: bool,
    pub disk_image: PathBuf,
    pub disk_vdi: PathBuf,
    pub serial_file: Option<PathBuf>,
    pub network: NetworkMode,
    pub headless: bool,
    pub gdb: bool,
    pub gdb_port: u16,
    pub storage_mode: StorageMode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StorageMode {
    Ahci,
    Ata,
    Nvme,
    Virtio,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            name: "NeoDOS".into(),
            memory_mb: 512,
            cpus: 2,
            efi: true,
            disk_image: PathBuf::from("disk_image.img"),
            disk_vdi: PathBuf::from("disk_image.vdi"),
            serial_file: None,
            network: NetworkMode::User,
            headless: false,
            gdb: false,
            gdb_port: 1234,
            storage_mode: StorageMode::Ahci,
        }
    }
}

pub trait VmInstance: Send {
    fn serial_path(&self) -> Option<&Path>;
    fn wait_timeout(&mut self, timeout: Duration) -> Result<Option<i32>>;
    fn kill(&mut self) -> Result<()>;
    fn pid(&self) -> Option<u32>;
}

pub trait HypervisorBackend: Send + Sync {
    fn name(&self) -> &str;
    fn check_prerequisites(&self, cfg: &Config) -> Result<()>;
    fn ensure_vm(&self, cfg: &Config, vmcfg: &VmConfig) -> Result<()>;
    fn delete_vm(&self, cfg: &Config, vmcfg: &VmConfig) -> Result<()>;
    fn run(&self, cfg: &Config, vmcfg: &VmConfig) -> Result<()>;
    fn start_headless(&self, cfg: &Config, vmcfg: &VmConfig) -> Result<Box<dyn VmInstance>>;
    fn stop(&self, cfg: &Config, vmcfg: &VmConfig) -> Result<()>;
    fn reset(&self, cfg: &Config, vmcfg: &VmConfig) -> Result<()>;
    fn status(&self, cfg: &Config, vmcfg: &VmConfig) -> Result<VmStatus>;
}

pub fn create_backend(name: &str) -> Result<Box<dyn HypervisorBackend>> {
    match name {
        "qemu" => Ok(Box::new(qemu::QemuBackend)),
        "virtualbox" => Ok(Box::new(vbox::VirtualBoxBackend)),
        _ => anyhow::bail!("Unknown backend '{}'. Use: qemu, virtualbox", name),
    }
}

pub fn vmcfg_from_config(cfg: &Config) -> VmConfig {
    vmcfg_from_config_net(cfg, None)
}

pub fn vmcfg_from_config_net(cfg: &Config, net_override: Option<&str>) -> VmConfig {
    let network = match net_override {
        Some("bridged") | Some("bridge") => NetworkMode::Bridged,
        Some("none") | Some("disabled") => NetworkMode::None,
        _ => match cfg.vm_network.to_lowercase().as_str() {
            "bridged" | "bridge" => NetworkMode::Bridged,
            "none" | "disabled" => NetworkMode::None,
            _ => NetworkMode::User,
        },
    };
    VmConfig {
        name: "NeoDOS".into(),
        memory_mb: cfg.vm_memory_mb,
        cpus: cfg.vm_cpus,
        efi: true,
        disk_image: cfg.project_root.join("disk_image.img"),
        disk_vdi: cfg.project_root.join("disk_image.vdi"),
        serial_file: Some(cfg.project_root.join("vbox_serial.log")),
        network,
        headless: false,
        gdb: false,
        gdb_port: 1234,
        storage_mode: StorageMode::Ahci,
    }
}

pub fn print_vm_status(status: &VmStatus) {
    match status {
        VmStatus::Running => println!("  Status: {} (running)", "[▶]".bold().green()),
        VmStatus::Paused => println!("  Status: {} (paused)", "[❚❚]".bold().yellow()),
        VmStatus::Stopped => println!("  Status: {} (stopped)", "[■]".bold().blue()),
        VmStatus::NotFound => println!("  Status: {} (not found)", "[?]".bold().red()),
    }
}

mod qemu;
mod vbox;
