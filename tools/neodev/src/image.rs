use crate::config::Config;
use crate::discovery::Discovery;
use anyhow::{Context, Result};
use colored::*;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

const BLOCK_SIZE: usize = 4096;
const SECTOR_SIZE: usize = 512;
const DIRENTRY_SIZE: usize = 128;
const NAME_MAX: usize = 48;
const INLINE_MAX: usize = 16;
const SUPERBLOCK_MAGIC_NE2: u32 = 0x0032454E;
const MODE_DIR: u16 = 0x0040;
const MODE_FILE: u16 = 0x0080;
const PERM_R: u16 = 0x0001;
const PERM_W: u16 = 0x0002;
const PERM_X: u16 = 0x0004;

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFFFFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    crc ^ 0xFFFFFFFF
}

fn default_perms(name: &str) -> u16 {
    let u = name.to_uppercase();
    if u.ends_with(".NXE") || u.ends_with(".COM") || u.ends_with(".EXE") {
        return PERM_R | PERM_X;
    }
    if u.ends_with(".NEM") {
        return PERM_R;
    }
    if u.ends_with(".NXL") {
        return PERM_R | PERM_X;
    }
    if u.ends_with(".BAT") || u.ends_with(".CMD") {
        return PERM_R | PERM_X;
    }
    if u.ends_with(".SYS") {
        return PERM_R;
    }
    if u.ends_with(".CFG") || u.ends_with(".INI") {
        return PERM_R | PERM_W;
    }
    if u.ends_with(".TXT") || u.ends_with(".MD") || u.ends_with(".LOG") {
        return PERM_R | PERM_W;
    }
    PERM_R | PERM_W
}

fn make_direntry(
    name: &str,
    mode: u16,
    size: u64,
    extent_lba: u64,
    extent_count: u32,
    inline_data: &[u8],
) -> Vec<u8> {
    let mut buf = vec![0u8; DIRENTRY_SIZE];
    let nl = name.len().min(NAME_MAX);
    buf[0] = nl as u8;
    buf[1..=nl].copy_from_slice(&name.as_bytes()[..nl]);
    let off_inline = 1 + NAME_MAX;
    let il = inline_data.len().min(INLINE_MAX);
    buf[off_inline..off_inline + il].copy_from_slice(&inline_data[..il]);

    let mut off = off_inline + INLINE_MAX;
    put_u16_le(&mut buf, off, mode);
    off += 2;
    put_u64_le(&mut buf, off, size);
    off += 8;
    put_u64_le(&mut buf, off, 0); // created
    off += 8;
    put_u64_le(&mut buf, off, 0); // modified
    off += 8;
    put_u32_le(&mut buf, off, 0); // checksum
    off += 4;
    put_u32_le(&mut buf, off, il as u32); // inline_len
    off += 4;
    put_u64_le(&mut buf, off, extent_lba);
    off += 8;
    put_u32_le(&mut buf, off, extent_count);
    buf
}

fn make_btree_leaf(entries: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let mut data = vec![0u8; BLOCK_SIZE];
    put_u16_le(&mut data, 0, 1); // Leaf
    put_u16_le(&mut data, 2, entries.len() as u16);
    let mut off = 8;
    for (key, value) in entries {
        let kl = key.len() as u16;
        let vl = value.len() as u16;
        if off + 4 + kl as usize + vl as usize > BLOCK_SIZE {
            break;
        }
        put_u16_le(&mut data, off, kl);
        off += 2;
        data[off..off + kl as usize].copy_from_slice(key);
        off += kl as usize;
        put_u16_le(&mut data, off, vl);
        off += 2;
        data[off..off + vl as usize].copy_from_slice(value);
        off += vl as usize;
    }
    let cksum = crc32(&data[8..]);
    put_u32_le(&mut data, 4, cksum);
    data
}

fn put_u16_le(buf: &mut [u8], off: usize, val: u16) {
    buf[off] = (val & 0xFF) as u8;
    buf[off + 1] = ((val >> 8) & 0xFF) as u8;
}

fn put_u32_le(buf: &mut [u8], off: usize, val: u32) {
    buf[off] = (val & 0xFF) as u8;
    buf[off + 1] = ((val >> 8) & 0xFF) as u8;
    buf[off + 2] = ((val >> 16) & 0xFF) as u8;
    buf[off + 3] = ((val >> 24) & 0xFF) as u8;
}

