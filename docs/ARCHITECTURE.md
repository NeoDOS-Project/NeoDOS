# NeoDOS Architecture

This document describes the *current* NeoDOS boot/runtime architecture as implemented in the repository.

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
  - CPU structures (GDT/IDT/PIC) + PS/2 + USB HID
  - physical memory init (UEFI mem map → frame allocator bitmap)
  - kernel heap allocator init (linked_list_allocator)
  - enable interrupts (STI)
  - ATA + PCI bus-master DMA + AHCI probe
  - GPT scan → NeoDOS partition → base_lba → block cache → mount NeoDOS FS on C:
  - FAT32 ESP mount on A:
  - custom page tables (4 GiB identity map + user window + demand-paging heap split)
  - DOS-like shell (37 kernel tests + user commands)
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

El kernel mantiene dos drivers ATA (`ATA_DRIVER` primario, `ATA_DRIVER_SECONDARY`) más un driver AHCI opcional (`AHCI_DRIVER`).

### ATA driver (`drivers/ata.rs`)
Expone dos familias de lecturas:
- **`read_sector` / `write_sector` / `read_dma` / etc.** — usan `base_lba` (offset de partición).
  El NeoDOS FS las invoca con LBAs relativos a la partición, y el driver suma `base_lba`
  antes de enviar el comando al disco.
- **`read_sector_master`** — lee LBAs absolutos (sin `base_lba`). FAT32 la usa para leer
  el sector de arranque en LBA 0 o 2048.

### PCI bus-master DMA
El kernel escanea PCI bus 0 en busca del controlador IDE (class 0x01, subclass 0x01) con capacidad bus-master. BAR4 da la I/O base. Dos buffers estáticos de 4 KB para PRDT + datos DMA. Polling-based. Soporta hasta 8 sectores (4 KB) por llamada.

### AHCI fallback
Si se encuentra un controlador AHCI tras el escaneo PCI, el driver ATA activa `ahci_fallback = true` y redirige las operaciones de disco al driver AHCI. El driver AHCI usa DMA polling por puerto con buffers separados, soporta ATA (READ/WRITE DMA EXT) y ATAPI (PACKET + READ_10 CDB).

`base_lba` se configura en `main.rs` después de parsear la GPT.

## RAM Disk

The bootloader loads the NeoDOS filesystem image into memory (as a raw byte buffer) and passes the address/size in `BootInfo`. The kernel stores these in `globals::RAM_DISK_BASE` / `RAM_DISK_SIZE` and provides `globals::ram_disk_buf() -> Option<&[u8]>`. The RAM disk is used by the shell's `run` command to load user binaries (flat `.BIN` files) without reading from the disk.

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
- The `MEM` shell command reports totals derived from the UEFI memory map, clamped to the first 4 GiB and with some reservations applied:
  - first 1 MiB
  - kernel image (`__kernel_start..__kernel_end`)
  - framebuffer range

## Driver Architecture (v0.16+)

NeoDOS implements a **layered driver architecture** with full hardware access mediation. No driver touches hardware directly; all access goes through `driver → HAL Binding Layer → HAL ABI v0.3 → hardware`.

```
┌──────────────────────────────────────────────────────────┐
│                    Drivers (NEM v3 / built-in)            │
│   AHCI · ATA · PS/2 · FAT32 · RTC · PCI · NVMe · USB    │
│   null · echo · timer_listener · reference drivers       │
└─────────────────────────┬────────────────────────────────┘
                          │ DeviceHandle (opaque, capability-limited)
┌─────────────────────────▼────────────────────────────────┐
│              Device Model + HAL Binding Layer             │
│   src/devices/mod.rs — DeviceRegistry (32 slots)          │
│   device_read/write/query/register_irq/ack_irq (stubs)    │
└─────────────────────────┬────────────────────────────────┘
                          │ Event Bus v1 (56-byte Event struct)
┌─────────────────────────▼────────────────────────────────┐
│                    Event Bus v1                            │
│   src/eventbus/mod.rs — SPSC lock-free (64 slots)         │
│   11 event types, 32 handlers max                         │
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

### 1. HAL ABI v0.3 (`src/hal/x64/`)

Lowest kernel layer. Provides **26 `extern "C` primitives** for pure hardware access:

| Module | File | Primitives |
|--------|---------|------------|
| CPU | `cpu.rs` | `enable_interrupts()`, `disable_interrupts()`, `halt()`, `poweroff()`, `read_cr2()`, `read_cr3()`, `write_cr3()`, `flush_tlb()`, `interrupts_enabled()`, `hlt_once()` |
| Port I/O | `io.rs` | `inb()`, `outb()`, `inw()`, `outw()`, `inl()`, `outl()` |
| Page Memory | `mem.rs` | `alloc_page()`, `free_page()`, `map_page()`, `unmap_page()`, `memory_barrier()` |
| Interrupts | `irq.rs` | `register_irq()` (stub), `ack_irq()` |
| Timing | `time.rs` | `get_ticks()`, `increment_ticks()`, `sleep_hint()` |

Non-ABI helpers (Rust ABI): `without_interrupts()`, `walk_ptes_4k()`, `cpu_info()`.

CPU initialization code (GDT, IDT, PIC, paging) stays in `arch/x64/` — it is architecture-specific and not part of the HAL contract.

---

### 2. Device Model (`src/devices/mod.rs`)

**Controlled hardware exposure layer**. All driver hardware access MUST go through this layer.

**Core types:**

| Type | Values |
|------|---------|
| `DeviceClass` | `Input=0`, `Storage=1`, `Timer=2`, `Communication=3`, `Virtual=4`, `Unknown=5` |
| `DeviceType` | `Keyboard=0`, `Disk=1`, `Timer=2`, `Serial=3`, `Framebuffer=4`, `PciController=5`, `AhciController=6`, `IdeController=7`, `UsbController=8`, `Generic=9` |
| `DeviceState` | `Offline=0`, `Online=1`, `Error=2` |

**Capabilities bitmap:** `CAP_READ=1`, `CAP_WRITE=2`, `CAP_IRQ=4`, `CAP_DMA=8`, `CAP_MMIO=16`

**`DeviceRegistry`**: Thread-safe 32-slot registry with 16-slot binding table, protected by `spin::Mutex`.

| Method | Description |
|--------|-------------|
| `register()` | Adds device in Online state |
| `find_by_id()` / `find_by_name()` | Lookup |
| `bind(driver, device_id) → DeviceHandle` | Creates capability-limited opaque handle |
| `unbind()` / `is_bound()` | Binding management |
| `set_state()` / `set_error()` | State control |

**`DeviceHandle`**: `{ device_id: u32, capabilities: u32 }` — no raw hardware access.

**Boot-time devices (5 registered in `register_boot_devices()`):**
| Name | Type | IRQ | Caps |
|--------|------|-----|------|
| `pit` | Timer | 32 | R |
| `com1` | Serial | — | RW |
| `ps2kbd` | Keyboard | 33 | R·I |
| `framebuffer` | Framebuffer | — | RW·M |
| `pci` | PCI Config | — | RW |

**HAL Binding Layer (stubs):** `device_read()`, `device_write()`, `device_register_irq()`, `device_ack_irq()` return `IoError` (pending implementation). `device_query_status()` works by querying the registry.

---

### 3. Event Bus v1 (`src/eventbus/mod.rs`)

**Centralized event routing layer**. Transforms raw IRQs into normalized events.

**`Event` structure** (56 bytes, `#[repr(C)]`):
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

**11 event types:**

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
| `EVENT_USER` | 0x1000 | User-defined event |
| `EVENT_WILDCARD` | 0xFFFFFFFF | Matches any type |

**Internal architecture:**
- **Queue**: SPSC (Single-Producer Single-Consumer) lock-free, 64 slots via `UnsafeCell<[Event; 64]>`, head/tail as `AtomicUsize`. Producer = IRQ context, consumer = scheduler/shell.
- **Handlers**: Up to 32 callbacks `fn(&Event)`, protected by `Mutex<[Option<RegisteredHandler>; 32]>`.
- **Dispatch**: `dispatch_one()` / `dispatch_pending()` — **never** executed in IRQ context. The scheduler calls `dispatch_pending()` from the idle loop.
- **IRQ integration**: `push_event()` from PIT IRQ0 (timer tick) and PS/2 IRQ1 (keyboard).
- **Isolation**: No driver execution in IRQ context. No recursive dispatch. Events immutable after enqueue.

**API:** `push_event()`, `register_handler()`, `unregister_handler()`, `dispatch_pending()`, `handler_count()`, `queue_available()`.

---

### 4. NEM v3 — Driver Format (`src/nem/mod.rs`)

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

