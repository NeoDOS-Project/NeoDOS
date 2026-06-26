# NeoDOS Architecture

This document describes the *current* NeoDOS boot/runtime architecture as implemented in the repository.

> **⚠️ NUEVA GUÍA ARQUITECTÓNICA:** Para la visión a largo plazo (v0.40 → v1.0), el plan director,
> el diagnóstico de arquitectura, el roadmap por versiones y las decisiones de diseño estratégicas,
> consultar [`ARCHITECTURAL_VISION.md`](ARCHITECTURAL_VISION.md).
>
> Este documento describe el estado actual. El documento de visión describe hacia dónde vamos.

## Boot Flow

```
UEFI Firmware (OVMF)
  └─ parses GPT → finds ESP → loads `bootloader.efi` from `/EFI/BOOT/BOOTX64.EFI`
       ↓
NeoDOS Bootloader (UEFI application)
  - initializes UEFI services + logging
  - initializes GOP framebuffer info
  - loads `kernel.elf` from the ESP (FAT32 partition)
  - loads ELF PT_LOAD segments into memory
  - loads NeoDOS FS image into memory (RAM disk)
  - calls ExitBootServices and captures the final UEFI memory map
  - calls the kernel entry point as: extern "sysv64" fn(&BootInfo) -> !
       ↓
NeoDOS Kernel (x86_64-unknown-none)
  - graphics init + RAM disk + serial + VGA console
  - CPU structures (GDT/IDT/MSI/PIC) + IOAPIC init (MADT) + legacy PIC disable
  - timers: HPET (ACPI) → APIC timer calibration → APIC timer active
  - PS/2 + USB HID init
  - physical memory init (UEFI mem map → buddy frame allocator)
  - kernel heap allocator init (slab allocator + linked_list_allocator fallback)
  - SMP: INIT-SIPI-SIPI + per-CPU KPRCB + IPI infrastructure
  - I/O APIC: detect from MADT, disable PIC, route ISA IRQs 0/1/4/12
  - enable interrupts (STI)
  - custom page tables (4 GiB identity map + user window + demand-paging heap split)
  - PCIe ECAM init: read MCFG → map MMIO as UC- → activate ECAM (PIO fallback)
  - ATA boot stub (BootAta) + AHCI probe + NVMe probe
  - GPT scan → NeoDOS partition → IoStack → block cache → mount NeoDOS FS on C:
  - FAT32 ESP mount on A:
  - Ring 3 shell (neoshell.nxe via NeoInit, 512 kernel tests + user commands)
```

## Disco único GPT

Todo el sistema cabe en una sola imagen de disco con tabla de particiones GUID (GPT):

```
┌──────────────────────────────┐
│  LBA 0:  Protective MBR     │
│  LBA 1:  GPT Header         │
│  LBA 2–33: Partition Table  │
│  LBA 34–2047: (alignment)   │
│  LBA 2048–206847: ESP/FAT32 │  ← bootloader.efi + kernel.elf
│  LBA 206848–227327: NeoDOS  │  ← Sistema de archivos NeoDOS
│  ... backup GPT ...         │
└──────────────────────────────┘
```

La imagen se genera con `scripts/create_gpt_image.py`, que utiliza `sfdisk` (util-linux)
para crear la tabla GPT y luego copia los datos de cada partición en su offset correcto.
El kernel incluye `drivers/gpt.rs` que parsea la tabla y encuentra la partición NeoDOS
por su GUID de tipo (`EBD0A0A2-B9E5-4433-87C0-68B6B72699C7`).

## ATA, PCI DMA y AHCI

El kernel usa una arquitectura de dos niveles para ATA:

### Boot stub (`neodos-kernel/src/drivers/ata.rs`)
`BootAta` — PIO only, primary channel only. Used during early boot (PHASE 3.6–3.8) for GPT
parsing, NeoDOS superblock read, and block cache warmup before NEM drivers are loaded.

### NEM v3 standalone driver (`drivers/ata/` → `ata.nem`, SYSTEM category)
Full-featured ATA driver loaded at PHASE 3.85 by the boot loader. Scans PCI for IDE controller
(class 0x01, subclass 0x01) with bus-master capability (prog-if bit 7). Enables bus-master bit
in PCI command register. Initializes primary + secondary channels. Supports DMA read/write (via
PRDT, up to 8 sectors / 4 KB) and PIO multi-sector fallback. Each active channel registers a
block device via `hst_register_block_device()` with the kernel's `NemBlockDevice` registry.

### AHCI fallback
If an AHCI controller is found after PCI scan, the storage manager uses AHCI in preference to
ATA. The AHCI driver uses DMA polling per port with separate buffers, supports ATA (READ/WRITE
DMA EXT) and ATAPI (PACKET + READ_10 CDB). If AHCI has no active ports, falls back to ATA boot
stub (or NEM ATA driver once loaded).

`base_lba` se configura en `main.rs` después de parsear la GPT.

## RAM Disk

The bootloader loads the NeoDOS filesystem image into memory (as a raw byte buffer) and passes the address/size in `BootInfo`. The kernel stores these in `globals::RAM_DISK_BASE` / `RAM_DISK_SIZE` and provides `globals::ram_disk_buf() -> Option<&[u8]>`. The RAM disk is used during boot to load the NeoDOS FS and user binaries.

## Boot ABI (`BootInfo`)

The bootloader passes a pointer to a `BootInfo` struct using the System V AMD64 ABI:

- `RDI` = `&BootInfo` (first argument)

`BootInfo` contains:

- `magic: u32` — must be `0x4E444F53` ("NDOS")
- `version: u32` — bootloader version code (`0x00MMmmPP`)
- GOP framebuffer info (base, size, width/height/stride)
- raw pointer to the final UEFI memory map buffer plus its metadata (`size`, `desc_size`, `desc_version`)
- `fs_image_addr: u64` / `fs_image_size: u64` — RAM disk buffer location

The memory map buffer is intentionally leaked by the bootloader after `ExitBootServices` so the kernel can read it.

## Memory Model (Current)

