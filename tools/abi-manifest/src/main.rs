//! Generate and verify the NeoDOS kernel ABI manifest.
//!
//! The manifest is a versioned JSON document consumed by external tools
//! (NeoTools, neodos-dev-server / MCP / LSP). It is derived from the kernel
//! sources so it cannot drift silently:
//!
//! - syscalls and permissions: `neodos-kernel/src/syscall/mod.rs`
//! - Ob types and info classes: `neodos-kernel/src/object/types.rs`
//! - NEM ABI: `neodos-kernel/src/drivers/nem/format.rs`
//! - syscall ABI version: `libneodos/src/export.rs`
//! - kernel version: `neodos-kernel/Cargo.toml`
//!
//! Usage:
//!   abi-manifest           # (re)write abi/kernel-abi.json
//!   abi-manifest --check   # fail if the checked-in file is stale
//!   abi-manifest --stdout   # print the manifest to stdout

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const OUTPUT_REL: &str = "abi/kernel-abi.json";

fn main() -> ExitCode {
    let root = match repo_root() {
        Some(p) => p,
        None => {
            eprintln!("abi-manifest: cannot locate the repository root");
            return ExitCode::FAILURE;
        }
    };

    let generated = match generate(&root) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("abi-manifest: {e}");
            return ExitCode::FAILURE;
        }
    };

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--check") {
        let out = root.join(OUTPUT_REL);
        let current = std::fs::read_to_string(&out).unwrap_or_default();
        if current != generated {
            eprintln!(
                "abi-manifest: {} is stale; run `cargo run --manifest-path tools/abi-manifest/Cargo.toml`",
                OUTPUT_REL
            );
            return ExitCode::FAILURE;
        }
        println!("abi-manifest: {OUTPUT_REL} is up to date");
        return ExitCode::SUCCESS;
    }

    if args.iter().any(|a| a == "--stdout") {
        print!("{generated}");
        return ExitCode::SUCCESS;
    }

    let out = root.join(OUTPUT_REL);
    if let Some(parent) = out.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("abi-manifest: cannot create {}: {e}", parent.display());
            return ExitCode::FAILURE;
        }
    }
    if let Err(e) = std::fs::write(&out, &generated) {
        eprintln!("abi-manifest: cannot write {}: {e}", out.display());
        return ExitCode::FAILURE;
    }
    println!("abi-manifest: wrote {}", out.display());
    ExitCode::SUCCESS
}

fn repo_root() -> Option<PathBuf> {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    here.parent()?.parent().map(Path::to_path_buf)
}

#[derive(Debug, Clone)]
struct Syscall {
    rax: u32,
    name: String,
    handler: String,
    permission: String,
}

#[derive(Debug, Clone)]
struct EnumVariant {
    value: u32,
    name: String,
}

