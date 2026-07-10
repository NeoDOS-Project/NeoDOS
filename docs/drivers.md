# Driver Overview

NeoDOS drivers are NEM-format binaries (.nem) loaded by the kernel and managed
through the Driver Runtime. The architecture provides ABI negotiation, dependency
resolution, capability-based access control, and optional memory isolation.

## NEM v2/v3 Format

Source: `src/nem/mod.rs`, `src/drivers/nem/v3loader.rs`.

### NEM v3 Header (80 bytes)

Source: `neodos-kernel/src/nem/mod.rs`. `#[repr(C)]` struct with implicit alignment padding.

| Offset | Size | Field         | Description                            |
|--------|------|---------------|----------------------------------------|
| 0      | 4    | magic         | "NEM3" = 0x334D454E                   |
| 4      | 4    | version       | 3 for v3                               |
| 8      | 4    | header_size   | 80                                     |
| 12     | 4    | flags         | Various flags                          |
| 16     | 2    | abi_min       | Min ABI version                        |
| 18     | 2    | abi_target    | Targeted ABI version                   |
| 20     | 2    | abi_max       | Max supported ABI version              |
| 22     | 2    | driver_type   | NemDriverType (0=Null..5=Burst)        |
| 24     | 2    | category      | 0=BOOT, 1=SYSTEM, 2=DEMAND            |
| 26     | 2    | (padding)     | Alignment padding                      |
| 28     | 4    | text_size     | .text section size                     |
| 32     | 4    | rodata_size   | .rodata section size                   |
| 36     | 4    | data_size     | .data section size                     |
| 40     | 4    | bss_size      | .bss section size                      |
| 44     | 4    | total_mem_size| Total memory required                  |
| 48     | 4    | entry_init    | Offset of init function                |
| 52     | 4    | entry_event   | Offset of event handler                |
| 56     | 4    | entry_fini    | Offset of fini function                |
| 60     | 4    | num_relocs    | Number of relocation entries           |
| 64     | 4    | relocs_offset | Relocation table offset                |
| 68     | 4    | syms_offset   | Symbol table offset                    |
| 72     | 4    | strtab_offset | String table offset                    |
| 76     | 4    | name_offset   | Driver name offset (ASCII)             |

### Relocation Types

| Constant      | Value | Formula        |
|---------------|-------|----------------|
| R_NEM_NONE   | 0     | None           |
| R_NEM_64     | 1     | S + A (64-bit) |
| R_NEM_PC32   | 2     | S + A - P      |
| R_NEM_32     | 3     | S + A (32-bit) |
| R_NEM_32S    | 4     | S + A (sign-ext)|
| R_NEM_PLT32  | 5     | S + A - P      |

### Section Types

| Constant          | Value |
|-------------------|-------|
| NEM_SECT_TEXT    | 0     |
| NEM_SECT_RODATA  | 1     |
| NEM_SECT_DATA    | 2     |
| NEM_SECT_BSS     | 3     |
| NEM_SECT_UNDEF   | 0xFF  |

ABI constants: `ABI_MIN_VALID = 1`, `ABI_TARGET = 1`, `ABI_MAX_VALID = 2`.

## ABI Negotiation Layer

Source: `src/drivers/abi/mod.rs`. Each driver carries an `AbiVersion`:

```rust
pub struct AbiVersion {
    pub min: u16,    // minimum ABI version required
    pub target: u16, // version the driver was compiled for
    pub max: u16,    // maximum ABI version supported
}
```

Negotiation rules:

1. `driver.min <= ABI_MAX_VALID` (kernel can satisfy minimum)
2. `driver.max >= ABI_MIN_VALID` (driver can run on this kernel)
3. `driver.target` within `[ABI_MIN_VALID, ABI_MAX_VALID]`

Result:

| Variant                     | Meaning                        |
|-----------------------------|--------------------------------|
| Compatible                  | Full compatibility             |
| CompatibleWithWarnings(w)   | Compatible, see warnings list  |
| Incompatible(reason)        | Cannot load                    |

Warnings: `driver.max < ABI_TARGET` (driver built for older kernel),
`driver.target > ABI_TARGET` (driver expects newer features).

10 unit tests cover all valid/invalid combinations.

## Driver Certification Pipeline

Source: `src/drivers/driver_runtime.rs`. Every driver follows an 8-state lifecycle:

```
                    ┌─────────┐
                    │  Loaded │
                    └────┬────┘
                         │ driver_init()
                    ┌────v────┐
                    │Initializ│
                    └────┬────┘
                         │ register()
                    ┌────v────┐
                    │Registrd │
                    └────┬────┘
                         │ bind()
                    ┌────v────┐
                    │  Bound  │
                    └────┬────┘
                         │ certify_and_activate()
                    ┌────v────┐
                    │ Active  │
                    └────┬────┘
                         │ unload()
                    ┌────v──────┐
              ┌─────┤ Unloading ├─────┐
              │     └────┬──────┘     │
              │          │ done       │
              │     ┌────v──────┐     │
              │     │ Unloaded  │     │
              │     └────┬──────┘     │
              │          │ reload     │
              │          └> Loaded    │
              │                       │
              └── Any → Faulted ──────┘
```

Error tracking:

| Code | Constant                |
|------|-------------------------|
| 0    | ERR_NONE                |
| 1    | ERR_INIT_FAILED         |
| 2    | ERR_REGISTRATION_FAILED |
| 3    | ERR_BIND_FAILED         |
| 4    | ERR_SANDBOX_REJECTED    |
| 5    | ERR_CERTIFICATION_FAILED|
| 6    | ERR_OUT_OF_MEMORY       |
| 7    | ERR_POLICY_VIOLATION    |
| 8    | ERR_LOAD_FAILED         |
| 9    | ERR_CAPABILITY_DENIED   |
| 10   | ERR_UNLOAD_FAILED       |
| 11   | ERR_UNLOAD_TIMEOUT      |

`certify_and_activate()` only transitions to ACTIVE if state == Bound,
`last_error == ERR_NONE`, and the driver is not in Faulted state.

## Driver Dependency Resolver

Source: `src/drivers/dependency/mod.rs`. Dependencies are declared via
`__dep_DRIVERNAME` symbols in the NEM symbol table.

```rust
pub struct DependencyGraph {
    edges: BTreeMap<String, Vec<String>>,
}
```

- `add_driver(name)` -- registers a driver node
- `add_dependency(driver, depends_on)` -- adds a directed edge
- `resolve_order()` -- returns a topological ordering via DFS
- `has_cycle()` -- detects circular dependencies

Max 32 deps per driver, max 16 drivers in dep graph. 13 unit tests.

## X3 Capability System

Source: `src/drivers/caps.rs`. Fine-grained resource access control.

| Bit | Constant          | Default BOOT | SYSTEM | DEMAND |
|-----|-------------------|:---:|:------:|:------:|
| 0   | CAP_IRQ           | Yes | Yes    |        |
| 1   | CAP_DMA           | Yes | Yes    |        |
| 2   | CAP_MMIO          | Yes | Yes    |        |
| 3   | CAP_PORTIO        | Yes | Yes    |        |
| 4   | CAP_ALLOC_PAGE    | Yes | Esc*   |        |
| 5   | CAP_BLOCK_DEVICE  | Yes | Esc*   |        |
| 6   | CAP_EVENT_BUS     | Yes | Yes    | Yes    |
| 7   | CAP_INPUT         | Yes | Yes    |        |
| 8   | CAP_LOG           | Yes | Yes    | Yes    |
| 9   | CAP_TIMING        | Yes | Yes    | Yes    |
| 10  | CAP_MEMORY        | Yes | Esc*   |        |
| 11  | CAP_ISOLATION     | Yes |        |        |
| 12  | CAP_NS_WRITE      | Yes |        |        |

Esc*: SYSTEM drivers may escalate via `EVENT_CAP_ESCALATION` (0x2000).
DEMAND drivers cannot escalate -- this is a hard security boundary.

Each `hst_*` export function calls `check_cap()` before executing.

## X4 Driver Isolation

Source: `src/drivers/isolation.rs`. Dedicated 16 MB region at
`DRIVER_ISO_BASE` (0x30000000) with 16 slots of 1 MB each.

### Isolation Modes

| Mode    | Description                                      |
|---------|--------------------------------------------------|
| None    | Legacy: heap-allocated, no memory restrictions   |
| Basic   | Page-isolated, export table bridge, arg validation|
| Sandbox | Full isolation: faults outside region = FAULTED  |

Key API:

```rust
pub fn allocate_driver_slot() -> Option<u64>;
pub fn free_driver_slot(base: u64);
pub fn alloc_isolated_page(virt: u64, flags: u64) -> bool;
pub fn free_isolated_page(virt: u64);
pub fn validate_driver_ptr(ptr: u64, size: u64) -> bool;
pub fn handle_isolated_page_fault(virt: u64) -> bool;
```

`validate_driver_ptr()` accepts: driver slot region, kernel heap, .rodata/.text,
user heap, mmap region, user code range, and kernel image. All other addresses
are rejected. 12 tests.

## Boot Driver Loader

Source: `src/drivers/boot_loader/mod.rs`. Called at Phase 3.85.

Sequence per category:

1. `driver_scan()` -- discover .nem files on NeoFS
2. `driver_load()` -- parse NEM header, allocate memory
3. `driver_init()` -- call init entry point
4. `driver_activate()` -- complete certification

BOOT drivers load first (hardware init required), then SYSTEM drivers.
Within each category, the dependency resolver determines topological order.

```rust
pub fn boot_load_all() -> BootSummary;
pub struct BootSummary {
    pub boot_total: u32,
    pub boot_ok: u32,
    pub boot_fail: u32,
    pub system_total: u32,
    pub system_ok: u32,
    pub system_fail: u32,
}
```

If a BOOT driver fails, the kernel continues (no panic); the driver is marked
FAULTED and logged. 8 tests.

## NDREG CLI

Shell command `NDREG` for driver diagnostics:

| Command | Purpose                                |
|---------|----------------------------------------|
| LIST    | List all loaded drivers + state        |
| SHOW    | Show driver details                    |
| QUERY   | Query driver capabilities              |
| RUNTIME | Runtime statistics                     |
| HEALTH  | Health check (state, errors, caps)     |
| DEBUG   | Debug-level info (isolation, memory)   |
| LOAD    | Load a .nem file                      |
| UNLOAD  | Unload a running driver                |
| RELOAD  | Hot-reload: Unload -> Load activate    |