- Kernel link/entry address starts at `0x200000` (see `neodos-kernel/kernel.ld`).
- Custom paging identity-maps the first 4 GiB.
- **User heap**: 32 MB virtual range `0x10000000..0x12000000`, organised as 16 × 2 MB huge pages. At boot `init_heap_demand_paging()` splits each 2 MB huge page into 4 KB page tables. Physical frames are allocated on demand by the page fault handler when user-space touches a new page.
- **Demand paging**: The page fault handler (`idt.rs`) checks if the faulting address falls in the heap range; if so, it calls `handle_heap_page_fault()` which walks the 4 KB page tables and allocates a physical frame via `allocate_frame()` marked as `USER_ACCESSIBLE`. On heap shrink (`sys_brk`), `heap_free_range()` unmaps pages and returns frames to the frame allocator. `heap_alloc_page()` touches pages to trigger page faults.
- The frame allocator manages the first 4 GiB of physical memory via a bitmap (`memory.rs`). A page of memory is 4096 bytes (0x1000).
- The `MEM` shell command (migrated to Ring 3 as `MEM.NXE`) reports totals derived from the UEFI memory map, clamped to the first 4 GiB and with some reservations applied:
  - first 1 MiB
  - kernel image (`__kernel_start..__kernel_end`)
  - framebuffer range

## Driver Architecture (v0.16+)

NeoDOS implements a **layered driver architecture** with full hardware access mediation. No driver touches hardware directly; all access goes through `driver → Event Bus → HAL ABI v0.3 → hardware`.

```
┌──────────────────────────────────────────────────────────┐
│                    Drivers (NEM v3 / built-in)            │
│   AHCI · ATA · PS/2 · FAT32 · RTC · PCI · NVMe · USB    │
│   null · echo · timer_listener · reference drivers       │
└─────────────────────────┬────────────────────────────────┘
                          │ Event Bus v2 (56-byte Event struct)
┌─────────────────────────▼────────────────────────────────┐
│                    Event Bus v2                            │
│   src/eventbus/mod.rs — priority queues, filters,         │
│   2 priority levels (high 16, normal 64 slots)            │
│   13 event types, 64 handlers max, dynamic payload        │
│   dispatch from scheduler (NEVER in IRQ context)          │
└─────────────────────────┬────────────────────────────────┘
                          │ 26 primitives extern "C"
┌─────────────────────────▼────────────────────────────────┐
│              HAL ABI v0.3                                  │
│   src/hal/x64/ — cpu, io, mem, irq, time                 │
│   Port I/O · CPU control · Page memory · IRQ · Timer      │
└─────────────────────────┬────────────────────────────────┘
                          │ inb/outb, STI/CLI, alloc_page, etc.
┌─────────────────────────▼────────────────────────────────┐
│                    Hardware (x86_64)                       │
│   PIT · COM1 · PS/2 · PCI · Framebuffer · ATA · AHCI     │
└──────────────────────────────────────────────────────────┘
```

---

### 0. Kernel Object Manager — KOBJ (`src/kobj/mod.rs`, `src/kobj/namespace.rs`, `src/vfs/mount.rs`)

Unified kernel object system for tracking, referencing, and enumerating kernel objects. Includes hierarchical object namespace (NT5) with symbolic links, case-insensitive path lookup, and VFS mount point integration.