fn put_u64_le(buf: &mut [u8], off: usize, val: u64) {
    put_u32_le(buf, off, (val & 0xFFFFFFFF) as u32);
    put_u32_le(buf, off + 4, ((val >> 32) & 0xFFFFFFFF) as u32);
}

struct FileEntry {
    name: String,
    content: Vec<u8>,
    mode: u16,
    is_dir: bool,
}

pub fn build_ne2_image(
    cfg: &Config,
    disc: &Discovery,
    output: &Path,
    label: &str,
    blocks: u64,
) -> Result<()> {
    let start = Instant::now();
    println!("{} Building NE2 filesystem image...", "[*]".bold().cyan());

    // Collect files
    let files = collect_files(cfg, disc)?;

    // Organize into directory tree
    let root_marker = "/";
    let mut dir_tree: HashMap<String, Vec<FileEntry>> = HashMap::new();
    dir_tree.insert(root_marker.to_string(), vec![]);

    for entry in &files {
        let path = entry.name.trim_start_matches('/');
        let parts: Vec<&str> = path.split('/').collect();
        let filename = parts.last().unwrap_or(&"");
        let parent = if parts.len() > 1 {
            format!("/{}", parts[..parts.len() - 1].join("/"))
        } else {
            root_marker.to_string()
        };

        dir_tree.entry(parent.clone()).or_default().push(FileEntry {
            name: filename.to_string(),
            content: entry.content.clone(),
            mode: entry.mode,
            is_dir: false,
        });

        // Create intermediate directory entries
        for i in 1..parts.len() {
            let dir_path = format!("/{}", parts[..i].join("/"));
            let dir_parent = if i > 1 {
                format!("/{}", parts[..i - 1].join("/"))
            } else {
                root_marker.to_string()
            };
            let dir_name = parts[i - 1].to_string();
            let already = dir_tree.get(&dir_parent).map_or(false, |entries| {
                entries.iter().any(|e| e.name == dir_name && e.is_dir)
            });
            if !already {
                dir_tree
                    .entry(dir_parent)
                    .or_default()
                    .push(FileEntry {
                        name: dir_name,
                        content: vec![],
                        mode: MODE_DIR | PERM_R | PERM_W | PERM_X | 0x0010,
                        is_dir: true,
                    });
            }
            dir_tree.entry(dir_path).or_default();
        }
    }

    // Sort directory paths by depth for allocation
    let mut dir_paths: Vec<&String> = dir_tree.keys().collect();
    dir_paths.sort_by_key(|k| k.matches('/').count());

    let mut next_lba: u64 = 2;
    let mut dir_lba_map: HashMap<String, u64> = HashMap::new();

    for dirpath in &dir_paths {
        dir_lba_map.insert((*dirpath).clone(), next_lba);
        next_lba += 1;
    }

    // Build B-tree entries per directory
    let mut dir_nodes: HashMap<String, Vec<(Vec<u8>, Vec<u8>)>> = HashMap::new();
    for dirpath in dir_paths.iter() {
        let mut node_entries = vec![];
        if let Some(entries) = dir_tree.get(*dirpath) {
            for entry in entries {
                if entry.is_dir {
                    let subdir_path = if *dirpath == root_marker {
                        format!("/{}", entry.name)
                    } else {
                        format!("{}/{}", dirpath.trim_start_matches('/'), entry.name)
                    };
                    let subdir_path = if subdir_path.starts_with('/') {
                        subdir_path
                    } else {
                        format!("/{}", subdir_path)
                    };
                    let subdir_lba = dir_lba_map.get(&subdir_path).copied().unwrap_or(0);
                    let dentry = make_direntry(&entry.name, entry.mode, 0, subdir_lba, 0, &[]);
                    node_entries.push((entry.name.as_bytes().to_vec(), dentry));
                } else if entry.content.len() <= INLINE_MAX {
                    let dentry = make_direntry(
                        &entry.name,
                        entry.mode,
                        entry.content.len() as u64,
                        0,
                        0,
                        &entry.content,
                    );
                    node_entries.push((entry.name.as_bytes().to_vec(), dentry));
                } else {
                    let extent_lba = next_lba;
                    let block_count =
                        (entry.content.len() + BLOCK_SIZE - 1) / BLOCK_SIZE;
                    let dentry = make_direntry(
                        &entry.name,
                        entry.mode,
                        entry.content.len() as u64,
                        extent_lba,
                        block_count as u32,
                        &[],
                    );
                    node_entries.push((entry.name.as_bytes().to_vec(), dentry));
                    next_lba += block_count as u64;
                }
            }
        }
        node_entries.sort_by(|a, b| a.0.cmp(&b.0));
        dir_nodes.insert((*dirpath).clone(), node_entries);
    }

    let total_blocks = next_lba.max(blocks);
    let image_size = (total_blocks as usize) * BLOCK_SIZE;
    let mut image = vec![0u8; image_size];

    // Superblock
    let root_lba = dir_lba_map.get(root_marker).copied().unwrap_or(1);
    let label_bytes = label.as_bytes();
    let mut sb = vec![0u8; SECTOR_SIZE];
    put_u32_le(&mut sb, 0, SUPERBLOCK_MAGIC_NE2);
    put_u32_le(&mut sb, 4, 2); // version
    put_u64_le(&mut sb, 8, root_lba);
    put_u64_le(&mut sb, 16, 1); // block_size_shift (log2(4096/512)?)
    put_u64_le(&mut sb, 24, 0); // reserved
    put_u64_le(&mut sb, 32, total_blocks);
    put_u64_le(&mut sb, 40, next_lba);
    put_u64_le(&mut sb, 48, total_blocks - next_lba);
    sb[56] = label_bytes.len().min(32) as u8;
    let lbl_len = label_bytes.len().min(32);
    sb[57..57 + lbl_len].copy_from_slice(&label_bytes[..lbl_len]);
    let cksum = crc32(&sb[..72]);
    put_u32_le(&mut sb, 109, cksum);
    image[..SECTOR_SIZE].copy_from_slice(&sb);

    // B-tree nodes
    for (dirpath, node_entries) in &dir_nodes {
        if let Some(&lba) = dir_lba_map.get(dirpath) {
            let node_data = make_btree_leaf(node_entries);
            let offset = (lba as usize) * BLOCK_SIZE;
            if offset + BLOCK_SIZE <= image.len() {
                image[offset..offset + BLOCK_SIZE].copy_from_slice(&node_data);
            }
        }
    }

    // File data blocks
    for (dirpath, entries) in &dir_tree {
        for entry in entries {
            if entry.is_dir || entry.content.len() <= INLINE_MAX {
                continue;
            }
            if let Some(node_entries) = dir_nodes.get(dirpath) {
                for (ename, ebytes) in node_entries {
                    if ename == entry.name.as_bytes() {
                        let extent_lba = read_u64_le(ebytes, 99);
                        let block_count =
                            (entry.content.len() + BLOCK_SIZE - 1) / BLOCK_SIZE;
                        let block_start = (extent_lba as usize) * BLOCK_SIZE;
                        for i in 0..block_count {
                            let chunk_start = i * BLOCK_SIZE;
                            let chunk_end =
                                (chunk_start + BLOCK_SIZE).min(entry.content.len());
                            let data = &entry.content[chunk_start..chunk_end];
                            let dest = block_start + i * BLOCK_SIZE;
                            if dest + data.len() <= image.len() {
                                image[dest..dest + data.len()].copy_from_slice(data);
                            }
                        }
                        break;
                    }
                }
            }
        }
    }

    std::fs::write(output, &image).context("Failed to write NE2 image")?;
    let actual = std::fs::metadata(output)?.len();
    let total_entries: usize = dir_nodes.values().map(|v| v.len()).sum();
    println!("{} NE2 image: {} ({} blocks, {} entries)", "[✓]".bold().green(), output.display(), total_blocks, total_entries);
    println!("  Size: {} ({:.1} MB)", report::fmt_size(actual), actual as f64 / 1_048_576.0);
    println!("  Duration: {:.1}s", start.elapsed().as_secs_f64());
    Ok(())
}