fn generate(root: &Path) -> Result<String, String> {
    let syscall_src = read(root, "neodos-kernel/src/syscall/mod.rs")?;
    let types_src = read(root, "neodos-kernel/src/object/types.rs")?;
    let nem_src = read(root, "neodos-kernel/src/drivers/nem/format.rs")?;
    let export_src = read(root, "libneodos/src/export.rs")?;
    let kernel_cargo = read(root, "neodos-kernel/Cargo.toml")?;

    let mut syscalls = parse_syscalls(&syscall_src)?;
    syscalls.sort_by_key(|s| s.rax);

    let ob_types = parse_enum(&types_src, "ObType")?;
    let ob_info = parse_enum(&types_src, "ObInfoClass")?;
    let ob_set_info = parse_enum(&types_src, "ObSetInfoClass")?;

    let syscall_abi = parse_u32_const(&export_src, "ABI_VERSION")?;
    let nem_min = parse_u16_const(&nem_src, "ABI_MIN_VALID")?;
    let nem_target = parse_u16_const(&nem_src, "ABI_TARGET")?;
    let nem_max = parse_u16_const(&nem_src, "ABI_MAX_VALID")?;
    let nem_format = parse_u32_const(&nem_src, "NEM_VERSION_3")?;
    let kernel_version = parse_package_version(&kernel_cargo)?;

    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"schema_version\": 1,\n");
    out.push_str("  \"generated_by\": \"tools/abi-manifest\",\n");
    out.push_str(&format!(
        "  \"kernel_version\": \"{}\",\n",
        escape(&kernel_version)
    ));
    out.push_str("  \"abi\": {\n");
    out.push_str(&format!("    \"syscall\": {syscall_abi},\n"));
    out.push_str("    \"nem\": {\n");
    out.push_str(&format!("      \"min\": {nem_min},\n"));
    out.push_str(&format!("      \"target\": {nem_target},\n"));
    out.push_str(&format!("      \"max\": {nem_max}\n"));
    out.push_str("    },\n");
    out.push_str(&format!("    \"nem_format\": {nem_format}\n"));
    out.push_str("  },\n");
    out.push_str("  \"counts\": {\n");
    out.push_str(&format!("    \"syscalls\": {},\n", syscalls.len()));
    out.push_str(&format!("    \"ob_types\": {},\n", ob_types.len()));
    out.push_str(&format!("    \"ob_info_classes\": {},\n", ob_info.len()));
    out.push_str(&format!(
        "    \"ob_set_info_classes\": {}\n",
        ob_set_info.len()
    ));
    out.push_str("  },\n");

    out.push_str("  \"syscalls\": [\n");
    for (i, s) in syscalls.iter().enumerate() {
        let _ = writeln!(
            out,
            "    {{ \"rax\": {}, \"name\": \"{}\", \"handler\": \"{}\", \"permission\": \"{}\" }}{}",
            s.rax,
            escape(&s.name),
            escape(&s.handler),
            escape(&s.permission),
            comma(i, syscalls.len())
        );
    }
    out.push_str("  ],\n");

    write_enum(&mut out, "ob_types", &ob_types);
    out.push_str(",\n");
    write_enum(&mut out, "ob_info_classes", &ob_info);
    out.push_str(",\n");
    write_enum(&mut out, "ob_set_info_classes", &ob_set_info);
    out.push_str("\n}\n");

    Ok(out)
}

fn write_enum(out: &mut String, key: &str, variants: &[EnumVariant]) {
    let _ = writeln!(out, "  \"{key}\": [");
    for (i, v) in variants.iter().enumerate() {
        let _ = writeln!(
            out,
            "    {{ \"value\": {}, \"name\": \"{}\" }}{}",
            v.value,
            escape(&v.name),
            comma(i, variants.len())
        );
    }
    out.push_str("  ]");
}

fn comma(i: usize, len: usize) -> &'static str {
    if i + 1 == len {
        ""
    } else {
        ","
    }
}

fn read(root: &Path, rel: &str) -> Result<String, String> {
    let path = root.join(rel);
    std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn parse_syscalls(src: &str) -> Result<Vec<Syscall>, String> {
    let table_start = src
        .find("pub static ref SYSCALL_TABLE")
        .ok_or("SYSCALL_TABLE not found")?;
    let perms_start = src
        .find("pub static ref SYSCALL_PERMISSIONS")
        .ok_or("SYSCALL_PERMISSIONS not found")?;
    let table_src = &src[table_start..perms_start];
    let perms_src = &src[perms_start..];

    let mut out = Vec::new();
    for line in table_src.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("t[") else {
            continue;
        };
        let Some((num, rest)) = rest.split_once("] = Some(") else {
            continue;
        };
        let Ok(rax) = num.trim().parse::<u32>() else {
            continue;
        };
        let Some((handler, _)) = rest.split_once(" as SyscallFn") else {
            continue;
        };
        let handler = handler.trim().to_string();
        let name = handler
            .strip_prefix("handler_")
            .unwrap_or(&handler)
            .to_string();
        out.push(Syscall {
            rax,
            name,
            handler,
            permission: String::new(),
        });
    }

    for line in perms_src.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("t[") else {
            continue;
        };
        let Some((num, rest)) = rest.split_once("] = SyscallPermission::") else {
            continue;
        };
        let Ok(rax) = num.trim().parse::<u32>() else {
            continue;
        };
        let permission = rest
            .split_once('(')
            .map(|(p, _)| p)
            .unwrap_or(rest)
            .trim()
            .to_string();
        if let Some(s) = out.iter_mut().find(|s| s.rax == rax) {
            s.permission = permission;
        }
    }

    if out.is_empty() {
        return Err("no syscall assignments parsed".into());
    }
    Ok(out)
}