### 5. NEM v3 Loader (`src/drivers/nem/v3loader.rs`)

Standalone NEM v3 binary driver loader. Loads a `.nem` from NeoFS or raw data, applies relocations, and resolves symbols against the **Kernel Export Table (KET)**.

**Load flow:**
1. Parse NEM v3 header with `parse_nem_v3()`
2. Validate ABI with `validate_v3_abi()`
3. Allocate contiguous memory via `alloc_driver_memory()` (max 1 MB per driver)
4. Copy sections (text, rodata, data, bss zero-fill)
5. Apply relocations: resolve UNDEF symbols against KET
6. Resolve entry points: `entry_init`, `entry_event`, `entry_activate`, `entry_fini`

**Kernel Export Table (KET):** 11 symbols exported to NEM v3 drivers:

| Symbol | Description |
|---------|-------------|
| `hst_push_input_byte(byte)` | Push byte to kernel input buffer |
| `hst_log(level, msg, len)` | Logging |
| `hst_get_ticks()` | Get tick counter |
| `hst_ack_irq(vector)` | IRQ acknowledge |
| `hst_push_event(et, src, dev, d0, d1, fl)` | Push event to Event Bus |
| `hst_inb(port)` / `hst_outb(port, val)` | 8-bit I/O |
| `hst_inw(port)` / `hst_outw(port, val)` | 16-bit I/O |
| `hst_inl(port)` / `hst_outl(port, val)` | 32-bit I/O |

**Event Bus Bridge:** `register_v3_event_bus_handler()` — bridge between the v3 driver calling convention (`driver_on_event(*const Event) → i32`) and the kernel Event Bus (`fn(&Event)`). Uses a static `AtomicUsize` to store the function pointer.

---

### 6. Driver Certification Pipeline (`src/drivers/driver_runtime.rs`)

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

**Error codes:** `ERR_NONE=0`, `ERR_INIT_FAILED=1`, `ERR_REGISTRATION_FAILED=2`, `ERR_BIND_FAILED=3`, `ERR_SANDBOX_REJECTED=4`, `ERR_CERTIFICATION_FAILED=5`, `ERR_OUT_OF_MEMORY=6`, `ERR_POLICY_VIOLATION=7`, `ERR_LOAD_FAILED=8`.

**`certify_and_activate(id)`**: Only promotes to Active if:
1. Current state == Bound (proves the sequence was followed)
2. `last_error == 0` (no prior errors)
3. Not Faulted

**Pipeline steps:** `PipelineStep::None=0`, `Load=1`, `Init=2`, `Registration=3`, `Binding=4`, `Certification=5`

**Global driver runtime:** `lazy_static! { DRIVER_RUNTIME: Mutex<DriverRuntime> }` with 16 slots max.

---

### 7. Boot Driver Loader (`src/drivers/boot_loader/mod.rs`)

Automatic NEM v3 driver loading orchestrator at system startup (PHASE 3.85 in `main.rs`).