| Concept | Description |
|---------|-------------|
| **KObjType** | Enum (u32): Unknown(0), Process, Driver, Device, Pipe, EventBus, BlockDevice, Filesystem, MemoryRegion, Symlink(9), MountPoint(10), Directory(11) |
| **KObjEntry** | Metadata per object: KObjId (u64, global sequential), refcount (u32), type, 24-byte name, flags, creation_tick, native_id |
| **KObjRegistry** | Dynamic `Vec<Option<KObjEntry>>`, no hard limit, protected by `spin::Mutex`, global via `lazy_static!` |
| **Namespace** | `src/kobj/namespace.rs` — hierarchical `\`-rooted tree with `DirectoryObject` nodes, `BTreeMap`-backed children, case-insensitive keys. Standard dirs: `\Device`, `\DosDevices`, `\Global`, `\Driver`, `\FileSystem`, `\Ob` |
| **Symlinks** | `SymlinkEntry` in namespace nodes, max 10 hop resolution with loop detection |
| **Mount points** | `src/vfs/mount.rs` — `MountManager` with `MountPoint` struct, `FilesystemType` enum, DosDevices symlink creation, global `MOUNT_MANAGER` |
| **API** | `kobj_register()`, `kobj_unregister()`, `kobj_ref()`, `kobj_unref()`, `kobj_lookup()`, `kobj_count()`, `kobj_iter_snapshot()`, `ob_insert_object_auto()`, `ob_remove_object_auto()`, `ob_lookup_by_path()` |
| **Integration** | Processes, drivers, pipes auto-register on creation and auto-unregister on destruction. Mount points register via `vfs_mount()` during boot |
| **CLI** | `KOBJ` via Ring 3 `kobj.nxe` (ob_enum RAX=64) — lists all namespace objects |

The KOBJ registry is populated at boot by driver loading and at runtime by process/pipe creation. Objects are automatically removed when their lifecycle ends (process exit, driver unload, pipe close). Directory KObjs for `\Device`, `\DosDevices`, `\Global`, `\Driver`, `\FileSystem`, `\Ob` are registered at boot via `init_object_namespace()`. MountPoints for `C:` (NeoDOS FS) and `A:` (FAT32 ESP) are registered during PHASE 3.6.

---

### 1. HAL ABI v0.4 — raw/safe split (`src/hal/`)

Lowest kernel layer. All inline assembly confined to `src/hal/` (55 asm calls). Zero `asm!()` outside.

| Layer | Module | Contents |
|-------|--------|----------|
| **raw** | `hal/raw/` | Bare asm primitives: `raw_read_msr`, `raw_write_msr`, `raw_pause`, `raw_sti/cli`, `raw_halt`, `raw_read_tsc`, `raw_cpuid`, `raw_read_cr0/2/3/4`, `raw_write_cr3`, `raw_invlpg`, `raw_invpcid`, `raw_read_rflags`, `raw_lgdt/lidt/ltr`, `raw_set_segment_regs`, `raw_gs_read/write_u64/u32/u16/u8`, `raw_inb/outb/inw/outw/inl/outl`, `raw_rep_stosd` |
| **safe** | `hal/safe/` | Type-safe wrappers: `Msr` trait with `read_msr<T: Msr>()` / `write_msr<T: Msr>()`, MSR constants (`GS_BASE`, `APIC_BASE_MSR`, `EFER`), `IsSafe` flag, `read_cr2()` |
| **x64 ABI** | `hal/x64/` | Extern "C" ABI surface (26 primitives), delegates to `hal/raw`. `cpu.rs`, `io.rs`, `mem.rs`, `irq.rs`, `time.rs`, `irql.rs`, `mod.rs` |

**Extern "C" primitives (ABI surface):**

| Module | Primitives |
|--------|------------|
| CPU | `enable_interrupts()`, `disable_interrupts()`, `halt()`, `poweroff()`, `read_cr2()`, `read_cr3()`, `write_cr3()`, `flush_tlb()`, `interrupts_enabled()`, `hlt_once()` |
| Port I/O | `inb()`, `outb()`, `inw()`, `outw()`, `inl()`, `outl()` |
| Page Memory | `alloc_page()`, `free_page()`, `map_page()`, `unmap_page()`, `memory_barrier()` |
| Interrupts | `register_irq()` (stub), `ack_irq()` |
| Timing | `get_ticks()`, `increment_ticks()`, `sleep_hint()` |
| IRQL | `raise_irql()`, `lower_irql()`, `current_irql()`, `at_or_above_dispatch()`, `IrqMutex<T>`, `at_dispatch()` |

Non-ABI helpers (Rust ABI): `without_interrupts()` (legacy), `walk_ptes_4k()`, `cpu_info()`.

CPU initialization code (GDT, IDT, PIC, paging) stays in `arch/x64/` — it is architecture-specific and not part of the HAL contract.

---

### 2. Event Bus v2 (`src/eventbus/mod.rs`)

**Centralized event routing layer** with priority queues, subscription filters, dynamic payload, and backpressure. Transforms raw IRQs into normalized events.

**`Event` structure** (56 bytes, `#[repr(C)]`, ABI-stable for NEM drivers):
```rust
struct Event {
    event_id: u64,       // monotonically increasing
    event_type: u32,     // TIMER_TICK, KEYBOARD_INPUT, etc.
    source: u32,         // SOURCE_HAL, DRIVER, KERNEL, USERLAND
    timestamp: u64,      // HAL tick at push time
    device_id: u32,
    driver_target: u32,
    data0: u64, data1: u64,
    flags: u32,          // URGENT, BROADCAST
}
```

**16 event types (FROZEN v0.42 — types 0–15 MUST NOT be reassigned):**

| Constant | Value | Description |
|-----------|-------|-------------|
| `EVENT_TIMER_TICK` | 0 | Timer tick (PIT IRQ0) |
| `EVENT_KEYBOARD_INPUT` | 1 | Key pressed (PS/2 IRQ1) |
| `EVENT_SERIAL_DATA` | 2 | Serial data received |
| `EVENT_DISK_IO_COMPLETE` | 3 | Disk operation complete |
| `EVENT_PROCESS_EXIT` | 4 | Process terminated |
| `EVENT_DRIVER_LOADED` | 5 | Driver loaded |
| `EVENT_DRIVER_CRASH` | 6 | Driver crashed |
| `EVENT_POLICY_VIOLATION` | 7 | Policy violation |
| `EVENT_FS_MOUNTED` | 8 | Filesystem mounted |
| `EVENT_KEYB_LAYOUT` | 9 | Keyboard layout switch |
| `EVENT_RTC_READ` | 10 | RTC read request |
| `EVENT_RTC_DATA` | 11 | RTC data ready |
| `EVENT_SHUTDOWN` | 12 | System shutdown request |
| `EVENT_DRIVER_UNLOAD` | 13 | Driver unload request |
| `EVENT_DRIVER_UNLOAD_ACK` | 14 | Driver unload acknowledgement |
| `EVENT_NMI_WATCHDOG` | 15 | NMI watchdog timeout |
| `EVENT_USER` | 0x1000 | User-defined event base |
| `EVENT_WILDCARD` | 0xFFFFFFFF | Matches any type |

**Internal architecture:**
- **Priority queues**: Two lock-free SPSC ring buffers — **high** (16 slots) for timers/IRQ completions, **normal** (64 slots) for system events. High queue drained first in all dispatch paths.
- **Handlers**: Up to 64 callbacks `fn(&Event)`, protected by `Mutex<[Option<RegisteredHandler>; 64]>`. Each handler has an `EventFilter` (event_type, source_mask bitfield, device_id).
- **Subscription**: `register_handler_v2(filter, callback, name)` with structured `EventFilter`. v1 `register_handler(type, callback, name)` creates a type-only filter (backward compatible).
- **Dynamic payload**: `push_event_with_dyn_payload()` allocates a copy on the kernel heap, stores pointer in `data0`/`data1`, and auto-frees after dispatch via the handlers table.
- **Backpressure**: When a queue is full, `push_event()` returns `Err(())` — producers must handle. `ERR_EVENT_BUS_FULL` constant (−16) for NEM drivers.
- **Unregistration**: `unregister_handler(callback)` or `unregister_handler_by_name(name)`.
- **Dispatch**: `dispatch_one()` / `dispatch_pending()` — **never** executed in IRQ context. Called from: (1) `clear_need_resched()` on every syscall return, (2) idle loop, (3) shell input loop.
- **IRQ integration**: `push_event()` from PIT IRQ0 (timer tick) and PS/2 IRQ1 (keyboard) — all lock-free pushes.
- **Isolation**: No driver execution in IRQ context. No recursive dispatch. Events immutable after enqueue.

