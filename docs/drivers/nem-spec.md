# NEM v3 — NeoDOS Driver Module Specification

**Version:** 3 | **Header size:** 80 bytes | **ABI:** v1

NEM (NeoDOS Executable Module) is the native driver binary format for NeoDOS.
Drivers use the `.nem` extension and are loaded by the kernel at boot (PHASE 3.85)
or on demand via `sys_driver_load` / `loadnem.nxe`.

## Format Overview

```text
┌─────────────────────────────┐
│ NEM v3 Header (80 bytes)    │
├─────────────────────────────┤
│ Text section (code)         │
├─────────────────────────────┤
│ Rodata section (constants)  │
├─────────────────────────────┤
│ Data section (initialized)  │
├─────────────────────────────┤
│ BSS section (zero-filled)   │  ← total_mem_size extends to here
├─────────────────────────────┤
│ Relocation entries (×N)     │
├─────────────────────────────┤
│ Symbol entries (×M)         │
├─────────────────────────────┤
│ String table                │
└─────────────────────────────┘
```

## NEM v3 Header (80 bytes)

| Offset | Size | Field | Description |
| -------- | ------ | ------- | ------------- |
| 0 | 4 | `magic` | `"NEM3"` (0x334D454E) |
| 4 | 4 | `version` | Format version (3) |
| 8 | 4 | `header_size` | 80 |
| 12 | 4 | `flags` | Reserved |
| 16 | 2 | `abi_min` | Minimum compatible ABI version |
| 18 | 2 | `abi_target` | ABI version this driver was built for |
| 20 | 2 | `abi_max` | Maximum compatible ABI version |
| 22 | 2 | `driver_type` | `0`=Null, `1`=Echo, `2`=Lifecycle, `3`=Mutation, `4`=Fault, `5`=Burst |
| 24 | 2 | `category` | `0`=BOOT, `1`=SYSTEM, `2`=DEMAND |
| 26 | 4 | `text_size` | Size of text section (bytes) |
| 30 | 4 | `rodata_size` | Size of rodata section (bytes) |
| 34 | 4 | `data_size` | Size of data section (bytes) |
| 38 | 4 | `bss_size` | Size of BSS section (bytes) |
| 42 | 4 | `total_mem_size` | Total memory = text + rodata + data + bss |
| 46 | 4 | `entry_init` | Initialization entry point offset |
| 50 | 4 | `entry_event` | Event handler entry point offset |
| 54 | 4 | `entry_fini` | Finalization entry point offset |
| 58 | 4 | `num_relocs` | Number of relocation entries |
| 62 | 4 | `relocs_offset` | Offset to relocation table (from header start) |
| 66 | 4 | `syms_offset` | Offset to symbol table (from header start) |
| 70 | 4 | `strtab_offset` | Offset to string table (from header start) |
| 74 | 4 | `name_offset` | Offset to driver name string (from header start) |

## Relocation Entry (12 bytes)

| Offset | Size | Field | Description |
| -------- | ------ | ------- | ------------- |
| 0 | 4 | `offset` | Byte offset from section base |
| 4 | 2 | `section` | `0`=text, `1`=rodata, `2`=data |
| 6 | 1 | `r_type` | Relocation type (`R_NEM_*`) |
| 7 | 1 | `sym_idx` | Symbol index (`0xFF` = none) |
| 8 | 4 | `addend` | Signed addend |

### Relocation types

| Name | Value | Formula |
| ------ | ------- | --------- |
| `R_NEM_NONE` | 0 | No operation |
| `R_NEM_64` | 1 | `S + A` (64-bit absolute) |
| `R_NEM_PC32` | 2 | `S + A - P` (32-bit PC-relative) |
| `R_NEM_32` | 3 | `S + A` (32-bit zero-extended) |
| `R_NEM_32S` | 4 | `S + A` (32-bit sign-extended) |
| `R_NEM_PLT32` | 5 | `S + A - P` (same as PC32) |

Where `S` = symbol value, `A` = addend, `P` = place address.

## Symbol Entry (16 bytes)

| Offset | Size | Field | Description |
| -------- | ------ | ------- | ------------- |
| 0 | 4 | `name_off` | Offset into string table |
| 4 | 4 | `value` | Section offset (0 for UNDEF) |
| 8 | 2 | `section` | Section index (`0xFFFF` = UNDEF) |
| 10 | 1 | `info` | Symbol type + binding |
| 11 | 5 | _pad | Reserved |

## Sections

Sections are laid out sequentially after the header in this order:

1. **Text** — Executable code (read-only)
2. **Rodata** — Read-only data (constants, string literals)
3. **Data** — Initialized read-write data
4. **BSS** — Zero-initialized read-write data (not stored in file, allocated at load)

`total_mem_size` = `text_size + rodata_size + data_size + bss_size`
The loader allocates `total_mem_size` bytes and loads text/rodata/data from the file; BSS is zero-filled.

## ABI Compatibility

Drivers declare three ABI version fields:

- `abi_min` — Minimum kernel ABI the driver can work with
- `abi_target` — The ABI version the driver was compiled for (preferred)
- `abi_max` — Maximum kernel ABI the driver can work with

The kernel validates: `kernel_abi >= abi_min && kernel_abi <= abi_max`

Current values: `abi_min=1`, `abi_target=1`, `abi_max=2`

## Kernel Export Table (KET)

NEM v3 drivers can import kernel symbols resolved at load time.
The KET provides HAL primitives for hardware access, memory allocation,
and driver lifecycle.

## Driver Lifecycle States

```text
Unloaded → Loaded → Initialized → Registered → Bound → Active
                             ↓                        ↓
                          Faulted                 Unloading → Unloaded
```

## Related Documentation

- [overview.md](overview.md) — Driver architecture, isolation, capabilities
- [architecture.md](../architecture/overview.md) — Boot flow, NEM loading (PHASE 3.85)
- [HAL ABI](../kernel/hal.md) — Hardware abstraction layer
- `neodos-kernel/src/nem/mod.rs` — NEM parser source
- `neodos-kernel/src/drivers/nem/v3loader.rs` — NEM v3 loader
