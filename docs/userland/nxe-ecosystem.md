# NXE/NXP Ecosystem — Complete Architecture

> **Version:** v1.0-draft
> **Status:** Design complete
> **Applies to:** NeoDOS v0.50+

---

## 1. Design Principles

1. **NXE stays ELF64.** The kernel ELF loader is unchanged. Zero format breakage.
2. **Metadata via ELF note.** A `.note.neodos` section carries product info, version, deps, resources — ignored by the kernel, readable by tools.
3. **Resources are files.** Every app gets a standard resource directory inside its package. No new kernel syscalls for resources.
4. **NXP = container.** Packages bundle NXE(s) + NXL(s) + resources + manifest into a single `.nxp` file.
5. **Ob namespace for resources.** Installed resources live at `\Resource\<app>\<path>` — accessible via standard `ob_open`/`ob_query_info(ReadContent)`.
6. **Userland-only.** Everything except a trivial helper lives in Ring 3. The kernel provides Ob + Cm syscalls; the ecosystem uses them.

---

## 2. NXE Format (extended)

### 2.1 Backwards compatibility

NXE = ELF64 `ET_DYN` (PIE). The kernel loader in `elf.rs` only reads `PT_LOAD` segments. All extensions live in a **`.note.neodos`** ELF note section (type `SHT_NOTE`) that the kernel ignores.

### 2.2 NXE Metadata Note

```
Offset  Size  Field
──────  ────  ──────────────────────
0       4     namesz: u32 = 8       ("NeoDOS\0\0")
4       4     descsz: u32 = N       (metadata block size)
8       4     type:  u32 = 1        (NT_NEODOS_META)
12      8     name:  "NeoDOS\0\0"
20      N     metadata block (TLV)
```

### 2.3 Metadata Block (TLV inside the note)

The metadata block uses a simple TLV encoding inside the note descriptor:

```
tag: u16 LE
length: u16 LE
value: [u8; length]
```

Repeated until the descriptor is consumed. All tags are **optional**. Unknown tags are silently skipped.

### 2.4 Metadata Tags

| Tag | Value | Type | Description |
|-----|-------|------|-------------|
| `NEM_PRODUCT_NAME` | 0x0001 | UTF-8 string | Human-readable product name |
| `NEM_PRODUCT_VER` | 0x0002 | UTF-8 string | Product version (semver) |
| `NEM_FILE_VERSION` | 0x0003 | UTF-8 string | File version (semver) |
| `NEM_DESCRIPTION` | 0x0004 | UTF-8 string | Short description |
| `NEM_AUTHOR` | 0x0005 | UTF-8 string | Author/maintainer |
| `NEM_LICENSE` | 0x0006 | UTF-8 string | SPDX license identifier |
| `NEM_ARCH` | 0x0007 | UTF-8 string | Target arch (e.g. "x86_64") |
| `NEM_SUBSYSTEM` | 0x0008 | u32 LE | 0=NATIVE, 1=CONSOLE, 2=GUI |
| `NEM_LANGUAGE` | 0x0009 | UTF-8 string | Default locale tag (e.g. "en-US") |
| `NEM_MIN_KERNEL` | 0x000A | UTF-8 string | Minimum kernel version |
| `NEM_DEPENDENCIES` | 0x000B | UTF-8 string | Semicolon-separated NXL deps |
| `NEM_RES_DIR` | 0x000C | UTF-8 string | Resource directory within package |
| `NEM_MANIFEST_FLAGS` | 0x000D | u32 LE | Bit 0=CORE, 1=BOOT, 2=NOASLR, 3=SINGLETON |
| `NEM_ORIGINAL_FILENAME` | 0x000E | UTF-8 string | Original NXE filename |
| `NEM_BUILD_TIMESTAMP` | 0x000F | u64 LE | Unix timestamp of build |
| `NEM_GIT_HASH` | 0x0010 | UTF-8 string | Git commit hash |
| `NEM_RESERVED_SIGNATURE` | 0x0011 | blob | Reserved for digital signature |

### 2.5 Subsystem enum

| Value | Name | Description |
|-------|------|-------------|
| 0 | `NATIVE` | No subsystem (raw _start) |
| 1 | `CONSOLE` | Console application (stdin/stdout) |
| 2 | `GUI` | GUI application (reserved for future) |

### 2.6 Manifest flags bitmask

| Bit | Flag | Description |
|-----|------|-------------|
| 0 | `CORE` | Critical system component — kernel guards deletion |
| 1 | `BOOT` | Loaded at boot time |
| 2 | `NOASLR` | Disable ASLR for this binary |
| 3 | `SINGLETON` | Only one instance allowed |

