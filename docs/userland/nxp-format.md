# NXP Format Reference

> **Extension:** `.NXP` (on disk: `.nxp`)
> **Magic:** `0x3150584E` (`"NXP1"`)
> **Tools:** [`NeoTools`](https://github.com/NeoDOS-Project/NeoTools) — `nxpkg` (create, extract, list, info, verify)

---

## 1. Container Layout

```
┌──────────────────────────────────────────────┐
│ Magic "NXP1" (4 bytes)                        │
├──────────────────────────────────────────────┤
│ Header (32 bytes)                             │
├──────────────────────────────────────────────┤
│ Manifest (TLV entries)                        │
├──────────────────────────────────────────────┤
│ File Entry Table (32 bytes × N)               │
├──────────────────────────────────────────────┤
│ String Pool (null-terminated UTF-8 paths)     │
├──────────────────────────────────────────────┤
│ File Data (raw blobs)                         │
├──────────────────────────────────────────────┤
│ Optional: Signature Block                     │
└──────────────────────────────────────────────┘
```

### Header (32 bytes)

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | `magic` = `0x3150584E` |
| 4 | 4 | `header_crc32` (CRC32 of bytes 8..31) |
| 8 | 2 | `version_major` (1) |
| 10 | 2 | `version_minor` (0) |
| 12 | 4 | `flags` (bit 0=SIGNED, 1=COMPRESSED) |
| 16 | 4 | `manifest_offset` |
| 20 | 4 | `manifest_size` |
| 24 | 4 | `entry_count` |
| 28 | 4 | `signature_offset` (0 if unsigned) |

### Manifest TLV tags

| Tag | Multi | Value | Example |
|-----|-------|-------|---------|
| `NAME` | No | Qualified name | `org.neodos/NeoTop` |
| `VER ` | No | Semver | `1.2.0` |
| `DESC` | No | Free text | `Process monitor` |
| `AUTH` | No | Author | `NeoDOS Team` |
| `LICE` | No | SPDX | `MIT` |
| `ARCH` | No | Target arch | `x86_64` |
| `CORE` | No | 0/1 | `0` |
| `BOOT` | No | 0/1 | `0` |
| `DEP ` | Yes | `name:ver_req` | `libneodos:>=1.0` |

### File Entry (32 bytes each)

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | `path_offset` (into string pool) |
| 4 | 4 | `data_offset` (from file start) |
| 8 | 4 | `data_size` (bytes) |
| 12 | 4 | `crc32` |
| 16 | 4 | `flags` (1=EXEC, 2=CONFIG, 4=LOCALE) |
| 20 | 4 | `mode` (permissions, reserved) |
| 24 | 4 | reserved |
| 28 | 4 | reserved |

---

## 2. Package Manifest (`neopkg.toml`)

Stored in the source directory alongside the files to be packaged:

```toml
name = "neoshell"
version = "1.0.0"
description = "NeoDOS Command Shell"
author = "NeoDOS Team"
license = "MIT"
```

---

## 3. Standard Package Layout

```
<name>/
├── neopkg.toml            # Package manifest
├── <name>.nxe             # Main executable (optional)
├── resources/
│   ├── manifest.toml
│   ├── locale/
│   │   ├── en-US/
│   │   │   ├── strings.nlt
│   │   │   └── help.txt
│   │   └── es-ES/
│   │       ├── strings.nlt
│   │       └── help.txt
│   └── icons/
│       └── app.ico
├── config/
│   └── defaults.ini
└── scripts/
    ├── pre-install.sh
    └── post-install.sh
```

---

## 4. Commands

```bash
# Create package from directory
nxpkg create ./myapp/ myapp.nxp

# Extract package to directory
nxpkg extract myapp.nxp ./myapp/

# List contents
nxpkg list myapp.nxp

# Show package metadata
nxpkg info myapp.nxp

# Verify CRC32 integrity
nxpkg verify myapp.nxp
```