**Load order:**
1. **BOOT drivers** — scanned from `C:\SYSTEM\DRIVERS\BOOT\` (essential for init)
2. **SYSTEM drivers** — scanned from `C:\SYSTEM\DRIVERS\SYSTEM\` (standard extensions)

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

### 8. Built-in Drivers (`src/drivers/builtin_drivers.rs`)

Drivers embedded in the kernel that register as Event Bus callbacks.

| Driver | NEM Type | Events received | Behavior |
|--------|----------|--------------------|----------------|
| `null` | Null | TIMER_TICK | Only counts events |
| `echo` | Echo | TIMER_TICK + KEYBOARD_INPUT | Counts events |
| `timer_listener` | Lifecycle | TIMER_TICK | Counts ticks, certification pipeline demo |

None execute external driver code — they only update `DriverRuntime` statistics.

---

### 9. Legacy: Driver Loader (`src/drivers/driver_loader.rs`)

Legacy mechanism for loading NEM drivers from the shell. Does NOT execute init or certification — the driver stays in **Loaded** state (not Active).

- `load_nem(path)` — loads and registers, emits `EVENT_DRIVER_LOADED`
- `unload_driver(id)` — removes from runtime
- `cmd_loadnem(path)` / `cmd_nemlist()` — shell commands
- `cmd_unloadnem(id)` — unload by ID

**`LOADNEM <path>` command**: loads but does NOT activate.

---

### 10. NDREG — Registry CLI (`src/shell/commands/ndreg.rs`)

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

### 11. TSR Modules (`src/tsr/mod.rs`)

Terminate-and-Stay-Resident: loads flat binaries into memory (0x430000+) and associates them with an interrupt vector. Up to 16 TSR programs, max 64 KB per program.

- `install_tsr(filename, interrupt_num, fs, cache, dev)` — loads from NeoFS, copies to memory, registers
- `dispatch_interrupt(interrupt_num)` — executes all registered TSRs for a given vector
- Shell command `TSR <FILE> <INT>`

---

### 12. Hardware Drivers (kernel-side)

Beyond the NEM driver framework, the kernel includes integrated hardware drivers:

| Driver | File | Description |
|--------|---------|-------------|
| ATA | `drivers/ata.rs` | PIO + bus-master DMA, primary + secondary, base_lba |
| AHCI | `drivers/ahci.rs` | DMA polling, per-port, ATA + ATAPI, PRDT scatter-gather |
| PS/2 | `drivers/ps2.rs` | IRQ1, scan code → ASCII via KLC layouts |
| PCI | `drivers/pci.rs` | Config space via 0xCF8/0xCFC |
| GPT | `drivers/gpt.rs` | GUID partition table parser |
| FAT32 | `drivers/fat32.rs` | ESP partition, absolute LBAs |
| RTC | `drivers/rtc.rs` | CMOS RTC |
| ACPI | `drivers/acpi.rs` | RSDP/XSDT, poweroff via PM1a |
| NVMe | `drivers/nvme.rs` | In progress |
| Storage Manager | `drivers/storage_manager.rs` | Unifies ATA/AHCI |
| Block Device | `drivers/block.rs` | Trait + block device manager |
| USB HID | `drivers/usb_hid/` | UHCI (non-functional on PIIX3) |

---

### 13. Test Coverage

The kernel testing framework includes **245+ tests** with suites dedicated to the driver architecture:

| Suite | Tests | Description |
|-------|-------|-------------|
| NEM | 23 | v1+v2+v3 parsing, ABI, relocations, edge cases |
| Event Bus | 9 | Creation, push/pop, order, overflow, IDs, dispatch, filters |
| Driver State | 21 | 7-state pipeline, transition matrix, certification |
| Boot Loader | 8 | Scan, load, init, activate, categories |
| PS/2 Kbd Ref | 10 | Reference PS/2 keyboard driver |
| Framebuffer Ref | 8 | Reference framebuffer driver |
| Storage Ref | 14 | Reference storage driver |
| ELF | 7 | ELF64 loader |
| Pipe | 13 | IPC pipes |
| Mmap | 6 | Memory mapping |
| FSCK | 6 | Filesystem integrity |

Tests run via the shell `test` command, which after passing kernel tests executes user-mode binaries (`SYSTEST.BIN`, `FILETEST.BIN`, `ALLTEST.BIN`).

---

### 14. Architecture Rules

- Drivers **never** touch hardware directly. All access via `device_read/write` or HAL ABI.
- Drivers **never** execute in IRQ context. Events are queued and dispatched from the scheduler.
- The certification pipeline is **strict**: states cannot be skipped.
- A driver is **ACTIVE** only after Loaded → Initialized → Registered → Bound → Active.
- HAL is the lowest layer. The kernel depends on HAL, never the reverse.
- `without_interrupts()` is used for critical sections that cannot be interrupted.

## Kernel Subsystems (High-Level)
- **arch/x64**: GDT, IDT, PIC, paging (4-level, 2 MB huge pages + 4 KB demand-paging), interrupt handlers (timer IRQ0, keyboard IRQ1, syscall INT 0x80)
- **drivers**: ATA (PIO + bus-master DMA + AHCI fallback), AHCI, PS/2 keyboard, USB HID, PCI scanner, device event infrastructure
- **buffer**: block cache (periodic flush via timer)
- **fs**: **VFS layer** (`fs/vfs.rs`) — `Vfs` struct with 26 drive slots (A-Z), `FileSystem` trait (`read`/`write`/`lookup`/`readdir`/`mkdir`/`create`/`stat`/`remove_file`/`remove_dir`/`rename`), `VfsNode { inode, mode, size }`, path resolution with `walk_components`, mount point support. Implementations: `NeoDosFs` (native format, mounted on C:), `Fat32Driver` (ESP, mounted on A:)
- **memory**: frame allocator (bitmap, 4 GiB max), external heap allocator (`linked_list_allocator` 16 MB @ 0x1000000), user heap demand-paging (0x10000000..0x12000000, 32 MB, 16 × 2 MB slots → 4 KB PTs)
- **process**: `Process` struct with PID, state, registers, `user_slot`, `cwd_drive`/`cwd_path`, `heap_base`/`heap_break`, `waiting_for`, `kernel_stack` (private `Option<Box<AlignedKStack>>`), `fd_table`, `mmap_regions`
- **scheduler**: round-robin (`schedule()`), timer-driven (`on_timer_tick` every 100 ticks ≈ 5.5 Hz), max 16 processes, idle process (PID 0) always present. `recycle_terminated(pid)` removes a process from the table, dropping its kernel stack and freeing the slot. `cleanup_terminated_process(pid)` is the public wrapper called from `cmd_run` (sys_exit path) and `sys_waitpid`.
- **usermode**: Ring 3 execution via `execute_usermode_asm` (IRETQ), process lifecycle in `spawn_usermode`/`wait_for_process`/`sys_exit` → `exit_to_kernel`. On exit: external resources freed in `syscall_dispatch`, then `cmd_run` calls `cleanup_terminated_process(pid)` to recycle the slot and free the kernel stack. The `KILL` command calls `kill_pid()` which does complete cleanup including heap, mmap, pipes, user slot, and kernel stack, then recycles the slot immediately.
- **shell**: DOS-like shell with 29+ built-in commands, TAB autocomplete, environment variables

## Kernel Safety and Synchronization (v0.10.4+)
The kernel architecture prioritizes memory safety and reentrancy:
- **Global State**: Managed via `spin::Mutex<Option<T>>` or `spin::Mutex<T>`. Access helpers: `with_vfs(f)`, `with_ata(f)`, `with_cache(f)` in `globals.rs`.
- **Atomic State**: `RAM_DISK_BASE`/`RAM_DISK_SIZE` (AtomicU64), `TIMER_TICKS` (AtomicU64), `NEED_CACHE_FLUSH` (AtomicBool), console cursor positions.
- **Periodic cache flush**: Timer tick handler sets `NEED_CACHE_FLUSH` every 180 ticks; flushed in `clear_need_resched()` before syscall returns.
- **Reentrancy**: This model prevents data races and undefined behavior when interrupts (like the timer) occur during syscall execution.
- **Input Buffer**: Implements a lock-free Single-Producer/Single-Consumer ring buffer (1024 bytes) using atomic head/tail indices.

## Syscall Table (INT 0x80)

Calling convention: RAX = syscall number, RBX = arg0, RCX = arg1, RDX = arg2. Return in RAX.

| # | Syscall | Args | Description |
|---|---------|------|-------------|
| 0 | sys_exit | RBX=code | Terminate process |
| 1 | sys_write | RBX=ptr, RCX=len | Write to console |
| 2 | sys_yield | — | Yield CPU |
| 3 | sys_getpid | — | Return current PID |
| 4 | sys_read | RBX=fd, RCX=buf, RDX=count | Read from stdin |
| 9 | sys_waitpid | RBX=pid | Wait for child process |
| 10 | sys_open | RBX=path_ptr, RCX=flags | Open file → inode |
| 11 | sys_readfile | RBX=inode, RCX=buf, RDX=count | Read from file |
| 12 | sys_writefile | RBX=inode, RCX=buf, RDX=count | Write to file |
| 13 | sys_close | RBX=fd | Close (no-op) |
| 14 | sys_ioctl | RBX=device_id, RCX=cmd, RDX=buf | Device I/O control |
| 15 | sys_register_device | RBX=device_id | Register as device handler |
| 16 | sys_chdir | RBX=path_ptr | Change working directory |
| 17 | sys_getcwd | RBX=buf, RCX=len | Get working directory path |
| 18 | sys_brk | RBX=addr | Set program break (demand-paged) |
| 19 | sys_mmap | RBX=size | Allocate zero-filled memory |

## Debug Interfaces

The provided script `scripts/qemu-debug.sh` runs QEMU with:

- Serial output to stdout (saved to `neodos/qemu_output.log`)
- QEMU monitor on `telnet 127.0.0.1:4444`
- GDB server on `tcp::1234`

See `docs/DEBUG.md` for a walkthrough.