### 2.7 Linker script update

The `libneodos/user.ld` linker script is extended to include the `.note.neodos` section:

```ld
.note : {
    *(.note*)
}
```

The metadata is emitted from Rust code using `#[link_section = ".note.neodos"]` on a static byte array. A build helper macro (`nxe_metadata!`) generates the binary TLV blob automatically.

---

## 3. NXP Package Format

### 3.1 Overview

NXP = NeoDOS eXecutable Package. Binary container, little-endian, self-validating with CRC32 and optional signature space.

```
┌──────────────────────────────────────────────────────┐
│ Magic "NXP1" (4 bytes)                                │
├──────────────────────────────────────────────────────┤
│ Header (32 bytes)                                     │
│   header_crc32 | version | flags | manifest_off/sz   │
│   entry_count | sig_off/sz | reserved                 │
├──────────────────────────────────────────────────────┤
│ Manifest (TLV) — package metadata                     │
├──────────────────────────────────────────────────────┤
│ Entry Table (32 bytes × N) — file index               │
├──────────────────────────────────────────────────────┤
│ String Pool (null-terminated UTF-8 paths)            │
├──────────────────────────────────────────────────────┤
│ File Data (raw blobs)                                 │
├──────────────────────────────────────────────────────┤
│ Optional Signature Block (Ed25519 / reserved)         │
├──────────────────────────────────────────────────────┤
│ Footer (12 bytes) — checksums                         │
└──────────────────────────────────────────────────────┘
```

### 3.2 Header (32 bytes)

| Offset | Size | Field | Notes |
|--------|------|-------|-------|
| 0 | 4 | `magic` | `0x3150584E` = "NXP1" |
| 4 | 4 | `header_crc32` | CRC32 of bytes 8..31 |
| 8 | 2 | `version_major` | Format version major (1) |
| 10 | 2 | `version_minor` | Format version minor (0) |
| 12 | 4 | `flags` | Bit 0=SIGNED, 1=COMPRESSED |
| 16 | 4 | `manifest_offset` | From file start |
| 20 | 4 | `manifest_size` | In bytes |
| 24 | 4 | `entry_count` | Number of file entries |
| 28 | 4 | `signature_offset` | 0 if unsigned |

### 3.3 Manifest TLV tags

| Tag | Multi | Value | Example |
|-----|-------|-------|---------|
| `NAME` | No | Qualified name | `org.neodos/NeoTop` |
| `VER` | No | Semver | `1.2.0` |
| `DESC` | No | Free text | `Process monitor` |
| `AUTH` | No | Author string | `NeoDOS Team` |
| `LICE` | No | SPDX | `MIT` |
| `ARCH` | No | Target arch | `x86_64` |
| `CORE` | No | `0` or `1` | `0` |
| `BOOT` | No | `0` or `1` | `0` |
| `DEP` | Yes | `name:version_req` | `libneodos:>=1.0` |
| `CONF` | Yes | Conflict | `oldtool:<2.0` |
| `REPL` | Yes | Replaces | `oldtool:<2.0` |
| `PROV` | Yes | Virtual provides | `shell` |
| `PREI` | No | Pre-install script | `scripts/preinst.sh` |
| `POSTI` | No | Post-install script | `scripts/postinst.sh` |

### 3.4 Entry Table (32 bytes per entry)

| Offset | Size | Field | Notes |
|--------|------|-------|-------|
| 0 | 4 | `path_offset` | Offset into string pool |
| 4 | 4 | `data_offset` | From file start |
| 8 | 4 | `data_size` | Uncompressed size |
| 12 | 4 | `crc32` | CRC32 of this blob |
| 16 | 4 | `flags` | Bit 0=EXECUTABLE, 1=CONFIG, 2=LOCALE |
| 20 | 4 | `mode` | File permissions (RWX bits) |
| 24 | 4 | reserved | 0 |
| 28 | 4 | reserved | 0 |

### 3.5 Standard package layout

```
<package>.nxp
├── program.nxe              [EXECUTABLE, /Programs/<name>/<name>.nxe]
├── resources/
│   ├── manifest.toml        [CONFIG]
│   ├── locale/
│   │   ├── en-US/
│   │   │   ├── strings.nlt  [LOCALE]
│   │   │   └── help.txt
│   │   └── es-ES/
│   │       ├── strings.nlt  [LOCALE]
│   │       └── help.txt
│   └── icons/
│       └── app.ico
├── config/
│   └── defaults.ini         [CONFIG]
└── scripts/
    ├── pre-install.sh
    └── post-install.sh
```

---

## 4. Resource System

### 4.1 Design