fn parse_enum(src: &str, name: &str) -> Result<Vec<EnumVariant>, String> {
    let marker = format!("pub enum {name} {{");
    let start = src
        .find(&marker)
        .ok_or_else(|| format!("enum {name} not found"))?
        + marker.len();
    let rest = &src[start..];
    let mut out = Vec::new();
    for line in rest.lines() {
        let line = line.trim();
        if line == "}" {
            break;
        }
        if line.is_empty()
            || line.starts_with("//")
            || line.starts_with("#[")
            || line.starts_with("pub ")
        {
            continue;
        }
        let Some((variant, value)) = line.split_once('=') else {
            continue;
        };
        let variant = variant.trim();
        if variant.is_empty() || variant.contains(' ') {
            continue;
        }
        let value = value.trim().trim_end_matches(',').trim();
        let Ok(value) = value.parse::<u32>() else {
            continue;
        };
        out.push(EnumVariant {
            value,
            name: variant.to_string(),
        });
    }
    if out.is_empty() {
        return Err(format!("no variants parsed for enum {name}"));
    }
    out.sort_by_key(|v| v.value);
    Ok(out)
}

fn parse_u32_const(src: &str, name: &str) -> Result<u32, String> {
    let marker = format!("{name}: u32 = ");
    let start = src
        .find(&marker)
        .ok_or_else(|| format!("constant {name} not found"))?
        + marker.len();
    let value = src[start..]
        .split(';')
        .next()
        .ok_or_else(|| format!("constant {name} has no value"))?
        .trim();
    value
        .parse::<u32>()
        .map_err(|e| format!("constant {name} is not a u32: {e}"))
}

fn parse_u16_const(src: &str, name: &str) -> Result<u16, String> {
    let marker = format!("{name}: u16 = ");
    let start = src
        .find(&marker)
        .ok_or_else(|| format!("constant {name} not found"))?
        + marker.len();
    let value = src[start..]
        .split(';')
        .next()
        .ok_or_else(|| format!("constant {name} has no value"))?
        .trim();
    value
        .parse::<u16>()
        .map_err(|e| format!("constant {name} is not a u16: {e}"))
}

fn parse_package_version(cargo: &str) -> Result<String, String> {
    for line in cargo.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("version = ") {
            return Ok(rest.trim().trim_matches('"').to_string());
        }
    }
    Err("package version not found".into())
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_syscalls_and_permissions() {
        let src = r#"
    pub static ref SYSCALL_TABLE: [Option<SyscallFn>; 256] = {
        let mut t: [Option<SyscallFn>; 256] = [None; 256];
        t[0] = Some(handler_exit as SyscallFn);
        t[40] = Some(handler_ob_open as SyscallFn);
        t
    };
    pub static ref SYSCALL_PERMISSIONS: [SyscallPermission; 256] = {
        let mut t: [SyscallPermission; 256] = [SyscallPermission::free(); 256];
        t[0] = SyscallPermission::user();
        t[40] = SyscallPermission::admin();
        t
    };
"#;
        let got = parse_syscalls(src).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "exit");
        assert_eq!(got[0].permission, "user");
        assert_eq!(got[1].rax, 40);
        assert_eq!(got[1].name, "ob_open");
        assert_eq!(got[1].permission, "admin");
    }

    #[test]
    fn parses_enum_with_doc_comments() {
        let src = r#"
pub enum ObType {
    /// doc
    Unknown = 0,
    Process = 1,
    Socket = 18,
}
"#;
        let got = parse_enum(src, "ObType").unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(got[1].name, "Process");
        assert_eq!(got[2].value, 18);
    }

    #[test]
    fn parses_constants() {
        let src = "pub const ABI_VERSION: u32 = 8;\npub const ABI_MIN_VALID: u16 = 1;";
        assert_eq!(parse_u32_const(src, "ABI_VERSION").unwrap(), 8);
        assert_eq!(parse_u16_const(src, "ABI_MIN_VALID").unwrap(), 1);
    }
}