fn read_u64_le(buf: &[u8], off: usize) -> u64 {
    let lo = u64::from(buf[off])
        | (u64::from(buf[off + 1]) << 8)
        | (u64::from(buf[off + 2]) << 16)
        | (u64::from(buf[off + 3]) << 24);
    let hi = u64::from(buf[off + 4])
        | (u64::from(buf[off + 5]) << 8)
        | (u64::from(buf[off + 6]) << 16)
        | (u64::from(buf[off + 7]) << 24);
    lo | (hi << 32)
}

fn collect_files(cfg: &Config, _disc: &Discovery) -> Result<Vec<FileEntry>> {
    let mut files: Vec<FileEntry> = vec![];
    let root = &cfg.project_root;

    // README
    files.push(FileEntry {
        name: "/README.TXT".into(),
        content: b"Welcome to NeoDOS v2!\r\n".to_vec(),
        mode: MODE_FILE | PERM_R | PERM_W,
        is_dir: false,
    });

    // Temp marker
    files.push(FileEntry {
        name: "/Temp/.empty".into(),
        content: vec![],
        mode: MODE_FILE | PERM_R | PERM_W,
        is_dir: false,
    });

    // Registry hive
    let hiv_path = root.join("scripts").join("system.hiv");
    if hiv_path.exists() {
        let content = std::fs::read(&hiv_path)?;
        files.push(FileEntry {
            name: "/System/Registry/SYSTEM.hiv".into(),
            content,
            mode: MODE_FILE | PERM_R,
            is_dir: false,
        });
    }

    // NXE binaries
    let programs_nxe = &[
        "neoshell", "neoinit", "cmdtest", "shtest", "stresscmd", "cd", "corehelp",
        "datetime", "ver", "neomem", "vol", "echo", "label",
        "coretype", "tree", "corecls", "corecopy", "coredel",
        "coreren", "coremd", "corerd", "drives", "ps", "keyb", "coredir",
        "poweroff", "reboot", "colors", "neokey",
    ];
    let tools_nxe = &[
        "kill", "pri", "fsck", "ndreg", "loadnem", "progress",
        "neotop", "dhcpd", "netcfg", "ipconfig", "cpuinfo", "neolocale",
    ];

    for name in programs_nxe.iter().chain(tools_nxe) {
        let subdir = if programs_nxe.contains(name) {
            "Programs"
        } else {
            "System/Tools"
        };
        let p = root.join("userbin").join(format!("{}.nxe", name));
        if p.exists() {
            let content = std::fs::read(&p)?;
            let perms = default_perms(&format!("{}.NXE", name));
            files.push(FileEntry {
                name: format!("/{}/{}.nxe", subdir, name),
                content,
                mode: MODE_FILE | perms,
                is_dir: false,
            });
        }
    }

    // NXL libraries
    let nxl_map = [
        ("libneodos.nxl", "fs.nxl"),
        ("libmath.nxl", "math.nxl"),
        ("console.nxl", "console.nxl"),
        ("net.nxl", "net.nxl"),
    ];
    for (src_name, dst_name) in &nxl_map {
        let p = root.join(src_name);
        if p.exists() {
            let content = std::fs::read(&p)?;
            files.push(FileEntry {
                name: format!("/System/Libraries/{}", dst_name),
                content,
                mode: MODE_FILE | PERM_R | PERM_X,
                is_dir: false,
            });
        }
    }

    // Keyboard layouts (.kbd)
    let kbd_layouts = &["US", "Spanish"];
    for layout_name in kbd_layouts {
        let p = root.join("data/keyboard").join(format!("{}.kbd", layout_name));
        if p.exists() {
            let content = std::fs::read(&p).unwrap_or_default();
            files.push(FileEntry {
                name: format!("/System/Keyboard/{}.kbd", layout_name),
                content,
                mode: MODE_FILE | PERM_R,
                is_dir: false,
            });
        }
    }

    // Locale / NLT translation files
    let locale_dir = root.join("data/locale");
    if locale_dir.exists() {
        if let Ok(lang_entries) = std::fs::read_dir(&locale_dir) {
            for lang_entry in lang_entries.flatten() {
                let lang_path = lang_entry.path();
                if !lang_path.is_dir() { continue; }
                let lang_name = match lang_path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n,
                    None => continue,
                };
                if let Ok(nlt_entries) = std::fs::read_dir(&lang_path) {
                    for nlt_entry in nlt_entries.flatten() {
                        let p = nlt_entry.path();
                        if p.extension().and_then(|e| e.to_str()) != Some("nlt") {
                            continue;
                        }
                        let content = match std::fs::read(&p) {
                            Ok(c) => c,
                            Err(_) => continue,
                        };
                        let fname = match p.file_name().and_then(|n| n.to_str()) {
                            Some(n) => n,
                            None => continue,
                        };
                        files.push(FileEntry {
                            name: format!("/System/Locale/{}/{}", lang_name, fname),
                            content,
                            mode: MODE_FILE | PERM_R,
                            is_dir: false,
                        });
                    }
                }
            }
        }
    }

    // NEM drivers
    let nem_dir = format!("/tmp/nem_drivers_{}", std::process::id());
    let boot_drivers = &["ps2kbd", "ps2mouse", "rtc", "serial"];
    let sys_drivers = &["acpi", "pci", "ata", "ahci", "e1000", "virtio-blk"];
    for nem_name in boot_drivers.iter().chain(sys_drivers) {
        let cat = if boot_drivers.contains(nem_name) {
            "BOOT"
        } else {
            "SYSTEM"
        };
        let p = Path::new(&nem_dir).join(cat).join(format!("{}.nem", nem_name));
        if p.exists() {
            let content = std::fs::read(&p)?;
            files.push(FileEntry {
                name: format!("/System/Drivers/{}.nem", nem_name),
                content,
                mode: MODE_FILE | PERM_R,
                is_dir: false,
            });
        }
    }

    Ok(files)
}

