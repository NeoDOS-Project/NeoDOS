use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize)]
pub enum ProjectKind {
    Kernel,
    Bootloader,
    UserBin,
    NxlLibrary,
    NemDriver,
    Tool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub name: String,
    pub kind: ProjectKind,
    pub path: PathBuf,
    pub cargo_toml: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Discovery {
    pub kernel: Option<Project>,
    pub bootloader: Option<Project>,
    pub user_bins: Vec<Project>,
    pub nxl_libs: Vec<Project>,
    pub nem_drivers: Vec<Project>,
    pub tools: Vec<Project>,
}

pub fn discover(project_root: &Path) -> Result<Discovery> {
    let kernel = find_kernel(project_root);
    let bootloader = find_bootloader(project_root);
    let user_bins = find_user_bins(project_root);
    let nxl_libs = find_nxl_libs(project_root);
    let nem_drivers = find_nem_drivers(project_root);
    let tools = find_tools(project_root);

    Ok(Discovery {
        kernel,
        bootloader,
        user_bins,
        nxl_libs,
        nem_drivers,
        tools,
    })
}

fn find_kernel(root: &Path) -> Option<Project> {
    let path = root.join("neodos-kernel");
    let cargo = path.join("Cargo.toml");
    if cargo.exists() {
        Some(Project {
            name: "neodos_kernel".into(),
            kind: ProjectKind::Kernel,
            path,
            cargo_toml: Some(cargo),
        })
    } else {
        None
    }
}

fn find_bootloader(root: &Path) -> Option<Project> {
    let path = root.join("neodos-bootloader");
    let cargo = path.join("Cargo.toml");
    if cargo.exists() {
        Some(Project {
            name: "neodos_bootloader".into(),
            kind: ProjectKind::Bootloader,
            path,
            cargo_toml: Some(cargo),
        })
    } else {
        None
    }
}

fn find_user_bins(root: &Path) -> Vec<Project> {
    let userbin_dir = root.join("userbin");
    if !userbin_dir.exists() {
        return vec![];
    }
    let mut bins = vec![];
    for entry in WalkDir::new(&userbin_dir).max_depth(2).into_iter().filter_map(|e| e.ok()) {
        if entry.file_name() == "Cargo.toml" {
            if let Some(parent) = entry.path().parent() {
                let name = parent
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if !name.is_empty() && parent != &userbin_dir {
                    bins.push(Project {
                        name,
                        kind: ProjectKind::UserBin,
                        path: parent.to_path_buf(),
                        cargo_toml: Some(entry.path().to_path_buf()),
                    });
                }
            }
        }
    }
    bins.sort_by(|a, b| a.name.cmp(&b.name));
    bins
}

fn find_nxl_libs(root: &Path) -> Vec<Project> {
    let mut libs = vec![];
    for entry in WalkDir::new(root).max_depth(1).into_iter().filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("lib") && name.ends_with("-nxl") && entry.path().is_dir() {
            let cargo = entry.path().join("Cargo.toml");
            if cargo.exists() {
                libs.push(Project {
                    name: name.trim_end_matches("-nxl").to_string(),
                    kind: ProjectKind::NxlLibrary,
                    path: entry.path().to_path_buf(),
                    cargo_toml: Some(cargo),
                });
            }
        }
    }
    libs.sort_by(|a, b| a.name.cmp(&b.name));
    libs
}

fn find_nem_drivers(root: &Path) -> Vec<Project> {
    let drivers_dir = root.join("drivers");
    if !drivers_dir.exists() {
        return vec![];
    }
    let mut drivers = vec![];
    for entry in WalkDir::new(&drivers_dir).max_depth(2).into_iter().filter_map(|e| e.ok()) {
        if entry.file_name() == "build_nem.py" {
            if let Some(parent) = entry.path().parent() {
                let name = parent
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if !name.is_empty() {
                    drivers.push(Project {
                        name,
                        kind: ProjectKind::NemDriver,
                        path: parent.to_path_buf(),
                        cargo_toml: None,
                    });
                }
            }
        }
    }
    drivers.sort_by(|a, b| a.name.cmp(&b.name));
    drivers
}

fn find_tools(root: &Path) -> Vec<Project> {
    let tools_dir = root.join("tools");
    if !tools_dir.exists() {
        return vec![];
    }
    let mut tools = vec![];
    for entry in WalkDir::new(&tools_dir).max_depth(2).into_iter().filter_map(|e| e.ok()) {
        if entry.file_name() == "Cargo.toml" {
            if let Some(parent) = entry.path().parent() {
                let name = parent
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if !name.is_empty() && name != "neodev" {
                    tools.push(Project {
                        name,
                        kind: ProjectKind::Tool,
                        path: parent.to_path_buf(),
                        cargo_toml: Some(entry.path().to_path_buf()),
                    });
                }
            }
        }
    }
    tools
}