Resources are files within an installed package, served through the Object Manager namespace at `\Resource\<app_name>\<path>`.

```
\Resource\
  neoshell\
    locale\en-US\strings.nlt     →  ob_open → ob_query_info(ReadContent)
    config\defaults.ini
  neoinit\
    locale\es-ES\strings.nlt
```

### 4.2 libneodos Resource API (`libneodos/src/res.rs`)

```rust
/// Open a resource file for the current application.
/// Path is relative to the package resource root.
pub fn res_open(path: &str) -> Result<u8, i64>;

/// Open a resource for a specific application.
pub fn res_open_app(app: &str, path: &str) -> Result<u8, i64>;

/// Open a localized resource (auto-resolve language).
/// Falls back: {lang} → {lang-only} → en-US → first available.
pub fn res_open_locale(app: &str, path: &str) -> Result<u8, i64>;

/// Read resource content into buffer.
pub fn res_read(fd: u8, buf: &mut [u8]) -> Result<usize, i64>;

/// Get resource metadata.
pub fn res_size(fd: u8) -> Result<u64, i64>;

/// List resources in a package directory.
pub fn res_enum(app: &str, dir: &str) -> Result<Vec<String>, i64>;
```

### 4.3 Resource resolution

1. `res_open_app("neoshell", "locale/en-US/strings.nlt")`
2. Constructs path: `\Global\FileSystem\C:\Programs\neoshell\resources\locale\en-US\strings.nlt`
3. Calls `ob_open(path, READ)`
4. Returns fd

### 4.4 Locale-aware resource resolution

`res_open_locale` implements the fallback chain:
1. `\{lang}\{path}` (e.g. `es-ES\strings.nlt`)
2. `\{lang-only}\{path}` (e.g. `es\strings.nlt`)
3. `\en-US\{path}`
4. `\{path}` (unlocalized fallback)

---

## 5. Internationalization Integration

### 5.1 NLT files as resources

NLTv2 files are stored as resources within the package:
```
resources/locale/{lang}/{app}.nlt
```

### 5.2 Updated i18n_load

```rust
pub fn i18n_load(app: &str) -> Result<(), ()> {
    // 1. Try installed package resources first
    if let Ok(fd) = res_open_locale(app, &format!("locale/{}/{}.nlt", i18n_language(), app)) {
        return load_nlt_from_fd(fd);
    }
    // 2. Fall back to system locale directory
    load_nlt_from_path(&format!("C:\\System\\Locale\\{}\\{}.nlt", i18n_language(), app))
}
```

### 5.3 i18n_load_from_package

New function that loads NLT from the app's own package resource directory:
```rust
pub fn i18n_load_from_package() -> Result<(), ()>;
```
This enables self-contained apps with bundled translations.

---

## 6. Official Tools

### 6.1 nxinfo — NXE binary inspector (host tool)

```text
nxinfo <file.nxe> [options]

Options:
  --brief         Show only essential info
  --metadata      Show metadata block
  --sections      Show ELF sections
  --headers       Show ELF + program headers
  --json          Output as JSON
  --check         Validate NXE format
```