pub fn create_esp_image(cfg: &Config) -> Result<std::path::PathBuf> {
    let start = Instant::now();
    println!("{} Creating ESP partition image (FAT32)...", "[*]".bold().cyan());

    let esp_image = cfg.project_root.join("tmp_esp.img");
    let esp_size = cfg.esp_size_mb;

    // Create empty image
    let status = Command::new("dd")
        .args([
            "if=/dev/zero",
            &format!("of={}", esp_image.display()),
            "bs=1M",
            &format!("count={}", esp_size),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to create ESP image with dd")?;
    if !status.success() {
        anyhow::bail!("dd failed");
    }

    // Format as FAT32
    let status = Command::new("mkfs.fat")
        .args(["-F", "32", &esp_image.to_string_lossy()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("mkfs.fat not found (install dosfstools)")?;
    if !status.success() {
        anyhow::bail!("mkfs.fat failed");
    }

    // Create directories and copy files with mtools
    if which("mmd").is_some() {
        for dir in &["/EFI", "/EFI/BOOT", "/EFI/NeoDOS"] {
            let status = Command::new("mmd")
                .args(["-i", &esp_image.to_string_lossy(), dir])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()?;
            if !status.success() {
                eprintln!("  Warning: mmd {} failed", dir);
            }
        }

        let bootloader_src = cfg.project_root.join("bootloader.efi");
        let kernel_src = cfg.project_root.join("kernel.elf");

        if bootloader_src.exists() {
            Command::new("mcopy")
                .args([
                    "-i",
                    &esp_image.to_string_lossy(),
                    &bootloader_src.to_string_lossy(),
                    "::/EFI/BOOT/BOOTX64.EFI",
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()?;

            Command::new("mcopy")
                .args([
                    "-i",
                    &esp_image.to_string_lossy(),
                    &bootloader_src.to_string_lossy(),
                    "::/EFI/NeoDOS/bootloader.efi",
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()?;
        }

        if kernel_src.exists() {
            Command::new("mcopy")
                .args([
                    "-i",
                    &esp_image.to_string_lossy(),
                    &kernel_src.to_string_lossy(),
                    "::/EFI/NeoDOS/kernel.elf",
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()?;
        }

        let fs_image = cfg.project_root.join("scripts").join("neodos_image.img");
        if fs_image.exists() {
            Command::new("mcopy")
                .args([
                    "-i",
                    &esp_image.to_string_lossy(),
                    &fs_image.to_string_lossy(),
                    "::/EFI/NeoDOS/neodos.fs",
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()?;
        }

        println!("{} Files copied to ESP", "[✓]".bold().green());
    } else {
        eprintln!("  mtools not found; files not copied to ESP image");
        eprintln!("  Install: sudo apt install mtools");
    }

    println!("  Duration: {:.1}s", start.elapsed().as_secs_f64());
    Ok(esp_image)
}

pub fn create_gpt_image(
    cfg: &Config,
    esp_image: &Path,
    neodos_image: &Path,
    output: &Path,
) -> Result<()> {
    let start = Instant::now();
    println!("{} Creating unified GPT disk image...", "[*]".bold().cyan());

    // Calculate sizes
    let esp_data = std::fs::read(esp_image)?;
    let neodos_data = std::fs::read(neodos_image)?;

    let esp_mb = cfg
        .esp_size_mb
        .max((esp_data.len() as f64 / 1_048_576.0).ceil() as u64);
    let neodos_mb = cfg
        .neodos_size_mb
        .max((neodos_data.len() as f64 / 1_048_576.0).ceil() as u64);
    let total_mb = esp_mb + neodos_mb + cfg.gpt_padding_mb;

    // Create blank image
    {
        let f = std::fs::File::create(output)?;
        f.set_len(total_mb * 1024 * 1024)?;
    }

    // Partition layout
    let esp_start = 2048u64;
    let esp_size_sectors = esp_mb * 1024 * 1024 / SECTOR_SIZE as u64;
    let neodos_start = esp_start + esp_size_sectors;
    let neodos_size_sectors = neodos_mb * 1024 * 1024 / SECTOR_SIZE as u64;

    let sfdisk_input = format!(
        "label: gpt\n\
         start={}, size={}, type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B\n\
         start={}, size={}, type=EBD0A0A2-B9E5-4433-87C0-68B6B72699C7\n",
        esp_start, esp_size_sectors, neodos_start, neodos_size_sectors
    );

    let mut result = Command::new("sfdisk")
        .arg(output)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("sfdisk not found (install util-linux)")?;

    {
        let mut stdin = result.stdin.take().context("sfdisk stdin not available")?;
        use std::io::Write;
        stdin.write_all(sfdisk_input.as_bytes())?;
    }

    let sfdisk_output = result.wait_with_output()?;
    if !sfdisk_output.status.success() {
        let stderr = String::from_utf8_lossy(&sfdisk_output.stderr);
        anyhow::bail!("sfdisk failed: {}", stderr);
    }

    // Write partition data
    let esp_offset = (esp_start as usize) * SECTOR_SIZE;
    let neodos_offset = (neodos_start as usize) * SECTOR_SIZE;

    use std::io::{Seek, Write};
    let mut disk = std::fs::OpenOptions::new()
        .write(true)
        .open(output)?;

    disk.seek(std::io::SeekFrom::Start(esp_offset as u64))?;
    disk.write_all(&esp_data)?;

    disk.seek(std::io::SeekFrom::Start(neodos_offset as u64))?;
    disk.write_all(&neodos_data)?;

    println!(
        "{} GPT disk image: {} ({:.0} MB)",
        "[✓]".bold().green(),
        output.display(),
        total_mb
    );
    println!("  Partition 1 (ESP):    LBA {} - {}", esp_start, esp_start + esp_size_sectors - 1);
    println!("  Partition 2 (NeoDOS): LBA {} - {}", neodos_start, neodos_start + neodos_size_sectors - 1);
    println!("  Duration: {:.1}s", start.elapsed().as_secs_f64());
    Ok(())
}

pub fn generate_registry_hive(cfg: &Config) -> Result<()> {
    println!("{} Generating SYSTEM.HIV registry hive...", "[*]".bold().cyan());
    let gen_script = cfg.project_root.join("scripts").join("gen_system_hiv.py");
    let output = cfg.project_root.join("scripts").join("system.hiv");

    let status = Command::new("python3")
        .arg(&gen_script)
        .arg(&output)
        .status()
        .context("Failed to run gen_system_hiv.py")?;

    if !status.success() {
        anyhow::bail!("Registry hive generation failed");
    }
    println!(
        "{} SYSTEM.HIV: {}",
        "[✓]".bold().green(),
        output.display()
    );
    Ok(())
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

mod report {
    pub fn fmt_size(size: u64) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
        let mut s = size as f64;
        for unit in UNITS {
            if s < 1024.0 {
                return format!("{:.1} {}", s, unit);
            }
            s /= 1024.0;
        }
        format!("{:.2} GB", s)
    }
}