**API:** `push_event()`, `push_event_with_dyn_payload()`, `register_handler()`, `register_handler_v2()`, `unregister_handler()`, `unregister_handler_by_name()`, `dispatch_pending()`, `dispatch_one()`, `handler_count()`, `queue_available()`.

---

### 3. NEM v3 — Driver Format (`src/nem/mod.rs`)

NeoDOS Driver Format v3. 80-byte header + sections (text, rodata, data, bss) + relocation table + symbol table + string table.

**`NemHeaderV3` (80 bytes):**
```
Offset  Size   Field              Description
0       4      magic              "NEM3"
4       4      version            3
8       4      header_size        80
12      4      flags              Various flags
16      2      abi_min            Minimum required ABI version
18      2      abi_target         Target ABI version
20      2      abi_max            Maximum supported ABI version
22      2      driver_type        NemDriverType (0-5)
24      2      category           DriverCategory (0=Boot,1=System,2=Demand)
26      4      text_size          Code size
30      4      rodata_size        Read-only data size
34      4      data_size          Initialized data size
38      4      bss_size           BSS size (zero-fill)
42      4      total_mem_size     Total memory required
46      4      entry_init         Offset from text base
50      4      entry_event        Offset from text base
54      4      entry_fini         Offset from text base
58      4      num_relocs         Number of relocations
62      4      relocs_offset      Relocation table offset
66      4      syms_offset        Symbol table offset
70      4      strtab_offset      String table offset
74      4      name_offset        Driver name offset (ASCII)
```

**Driver types (`NemDriverType`):** `Null=0`, `Echo=1`, `Lifecycle=2`, `Mutation=3`, `Fault=4`, `Burst=5`

**Categories (`DriverCategory`):** `Boot=0` (boot-time load), `System=1` (system load), `Demand=2` (on-demand)

**Relocations (`NemReloc`, 12 bytes):** `offset`, `section`, `r_type`, `sym_idx`, `addend`

**Relocation types:** `R_NEM_NONE=0`, `R_NEM_64=1`, `R_NEM_PC32=2`, `R_NEM_32=3`, `R_NEM_32S=4`, `R_NEM_PLT32=5`

**Sections:** `NEM_SECT_TEXT=0`, `NEM_SECT_RODATA=1`, `NEM_SECT_DATA=2`, `NEM_SECT_BSS=3`

**Parser:** `parse_nem_v3(data) → Option<ParsedNemV3>` — zero-copy, no alloc. Validates magic, version, size, ranges, offsets.