**Location:** [`NeoTools`](https://github.com/NeoDOS-Project/NeoTools) — `nxeinfo`, runs on host (not Ring 3).

### 6.2 nxpkg — Package manager (host tool)

```text
nxpkg <command> [options]

Commands:
  create <dir> <output.nxp>     Create NXP from directory
  extract <nxp> <output-dir>    Extract NXP to directory
  list    <nxp>                 List contents
  info    <nxp>                 Show package metadata
  verify  <nxp>                 Verify CRC32 integrity
  check   <nxp>                 Full validation
```

**Location:** [`NeoTools`](https://github.com/NeoDOS-Project/NeoTools) — `nxpkg`, runs on host.

### 6.3 nxdump — Technical dump (host tool)

```text
nxdump <file.nxe|.nxl|.nem> [options]

Options:
  --hex           Full hex dump  
  --elf           ELF structures
  --relocs        Relocation entries
  --strings       String table dump
  --segments      Segment map with addresses
```

**Location:** [`NeoTools`](https://github.com/NeoDOS-Project/NeoTools) — `nxdump`, runs on host.

### 6.4 nxres — Resource explorer (Ring 3 .NXE)

```text
nxres <command> [options]

Commands:
  list    <app>                 List resources
  cat     <app> <path>          Display resource content
  tree    <app>                 Tree view of resources
  find    <app> <pattern>       Search resources
  locale  <app>                 Show available locales
```

**Location:** `userbin/nxres/` — Ring 3 .NXE binary.

### 6.5 nxlocale — Locale manager (Ring 3 .NXE)

```text
nxlocale <command> [options]

Commands:
  list                    List installed locales
  current                 Show current locale
  set     <locale>        Change system locale
  check   [app]           Check translation coverage
  stats   [app]           Translation statistics
  show    <app>           Show app's loaded strings
```

**Location:** `userbin/nxlocale/` — Ring 3 .NXE binary.
*(Note: This replaces the existing `userbin/neolocale` which remains for NLT file operations.)*

### 6.6 nxverify — Integrity verification (Ring 3 .NXE)

```text
nxverify <command> [options]

Commands:
  file    <path>            Verify single NXE/NXP
  app     <name>            Verify installed app
  all                       Verify all installed apps
  package <nxp>             Verify package CRC32
```

**Location:** `userbin/nxverify/` — Ring 3 .NXE binary.

---

## 7. Integration with NeoDev

### 7.1 Build pipeline additions

```
cargo build --release    →  NXE ELF binary
  └── nxeinfo --embed    →  Inject .note.neodos metadata section
      └── nxpkg create   →  Bundle into .nxp package
```

### 7.2 NeoDev extensions

```rust
// New subcommands:
neodev nxpkg <name>              // Build NXP for named app
neodev nxpkg --all               // Build NXPs for all apps
neodev nxpkg --image             // Build + image with packages

// build.rs additions:
pub fn build_nxp_packages(disc: &Discovery) -> Result<()> {
    // For each user_bin, build NXE, inject metadata, create NXP
}
```

### 7.3 NXE metadata generation

Each NXE binary gets a `build.rs` (or NeoDev injects post-build) that:
1. Reads `Cargo.toml` for name/version/description
2. Generates the `.note.neodos` TLV metadata blob
3. Injects it as a new ELF section via `objcopy --add-section`

### 7.4 File placement in image

```
C:\Programs\<name>\<name>.nxe     ← NXE binary
C:\Programs\<name>\resources\...  ← Resource files
C:\Packages\<name>.nxp            ← Full package (optional)
```

---

## 8. Installation Architecture

### 8.1 Package install directory

```
C:\Programs\<name>\          ← Application root
  <name>.nxe                 ← Main executable
  resources\                 ← Read-only resource files
    locale\en-US\...         ← Translations
    icons\...                ← Graphics
  config\                    ← Default configuration
  scripts\                   ← Lifecycle scripts
```

### 8.2 Registry database

```
\Registry\Machine\Packages\
  <name>\
    Version       REG_SZ     "1.2.0"
    InstallPath   REG_SZ     "C:\Programs\<name>"
    State         REG_DWORD  3 (Current)
    Description   REG_SZ     "..."
    Author        REG_SZ     "..."
    License       REG_SZ     "MIT"

    Files\
      program.nxe   REG_SZ  "crc32:0xA1B2C3D4"

    Dependencies\
      libneodos     REG_SZ  ">=1.0"
```

### 8.3 Install steps

1. Extract NXP to `C:\Programs\<name>\`
2. Execute `scripts/pre-install` (if present)
3. Write Registry keys under `\Packages\<name>`
4. Execute `scripts/post-install`
5. Register `\Resource\<name>\` in Ob namespace
6. Set State = Current

### 8.4 Remove steps

1. Execute `scripts/pre-remove`
2. Delete files
3. Remove Registry keys
4. Execute `scripts/post-remove`

---

## 9. API Surface Summary

### libneodos new APIs

| Module | Function | Description |
|--------|----------|-------------|
| `res` | `res_open(path)` | Open resource for current app |
| `res` | `res_open_app(app, path)` | Open resource for any app |
| `res` | `res_open_locale(app, path)` | Open localized resource |
| `res` | `res_read(fd, buf)` | Read resource content |
| `res` | `res_size(fd)` | Get resource size |
| `res` | `res_enum(app, dir)` | List resources |
| `i18n` | `i18n_load_from_package()` | Load NLT from own package |
| `i18n` | `i18n_available_locales()` | List available locales |
| `i18n` | `i18n_format(template, args)` | Format with `{0}` placeholders |

### Ob namespace additions

| Path | Type | Description |
|------|------|-------------|
| `\Resource\<app>\<path>` | Virtual | Resource directory/file |

---

## 10. Future Improvements

See `roadmap/improvements.md` section **NXE-ECOSYSTEM** for the complete future roadmap including:

- NXP repository support
- Digital signatures (Ed25519)
- Package auto-update
- NeoStore GUI
- Dependency graphs
- Installation policies (GPO-like)
- Transactional install (rollback)
- Content-addressable package store
- Build-time dependency resolution
- Cross-compilation support
