use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub project_root: PathBuf,
    pub esp_size_mb: u64,
    pub neodos_size_mb: u64,
    pub gpt_padding_mb: u64,
    pub ovmf_code: PathBuf,
    pub ovmf_vars_template: PathBuf,
    pub qemu_memory: String,
    pub kernel_target: String,
    pub bootloader_target: String,
}

impl Config {
    pub fn load(project_root: &Path) -> Result<Self> {
        let config_path = project_root.join("neodev.toml");
        if config_path.exists() {
            let data = std::fs::read_to_string(&config_path)?;
            let mut cfg: Config = toml::from_str(&data)?;
            cfg.project_root = project_root.to_path_buf();
            return Ok(cfg);
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
            qemu_memory: "512M".into(),
            kernel_target: "x86_64-unknown-none".into(),
            bootloader_target: "x86_64-unknown-uefi".into(),
        }
    }
}
