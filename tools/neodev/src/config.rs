use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub project_root: PathBuf,
    pub esp_size_mb: u64,
    pub neodos_size_mb: u64,
    pub gpt_padding_mb: u64,
    // OVMF (QEMU-specific)
    pub ovmf_code: PathBuf,
    pub ovmf_vars_template: PathBuf,
    // VM configuration (hypervisor-agnostic)
    pub vm_backend: String,
    pub vm_memory_mb: u32,
    pub vm_cpus: u32,
    pub vm_network: String,
    // QEMU-specific options
    pub qemu_kvm: bool,
    pub qemu_bdm: bool,
    // Rust targets
    pub kernel_target: String,
    pub bootloader_target: String,
}

impl Config {
    pub fn load(project_root: &Path) -> Result<Self> {
        let config_path = project_root.join("neodev.toml");
        if config_path.exists() {
            let data = std::fs::read_to_string(&config_path)?;
            let mut cfg: ConfigFile = toml::from_str(&data)?;
            cfg.project.project_root = Some(project_root.to_path_buf());
            return Ok(Config::from_file(cfg, project_root));
        }
        Ok(Config::default(project_root))
    }

    pub fn default(project_root: &Path) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            esp_size_mb: 100,
            neodos_size_mb: 10,
            gpt_padding_mb: 12,
            ovmf_code: PathBuf::from("/usr/share/OVMF/OVMF_CODE.fd"),
            ovmf_vars_template: PathBuf::from("/usr/share/OVMF/OVMF_VARS.fd"),
            vm_backend: "virtualbox".into(),
            vm_memory_mb: 512,
            vm_cpus: 2,
            vm_network: "bridged".into(),
            qemu_kvm: false,
            qemu_bdm: false,
            kernel_target: "x86_64-unknown-none".into(),
            bootloader_target: "x86_64-unknown-uefi".into(),
        }
    }

    fn from_file(cfg: ConfigFile, project_root: &Path) -> Self {
        let p = cfg.project;
        let v = cfg.vm.unwrap_or_default();
        let q = cfg.qemu.unwrap_or_default();
        Self {
            project_root: p.project_root.unwrap_or_else(|| project_root.to_path_buf()),
            esp_size_mb: p.esp_size_mb.unwrap_or(100),
            neodos_size_mb: p.neodos_size_mb.unwrap_or(10),
            gpt_padding_mb: p.gpt_padding_mb.unwrap_or(12),
            ovmf_code: q.ovmf_code.unwrap_or_else(|| PathBuf::from("/usr/share/OVMF/OVMF_CODE.fd")),
            ovmf_vars_template: q.ovmf_vars_template.unwrap_or_else(|| PathBuf::from("/usr/share/OVMF/OVMF_VARS.fd")),
            vm_backend: v.backend.unwrap_or_else(|| "qemu".into()),
            vm_memory_mb: v.memory.unwrap_or(512),
            vm_cpus: v.cpus.unwrap_or(2),
            vm_network: v.network.clone().unwrap_or_else(|| "bridged".into()),
            qemu_kvm: q.kvm.unwrap_or(false),
            qemu_bdm: q.bdm.unwrap_or(false),
            kernel_target: p.kernel_target.unwrap_or_else(|| "x86_64-unknown-none".into()),
            bootloader_target: p.bootloader_target.unwrap_or_else(|| "x86_64-unknown-uefi".into()),
        }
    }
}

// ─── TOML config file structure ───

#[derive(Debug, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    project: ProjectConfig,
    #[serde(default)]
    vm: Option<VmConfig>,
    #[serde(default)]
    qemu: Option<QemuConfig>,
}

#[derive(Debug, Deserialize)]
struct ProjectConfig {
    #[serde(skip)]
    project_root: Option<PathBuf>,
    esp_size_mb: Option<u64>,
    neodos_size_mb: Option<u64>,
    gpt_padding_mb: Option<u64>,
    kernel_target: Option<String>,
    bootloader_target: Option<String>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            project_root: None,
            esp_size_mb: None,
            neodos_size_mb: None,
            gpt_padding_mb: None,
            kernel_target: None,
            bootloader_target: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct VmConfig {
    backend: Option<String>,
    memory: Option<u32>,
    cpus: Option<u32>,
    network: Option<String>,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            backend: None,
            memory: None,
            cpus: None,
            network: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct QemuConfig {
    kvm: Option<bool>,
    bdm: Option<bool>,
    ovmf_code: Option<PathBuf>,
    ovmf_vars_template: Option<PathBuf>,
}

impl Default for QemuConfig {
    fn default() -> Self {
        Self {
            kvm: None,
            bdm: None,
            ovmf_code: None,
            ovmf_vars_template: None,
        }
    }
}