**ABI validation:** A driver is ABI-compatible if:
- `abi_min ≤ ABI_MAX_VALID` (driver doesn't require a newer kernel)
- `abi_max ≥ ABI_MIN_VALID` (driver is not too old)
- `ABI_MIN_VALID ≤ abi_target ≤ ABI_MAX_VALID`

ABI constants: `ABI_MIN_VALID=1`, `ABI_TARGET=1`, `ABI_MAX_VALID=2`

---

### 4. NEM v3 Loader (`src/drivers/nem/v3loader.rs`)

Standalone NEM v3 binary driver loader. Loads a `.nem` from NeoFS or raw data, applies relocations, and resolves symbols against the **Kernel Export Table (KET)**.

**Load flow:**
1. Parse NEM v3 header with `parse_nem_v3()`
2. Validate ABI with `validate_v3_abi()`
3. Allocate contiguous memory via `alloc_driver_memory()` (max 1 MB per driver)
4. Copy sections (text, rodata, data, bss zero-fill)
5. Apply relocations: resolve UNDEF symbols against KET
6. Resolve entry points: `entry_init`, `entry_event`, `entry_activate`, `entry_fini`

**Kernel Export Table (KET):** 13 symbols exported to NEM v3 drivers. Each symbol requires the calling driver to hold the corresponding capability (X3 Capability System):

| Symbol | Description | Required Capability |
|---------|-------------|-------------------|
| `hst_push_input_byte(byte)` | Push byte to kernel input buffer | `CAP_INPUT` |
| `hst_log(level, msg, len)` | Logging | `CAP_LOG` |
| `hst_get_ticks()` | Get tick counter | `CAP_TIMING` |
| `hst_ack_irq(vector)` | IRQ acknowledge | `CAP_IRQ` |
| `hst_push_event(et, src, dev, d0, d1, fl)` | Push event to Event Bus | `CAP_EVENT_BUS` |
| `hst_inb(port)` / `hst_outb(port, val)` | 8-bit I/O | `CAP_PORTIO` |
| `hst_inw(port)` / `hst_outw(port, val)` | 16-bit I/O | `CAP_PORTIO` |
| `hst_inl(port)` / `hst_outl(port, val)` | 32-bit I/O | `CAP_PORTIO` |
| `hst_register_block_device(...)` | Register block device with kernel | `CAP_BLOCK_DEVICE` |
| `hst_unregister_block_device(idx)` | Unregister block device | `CAP_BLOCK_DEVICE` |

**Event Bus Bridge:** `register_v3_event_bus_handler()` — bridge between the v3 driver calling convention (`driver_on_event(*const Event) → i32`) and the kernel Event Bus (`fn(&Event)`). Uses a static `AtomicUsize` to store the function pointer.

---

### 5. Driver Certification Pipeline (`src/drivers/driver_runtime.rs`)

Strict **7-state state machine** for driver lifecycle management.

```
Loaded(0) → Initialized(1) → Registered(2) → Bound(3) → Active(4)
Any state → Faulted(5) | Unloaded(6)
```

| State | Description |
|--------|-------------|
| `Loaded` | Binary loaded in memory, not verified |
| `Initialized` | `driver_init()` executed successfully |
| `Registered` | Registered in Driver Runtime + Event Bus notified |
| `Bound` | Event Bus / Device binding completed |
| `Active` | Fully operational, certified |
| `Faulted` | Runtime failure |
| `Unloaded` | Removed from system (terminal) |

**Transition rules:** Only sequential forward transitions are allowed. Any skip (e.g., Loaded → Active) is rejected with `TransitionError`. Any state can transition to Faulted or Unloaded.

**`DriverInstance`** — per-driver struct with:
- `id`, `name[8]`, `driver_type`, `state`
- `api_version`, `compat_flags`, `abi_min/target/max`, `category`
- `events_received`, `tick_count`, `last_event_type/tick`, `registered_at_tick`
- `last_error: u32` — error code (0 = OK)
- `certification_step: u8` — pipeline step where failure occurred
- `caps: u64` — capability bitmap (X3 Capability System)

**Error codes:** `ERR_NONE=0`, `ERR_INIT_FAILED=1`, `ERR_REGISTRATION_FAILED=2`, `ERR_BIND_FAILED=3`, `ERR_SANDBOX_REJECTED=4`, `ERR_CERTIFICATION_FAILED=5`, `ERR_OUT_OF_MEMORY=6`, `ERR_POLICY_VIOLATION=7`, `ERR_LOAD_FAILED=8`, `ERR_CAPABILITY_DENIED=9`.

**`certify_and_activate(id)`**: Only promotes to Active if:
1. Current state == Bound (proves the sequence was followed)
2. `last_error == 0` (no prior errors)
3. Not Faulted

**Pipeline steps:** `PipelineStep::None=0`, `Load=1`, `Init=2`, `Registration=3`, `Binding=4`, `Certification=5`

**Global driver runtime:** `lazy_static! { DRIVER_RUNTIME: Mutex<DriverRuntime> }` with 16 slots max.

#### 5.5. X3 Capability System (`src/drivers/caps.rs`)

Fine-grained resource access control for NEM drivers. Each driver inherits a 64-bit capability bitmap at load time based on its category:

| Category | Default Capabilities |
|----------|---------------------|
| **BOOT** | All 11 flags (`CAP_ALL`) |
| **SYSTEM** | `CAP_PORTIO \| CAP_IRQ \| CAP_MMIO \| CAP_DMA \| CAP_EVENT_BUS \| CAP_INPUT \| CAP_LOG \| CAP_TIMING` |
| **DEMAND** | `CAP_EVENT_BUS \| CAP_LOG \| CAP_TIMING` |

**Runtime enforcement:** Every `hst_*` function in the KET calls `check_cap()` before executing. If the calling driver lacks the required capability, the function returns a sentinel error (0, -1, or no-op) instead of executing. The `current_driver_id()` static tracks which driver is active — set before each `driver_init`/activate/event dispatch call.

**Capability escalation:** A SYSTEM driver may request `CAP_ALLOC_PAGE`, `CAP_BLOCK_DEVICE`, or `CAP_MEMORY` via `EVENT_CAP_ESCALATION` (type `0x2000`). The kernel audits and may grant. DEMAND drivers cannot escalate — this is a security boundary.

See `AGENTS.md` for the complete flag table and implementation details.

---

### 6. Boot Driver Loader (`src/drivers/boot_loader/mod.rs`)

Automatic NEM v3 driver loading orchestrator at system startup (PHASE 3.85 in `main.rs`).

**Load order:**
1. **BOOT drivers** — scanned from `C:\System\Drivers\` (essential for init)

2. **SYSTEM drivers** — scanned from `C:\System\Drivers\` (standard extensions)
If any BOOT driver fails, boot continues (no panic) and the driver is marked FAULTED.

**API:**
```rust
fn driver_scan(path: &str) -> Vec<String>     // Scan directory for *.nem files
fn read_nem_file(path: &str) -> Result<Vec<u8>, &'static str>
fn boot_load_all()                             // Full orchestrator
```

**Per-driver flow:**
```
read_nem_file() → parse_nem_v3() → validate ABI → load_nem_v3()
  → register_driver() [Loaded]
  → driver_init() [Initialized]
  → try_transition(Registered) [Registered]
  → register_v3_event_bus_handler() [Bound]
  → entry_activate() [optional]
  → certify_and_activate() [Active]
  → on failure → set_error() + Faulted
```

**`BootSummary`**: returns totals per category (boot/system) with ok/fail counters.

---

### 7. Built-in Drivers (`src/drivers/builtin_drivers.rs`)

Drivers embedded in the kernel that register as Event Bus callbacks.

| Driver | NEM Type | Events received | Behavior |
|--------|----------|--------------------|----------------|
| `null` | Null | TIMER_TICK | Only counts events |
| `echo` | Echo | TIMER_TICK + KEYBOARD_INPUT | Counts events |
| `timer_listener` | Lifecycle | TIMER_TICK | Counts ticks, certification pipeline demo |

None execute external driver code — they only update `DriverRuntime` statistics.

---

### 8. Legacy: Driver Loader (`src/drivers/driver_loader.rs`)

Legacy mechanism for loading NEM drivers from the shell. Does NOT execute init or certification — the driver stays in **Loaded** state (not Active).

- `load_nem(path)` — loads and registers, emits `EVENT_DRIVER_LOADED`
- `unload_driver(id)` — removes from runtime
- `LOADNEM` / `NDREG` — Ring 3 commands via `ob_create(Driver)` and `ob_query_info(Drivers)`
- `cmd_unloadnem(id)` — unload by ID

**`LOADNEM <path>` command**: loads but does NOT activate.

---

### 8.5. NXL System (`src/nxl.rs`)

Shared library (NXL) loading subsystem for user-mode processes.

**NXL region**: `0x1e000000..0x1e200000` (2 MB, 8 slots of 256 KB each). Split into 4 KB page tables during boot (PHASE 3.87).

**Available NXLs**:
| NXL | Slot | Address | Load |
|-----|------|---------|------|
| `libneodos.nxl` | 0 | `0x1e000000` | Auto-loaded at boot |
| `libmath.nxl` | 1 | `0x1e040000` | Manual via `LOADLIB` |

**sys_loadlib (RAX=21)**: Loads a NeoDOS NXL from NeoFS into the next free slot. Returns base address. The NXL ELF is parsed, sections mapped as USER_ACCESSIBLE (read-only), and the export table (`AbiTable`) becomes accessible at the base address.

**Shell command**: `LOADLIB C:\System\Libraries\fs.nxl` loads a shared library (e.g., `LOADLIB C:\System\Libraries\math.nxl` for the math library).

**libneodos wrapper**: `libneodos::loadlib(path)` invokes `sys_loadlib` and returns the NXL base address for user-mode `extern "C"` function dispatch.

---

### 9. NDREG — Registry CLI (`userbin/ndreg/` → `ndreg.nxe`)

A `regedit`-style tool for inspecting the driver registry.

| Subcommand | Description |
|-----------|-------------|
| `NDREG LIST [path]` | List .nem drivers with state + error + visual progress bar (5 chars: L-I-R-B-A) |
| `NDREG SHOW <name>` | Full details + certification check + error diagnostics |
| `NDREG QUERY` | Summary: FS total, runtime state breakdown |
| `NDREG RUNTIME` | Runtime snapshot: loaded/active/faulted with pipeline display |
| `NDREG HEALTH` | Validate NEM header integrity for all .nem files |
| `NDREG DEBUG <name>` | 5-stage diagnosis (LOAD → INIT → REG → BIND → CERTIFY) |
| `NDREG LOAD <path>` | Load driver through full pipeline → Active if all pass |

**Pipeline visualization:** `█████` = 5/5 steps completed, `█    ` = only Loaded.

---

### 10. Hardware Drivers (kernel-side)

Beyond the NEM driver framework, the kernel includes integrated hardware drivers:

| Driver | File | Description |
|--------|---------|-------------|
| ATA (boot stub) | `drivers/ata.rs` | PIO only, primary channel, used before NEM driver loads |
| ATA (NEM v3) | `drivers/ata/` (standalone) | DMA + PIO, primary + secondary, ~137 GB, registered via NemBlockDevice |
| AHCI | `drivers/ahci.rs` | DMA polling, per-port, ATA + ATAPI, PRDT scatter-gather |
| PS/2 | `drivers/ps2.rs` | IRQ1, scan code → ASCII via KLC layouts |
| PCI | `drivers/pci.rs` | Config space primitives via ECAM MMIO with legacy PIO fallback (0xCF8/0xCFC). Init at Phase 2.3 from ACPI MCFG. BAR read/map utilities. |
| GPT | `drivers/gpt.rs` | GUID partition table parser |
| FAT32 | `drivers/fat32.rs` | ESP partition, absolute LBAs |
| RTC | `drivers/rtc.rs` | CMOS RTC |
| ACPI | `drivers/acpi.rs` | RSDP/XSDT, poweroff via PM1a |
| NVMe | `drivers/nvme.rs` | In progress |
| Storage Manager | `drivers/storage_manager.rs` | Unifies NVMe / AHCI / ATA (boot stub) |
| Block Device | `drivers/block.rs` | Trait + block device manager |
| USB HID | `drivers/usb_hid/` | UHCI (non-functional on PIIX3) |
| ECAM PCIe | `hal/pci.rs` | MMIO ECAM config space: set_ecam_base, ecam_is_active, ecam_read/write_config_dword/word/byte |
| IOAPIC | `interrupts/ioapic.rs` | MADT-detected I/O APIC: init, mask/unmask, ISA IRQ routing, PIC disable |
| MSI-X | `interrupts/msi.rs` | Per-entry MSI-X table programming: configure_msix_entry/configures |

---

### 11. Test Coverage

The kernel testing framework includes **520 tests** (49+ suites) with suites dedicated to the driver architecture:

| Suite | Tests | Description |
|-------|-------|-------------|
| NEM | 23 | v3 parsing, ABI, relocations, edge cases |
| Event Bus | 17 | Creation, push/pop, order, overflow, IDs, dispatch, filters |
| Driver State | 21 | 7-state pipeline, transition matrix, certification |
| Boot Loader | 8 | Scan, load, init, activate, categories |
| PS/2 Kbd Ref | 10 | Reference PS/2 keyboard driver |
| Framebuffer Ref | 8 | Reference framebuffer driver |
| Storage Ref | 14 | Reference storage driver |
| ELF | 20 | ELF64 loader with A4.3 address space validation, PIE/ASLR support |
| Pipe | 13 | IPC pipes |
| Mmap | 6 | Memory mapping |
| FSCK | 6 | Filesystem integrity |
| Page Cache | 13 | Page cache (advanced): hash map O(1), LRU doubly-linked, create, peek, dirty, invalidate, capacity, stats, hit_rate, pending_writes |
| PCI Enumeration | 3 | PCI bus 0 devices, bus 1 empty, bridge detection |
| IRQL | 5 | IRQL raise/lower, page fault invariant, spinlock implicit raise, nesting, preemption threshold |
| DPC | 5 | DPC engine: enqueue/dispatch, IRQ transition, nesting, callback order, stress 100 IRQs |
| APC | 5 | APC engine: kernel dispatch, alertable wait, queue overflow, IRP→APC completion, stress 100 concurrent IRPs |
| KWait | 10 | Unified Wait Engine: block/wake 7 WaitReason variants, PipeRead, IrpComplete, ThreadJoin, ChildExit, Event, Timer, Alertable |
| ABI Freeze | 4 | Frozen event types 0–15, capability bits 0–11, IOAPIC API |
| KOBJ | 8 | Kernel Object Manager: register/unregister, refcount, type enum, lookup |
| Object (Ob) | 14 | ObObjectTable: create/lookup/destroy, refcount, close auto-destroy |
| Slab | 9 | Slab allocator: per-size alloc/free, multi-page, realloc fallback |
| Per-CPU Slab | 5 | Per-CPU slab alloc/free, refill/drain batching, scaling |
| IPI | 5 | Inter-processor interrupts: constants, TLB shootdown, call function |
| Work Queue | 6 | Deferred work queue: push/pop, FIFO, empty, overflow, isolation |
| DPC | 5 | DPC engine: enqueue/dispatch, IRQ transition, nesting, callback order |
| Stress | 14 | Stress: sched, syscall, mem, buddy allocator, handle table |
| Hot Reload | 11 | Hot reload: resource tracking, registry, state transitions |
| Security | 12 | NT6 Security: SID format, Token, ACL allow/deny, SeAccessCheck, admin bypass |
| URN | 15 | NT5.5 Unified Resource Namespace: parse schemes, resolve file/device, Ob frontend (OB-025) |
| KDrive | 12 | NT5.6 Virtual FS K:\: root readdir, lookup, case-insensitive, stats |

Tests run automatically at boot. The kernel runs 520 tests, then optionally executes user-mode binaries (`C:\Programs\cpuinfo.nxe`, `C:\Programs\dir.nxe`, `C:\Programs\datetime.nxe`, `C:\Programs\ver.nxe`).

---

### 12. Architecture Rules

- Drivers **never** touch hardware directly. All access via Event Bus or HAL ABI.
- Drivers **never** execute in IRQ context. Events are queued and dispatched from the scheduler.
- The certification pipeline is **strict**: states cannot be skipped.
- A driver is **ACTIVE** only after Loaded → Initialized → Registered → Bound → Active.
- HAL is the lowest layer. The kernel depends on HAL, never the reverse.
- `without_interrupts()` is used for critical sections that cannot be interrupted.

## Kernel Subsystems (High-Level)
- **apc**: `src/apc/mod.rs` — Asynchronous Procedure Call engine. Per-thread kernel/user APC queues (max 64 each). Kernel APCs dispatched at PASSIVE_LEVEL on syscall return. User APCs dispatched one-at-a-time before IRETQ to Ring 3. Used for IRP completion delivery (DIRQL→DPC→APC flow) and deferred callback execution.
- **kobj**: `src/kobj/mod.rs` + `src/kobj/namespace.rs` — Kernel Object Manager. Unified object tracking with reference counting, type identification (KObjType=12 variants), 24-byte names, and dynamic Vec-based global registry (no hard limit). Hierarchical object namespace with symlinks, case-insensitive path lookup, and Directory KObjs. `KOBJ` via Ring 3 `kobj.nxe` lists all live objects.
- **arch/x64**: GDT, IDT, PIC, paging (4-level, 2 MB huge pages + 4 KB demand-paging), interrupt handlers (timer IRQ0, keyboard IRQ1, syscall INT 0x80)
- **drivers**: ATA (PIO boot stub + NEM v3 standalone DMA driver), AHCI, PS/2 keyboard, USB HID, PCI NEM driver (bus scan + Event Bus service), device event infrastructure
- **buffer**: `buffer/block_cache.rs` — block cache (periodic flush via timer); `buffer/page_cache.rs` — page cache (128-entry, 512 KB hash map O(1) + LRU cache for file data I/O, dirty write-back with `flush_batch()`, timer-driven via `NEED_PAGE_CACHE_FLUSH`)
- **fs**: **VFS layer** (`fs/vfs.rs`) — `Vfs` struct with 26 drive slots (A-Z), `FileSystem` trait (`read`/`write`/`lookup`/`readdir`/`mkdir`/`create`/`stat`/`remove_file`/`remove_dir`/`rename`), `VfsNode { inode, mode, size }`, path resolution with `walk_components`, mount point support. Implementations: `NeoDosFs` (native format, mounted on C:), `Fat32Driver` (ESP, mounted on A:). **Mount points** (`vfs/mount.rs`) register KObjType::MountPoint entries and DosDevices symlinks via `MountManager`.
- **memory**: frame allocator (bitmap, 4 GiB max), external heap allocator (`linked_list_allocator` 16 MB @ 0x1000000), user heap demand-paging (0x10000000..0x12000000, 32 MB, 16 × 2 MB slots → 4 KB PTs)
- **process**: `Process` struct with PID, state, registers, `user_slot`, `cwd_drive`/`cwd_path`, `heap_base`/`heap_break`, `waiting_for`, `kernel_stack` (private `Option<Box<AlignedKStack>>`), `handle_table` (unified handle table: files, pipes, devices, events), `mmap_regions`, `kobj_id` (optional KOBJ reference)
- **scheduler**: round-robin (`schedule()`), timer-driven (`on_timer_tick` every 100 ticks ≈ 5.5 Hz), max 16 processes, idle process (PID 0) always present. `recycle_terminated(pid)` removes a process from the table, dropping its kernel stack and freeing the slot. `cleanup_terminated_process(pid)` is the public wrapper called from `cmd_run` (sys_exit path) and `sys_waitpid`.
- **usermode**: Ring 3 execution via `execute_usermode_asm` (IRETQ), process lifecycle in `spawn_usermode`/`wait_for_process`/`sys_exit` → `exit_to_kernel`. On exit: external resources freed in `syscall_dispatch`, then `cmd_run` calls `cleanup_terminated_process(pid)` to recycle the slot and free the kernel stack. The `KILL` command calls `kill_pid()` which does complete cleanup including heap, mmap, pipes, user slot, and kernel stack, then recycles the slot immediately.
- **shell**: Ring 3 shell (`neoshell.nxe`) via NeoInit (PID 1), PATH dispatch to .NXE commands, TAB autocomplete, pipeline support, environment variables

## Kernel Safety and Synchronization (v0.10.4+)
The kernel architecture prioritizes memory safety and reentrancy:
- **Global State**: Managed via `spin::Mutex<Option<T>>` or `spin::Mutex<T>`. Access helpers: `with_vfs(f)`, `with_ata(f)`, `with_cache(f)` in `globals.rs`.
- **Atomic State**: `RAM_DISK_BASE`/`RAM_DISK_SIZE` (AtomicU64), `TIMER_TICKS` (AtomicU64), `NEED_CACHE_FLUSH` (AtomicBool), console cursor positions.
- **Periodic cache flush**: Timer tick handler sets `NEED_CACHE_FLUSH` every 180 ticks; flushed in `clear_need_resched()` before syscall returns.
- **Reentrancy**: This model prevents data races and undefined behavior when interrupts (like the timer) occur during syscall execution.
- **Input Buffer**: Implements a lock-free Single-Producer/Single-Consumer ring buffer (1024 bytes) using atomic head/tail indices.

## Syscall Table (INT 0x80)

Calling convention: RAX = syscall number, RBX = arg0, RCX = arg1, RDX = arg2, R8 = arg3, R9 = arg4. Return in RAX.

| # | Syscall | Args | Description |
|---|---------|------|-------------|
| 0 | sys_exit | RBX=code | Terminate process |
| 1 | sys_write | RBX=fd, RCX=ptr, RDX=len | Write to fd (1=console, pipe writer) |
| 2 | sys_yield | — | Yield CPU |
| 3 | sys_getpid | — | Return current PID |
| 4 | sys_read | RBX=fd, RCX=buf, RDX=count | Read from fd (0=stdin, pipe reader) |
| 5 | sys_pipe | RBX=fds_ptr | Create pipe, returns [read_fd, write_fd] |
| 6 | sys_dup2 | RBX=old_fd, RCX=new_fd | Duplicate file descriptor |
| 9 | sys_waitpid | RBX=pid | Wait for child process |
| 10 | sys_open | RBX=path_ptr, RCX=flags | Open file → fd |
| 11 | sys_readfile | RBX=fd, RCX=buf, RDX=count | Read from file (uses handle offset) |
| 13 | sys_close | RBX=fd | Close handle (pipe, file, device, event) |
| 16 | sys_chdir | RBX=path_ptr | Change current directory (legacy) |
| 18 | sys_brk | RBX=new_break | Set program break (demand-paged) |
| 19 | sys_mmap | RBX=hint, RCX=len, RDX=prot, R8=flags, R9=fd | Lazy mapping (anonymous or file-backed) |
| 20 | sys_munmap | RBX=addr, RCX=len | Free mmap mapping |
| 21 | sys_loadlib | RBX=path_ptr | Load NXL from NeoFS into NXL region slot |
| 22 | sys_thread_create | RBX=entry, RCX=stack | Create thread in current process |
| 23 | sys_thread_join | RBX=tid | Wait for thread termination |
| 40 | sys_wait_alertable | — | Alertable wait: dispatch pending APC or block |
| 41 | sys_sleep_ex | — | Alertable yield: check APC before/after yielding |
| 42 | sys_poweroff | — | Power off the machine |
| 47 | sys_chdir_parent | RBX=path_ptr | Change parent process cwd (legacy) |
| 53 | sys_cursor_blink | RBX=0/1 | Enable/disable cursor blink |
| 55 | sys_fsck | RBX=buf, RCX=drive, RDX=repair | Filesystem integrity check |
| 58 | sys_driver_unload | RBX=name, RCX=force | Unload NEM driver (admin) |
| 59 | sys_poll | RBX=pfds, RCX=nfds, RDX=timeout | Poll fds for ready I/O |
| 60 | sys_ob_open | RBX=path, RCX=access | Open Ob namespace object |
| 61 | sys_ob_create | RBX=path, RCX=type, RDX=fds, R8=attrs | Create Ob object |
| 62 | sys_ob_query_info | RBX=fd, RCX=class, RDX=buf, R8=size | Query Ob object info |
| 63 | sys_ob_set_info | RBX=fd, RCX=class, RDX=buf, R8=size | Set Ob object info |
| 64 | sys_ob_enum | RBX=dir_fd, RCX=buf, RDX=max | Enumerate Ob directory |
| 65 | sys_ob_wait | RBX=count, RCX=handles, RDX=type, R8=timeout | Wait on Ob objects |
| 66 | sys_ob_destroy | RBX=fd | Destroy/delete Ob object |

## Debug Interfaces

The provided script `scripts/qemu-debug.sh` runs QEMU with:

- Serial output to stdout (saved to `neodos/qemu_output.log`)
- QEMU monitor on `telnet 127.0.0.1:4444`
- GDB server on `tcp::1234`

See `docs/DEBUG.md` for a walkthrough.

---

## Current vs. Ideal Architecture Summary

| Aspecto | Actual | Ideal (v1.0) | Prioridad |
|---------|--------|---------------|-----------|
| **Arrays fijos** | 8 subsistemas con límites duros (16 EPROCESS, 32 KTHREAD, 16 pipes, etc.) | Slab<T> dinámico + Vec overflow | **ALTA — v0.41** |
| **Buddy bitmap** | 16384 words → 4GB máximo | Bitmap dinámico por rango o radix tree | **ALTA — v0.40** |
| **User window** | 4 MB (0x400000..0x800000) | 32+ MB mínimo | **ALTA — v0.40** |
| **Static buffers** | BIN_BUF[64KB], CMD_BUF[64KB] globales | Allocación dinámica por llamada | **ALTA — v0.40** |
| **ASLR** | No existe | ASLR v1 base, v2 pila+heap, v3 full | MEDIA — v0.44 |
| **Scheduler lookup** | O(n) linear scan | Hash map o radix tree por TID | MEDIA — v0.41 |
| **Seguridad** | SID+Token+ACL, admin bypass completo | Grupos, privilegios, SACL, audit | MEDIA — v0.43 |
| **KWait** | Ad-hoc: pipe STI/HLT, waitpid STI/HLT, IRP blocking | Unified Wait Engine v1 (7 WaitReason variants, kwait_block/wake) | **COMPLETADO — v0.42** |
| **Device Tree** | Detección ad-hoc (PCI scan, ACPI, HPET, IOAPIC) | Árbol jerárquico + Resource Manager | MEDIA — v0.45 |
| **Registry** | boot.cfg, system.cfg, input.cfg textuales | Hive persistente cell-based (NT Cm) | MEDIA — v0.44 |
| **Networking** | No existe | NIC driver NEM + TCP/IP stack | MEDIA — v0.47 |
| **IOCP** | No existe (IRP sí, pero sin completion ports) | IoCompletionPort para apps async | BAJA — v0.48 |

Forward-looking architecture reference: [`ARCHITECTURAL_VISION.md`](ARCHITECTURAL_VISION.md).
