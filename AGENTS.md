# NeoDOS — AGENTS.md
## Versión Actual

v0.48.1 (VFS-1.3 stale namespace cleanup + VFS-1.4 HandleTable→Ob consistency, 588 tests)

## Architecture Governance

See `docs/ARCHITECTURE_SOURCE_OF_TRUTH.md` — all architectural invariants are
enforceable rules, not suggestions. See `docs/OBJECT_MANAGER_ARCHITECTURE.md`
for the Object Manager (Ob) evolution plan — handles, URN, and security
unification.

**New audit docs:**
- `docs/NEOFS_AUDIT.md` — complete NeoFS + driver/namespace audit (2026-06-28)
- `docs/NEOFS_ROADMAP.md` — NeoFS vNext phased roadmap (stability → performance)
- `docs/NEOFS_TESTS.md` — 26 proposed tests for NeoFS, namespace, and drivers
- `docs/NETWORK_USERLAND_ARCHITECTURE.md` — userland networking (net.nxl), Registry config model, NeoPkg design (2026-07-01)

**Next priorities after v0.48:**
1. ~~**VFS-1.1** Unificar MountManager (dual mount sync)~~ **COMPLETADO**
2. ~~**VFS-1.2** Arreglar ownership ObOpen → VFS~~ **COMPLETADO**
3. ~~**VFS-1.3** Eliminar stale namespace entries~~ **COMPLETADO**
4. ~~**VFS-1.4** HandleTable → ObObject consistency~~ **COMPLETADO**
5. **VFS-2.1** Privatizar métodos de NeoFS (encapsulation)
6. **VFS-4.1** Device IDs estables (no índice numérico)
7. Namespace ownership tracking (NS-1)
8. Dynamic inode allocator (FS-1)
9. Dynamic block bitmap (FS-2)
10. Eliminar hardcoded sector offsets (FS-4)
11. **NET-1** Implementar transmit path socket→NIC (Fase 1 red userland)

Run `python3 scripts/auto_test.py` and `scripts/check_deps.py` before any commit.

**IMPORTANTE:** Antes de implementar cualquier cambio, leer
[ARCHITECTURAL_VISION.md](docs/ARCHITECTURAL_VISION.md). Este documento define
la estrategia a largo plazo (v0.40 → v1.0) y las decisiones de no-cambio. Las
prioridades actuales son:

1. ~~**v0.40**: Buddy bitmap >4GB, user window 4MB→32MB, static buffers→heap~~ **COMPLETADO**
2. ~~**v0.41**: ObObjectTable (refactor KOBJ → Object Manager), HandleEntry object_id field~~ **COMPLETADO**
3. ~~**v0.42**: Unified Wait Engine (KWait), congelar interfaces ABI~~ **COMPLETADO**
4. ~~**v0.43**: SeAccessCheck NT-compatible, sys_poll()~~ **COMPLETADO**
5. ~~**v0.44–v0.45**: ASLR v1~~ ~~**v0.44–v0.45 COMPLETADO**~~
   - ~~**OB-010 (ObOpen RAX 60), OB-011 (ObCreate RAX 61), OB-012 (ObQueryInfo RAX 62), OB-013 (ObSetInfo RAX 63), OB-014 (ObEnum RAX 64)**~~ COMPLETADO en v0.44.1
   - ~~**OB-015 (legacy paths via Ob), OB-016 (pipe via Ob), OB-017 (readfile/writefile via Ob), OB-018 (URN todos schemes via Ob)**~~ COMPLETADO en v0.44.2
   - ~~**OB-021 (ps.nxe), OB-022 (kill.nxe), OB-023 (pri.nxe)**~~ COMPLETADO en v0.44.1
6. ~~**v0.50**: **ObWait (RAX 65) + KWait integration**, URN rewrite como frontend de Ob~~
   - ~~**OB-020 (ObWait multi-type), OB-025 (URN frontend), OB-031 (KWait full integration), OB-046 (processes as ObObjects)**~~ COMPLETADO en v0.44.2
   - **libneodos**: wrappers Ob (ob_open, ob_create, ob_query_info, ob_set_info, ob_enum, ob_wait, ob_destroy), ObBasicInfo/ObEnumEntry/ObProcessInfo/ObPipeInfo structs, AbiTable v7 (legacy dead entries removed, ABI_VERSION 7)
7. ~~**v1.0**: **Security integration in ObOpen**, **Full Ob API stable**~~
   - ~~**OB-030 (SeAccessCheck en todas las rutas)**~~ COMPLETADO en v0.44.2
   - **OB-032 (Documentación de API)** 🔶 PARCIAL
8. **v0.44.4**: **Fix 3 bugs SMP-unsafe** (WAIT_PID, ISOLATED_REGIONS, NXL_REGISTRY)
9. **v0.44.5**: **Actualizar documentación** (README, ARCHITECTURE_SOURCE_OF_TRUTH)
10. ~~**v0.44.6**: **Completar libneodos wrappers** (thread_create/join, sleep_ex, poll, ob_destroy, driver_unload)~~ **COMPLETADO** — `ob_thread_create/join`, `sys_ob_destroy`, `sys_driver_unload` ya existían; añadidos `sys_poll` (RAX 59) y `sys_sleep_ex` (RAX 41) con `PollFd` struct
11. **v0.44.7**: **Arreglos arquitectónicos menores + Fase 1 Objectification** (ObType::Thread, InfoClass enums completos, ObError/SyscallError unificado, exports duplicados, TOCTOU race)
12. ~~**v0.46**: **Fase 2 Objectification** (Timer Object, Semaphore Object, Section Object, Device Tree)~~ **COMPLETADO**
13. ~~**VFS-1.3** Eliminar stale namespace entries~~ **COMPLETADO** en v0.48.1
14. ~~**VFS-1.4** HandleTable→ObObject consistency~~ **COMPLETADO** en v0.48.1
    - ~~**OBF-10 (Timer Object)**~~ COMPLETADO — ob_create(Timer), ob_set_info(TimerStart/TimerCancel), ob_wait(Timer)
    - ~~**OBF-11 (Semaphore Object)**~~ COMPLETADO — ob_create(Semaphore), ob_set_info(SemaphoreRelease), ob_wait(Semaphore)
    - ~~**OBF-12 (Section Object)**~~ COMPLETADO — ob_create(Section), ob_set_info(SectionMapView/SectionUnmapView)
    - ~~**OBF-05 (legacy kobj module removal)**~~ COMPLETADO — `kobj/` eliminado, namespace movido a `object/namespace.rs`, todos los callers migrados a `object::ob_*`
    - **OBF-13 (Registry Key)** ~~COMPLETADO~~ en v0.48.0 — B2.1 Z6 cell-based hive, \Registry\Machine namespace, Cm syscalls RAX 67-76
    - **OBF-13 (Device Tree)** 🔶 PENDIENTE
13. **v0.46.1**: **Bugfixes estabilidad OB-046** — lifecycle de procesos via Ob (handler_exit recicla EPROCESS, handler_ob_wait check-and-block atomico, handler_close decrementa pipe refs, threads registrados como ObType::Thread). **COMPLETADO**
    - ~~**Stress test 300 comandos**~~ (`scripts/stress_300.py`) — PASS 300/300 sin crash
14. **v0.46.2**: **A5.3 AHCI NCQ** — Native Command Queuing AHCI (IrpTagMap, FPDMA batch, 32 tags, detect+fallback). **COMPLETADO**
15. ~~**v0.47**: Networking (TCP/IP stack)~~ **COMPLETADO**
    - ~~**B3.1 D9 (Network I/O): \Device\Tcp, \Device\Udp, Socket ObType**~~ COMPLETADO en v0.47
    - ~~**B3.2 E3 (TCP/IP stack): Ethernet, ARP, IPv4, ICMP, UDP, TCP, e1000 NIC**~~ COMPLETADO en v0.47

**Regla de oro:** No añadir features nuevas antes de completar la fase de
maduración (v0.40–v0.45). Cada feature nueva se apoya en abstracciones
existentes; si esas abstracciones son frágiles, la feature será frágil.

**Regla de oro 2 (auditoría v0.44.3):** Los bugs SMP-unsafe (WAIT_PID,
ISOLATED_REGIONS, NXL_REGISTRY) tienen prioridad CRÍTICA sobre cualquier
otra tarea. Estos 3 bugs deben corregirse antes de continuar con la Fase 2.

**Objectification Roadmap:** Ver `docs/IMPROVEMENTS.md` sección **"Objectification
Roadmap"** para el plan completo de migración de syscalls a Objects del Object
Manager (Ob): Fase 1 (v0.44.7: Thread Object), Fase 2 (v0.46: Timer/Semaphore/
Section Objects), Fase 3 (v0.47+: Token/Network/Registry Objects).

## Build & Run

All commands from `neodos/`. Dependencies: `rustup`, `qemu-system-x86`, `ovmf`, `gdb`, `mtools`, `dosfstools`, `util-linux` (sfdisk).

```bash
bash scripts/build.sh                  # bootloader + kernel + GPT disk image
bash scripts/build.sh --neodos-image   # + NeoDOS FS image + user binaries
bash scripts/qemu-debug.sh             # QEMU + OVMF, serial to stdout, GDB :1234
gdb -x .gdbinit                         # from neodos/, connects to QEMU
python3 scripts/auto_test.py            # Automated headless test runner
```

QEMU accelerator via `QEMU_ACCEL` env var (default: TCG):
```bash
QEMU_ACCEL=kvm bash scripts/qemu-debug.sh
QEMU_ACCEL=kvm python3 scripts/auto_test.py
```

**Note**: Default machine is now `-machine q35` (PCIe with ECAM). Q35 + TCG
works reliably. If KVM produces `KVM: entry failed, hardware error 0x0` with
OVMF, override with `-machine pc` (PIIX3, legacy PIO).

## Git workflow (testing primero)

**IMPORTANTE: nunca subir código sin testear antes.**

1. `cargo build` en `neodos-kernel/` — comprueba que compila
  2. `python3 scripts/auto_test.py` — 528 kernel tests (auto-run at boot) + user-mode binaries
3. Solo si todo pasa: `git commit && git push`

**Antes de decidir sobre arquitectura:** consultar primero
`docs/ARCHITECTURE_SOURCE_OF_TRUTH.md`. Si una regla está definida allí,
el código debe cumplirla — no al revés.

**Cada vez que se complete una tarea:**
- Actualizar `docs/IMPROVEMENTS.md` (mover item a Completado con descripción)
- Actualizar `AGENTS.md` si es necesario (nuevas secciones, tablas de syscalls, comandos, etc.)
- Actualizar `docs/ARCHITECTURE.md`, `docs/KERNEL.md` u otros doc si la feature afecta al diseño
- Si se añade una syscall nueva: actualizar tabla de syscalls en `AGENTS.md` y `src/syscall/mod.rs`
- Si se añade un comando del shell: actualizar `AGENTS.md` en la sección de comandos
- `git add -A && git commit -m "feat: ..." && git push`

## Three packages, no workspace

- `neodos-bootloader/` — UEFI app, target `x86_64-unknown-uefi`, produces `bootloader.efi`
- `neodos-kernel/` — freestanding kernel, target `x86_64-unknown-none`, produces `kernel.elf`
- `libneodos/` — no_std user-mode library, target `x86_64-unknown-none`, syscall wrappers, IO, FS, mem, macros

Each has its own `Cargo.toml`, `Cargo.lock`, `.gitignore`. No root workspace.

## libneodos — User-mode Standard Library

`libneodos/` is a `no_std` library for Ring 3 user-mode processes written in Rust.

| Module | File | Contents |
|--------|------|----------|
| Syscall | `src/syscall/mod.rs` | SSDT dispatch table (256-slot `lazy_static!`), permission table, 32 handlers. `table.rs` = Registers/SyscallFn types. `permission.rs` = SyscallPermission/CAP_ADMIN. All return `Result<T, i64>` |
| IO | `src/io.rs` | `Stdout`/`Stdin`/`Stderr` structs with `write()`/`read().` `core::fmt::Write` impls. Stack-buffered `_print()`/`_eprint()` (1024 bytes) |
| FS | `src/fs.rs` | `File::open(path)` → handle, `File::read(buf)`, `File::write(buf)` |
| Mem | `src/mem.rs` | `brk()`, `sbrk()`, `mmap()`, `munmap()`. Constants: `PROT_READ`, `PROT_WRITE`, `MAP_ANONYMOUS` |
| Console | `src/console.rs` | `read_byte()`, `history_add_raw()`, `history_prev/next/reset/get_count/get_entry()`, `register_completion()`, `progress_*()`. Lazy-loaded via `sys_loadlib` on first use |
| Macros | `src/macros.rs` | `print!`, `println!`, `eprint!`, `eprintln!` with CRLF (`\r\n`) |

### Using libneodos

A user-mode binary project needs:
1. Depend on `libneodos = { path = "../libneodos" }` in `Cargo.toml`
2. Target `x86_64-unknown-none` with `relocation-model=static`, `link-arg=-Tuser.ld`
3. A `user.ld` linker script placing code at `0x400000`
4. `#![no_std]` + `#![no_main]` + `#[no_mangle] pub extern "C" fn _start() -> !`

See `userbin/hello_lib/` for a complete working example. Compile:
```bash
cd userbin/hello_lib
cargo build --release
```

The resulting ELF binary can be loaded by the kernel's `RUN` command.

## Kernel quirks

- **Nightly** pinned in `rust-toolchain.toml` (needs `abi_x86_interrupt`).
- **Custom linker** via `kernel.ld` + `.cargo/config.toml`: `-Tkernel.ld`, `-melf_x86_64`, `-no-pie`, `relocation-model=static`, `rust-lld`.
- **Entry**: `_start` in `.text.entry` at `0x200000`, called `extern "sysv64" fn(&BootInfo) -> !` (RDI = `&BootInfo`).
- **Heap**: 16 MB @ `0x1000000`, uses kernel slab allocator (9 size classes 8B–2KB) with `linked_list_allocator` fallback for large objects. `Box`, `Vec`, `String` disponibles.
- **Profiles**: release with `opt-level=3`, `lto=true`, `debug=true`, `panic="abort"`.
- A shared `.cargo/config.toml` at `neodos/` adds extra linker flags (`-melf_x86_64`, `rust-lld`) for the kernel target only.
- **Timers**: HPET (High Precision Event Timer) detected via ACPI RSDP/RSDT table scan. Configured at 1 KHz periodic mode with legacy replacement routing to IRQ0. Local APIC timer calibrated against HPET, used as primary timer source when available. APIC timer disables HPET legacy replacement and masks PIC IRQ0 to prevent double interrupts. Fallback to PIT (8254) at 18.2 Hz when HPET not available. `sleep_hint()` uses HPET counter for µs-resolution delays.
- **SMP**: Boot via INIT-SIPI-SIPI from BSP. Per-CPU KPRCB (4 KB page, `#[repr(C, align(4096))]`) accessed via GS segment base (`MSR IA32_GS_BASE`). Up to 16 CPUs. AP trampoline at physical 0x800000 for real→long mode transition. IPI via Local APIC ICR: reschedule (vector 0xF0), TLB shootdown (vector 0xF1). Per-CPU run queues (`CpuRunQueue`), per-CPU slab caches (`PerCpuSlabCache[9]`), per-CPU `need_resched` flag (GS:0x015), per-CPU exit trampoline (exit_rsp/exit_rip via GS). Compile-time `offset_of!` assertions enforce KPRCB layout correctness.

## Memory Architecture (A0)

### A0.1 Buddy System Frame Allocator

`src/memory/buddy.rs` — Reemplaza el bitmap O(n) con buddy system O(log n).

| Concept | Description |
|---------|-------------|
| **Orders** | 11 power-of-2 levels: 0=4KB, 1=8KB, … 10=4MB |
| **Free lists** | Fixed-size arrays (512 slots per order), no writes to freed frames |
| **Allocation** | `alloc_frames(order)` — finds smallest free block ≥ requested order, splits |
| **Free** | `free_frames(addr, order)` — frees block, merges with buddy if free |
| **Bitmap** | 16384 u64 words for O(1) buddy-status check (within 4 GB) |
| **Max RAM** | No hard limit; detects from UEFI memory map |
| **API** | `allocate_frame()`/`free_frame(phys)` wrappers for order 0 |

### A0.2 Dynamic Physical Memory

`src/memory/mod.rs` — `MemoryMap { total_phys, highest_page }` parsed from `BootInfo` UEFI memory map entries.

- `PHYS_MEM_END` is dynamic: detected from actual UEFI memory descriptors
- Frame allocator initialized with all free regions after reserving low 1 MiB, kernel image, and framebuffer
- Supports >4 GB physical RAM natively (frame allocator hands out any physical address)

### A0.3 Dynamic Memory Layout Manager

`src/memory/layout.rs` — Centralized region registry replacing hardcoded constants.

| Feature | Description |
|---------|-------------|
| `MemoryRegion` | base, size, 24-byte name, flags |
| `MemoryLayout` | 32-slot fixed-size array + `reserve_region()` |
| `init_default()` | Replicates legacy layout: kernel_image, user_window, kernel_heap, user_heap, nxl_region, mmap_region, driver_iso |
| Overlap detection | `panic!()` on any overlapping reservation |

Boot-time `validate_layout_consistency()` asserts layout-registered bases match hardcoded constants.

## Boot ABI

Bootloader loads ELF segments manually, calls `ExitBootServices` (memory map leaked via `forget`), jumps to kernel. `BootInfo` has: framebuffer info, raw memory map pointer/metadata, and ACPI RSDP address (`acpi_rsdp_addr`).

## Code generation

`neodos/drivers/ps2kbd/build.rs` parses `KBDUS.klc`/`KBDSP.klc` (UTF-16LE keyboard layouts) at build time into `$OUT_DIR/kbd_layout.rs` with scan code → ASCII tables. Copied to `neodos-kernel/src/drivers/nem/drivers/kbd_layout.rs` for reference. Two layouts: US (index 0), SP (index 1, default). Layout switching at runtime via Event Bus (`EVENT_KEYB_LAYOUT` type 9) sent from the `KEYB US|SP` shell command to the NEM ps2kbd driver.

## Input system (A4.4)

Solo **PS/2** (IRQ1). `src/input/` directory con 3 módulos (`mod.rs`, `vt.rs`, `manager.rs`). `InputManager` con 4 VT queues (`VtInputQueue`, ring buffer de 4 KB por VT). Per-VT input routing: PS/2 IRQ1 handler inserta bytes en `vt_queues[active_vt]`. Productor = IRQ1, consumidor = proceso foreground del VT activo via `handler_read`.

### Virtual Terminal Switching (B4.5)

| Atajo | Acción |
|-------|--------|
| **Alt+F1** | Cambia a VT 0 (NeoShell principal) |
| **Alt+F2** | Cambia a VT 1 (sesión secundaria) |
| **Alt+F3** | Cambia a VT 2 |
| **Alt+F4** | Cambia a VT 3 |

Detectado en `arch/x64/idt.rs` PS/2 handler: scan code de Alt+F1-F4 → `input::switch_vt(vt)`. Cada VT mantiene su propio `VtShadowBuffer` (caracteres en pantalla) y `ConsoleState` (cursor, color, scroll). Al cambiar de VT se restaura el framebuffer desde la shadow buffer del VT destino. El banner de NeoShell muestra `[VTn]` indicando el terminal activo.

## Adding a shell command

1. Create `src/shell/commands/<name>.rs` with an `impl DosShell` method.
2. Add `mod <name>;` to `src/shell/commands/mod.rs`.
3. Add a `CommandEntry` to `handler::COMMANDS` in `handler.rs`. Help text is automatic.

## AHCI Driver

### BootAhci (built-in kernel stub, Phase 3)
- `drivers/boot_ahci.rs` — DMA polling, single port, single command slot
- 8-sector PRDT (4 KB max per DMA), static buffers `PORT_CMD_LIST[]`, `PORT_RECV_FIS[]`, `PORT_CMD_TABLE[]`, `PORT_DMA_BUF[]`
- Priority in `storage_manager.rs`: NVMe > BootAhci > BootAta PIO
- Required for Q35 machine (no PATA controller)

### NEM AHCI (standalone, SYSTEM category)
- `drivers/ahci/` — NEM v3 standalone driver, loaded at Phase 3.85
- Scans PCI for AHCI controllers (class 0x01 subclass 0x06)
- Initializes HBA, detects ATA/ATAPI per port
- Registers block devices via `hst_register_block_device()`
- Per-port buffers separated by logical port index
- **ATA**: READ/WRITE DMA EXT (0x25/0x35), multi-sector hasta 8 sectores (4KB)
- **ATAPI**: PACKET command (0xA0) con DMA, READ_10 CDB, sectores de 2048 bytes
- **Port reset**: ciclo DET vía SCTL para recuperación de errores
- **PRDT**: hasta 8 entradas scatter-gather

## Directory Structure (NeoDOS FS)

The NeoDOS filesystem uses the following directory layout:

```
/
├─ System/
│   ├─ Kernel/
│   │   ├─ boot.cfg      # Boot configuration
│   │   └─ neodos.krn    # Kernel image (reference)
│   │
│   ├─ Drivers/
│   │   ├─ keyboard.nem  # PS/2 keyboard driver
│   │   ├─ serial.nem    # Serial port driver
│   │   ├─ rtc.nem       # RTC driver
│   │   ├─ acpi.nem      # ACPI driver
│   │   ├─ pci.nem       # PCI enumerator
│   │   ├─ disk.nem      # ATA disk driver
│   │   └─ ahci.nem      # AHCI disk driver
│   │
│   ├─ Libraries/
│   │   ├─ fs.nxl        # Filesystem library (libneodos)
│   │   ├─ io.nxl        # I/O library (libneodos)
│   │   ├─ process.nxl   # Process library (libneodos)
│   │   ├─ math.nxl      # Math library (libmath)
│   │   └─ console.nxl   # Console library (readline, history, completion, progress)
│   │
│   ├─ Layouts/
│   │   ├─ es-ES.nkb     # Spanish keyboard layout
│   │   └─ en-US.nkb     # US English keyboard layout
│   │
│   └─ Config/
│       ├─ system.cfg    # System configuration
│       └─ input.cfg     # Input configuration
│
├─ Programs/
│   ├─ NeoShell.nxe      # Ring 3 shell
│   ├─ NeoInit.nxe       # PID 1 init process
│   ├─ cpuinfo.nxe       # CPU info tool
│   ├─ dir.nxe           # Directory listing
│   ├─ help.nxe          # Help system
│   ├─ cls.nxe           # Clear screen
│   ├─ copy.nxe          # Copy files
│   ├─ del.nxe           # Delete files
│   ├─ ren.nxe           # Rename files
│   ├─ md.nxe            # Create directories
│   ├─ rd.nxe            # Remove directories
│   ├─ cmdtest.nxe       # Command test suite
│   ├─ hello.nxe         # Hello world test
│   ├─ systest.nxe       # Syscall test
│   ├─ filetest.nxe      # File I/O test
│   ├─ alltest.nxe       # Comprehensive test
│   ├─ cputest.nxe       # CPU stress test
│   └─ test.nxe          # libmath self-test
│
├─ Packages/             # Package files (.NXP)
├─ Users/
│   ├─ Default/          # Default user profile
│   └─ Alejandro/        # User directories
├─ Temp/                 # Temporary files
├─ Data/                 # User data
└─ Logs/                 # System logs
```

## Un disco GPT unificado

El sistema usa una **sola imagen de disco con tabla GPT** que contiene dos particiones:

| Partición | Tipo | LBA | Contenido |
|-----------|------|-----|-----------|
| 1 | ESP (FAT32) | 2048–206847 | bootloader.efi + kernel.elf |
| 2 | NeoDOS FS | 206848–227327 | Sistema de archivos NeoDOS |

El kernel parsea la GPT al arrancar mediante `drivers/gpt.rs`, busca la partición de tipo
`EBD0A0A2-B9E5-4433-87C0-68B6B72699C7`, y ajusta `base_lba` en el driver de bloques para que
el FS vea el superbloque en LBA 0 relativo a la partición.

### BootAhci (kernel stub, Phase 3)

**Kernel boot stub** (`neodos-kernel/src/drivers/boot_ahci.rs`): `BootAhci` — AHCI DMA polling,
single port, single command slot, 8-sector PRDT. Used during early boot (PHASE 3.6–3.8 in
`main.rs`) for GPT parsing, NeoDOS superblock read, and block cache warmup before NEM drivers
are loaded.

Priority in `storage_manager.rs`: NVMe > BootAhci > BootAta (PIO fallback).

### ATA PIO boot stub (fallback)

**Kernel boot stub** (`neodos-kernel/src/drivers/ata.rs`): `BootAta` — PIO only, primary channel
only. Fallback when no NVMe or AHCI is found. Used for PIIX3/QEMU with PATA controller.

### NEM ATA (standalone, SYSTEM category)

**NEM v3 standalone driver** (`drivers/ata/` → `ata.nem`, SYSTEM category): Full-featured ATA
driver loaded at PHASE 3.85 by the boot loader. Scans PCI for IDE controller (bus-master capable),
initializes primary + secondary channels, supports DMA read/write (via PRDT) and PIO multi-sector
fallback. Each active channel registers a block device via `hst_register_block_device()` with the
kernel's `NemBlockDevice` registry. Up to 8 sectors per transfer, ~137 GB addressable.

The NEM driver replaces the legacy inline DMA + multi-sector ATA code. The kernel boot stub is
replaced once the NEM driver activates. The FAT32 driver reads from the master drive using
absolute LBAs (no `base_lba`).

## Kernel Slab Allocator (A3)

`src/slab.rs` — Per-CPU lookaside lists with global fallback for kernel object allocation.

| Concept | Description |
|---------|-------------|
| **Size classes** | 9 power-of-2 caches: 8, 16, 32, 64, 128, 256, 512, 1024, 2048 bytes |
| **Per-CPU hot cache** | 32-object free list in KPRCB (GS-segment), O(1) alloc/free without locks |
| **Slab pages** | 4 KB pages from `hal::alloc_page()` (physical frames) |
| **Page header** | 32-byte `#[repr(C, align(16))]` header at offset 0: magic "SLAB" (u32), slot_size (u16), capacity (u16), allocated (u16), free_head (u16), next pointer |
| **Free list** | Inline `u16` indices stored in each free slot — O(1) alloc and free |
| **Alignment** | Minimum 16 bytes per slot (from header alignment) |
| **Fallback** | `linked_list_allocator::LockedHeap` for objects >2048 bytes or alignment >16 |
| **Isolation** | Heap region (0x01000000..0x02000000) reserved in frame bitmap to prevent slab/heap overlap |
| **Global allocator** | `SlabAllocator` implements `GlobalAlloc`, set as `#[global_allocator]` in `allocator.rs` |
| **Locking** | Global `spin::Mutex` protects 9 global slab caches; per-CPU hot caches lock-free via GS |
| **Refill/drain** | `refill_from_global()` moves batch from global to local; `drain_to_global()` returns full local batch |
| **Tests** | 14 tests: per-size alloc/free, multi-page stress, mix sizes, large fallback, free-reuse, per-CPU alloc/free, refill/drain, scaling, dispatch-level, stress 100k |

## Demand Paging (heap 4 KB)

**Archivos:** `arch/x64/paging.rs` (split_2mb_page, walk_ptes_4k, heap_alloc_page, heap_free_page), `memory.rs` (allocate_frame, free_frame), `arch/x64/idt.rs` (page_fault_handler → handle_heap_page_fault)

El kernel identity-maps 4 GiB con páginas enormes de 2 MB. Para el heap de usuario (0x10000000..0x12000000) se **dividen** esas páginas enormes en Page Tables de 4 KB durante el arranque (`init_heap_demand_paging`).

- **Frame allocator** (`memory.rs`): bitmap de 1048576 frames (4 GiB / 4 KB), `allocate_frame()`/`free_frame()`
- **split_2mb_page()**: asigna un marco físico para una Page Table, rellena 512 entradas con mapeo identidad, actualiza el PD entry
- **walk_ptes_4k()**: recorre PML4 → PDPT → PD → PT para obtener el PTE de una dirección virtual
- **heap_alloc_page()**: asigna un marco físico vía `allocate_frame()` y lo mapea como USER_ACCESSIBLE en la PT
- **heap_free_page()**: libera el marco físico y marca el PTE como not-present
- **heap_free_range()**: libera todas las páginas del heap de un proceso al salir (`sys_exit`)
- **Page fault handler** (`idt.rs:page_fault_handler`): si es un fault de usuario en rango heap, llama a `handle_heap_page_fault()` que asigna una página bajo demanda

### Flujo de crecimiento del heap
```
sys_brk(new_break)           # proceso pide más heap
  → escribe a cada nueva página 4 KB
  → si la página no está mapeada → page fault
  → page_fault_handler → handle_heap_page_fault
  → heap_alloc_page → allocate_frame() + map USER_ACCESSIBLE
  → se re-ejecuta la instrucción (escritura ok)
```

### Flujo de destrucción
```
sys_exit
  → heap_free_range(heap_base, heap_base + PROCESS_HEAP_SIZE)
  → por cada página presente con phys != virt: free_frame() + set_unused()
  → mmap_free_range por cada mmap_region registrada
```

## mmap Lazy (anonymous + file-backed)

**Archivos:** `arch/x64/paging.rs` (mmap_alloc_page, mmap_free_range, handle_mmap_page_fault, load_file_mmap_page), `scheduler/mod.rs` (MmapRegion, VMA list per-EPROCESS), `syscall/mod.rs` (sys_mmap/sys_munmap dispatch), `arch/x64/idt.rs` (page_fault_handler → handle_mmap_page_fault)

Región dedicada: `0x20000000..0x22000000` (32 MB), dividida en páginas 4 KB durante el arranque (`init_mmap_demand_paging`).

- **MmapRegion**: base, len, prot (1=R, 2=W), flags (bit0=1 anonymous, 0=file-backed), drive, inode, file_size
- **sys_mmap (RAX=19)**: RBX=hint, RCX=len, RDX=prot, R8=flags, R9=file_handle — solo registra VMA, no aloca páginas
- **sys_munmap (RAX=20)**: RBX=addr, RCX=len — libera páginas y elimina VMA
- **Anonymous**: page fault → allocate_frame() + map USER_ACCESSIBLE
- **File-backed**: page fault → with_vfs → vfs.read() a frame identity-mapped → map USER_ACCESSIBLE
- **is_user_ptr_valid()**: extendido para cubrir regiones mmap
- **sys_exit**: libera todas las regiones mmap del EPROCESS (cuando el último thread termina)

## User-mode process lifecycle

`cmd_run` in `shell/commands/run.rs` loads a flat binary to `USER_BASE` (0x400000) and calls `execute_usermode()`.

`execute_usermode()` in `usermode.rs` saves the kernel RSP/RIP into `EXIT_RSP`/`EXIT_RIP` statics, then IRETQs to Ring 3. The function is **not** `options(noreturn)` — it can return.

On `sys_exit` (INT 0x80, RAX=0): `syscall_dispatch` marks the calling KTHREAD `Terminated`, decrements the EPROCESS `thread_count`. When the last thread exits, all EPROCESS resources (user slot, heap pages, mmap regions, pipe refcounts) are freed, and `cleanup_terminated_process(pid)` recycles the EPROCESS slot. The `syscall_handler_asm` trampoline detects RAX==0 and jumps to `exit_to_kernel`, restoring `EXIT_RSP`/`EXIT_RIP` plus all callee-saved registers — preventing user-mode register clobber from corrupting shell local variables. Control returns to `execute_usermode`'s caller (`cmd_run`). The `KILL` command (`kill_pid()`) does full cleanup (heap, mmap, pipes, user slot, kernel stack, all threads) and recycles the slot immediately. `sys_waitpid` recycles the waited-for EPROCESS slot after detecting all threads are `Terminated`.

Key files: `usermode.rs` (trampoline & context save/restore), `idt.rs` (syscall_handler_asm exit path), `syscall/mod.rs` (dispatch & Terminated marking, sys_thread_create/join), `scheduler/mod.rs` (EPROCESS/KTHREAD lifecycle, kill_pid).

## Shell: TAB autocomplete + history + pipeline

La shell Ring 3 (`userbin/neoshell/src/main.rs`) usa `console.nxl` para historial y
autocompletado:

- **TAB completion**: La shell registra `shell_complete()` como callback via
  `console::register_completion()`. console.nxl invoca el callback que busca en PATH
  comandos `.NXE` que matcheen el token actual.
- **Historial ↑/↓**: console.nxl mantiene un buffer circular de 32 entradas.
  La shell llama `console::history_add_raw()` al ejecutar un comando y
  `console::history_prev/next()` al presionar ↑/↓ (bytes sentinela 0x01/0x02).
- **Display**: La shell maneja directamente el eco de caracteres, cursor `_` y
  backspace con `sys_write` (no routing through console.nxl) — probado como fiable
  con serial QEMU.

La shell tiene soporte de **pipeline** (operador `|`):
- `cmd1 | cmd2 | cmd3` — hasta 16 comandos encadenados
- Crea pipes nativos via `sys_pipe`, redirige stdin/stdout con `sys_spawn`
- Built-ins no son pipeables (error explícito)
- Cierre limpio de fds en cada etapa del pipeline

## Shell: DEL, REN, RD

Comandos de gestión de archivos que operan via VFS (`vfs.rs`):

| Comando | Descripción | VFS method |
|---------|-------------|------------|
| `DEL file` | Elimina archivo (libera bloques, inodo, marca entry 0xE5) | `vfs.remove_file()` |
| `REN old new` | Renombra archivo en el mismo directorio | `vfs.rename()` |
| `RD dir` | Elimina directorio vacío | `vfs.remove_dir()` |

Métodos del trait `FileSystem`: `remove_file()`, `remove_dir()`, `rename()` — con default `NotImplemented`.

## A5.1 Unified Block I/O Layer (IoStack)

`src/vfs/io.rs`, `src/vfs/partition.rs` — Abstraction unificada para I/O de bloques con cache, manejo de particiones.

### IoStack Structure

```rust
struct IoStack {
    device_id: usize,              // Index into BLOCK_DEVICES
    partition: Option<PartitionInfo>,  // LBA base + size
    cache_level: PageCacheLevel,   // None / L1 (sector cache) / L2
}
```

### PartitionInfo

```rust
struct PartitionInfo {
    base_lba: u64,
    sector_count: u64,
    partition_type: [u8; 16],     // GPT partition type GUID
}
```

### Unified API

| Function | Description |
|----------|-------------|
| `iostack_read_sectors(lba, count, buf)` | 1. Translate LBA (+ partition base) → 2. Check cache → 3. Read from device → 4. (future) Decrypt |
| `iostack_write_sectors(lba, count, buf)` | Translate LBA + write to device |
| `read_sector(lba)` | Single-sector convenience wrapper |
| `write_sector(lba, data)` | Single-sector convenience wrapper |
| `with_device(f)` | Borrow the underlying BlockDevice for cache operations |

### Recent Changes

| File | Change |
|------|--------|
| `src/vfs/io.rs` | New: IoStack + iostack_read/write_sectors |
| `src/vfs/partition.rs` | New: PartitionInfo, GPT partition lookup by GUID |
| `src/vfs/mod.rs` | New: module declarations |
| `src/drivers/fat32.rs` | Refactored: uses IoStack instead of `base_lba` save/restore |
| `src/fs/neodos_fs.rs` | Refactored: uses `io_stack.device_id` instead of hardcoded 0 |
| `src/main.rs` | Updated: creates IoStacks from GPT, passes to filesystems |
| `src/drivers/gpt.rs` | Added: `PART_TYPE_ESP`, `find_all_esp_partitions()` |

### Criterio de aceptación

- FAT32 y NeoFS en mismo disco, ambos acceden vía `iostack_read_sectors()`
- Cache hit ratio idéntico para ambos (mejora transversal)
- Tests: 5 (ver tabla en Testing)

## Default File Permissions by Context

Al crear un archivo o directorio en NeoFS, se asignan permisos RWXSD según el tipo de archivo (extensión). Los permisos se almacenan en el campo `mode` del inodo y coexisten con `MODE_FILE`/`MODE_DIR`.

| Tipo | Extensiones | Permisos (RWXSD) | Uso |
|------|-------------|------------------|-----|
| Ejecutable | `.NXE`, `.COM`, `.EXE` | `R-X--` | Leer + ejecutar, protección contra modificación accidental |
| Driver | `.NEM` | `R----` | Solo lectura, archivos críticos del sistema |
| Librería | `.NXL` | `R-X--` | Cargar desde user-mode, no modificar |
| Script | `.BAT`, `.CMD` | `R-X--` | Leer + ejecutar por el shell |
| Sistema | `.SYS` | `R----` | Configuración crítica del sistema |
| Configuración | `.CFG`, `.INI` | `RW---` | Leer y modificar |
| Texto | `.TXT`, `.MD`, `.LOG`, `.ASC` | `RW---` | Edición normal |
| Otros | cualquier otra extensión | `RW---` | Por defecto |
| Directorios | — | `RWXD-` | Permisos completos sobre el directorio |

La asignación se realiza en `NeoDosFs::create_file_at()` (vía `default_perms_for_filename()`) y `create_directory_at()`. El script `create_neodos_image.py` aplica los mismos criterios al generar la imagen inicial del FS.

## Shell: FSCK (Ring 3)

Comando de verificación de integridad del sistema de archivos NeoDOS (implementado como `fsck.nxe`):

| Comando | Descripción |
|---------|-------------|
| `FSCK [drive:] [/F]` | Verifica integridad del FS vía `sys_fsck` (RAX=55). Sin /F: solo comprueba. Con /F: repara errores |

Checks: superblock (magic, block_size, num_blocks, label), inode table (mode, inode_num mismatch, block pointers, cross-links), directory tree walk (orphans, dangling entries, entry-type vs mode mismatches). 6 unit tests (kernel-side).

## Shell: LOADLIB

Comando para cargar shared libraries (DLLs) desde el filesystem:

| Comando | Descripción |
|---------|-------------|
| `LOADLIB <path>` | Carga un DLL en un slot libre de la región de DLLs |

El DLL se carga en la región `0x1e000000..0x1e200000` (8 slots de 256 KB cada uno). El kernel parsea el ELF, marca las páginas como USER_ACCESSIBLE (read-only), y devuelve la dirección base. La tabla de exportaciones del DLL queda accessible en la dirección base.

DLLs disponibles:
- `libneodos.nxl` — Librería estándar (slot 0, `0x1e000000`), cargada automáticamente en boot
- `libmath.nxl` — Librería de matemáticas (slot 1, `0x1e040000`), carga manual con `LOADLIB C:\System\Libraries\math.nxl`
- `console.nxl` — Librería de consola (slot 2, `0x1e080000`), cargada automáticamente por `libneodos::console` en primer uso

Para usar desde user-mode: llamar a `libneodos::loadlib(path)` que invoca `sys_loadlib` (RAX=21) y devuelve la dirección base del DLL.

## Syscall Table (INT 0x80)

### Architecture

- `SyscallNum` enum (`from_u64()`) — maps RAX values to typed dispatch arms
- `SyscallError` enum (16 codes) — returned as negative `u64` via `err_to_u64()` (e.g., `NoEnt=2` → `0xFFFF_FFFF_FFFF_FFFE`)
- `syserr!` macro — `syserr!(NoEnt)` expands to `return err_to_u64(SyscallError::NoEnt)`
- `validate_abi()` — called at boot from `main.rs`, asserts all syscall numbers have handlers and error encoding is correct
- Return convention: `≥ 0` success, `< 0` error (user checks `cmp rax, -1`)

Calling convention: RAX = syscall number, RBX = arg0, RCX = arg1, RDX = arg2, R8 = arg3, R9 = arg4. Return in RAX.

| RAX | Syscall | Args | Descripción | Estado |
|-----|---------|------|-------------|--------|
| 0 | `sys_exit` | RBX=code | Termina proceso | Activo |
| 1 | `sys_write` | RBX=fd, RCX=ptr, RDX=len | Escribe a fd (1=consola, pipe writer) | Activo |
| 2 | `sys_yield` | — | Cede CPU | Activo |
| 3 | `sys_getpid` | — | Retorna PID actual | Activo |
| 4 | `sys_read` | RBX=fd, RCX=buf, RDX=count | Lee de fd (0=stdin, pipe reader); bloquea con -EAGAIN | Activo |
| 5 | `sys_pipe` | RBX=fds_ptr | Crea pipe, escribe [read_fd, write_fd] en fds_ptr. Usar `ob_create(Pipe)` | Activo (Ob wrapper) |
| 6 | `sys_dup2` | RBX=old_fd, RCX=new_fd | Duplica old_fd a new_fd (redirección) | Activo |
| 7 | `sys_spawn` | RBX=path_ptr, RCX=stdin_fd, RDX=stdout_fd, R8=stderr_fd | Carga y ejecuta un binario ELF64 desde NeoFS. stdfd=0xFF hereda. Retorna PID del hijo | Activo |
| 8 | `sys_readdir` | RBX=fd, RCX=buf_ptr | Lee entrada de directorio del handle fd; retorna 1 si hay entrada, 0 si fin | Legacy |
| 9 | `sys_waitpid` | RBX=pid | Espera proceso hijo | Legacy |
| 10 | `sys_open` | RBX=path_ptr, RCX=flags | Abre archivo → fd (handle); directorios → HANDLE_DIR | Activo |
| 11 | `sys_readfile` | RBX=fd, RCX=buf, RDX=count | Lee desde archivo (usa offset del handle). Delega en Ob internamente | Activo |
| 13 | `sys_close` | RBX=fd | Cierra handle (pipe, file, device, event) | Activo |
| 16 | `sys_chdir` | RBX=path_ptr | Cambia directorio actual | Legacy |
| 18 | `sys_brk` | RBX=new_break | Ajusta program break (paginación bajo demanda) | Activo |
| 19 | `sys_mmap` | RBX=hint, RCX=len, RDX=prot, R8=flags, R9=fd | Mapeo lazy: anónimo (flags=1) o file-backed (flags=0, R9=fd) | Activo |
| 20 | `sys_munmap` | RBX=addr, RCX=len | Libera mapeo mmap | Activo |
| 21 | `sys_loadlib` | RBX=path_ptr | Carga un DLL desde NeoFS en un slot libre de la región de DLLs | Activo |
| 22 | `sys_thread_create` | RBX=entry, RCX=stack | Crea un nuevo thread en el EPROCESS actual; retorna TID | Activo |
| 23 | `sys_thread_join` | RBX=tid | Espera a que un thread termine | Activo |
| 29 | `sys_set_exception_handler` | RBX=handler_fn | Sets SEH handler for current thread (A3.4). handler_fn=0 clears chain | Activo |
| 40 | `sys_wait_alertable` | — | Alertable wait: si APC pendiente, despacha y retorna `APC_ALERTED` (1). Si no, bloquea | Activo |
| 41 | `sys_sleep_ex` | — | Yield alertable: cede CPU, chequea APCs | Activo |
| 42 | `sys_poweroff` | — | Apaga la máquina (QEMU debug port + ACPI S5 + PS/2 reset) | Activo |
| 47 | `sys_chdir_parent` | RBX=path_ptr | Cambia el directorio actual del proceso padre | Legacy |
| 53 | `sys_cursor_blink` | RBX=0 (disable), 1 (enable) | Enable/disable automatic cursor blinking from Ring 3 | Activo |
| 55 | `sys_fsck` | RBX=buf_ptr, RCX=drive_char, RDX=repair_flag | Run filesystem integrity check. Returns FsckStats | Activo |
| 58 | `sys_driver_unload` | RBX=name_ptr, RCX=force_flag | Unload a NEM driver by name (admin) | Activo |
| 59 | `sys_poll` | RBX=pfds_ptr, RCX=nfds, RDX=timeout_ms | Poll fds for ready I/O. Returns ready count | Activo |
| 60 | `sys_ob_open` | RBX=path_ptr, RCX=desired_access | Open an Ob namespace object by path. Security access check (SeAccessCheck), allocates handle. Returns fd (≥3) | Activo |
| 61 | `sys_ob_create` | RBX=path_ptr, RCX=type, RDX=fds_out, R8=attrs | Create Ob object. type=1 Process, 2 Driver, 4 Pipe, 11 Directory, 13 Event. Returns fd | Activo |
| 62 | `sys_ob_query_info` | RBX=fd, RCX=class, RDX=buf, R8=size | Query object info. class=15 ReadContent, 16 VolumeLabel | Activo |
| 63 | `sys_ob_set_info` | RBX=fd, RCX=class, RDX=buf, R8=size | Set object info. class=6 VfsRename, 7 WriteContent, 8 SetCwd, 9 SetVolumeLabel | Activo |
| 64 | `sys_ob_enum` | RBX=dir_fd, RCX=buf, RDX=max | Enumerate directory | Activo |
| 65 | `sys_ob_wait` | RBX=count, RCX=handles, RDX=type, R8=timeout | Wait on objects | Activo |
| 66 | `sys_ob_destroy` | RBX=fd | Destroy/delete object by fd | Activo |
| 67 | `sys_cm_open_key` | RBX=path_ptr | Open registry key by path → fd | Activo |
| 68 | `sys_cm_create_key` | RBX=fd, RCX=name_ptr | Create subkey under fd → new fd | Activo |
| 69 | `sys_cm_query_value` | RBX=fd, RCX=name_ptr, RDX=buf, R8=len | Query value by name → type+size+data | Activo |
| 70 | `sys_cm_set_value` | RBX=fd, RCX=name_ptr, RDX=type, R8=data_ptr, R9=len | Set value (SZ/DWORD/BINARY) | Activo |
| 71 | `sys_cm_enum_key` | RBX=fd, RCX=index, RDX=buf | Enumerate subkeys by index | Activo |
| 72 | `sys_cm_enum_value` | RBX=fd, RCX=index, RDX=buf | Enumerate values by index | Activo |
| 73 | `sys_cm_delete_key` | RBX=fd | Delete key and all subkeys | Activo |
| 74 | `sys_cm_flush_key` | RBX=fd | Flush hive to disk | Activo |
| 75 | `sys_cm_load_hive` | RBX=name_ptr, RCX=mount_ptr | Load hive from file (admin) | Activo |
| 76 | `sys_cm_unload_hive` | RBX=mount_ptr | Unload hive (admin) | Activo |

**Syscalls legacy eliminadas del SSDT (RAX 12, 25–28, 46, 48, 50–52, 54, 57):** migradas a la API Object Manager (Ob). Ver secuencia `ob_open → ob_query_info/ob_set_info/ob_create/ob_destroy`.

**Estado Objectification:**
| Estado | Syscalls |
|--------|----------|
| ✅ Ya Objects (Ob API RAX 60-66) | ob_open, ob_create, ob_query_info, ob_set_info, ob_enum, ob_wait, ob_destroy |
| 🔶 Fase 1 (v0.44.7) — Thread como Object | Thread create/wait/priority via Ob (RAX 61/65/63) |
| 🔶 Fase 2 (v0.46) — Timer/Semaphore/Section | Timer create/wait, Semaphore create/release/wait, Section create/map |
| ❌ Legacy removed (SSDT=None, usar Ob) | sys_pipe, sys_spawn, sys_open, sys_close, sys_readfile, sys_readdir, sys_waitpid, sys_chdir, sys_writefile, sys_mkdir, sys_unlink, sys_rmdir, sys_rename, sys_chdir_parent |
| ⬜ Foundation (no Object, mecanismos puros) | sys_exit, sys_write, sys_read, sys_yield, sys_getpid, sys_brk, sys_mmap, sys_munmap, sys_loadlib, sys_poweroff, sys_cursor_blink, sys_fsck, sys_poll, sys_set_exception_handler, sys_wait_alertable, sys_sleep_ex |
| 🆕 Futuras (RAX ≥ 77, DEBEN ser Ob) | sys_ob_logon, sys_ob_query_token, sys_ob_set_security, sys_ob_create_timer, sys_ob_create_section, sys_ob_socket, etc. |

Ver `docs/IMPROVEMENTS.md` sección **"Objectification Roadmap"** para plan detallado por fases.

**Regla de arquitectura:** Toda syscall nueva (RAX ≥ 77) DEBE implementarse como `sys_ob_*` — opera sobre objetos en el namespace Ob, recibe/entrega fds obtenidos via `ob_open`/`ob_create`, y se apoya en el Object Manager para lifecycle y seguridad. Excepción: las syscalls Cm (RAX 67-76, `sys_cm_*`) operan sobre objetos `ObType::Key` en el namespace `\Registry\` y siguen la misma convención Ob internamente, pero usan prefijo `cm_` por consistencia con el subsistema NT Configuration Manager. No se aceptan syscalls planas legacy para funcionalidad nueva. Ver secciones USR y **Objectification Roadmap** en `docs/IMPROVEMENTS.md`.

## IPC / Pipes

`src/pipe.rs` — Pipe IPC implementation for inter-process communication.

### Pipe Manager
- **16 static pipe buffers** of 4 KB each, protected by `spin::Mutex`
- Reference-counted: auto-freed when all reader/writer fds are closed
- `sys_pipe` allocates a pipe, returns two fds (reader + writer)
- `sys_close` on a pipe fd decrements refcount; pipe freed when refs reach 0
- `sys_dup2` copies an fd to another slot (increments refcount for pipe fds)

### Per-EPROCESS Handle Table
- `Eprocess.handle_table: HandleTable` — `Vec<HandleEntry>`-backed, grows dynamically, unlimited capacity
- `HandleEntry` types: `Closed`, `Stdin`, `Stdout`, `Stderr`, `PipeReader(id)`, `PipeWriter(id)`, `File(drive, inode, offset)`, `Device(id)`, `Event(type)`, `Dir(drive, inode)`
- File handles carry a per-open `offset` cursor for independent read/write positioning
- fd 0 = stdin (keyboard), fd 1 = stdout (console), fd 2 = stderr (console)
- fds 3+ available for pipes/files/devices/events (unlimited, grows dynamically)
- Default table for Ring 3 processes; `closed_handle_table()` for Ring 0
- `sys_exit` iterates handle table and cleans up all resource types (pipes decrement refcount, files closed cleanly)

### Blocking Reads
- When a thread reads from an empty pipe with write end open:
  1. Thread state set to `ThreadState::Blocked { waiting_for: 0xFFFF_0000 | pipe_id }`
  2. `NEED_RESCHED` flag set
  3. `syscall_dispatch` returns `-EAGAIN` to user space
  4. Assembly resched picks a different thread
- On pipe write: `wake_pipe_readers()` scans scheduler threads, sets Blocked→Ready
- When woken thread runs: user code retries `read()` syscall (handles -EAGAIN)

### Syscall Changes
| RAX | Syscall | Cambio |
|-----|---------|--------|
| 1 | `sys_write` | RBX=fd (antes RBX=ptr). Soporta fd 1 (stdout) y pipe writer fds |
| 4 | `sys_read` | Soporta fd 0 (stdin) y pipe reader fds |
| 5 | `sys_pipe` | Nuevo: crea pipe, devuelve [read_fd, write_fd] |
| 6 | `sys_dup2` | Nuevo: duplica fd (redirección) |
| 13 | `sys_close` | Ahora cierra pipe fds correctamente (decrementa refcount) |

### Scheduler Integration
- `syscall_try_resched` modified: only transitions `Running → Ready` (does not override `Blocked`)
- `wake_pipe_readers()` in `pipe.rs` iterates scheduler threads via `Scheduler::kthreads`
- `block_current_for_pipe()` sets current thread to `Blocked` + sets `NEED_RESCHED`

## Priority Scheduler (A2)

`src/scheduler/mod.rs` — Planificador prioritario con time-slicing dinámico y aging.

### Priority Levels
| Nivel | Constante | Time Slice | Descripción |
|-------|-----------|-----------|-------------|
| 0 | `PRIORITY_HIGH` | 400 ticks | Procesos críticos del sistema |
| 1 | `PRIORITY_ABOVE_NORMAL` | 200 ticks | Procesos importantes de usuario |
| 2 | `PRIORITY_NORMAL` | 100 ticks | Prioridad por defecto (nuevos procesos) |
| 3 | `PRIORITY_IDLE` | 50 ticks | Background, solo se ejecuta si no hay nada más |

### Algorithm
- **schedule()**: intenta cola local (CpuRunQueue) → work stealing → global fallback. Escanea por nivel de prioridad (HIGH→IDLE), round-robin dentro del mismo nivel
- **on_timer_tick()**: decrementa `time_slice_remaining` cada tick; al expirar marca Ready + `this_cpu_set_need_resched()`. Si cola local vacía, intenta work stealing de otro CPU.
- **sys_yield**: Running→Ready + resetea time slice + fuerza re-schedule
- **Preemption from Ring 3**: timer handler detecta CS=0x1B (user mode), guarda RSP, llama schedule(), cambia TSS.RSP0
- **Aging** (cada 100 ticks): boostea prioridad si un proceso Ready no se ha ejecutado en >= 1000 ticks
- **Work stealing**: `try_work_steal()` roba el thread más viejo de la cola IDLE de otro CPU cuando la cola local está vacía. Escaneo round-robin entre CPUs.

### Implementation
- `Kthread` struct: `priority` (u8), `time_slice_remaining` (u16), `ticks_since_scheduled` (u64), `tid`, `pid`, `teb_base`, `kernel_stack_top`, `cpu_ticks`, `cpu` (target CPU for local queue)
- `timer_handler_inner`: lee CS del stack frame, solo preemptea si interrumpió Ring 3
- Afecta solo threads user-mode (Ring 3); el shell Ring 0 es bootstrap heredado y no debe ejecutar comandos de operador ni pasar por schedule()
- 7 tests de scheduler: prioridad, round-robin, time-slice, aging

## ELF64 Loader (A4.3)

`src/elf.rs` — ELF64 loader with range validation for user-mode binaries.

### Core Loading
- Validates ELF magic (`\x7fELF`), class (64-bit), endianness (LSB), machine (x86-64), type (EXEC or DYN)
- Parses program headers; loads `PT_LOAD` segments at `p_vaddr + load_offset`
- Zero-fills `.bss` (`p_memsz - p_filesz`)
- Load offset: `load_offset: u64` parameter — used for ASLR v1; PIE binaries (ET_DYN) loaded at random slot base, all segment vaddrs shifted by load_offset
- RELA relocations: parses `.rela.dyn` section via `PT_DYNAMIC`, applies `R_X86_64_RELATIVE` at load time (3 entries in typical neoshell binary)
- Entry point returned via `ElfLoadResult { entry, segments }` (entry = `e_entry + load_offset`)
- Backward compatible: `cmd_run` detects ELF vs flat binary by checking the first 4 bytes

### A4.3 Address Space Validation
`load_elf(data, Option<&mut AddressSpace>)` performs 5 security checks per segment:

1. **Null vaddr** — `p_vaddr == 0` → `ELF_ERR_ZERO_VADDR (-2)`
2. **Protected regions** — collision with kernel_image (0x200000–0x400000), kernel_heap (0x1000000–0x2000000), user_heap (0x10000000–0x12000000), nxl_region, mmap_region, driver_isolation → specific error codes
3. **User window** — `p_vaddr < 0x400000 || p_vaddr + p_memsz > 0x800000` → `ELF_ERR_VADDR_OUT_OF_RANGE (-1)`
4. **Inter-segment overlap** — overlapping PT_LOAD segments → `ELF_ERR_SEGMENT_OVERLAP (-3)`
5. **Entry containment** — warning if entry not in any PT_LOAD (log only, not fatal)

`AddressSpace` (`src/scheduler/address_space.rs`) per-EPROCESS:
- `loaded_segments: Vec<SegmentInfo>` tracks loaded ranges
- `validate_segment()` — static range + region check
- `add_segment()` — validates + registers (overlap check against prior segments)
- `Eprocess.address_space` field for tracking per-process loaded segments

### API
```rust
pub fn load_elf(data: &[u8], addr_space: Option<&mut AddressSpace>, load_offset: u64) -> Result<ElfLoadResult, ElfLoadError>
pub struct ElfLoadResult { pub entry: u64, pub segments: Vec<SegmentInfo> }
pub enum ElfLoadError { InvalidHeader, InvalidMagic, ..., AddressSpaceViolation(i64) }
```

- `Some(&mut addr_space)` — full validation (used by `cmd_run` for user ELF loading)
- `None` — skip address space checks (used by NXL loader, tests)

### Tests
- 15 kernel tests registered in `testing.rs` via `register_elf_tests()`:
  - 7 original parser tests (header, magic, class, machine, truncated, load, bad_phentsize)
  - 8 A4.3 validation tests: `elf_validation_valid_range`, `elf_reject_zero_vaddr`, `elf_reject_kernel_collision`, `elf_reject_heap_collision`, `elf_reject_mmap_collision`, `elf_malicious_no_triple_fault`, `elf_overlap_segments`, `elf_user_heap_collision`

## User-mode binaries

Ubicados en `userbin/`. Generados por scripts Python (no requieren NASM).

| Binario | Generador | Tamaño | Prueba |
|---------|-----------|--------|--------|
| `cpuinfo.nxe` | Rust `userbin/cpuinfo/` | ~6 KB | CPU info via `ob_open("\Global\Info\CpuInfo")` + `ob_query_info(class=7)`: vendor, brand, features, topology, timers |
| `neoshell.nxe` | Rust `userbin/neoshell/` | ~27 KB | Ring 3 shell: built-in CWD, SET, POWEROFF, EXIT, CALL; TAB completion via `console.nxl`; PATH dispatch for external .NXE commands (CD, ECHO, DIR, HELP, MEM, VOL...); history via `console.nxl` (32); drive change; batch file execution via CALL |
| `cd.nxe` | Rust `userbin/cd/` | ~4 KB | Ring 3 cwd changer: writes result to ARGS_ADDR for parent neoshell to read; validates via ob_open; no shell integration required |
| `neoinit.nxe` | Rust `userbin/neoinit/` | ~8 KB | PID 1 init process: spawns NEOSHELL.NXE via ob_create(Process) + ob_wait, respawns on EXIT |
| `coredir.nxe` | Rust `userbin/coredir/` | ~11 KB | Standalone DIR command: `ob_open` (dir) + `ob_enum`, multi-column output, `/W` (wide), `/P` (pause) |
| `corehelp.nxe` | Rust `userbin/corehelp/` | ~14 KB | NT-style HELP: scans `C:\Programs\*.NXE` for embedded `::HELP::` markers, lists all commands with descriptions. `HELP <cmd>` spawns `<cmd>.NXE /?` via pipe and shows output |
| `datetime.nxe` | Rust `userbin/datetime/` | ~6 KB | ob_open("\Global\Info\DateTime") + ob_query_info: muestra fecha/hora RTC. Flags `/D` (date), `/T` (time) |
| `ver.nxe` | Rust `userbin/ver/` | ~5 KB | ob_open("\Global\Info\Version") + ob_query_info: muestra versión del kernel NeoDOS |
| `neomem.nxe` | Rust `userbin/neomem/` | ~6 KB | NeoMem v0.1: ob_open("\Global\Info\Memory") + ob_query_info: memoria física, kernel heap, user memory, paging. Reemplaza `mem.nxe` |
| `echo.nxe` | Rust `userbin/echo/` | ~4 KB | ECHO command: imprime texto via sys_write |
| `vol.nxe` | Rust `userbin/vol/` | ~5 KB | VOL command: ob_open("\Global\Info\VolumeLabel") + ob_query_info |
| `type.nxe` | Rust `userbin/coretype/` | ~6 KB | TYPE command: ob_open + ob_query_info(ReadContent) |
| `tree.nxe` | Rust `userbin/tree/` | ~7 KB | TREE command: muestra árbol de directorios con `├──`/`└──`. Recursivo hasta 6 niveles. Directorios primero, orden alfabético case-insensitive. Path opcional (default: CWD) |
| `cls.nxe` | Rust `userbin/corecls/` | ~4 KB | CLS command: limpia la pantalla via ANSI escape codes |
| `copy.nxe` | Rust `userbin/corecopy/` | ~8 KB | COPY command: ob_query_info(ReadContent) + ob_set_info(WriteContent) |
| `del.nxe` | Rust `userbin/coredel/` | ~4 KB | DEL command: ob_destroy |
| `ren.nxe` | Rust `userbin/coreren/` | ~4 KB | REN command: ob_set_info(VfsRename) |
| `md.nxe` | Rust `userbin/coremd/` | ~4 KB | MD command: ob_create(Directory) |
| `rd.nxe` | Rust `userbin/corerd/` | ~4 KB | RD command: ob_destroy |
| `drives.nxe` | Rust `userbin/drives/` | ~14 KB | Lists mounted drives via ob_open("\Global\Info\Drives") + ob_query_info |
| `ps.nxe` | Rust `userbin/ps/` | ~4 KB | PS command: ob_enum("\Ob\Process") + ob_query_info(Process) per process. Shows PID, PPID, priority, thread_count, state |
| `keyb.nxe` | Rust `userbin/keyb/` | ~4 KB | KEYB command: ob_set_info(KeyboardLayout=5) via \Global\Info\Keyboard object. US or SP |
| `kill.nxe` | Rust `userbin/kill/` | ~4 KB | KILL command: ob_open("\Ob\Process\{pid}") + ob_set_info(ProcessTerminate) |
| `pri.nxe` | Rust `userbin/pri/` | ~4 KB | PRI command: ob_open("\Ob\Process\{pid}") + ob_set_info(ProcessPriority). Levels 0-3 |
| `cmdtest.nxe` | Rust `userbin/cmdtest/` | ~14 KB | Comando de testeo automático via Ob API: ob_create(Directory), ob_destroy, ob_set_info(VfsRename), ob_query_info(ReadContent). Se lanza automáticamente tras los 528 tests de kernel |
| `label.nxe` | Rust `userbin/label/` | ~4 KB | LABEL command: ob_open + ob_query_info(VolumeLabel) + ob_set_info(SetVolumeLabel) |
| `fsck.nxe` | Rust `userbin/fsck/` | ~6 KB | FSCK command: filesystem integrity check via sys_fsck (RAX=55). /F for repair |
| `ndreg.nxe` | Rust `userbin/ndreg/` | ~7 KB | NDREG command: driver registry inspector via ob_open("\Global\Info\Drivers") + ob_query_info(Drivers) |
| `loadnem.nxe` | Rust `userbin/loadnem/` | ~4 KB | LOADNEM command: ob_create(Driver) to load, sys_driver_unload (RAX=58) for /U unload flag |
| `progress.nxe` | Rust `userbin/progress/` | ~4 KB | Progress bar demo: tests console.nxl progress_create/update/finish API |
| `neotop.nxe` | Rust `userbin/neotop/` | ~24 KB | System monitor: lista procesos con PID/PPID/prioridad/threads/estado + barra de memoria via console.nxl + `/W` watch mode |

**Regla operativa:** no se deben añadir nuevos comandos interactivos al shell Ring 0. Toda interacción de operador debe ir a `userbin/` y ejecutarse en Ring 3 vía `neoshell` o `NeoInit`.

User window (code+stack): `0x400000` .. `0x2400000` (32 MB, 32 slots de 128 KB).
Con ASLR v1 (v0.44): cada proceso carga su binario PIE (ET_DYN) en un slot
aleatorio. `alloc_user_slot()` usa RDRAND con fallback TSC.
User heap (demand-paged 4 KB): `0x10000000` .. `0x12000000` (32 MB, 16 slots de 2 MB)

## Async I/O (IRP System, X6)

`src/irp/mod.rs` — Unified I/O Request Packet model for all kernel block operations.

| Concept | Description |
|---------|-------------|
| **IRP struct** | `#[repr(C)]` with `IrpOp` (Read/Write/Flush/IoCtl), buffer ptr + len, LBA + count, `IrpStatus` (Pending/Completed/Error), callback + ctx, chain_next, waiting_pid |
| **Global pool** | 64 slots protected by `Spin::Mutex`, sequential IDs via `AtomicU32`. `irp_alloc()`/`irp_free()`/`irp_get_params()` — last returns a snapshot to avoid double-lock deadlock |
| **IrpQueue** | Per-device FIFO ring buffer (32 entries) for queuing async operations. `push()`, `pop()`, `peek()`, `len()` |
| **Completion** | `irp_complete(id, status)` — sets status, wakes waiter (`irp_wake_waiter` via `IRP_WAIT_MAGIC`), handles chaining, dispatches callback via `WORK_QUEUE.push_high()` using `Box<IrpCbDispatch>` |
| **Scheduler** | `irp_block_current(id)` sets `ThreadState::Blocked { waiting_for: IRP_WAIT_MAGIC \| id }`. `irp_complete` wakes via `irp_wake_waiter()` — same pattern as pipe blocking |
| **Chaining** | `chain_next: Option<IrpId>` — auto-cleared on complete. Device driver responsible for submitting chained IRPs |
| **Sync helpers** | `irp_sync_read()`/`irp_sync_write()` — allocate IRP, submit, block, free. For code that wants synchronous IRP path |
| **BlockDevice** | Trait extended with `submit_irp(irp_id)` and `poll_irp(irp_id)`. All 5 implementors (RamDisk, BootAta, AhciDriver, NvmeDriver, NemBlockDevice) implement `submit_irp` via `irp_get_params()` → sync I/O → `irp_complete_result()` |
| **Tests** | 11 tests: alloc/free, status transitions, error codes, unique IDs, slot reuse, queue FIFO/wraparound, callback dispatch via work queue, flush/ioctl ops, params extraction |

## NT5.5 Unified Resource Namespace (URN) — OB-025

`src/urn/mod.rs` — OB-025 rewrite: URN es un frontend completo del Ob (Object Manager).
Todos los schemes se resuelven mediante `ob_open_path()` en el namespace Ob.
`UrnHandle` es un wrapper sobre un kernel fd (handle table index).

### Supported Schemes

| Scheme | Mapping | Example |
|--------|---------|---------|
| `file` | `ob_open_path("\\Global\\FileSystem\\...")` → ObObject + fd | `neodos://file/C:/System/boot.cfg` |
| `device` | `ob_open_path("\\Device\\...")` → ObObject + fd | `neodos://device/Harddisk0/Partition1` |
| `registry` | `ob_open_path("\\Registry\\...")` → ObObject + fd | `neodos://registry/Machine/System` |
| `kobj` | `ob_open_path("\\Ob\\...")` → ObObject + fd | `neodos://kobj/Driver/ahci` |

### API

```rust
pub struct UrnHandle { pub fd: u8 }
impl UrnHandle { pub fn new(fd: u8) -> Self }

pub fn urn_parse(urn_str: &str) -> Result<Urn, &'static str>
pub fn urn_open(urn_str: &str) -> Result<UrnHandle, &'static str>
pub fn urn_read(handle: &mut UrnHandle, buf: &mut [u8]) -> Result<usize, &'static str>
pub fn urn_write(handle: &mut UrnHandle, buf: &[u8]) -> Result<usize, &'static str>
pub fn urn_seek(handle: &mut UrnHandle, pos: u64)
```

### Tests

19 tests: parse (4 scheme variants + 4 error cases), open error (2: file not found, device not found), roundtrip, OB-025 (5: registry/kobj namespace + file via Ob + UrnHandle), OB-018 ObObjectTable integration.

## ~~NT5.6 Virtual FS Objects (K:\ drive)~~ [REMOVED]

KDrive fue eliminado del código porque `\Global\Info\*` objects en el Object Manager
reemplazan su funcionalidad. Los comandos `neomem.nxe`, `datetime.nxe`, `ver.nxe`,
`drives.nxe`, `ndreg.nxe` y `ps.nxe` usan `ob_open` + `ob_query_info` directamente.

## Deferred Work Queue (X5)

`src/work_queue.rs` — Bottom-half system for deferred execution outside IRQ context.

### Two-Level Architecture

| Level | Processing | Use cases |
|-------|-----------|-----------|
| High-priority | Syscall return (`clear_need_resched()`) | Wake blocked pipe readers, signal completion |
| Low-priority | Idle loop (before HLT) | Page cache flush, KOBJ cleanup, process reaping |

### API

```rust
// IRQ-safe push
WORK_QUEUE.push_high(callback, data);   // high-priority
WORK_QUEUE.push_low(callback, data);    // low-priority

// Consumer (must call with interrupts disabled)
WORK_QUEUE.process_high();  // drain all high-priority items
WORK_QUEUE.process_low();   // drain all low-priority items
```

### Implementation

- Lock-free SPSC ring buffer (64 slots per level), same pattern as EventBus
- `WorkEntry` stores `(fn(*mut u8), *mut u8)` — function pointer + opaque data
- `pending` AtomicBool: set on push, cleared when queue drains
- 6 tests: push/pop, FIFO, empty, overflow, high/low isolation, pending flag

## In-Kernel Test Framework

528+ tests en 50+ suites. Registrados en `testing.rs`, ejecutados por el comando `test` del shell.

| Suite | Tests | Descripción |
|-------|-------|-------------|
| FSCK | 6 | Inode validation helpers, block pointer logic, mode checks, range checks |
| HELP | 4 | Ring 0 stub output, Ring 3 command scan, detail pipe spawn |
| Isolation | 12 | X4 Driver Isolation Layer: constants, bounds, alloc/free, driver_id lookup, layout, pointer validation, overflow, max slots, str ptr, mode for category, mode string |
| Boot Loader | 8 | Boot driver loader: scan, load, init, activate, unload, category ordering |
| ABI Negotiation | 10 | ABI version negotiation, window overlap, compatibility warnings, edge cases |
| Dependency | 13 | Dependency graph, topological sort, cycle detection, symbol extraction, case-insensitive |
| Storage Ref | 14 | Reference storage driver: entrypoints, lifecycle, R/W, geometry, error handling |
| IRP | 11 | Async I/O: IRP alloc/free, completion/error, pool reuse, queue FIFO/wraparound, callback dispatch, flush/ioctl ops, get_params |
| PS/2 Kbd Ref | 10 | Reference PS/2 keyboard driver: entrypoints, lifecycle, key events, error handling |
| Framebuffer Ref | 8 | Reference framebuffer driver: entrypoints, lifecycle, clear/pixel/scroll, error handling |
| Object (Ob) | 14 | ObObjectTable: create/lookup/destroy, refcount, close auto-destroy (OB-004), init root + type entries (OB-005) |
| Page Cache | 13 | Page cache (advanced): hash map O(1), LRU doubly-linked, create, peek, dirty, invalidate, capacity, stats, hit_rate, pending_writes |
| PCI Enumeration | 3 | PCI bus 0 devices, bus 1 empty, bridge detection algorithm |
| Work Queue | 6 | Deferred work queue: push/pop, FIFO, empty, overflow, high/low isolation, pending flag |
| IPI | 5 | Inter-processor interrupts: constants, TLB shootdown struct, call function struct, local-only, no-targets |
| Per-CPU Slab | 5 | Per-CPU slab alloc/free, refill/drain batching, scaling |
| IRQL | 5 | IRQL raise/lower, page fault invariant, spinlock implicit raise, nesting, preemption threshold |
| DPC | 5 | DPC engine: enqueue/dispatch, IRQ transition, nesting, callback order, stress 100 IRQs |
| Stress | 14 | Stress: sched, syscall, mem, buddy allocator, handle table |
| Hot Reload | 11 | Hot reload: resource tracking, registry, state transitions, unload/reload, error codes |
| Per-CPU (KPRCB) | 5 | KPRCB size, slab cache count, run queue ops, init, offset sanity |
| Syscall | 13 | SSDT dispatch, permissions, A4.6 spawn/readdir/mkdir/unlink/rmdir/rename, OB-004 handler_close file+pipe |
| SMP | 3 | Constants, trampoline size, BSP is CPU 0 |
| ANSI | 3 | ANSI terminal: color fg/bg, cursor position, clear screen |
| Security | 12 | NT6 Security Reference Monitor: SID format, Token inheritance, ACL allow/deny, SeAccessCheck, admin vs user token, admin bypass |
| URN | 11 | NT5.5 Unified Resource Namespace: parse schemes, invalid/missing paths, resolve file/device, roundtrip |
| KDrive | ~~12~~ | [ELIMINADO] Reemplazado por \Global\Info\* en Ob |
| Input | 13 | Input buffer (ring buffer), VT switching, independent queues, framebuffer swap |
| Keyboard | 5 | UTF-8 encoding, compose keys |
| Process/Thread | 4 | Kthread struct, ThreadState, Eprocess constructor |
| Scheduler | 7 | Priority scheduling, time-slice, round-robin, aging |
| UTF-8 | 6 | Validación UTF-8 |
| Allocator | 8 | Box, Vec, String |
| Sync | 4 | Atomic flags (NEED_RESCHED) |
| NeoFS | 75 | Inode metadata, permissions, timestamps, block count, DOS attrs, serialization, stress, corruption, rendering |
| NEM | 23 | NEM v1+v2 driver format parsing (header, types, v2 ABI fields, categories) |
| ELF | 20 | ELF64 loader: header validation, segment loading, edge cases, PIE offset/relocation |
| Capability | 11 | X3 Capability flags, CapabilitySet, category defaults, check/enforce, escalation policy |
| Event Bus | 17 | Unified v2: priority queues, subscription filters (type/source/device), dynamic payload, backpressure, 17 tests |
| Slab | 9 | Slab allocator: per-size alloc/free, multi-page, realloc fallback, reuse |
| Driver State | 21 | Driver certification pipeline: 7-state lifecycle, transition matrix, certify_and_activate(), last_error tracking, inactive_reason debug |
| Pipe | 13 | IPC pipes: alloc/free, write/read, EOF, EPIPE, blocking, fd table |
| IoStack | 5 | Unified block I/O: partition offset, no-partition passthrough, cache levels, device read, offset correctness |
| Mmap | 6 | MmapRegion struct, flags, address bounds, VMA add/remove |
Comando `test`:
1. Ejecuta `testing::run_all()` (528+ tests kernel)
2. Si pasan, ejecuta `run CPUINFO.NXE`, `run DIR.NXE`, `run DATETIME.NXE`, `run VER.NXE` (user-mode)

La shell Ring 3 (`neoshell.nxe`) se carga via NeoInit (PID 1) y ofrece built-ins + dispatch a comandos externos .NXE via PATH.

## SMP & Per-CPU Architecture (A1)

### Per-CPU Data Structures

`src/arch/x64/cpu_local.rs` — Kprcb struct (4 KB page per CPU, `#[repr(C, align(4096))]`), accessed via GS segment base.

| Field | Offset | Type | Description |
|-------|--------|------|-------------|
| `cpu_id` | 0x000 | u32 | Logical CPU index (0–15) |
| `apic_id` | 0x004 | u32 | Local APIC ID |
| `current_thread` | 0x008 | Option<NonNull<Kthread>> | Currently running thread |
| `current_pid` | 0x010 | u64 | Current process PID |
| `idle` | 0x014 | bool | Is this CPU idle? |
| `need_resched` | 0x015 | bool | Per-CPU reschedule flag (GS:0x015) |
| `current_irql` | 0x016 | u8 | Current IRQL level (0=PASSIVE, 2=DISPATCH) |
| `run_queue` | 0x018 | CpuRunQueue | 64-entry ring buffer per priority |
| `slab_caches` | 0x120 | [PerCpuSlabCache; 9] | 9 size classes (8B–2KB), 288 bytes each |
| `interrupt_count` | 0xB40 | u64 | Per-CPU interrupt counter |
| `context_switch_count` | 0xB48 | u64 | Context switch counter |
| `timer_tick_count` | 0xB50 | u64 | Timer tick counter |
| `exit_rsp` | 0xB58 | u64 | Exit trampoline RSP |
| `exit_rip` | 0xB60 | u64 | Exit trampoline RIP |
| `exit_rbx` | 0xB68 | u64 | Saved RBX for exit |
| `exit_r12` | 0xB70 | u64 | Saved R12 for exit |
| `exit_r13` | 0xB78 | u64 | Saved R13 for exit |
| `exit_r14` | 0xB80 | u64 | Saved R14 for exit |
| `exit_r15` | 0xB88 | u64 | Saved R15 for exit |
| `exit_rbp` | 0xB90 | u64 | Saved RBP for exit |
| `exit_now` | 0xB98 | bool | Exit flag (GS:0xB98) |

20 compile-time `offset_of!` assertions enforce layout correctness.

### Per-CPU Run Queues

- `CpuRunQueue`: 64-entry ring buffer (tail + head u16 indices)
- `schedule()` tries: local queue → `try_work_steal()` → global fallback
- `try_work_steal()`: steals from another CPU's IDLE queue, round-robin scan
- IPI_RESCHEDULE (vector 0xF0): sent when thread enqueued on another CPU

### SMP Boot

`src/arch/x64/smp.rs` — INIT-SIPI-SIPI sequence, AP trampoline at physical 0x800000.

- AP trampoline: 16-bit real mode → 32-bit protected → 64-bit long mode
- `ap_entry()`: sets GS base via `wrmsr(IA32_GS_BASE)`, signals readiness via `AP_READY`
- BSP polls `AP_READY_COUNT` until all APs are ready

### IPI Infrastructure

`src/arch/x64/ipi.rs` — Unified IPI module for inter-processor communication.

| Vector | Name | Purpose |
|--------|------|---------|
| 0xF0 | IPI_RESCHEDULE | Wake remote CPU's scheduler (sets per-CPU `need_resched`) |
| 0xF1 | IPI_TLB_SHOOTDOWN | Synchronous TLB invalidation with ACK protocol |
| 0xF2 | IPI_CALL_FUNCTION | Execute function on remote CPUs with ACK |

- **TLB shootdown**: `tlb_shootdown(start, end, target_mask)` — shared `TlbShootdownPayload` with atomic ack counter. Target CPUs execute `invlpg` for each page and ACK. Used by `paging.rs` for heap/mmap page free and protection changes.
- **Call function**: `call_function_all(func, arg, target_mask)` — `CallFunctionPayload` with atomic func pointer and ack counter. Generic cross-CPU function dispatch.
- **Scheduler integration**: `enqueue_to_cpu_run_queue()` sends `IPI_RESCHEDULE` to remote CPU when thread is enqueued.
- **EOI**: `ack_irq()` in `hal/x64/irq.rs` sends APIC EOI for ALL vectors >= 32 (fixed bug where IPI vectors were not acknowledged).

## Ob Object Manager (migrated de KOBJ v1)

El módulo `kobj/` fue eliminado en v0.46. Todo su contenido se migró a `object/`:
- `kobj/mod.rs` (compat shim `KObjType`/`kobj_register`/etc.) → eliminado, los callers usan directamente `object::ob_create_object`/`ObType`
- `kobj/namespace.rs` (namespace tree) → `object/namespace.rs`, los tipos internos `KObjId`/`KObjType` reemplazados por `ObId`/`ObType`

Referencias históricas de la API legacy vs actual:
| Concepto legacy | Equivalente actual |
|----------------|-------------------|
| `KObjType::X` | `ObType::X` |
| `kobj_register(t, n, id)` | `ob_create_object(t, n, id, 0, None)` |
| `kobj_unregister(id)` | `ob_destroy_object(id)` |
| `kobj_lookup(id)` | `ob_lookup(id)` |
| `kobj_ref/id` | `ob_reference/ob_dereference` |
| `crate::kobj::namespace::ob_*` | `crate::object::namespace::ob_*` |

### KOBJ Command (Ring 3)

Comando de usuario (`userbin/kobj/`, produce `kobj.nxe`). Usa `ob_enum` (RAX=64) para enumerar el namespace Ob.

| Subcommand | Description |
|-----------|-------------|
| `KOBJ` | Lists all Ob namespace objects: ID, type, name, refcount, native ID |

### PRI Command

| Subcommand | Description |
|-----------|-------------|
| `PRI <pid> <priority>` | Set scheduling priority for a running process (0=HIGH, 1=ABOVE_NORMAL, 2=NORMAL, 3=IDLE) |

## Event Bus v2

`src/eventbus/mod.rs` — Centralized event routing layer with priority, subscription filters, dynamic payload, and backpressure.

| Concept | Description |
|---------|-------------|
| **Event** | `#[repr(C)]` struct (56 bytes): `event_id`, `event_type`, `source`, `timestamp`, `device_id`, `data0`, `data1`, `flags` — ABI-stable for NEM drivers |
| **Event types** | 15 named constants: TIMER_TICK, KEYBOARD_INPUT, SERIAL_DATA, DISK_IO_COMPLETE, PROCESS_EXIT, DRIVER_LOADED, DRIVER_CRASH, POLICY_VIOLATION, FS_MOUNTED, KEYB_LAYOUT, EVENT_SHUTDOWN, EVENT_DRIVER_UNLOAD, EVENT_DRIVER_UNLOAD_ACK, USER(0x1000+). PCI NEM driver adds 0x1000–0x1003 |
| **Event sources** | SOURCE_HAL, SOURCE_DRIVER, SOURCE_KERNEL, SOURCE_USERLAND |
| **Priority queues** | Two lock-free SPSC ring buffers: **high** (16 slots) for timers/IRQ completions, **normal** (64 slots) for system events. High always drained first |
| **Subscription filters** | `register_handler_v2(filter, callback, name)` with `EventFilter`: filter by event_type, source_mask bitfield, device_id. v1 `register_handler()` creates a type-only filter |
| **Dynamic payload** | `push_event_with_dyn_payload()` — allocates a copy, stores pointer in `data0`/`data1`, auto-freed after dispatch via the handlers table |
| **Backpressure** | Queue full → `Err(())` returned to producer. `ERR_EVENT_BUS_FULL` constant (−16) for drivers |
| **Callbacks** | `register_handler()` / `register_handler_v2()` — max 64 handlers. Unregister by callback pointer (`unregister_handler`) or by name (`unregister_handler_by_name`) |
| **Dispatch** | `dispatch_one()`/`dispatch_pending()` — drains high queue first, then normal. Called from: (1) `clear_need_resched()` on every syscall return, (2) idle loop, (3) shell input loop |
| **IRQ integration** | TimerTick pushed from PIT IRQ0 (normal priority), KeyboardInput from PS/2 IRQ1 (normal priority). All lock-free pushes |
| **Scheduler integration** | `EVENT_BUS.dispatch_pending()` in `clear_need_resched()` + idle loop. Events dispatched on every syscall boundary |
| **Isolation** | No driver execution in IRQ context. No recursive dispatch. Events immutable after enqueue |

See `docs/ARCHITECTURE_SOURCE_OF_TRUTH.md` §12 NEM format for full NEM format spec.

## Driver Certification Pipeline v1

`src/drivers/driver_runtime.rs` — Strict driver lifecycle state machine.

### Lifecycle States (8-state, W2 Hot Reload)

```rust
DriverState::Loaded      // binary loaded, not verified
DriverState::Initialized // driver_init() executed, process spawned
DriverState::Registered  // registry committed, Event Bus notified
DriverState::Bound       // bound to Event Bus / Device
DriverState::Active      // fully operational, certified
DriverState::Faulted     // runtime failure (recoverable? → Unloaded)
DriverState::Unloaded    // removed from system (terminal)
DriverState::Unloading   // graceful drain in progress (W2 hot reload)
```

### Transition Rules

Only these transitions are valid:
```
Loaded → Initialized → Registered → Bound → Active
Active → Unloading → Unloaded → Loaded (reload path)
Any → Faulted
Any → Unloaded
All others → ERROR (TransitionError)
```

### Error Tracking

Each `DriverInstance` has:
- `last_error: u32` — error code from `ERR_*` constants
- `certification_step: u8` — which pipeline step failed (`PipelineStep`)
- `caps: u64` — capability bitmap (X3 Capability System)

Error codes: `ERR_NONE=0`, `ERR_INIT_FAILED`, `ERR_REGISTRATION_FAILED`,
`ERR_BIND_FAILED`, `ERR_SANDBOX_REJECTED`, `ERR_CERTIFICATION_FAILED`,
`ERR_OUT_OF_MEMORY`, `ERR_POLICY_VIOLATION`, `ERR_LOAD_FAILED`,
`ERR_CAPABILITY_DENIED=9`.

### Certification (`certify_and_activate`)

A driver is **only ACTIVE** if:
1. State == Bound (all prior transitions completed in order)
2. `last_error == 0` (no unresolved errors)
3. Not Faulted

Otherwise remains in current state with `last_error = ERR_CERTIFICATION_FAILED`.

### Debugging LOADED ≠ ACTIVE

Use `NDREG DEBUG <name>` to run a 5-stage checklist:
1. **LOAD**: Is driver in registry?
2. **INIT**: Was driver_init() called?
3. **REG**: Was registry committed?
4. **BIND**: Was Event Bus bound?
5. **CERTIFY**: All checks passed?

Each stage shows a clear PASS/FAIL and explains the next step. The `inactive_reason()` method on `DriverInstance` returns a human-readable explanation.

## ABI Negotiation Layer v1

`src/drivers/abi/mod.rs` — Formalized ABI version negotiation between kernel and NEM drivers.

### Core Types

```rust
pub struct AbiVersion { min: u16, target: u16, max: u16 }

pub enum NegotiationResult {
    Compatible,
    CompatibleWithWarnings(&'static [&'static str]),
    Incompatible(&'static str),
}
```

### Negotiation Rules

A driver is ABI-compatible iff:
- `driver.min > 0 && driver.target > 0 && driver.max > 0` (valid fields)
- `driver.min <= ABI_MAX_VALID` (driver not too new)
- `driver.max >= ABI_MIN_VALID` (driver not too old)
- `driver.target` within `[ABI_MIN_VALID, ABI_MAX_VALID]` (target in range)

### Warning Levels

- `CompatibleWithWarnings("Driver ABI predates kernel target...")` — driver.max < kernel.target
- `CompatibleWithWarnings("Driver targets a newer ABI than kernel default...")` — driver.target > kernel.target

### Integration

The v3loader's `validate_v3_abi()` delegates to `drivers::abi::negotiate_default()` instead of inline checks. 10 unit tests cover valid/invalid/warning scenarios.

## Driver Dependency Resolver v1

`src/drivers/dependency/mod.rs` — Automatic dependency resolution between NEM drivers.

### DependencyGraph

```rust
pub struct DependencyGraph { edges: BTreeMap<String, Vec<String>> }

fn add_driver(name: &str)
fn add_dependency(driver: &str, depends_on: &str) -> Result<(), DepError>
fn resolve_order() -> Result<Vec<String>, DepError>
fn has_cycle() -> bool
```

### Dependency Declaration Convention

Drivers declare dependencies via special `__dep_DRIVERNAME` symbols in the NEM symbol table. The function `resolve_nem_symbol_dependencies()` scans symbols for the `__dep_` prefix and extracts dependency names.

```rust
pub fn resolve_nem_symbol_dependencies(
    symbols: &[NemSymbol], strtab: &[u8]
) -> Vec<String>
```

### Resolution Algorithm

1. Build directed graph from driver declarations + `__dep_` symbols
2. Run DFS-based topological sort (Kahn's algorithm alternative)
3. Detect cycles via DFS in-stack tracking
4. Return ordered list: dependencies before dependents

### Boot Loader Integration

`boot_load_all()` v2 scans drivers, builds a `DependencyGraph` per category, resolves load order, and loads in dependency-sorted sequence. Falls back to filesystem order on cycle detection. 13 unit tests.

## X3 Capability System

`src/drivers/caps.rs` — Fine-grained resource access control for NEM drivers.

Each driver has a 64-bit capability bitmap set at load time. Every `hst_*` export
function checks that the calling driver holds the required capability before executing.

### Capability Flags

| Flag | Value | Resource | Required for |
|------|-------|----------|-------------|
| `CAP_IRQ` | 1 | IRQ | `hst_ack_irq()` |
| `CAP_DMA` | 2 | DMA | DMA transfers |
| `CAP_MMIO` | 4 | MMIO | Memory-mapped I/O access |
| `CAP_PORTIO` | 8 | Port I/O | `hst_inb/outb/inw/outw/inl/outl()` |
| `CAP_ALLOC_PAGE` | 16 | Physical frames | Frame allocation |
| `CAP_BLOCK_DEVICE` | 32 | Block devices | `hst_register/unregister_block_device()` |
| `CAP_EVENT_BUS` | 64 | Event Bus | `hst_push_event()` |
| `CAP_INPUT` | 128 | Input | `hst_push_input_byte()` |
| `CAP_LOG` | 256 | Logging | `hst_log()` |
| `CAP_TIMING` | 512 | Timing | `hst_get_ticks()` |
| `CAP_MEMORY` | 1024 | Memory mapping | mmap operations |
| `CAP_ISOLATION` | 2048 | Driver Isolation | Loading into isolated region |

### Category Defaults (Inheritance)

- **BOOT** → All capabilities (`CAP_ALL` = all 11 flags)
- **SYSTEM** → `CAP_PORTIO | CAP_IRQ | CAP_MMIO | CAP_DMA | CAP_EVENT_BUS | CAP_INPUT | CAP_LOG | CAP_TIMING` (no `CAP_ALLOC_PAGE`, no `CAP_BLOCK_DEVICE`, no `CAP_MEMORY`)
- **DEMAND** → `CAP_EVENT_BUS | CAP_LOG | CAP_TIMING` (sandboxed)

### Capability Escalation

A SYSTEM driver may request additional capabilities (`CAP_ALLOC_PAGE`, `CAP_BLOCK_DEVICE`,
`CAP_MEMORY`) via `EVENT_CAP_ESCALATION` (type `0x2000`). The kernel audits and may grant.
DEMAND drivers cannot escalate — this is a security boundary.

### Implementation

- Each export function in `v3loader.rs` and `hst.rs` calls `check_cap()` before executing
- `current_driver_id()` tracks which driver is active (set before `driver_init`/activate/event calls)
- Capability denial returns error/sentinel (0, -1, or no-op) instead of executing
- `NDREG SHOW` displays capabilities in hex and human-readable format

## X4. Driver Isolation Layer

`src/drivers/isolation.rs` — Page-isolated memory region for NEM drivers to limit the impact of driver bugs.

### Constants & Region

| Constant | Value | Description |
|----------|-------|-------------|
| `DRIVER_ISO_BASE` | `0x3000_0000` | Start of isolated region (16 MB) |
| `DRIVER_ISO_SIZE` | `0x100_0000` | Total region size |
| `DRIVER_ISO_END` | `0x3100_0000` | End of isolated region |
| `DRIVER_SLOT_SIZE` | `0x10_0000` | Per-driver slot size (1 MB) |
| `MAX_ISOLATED_DRIVERS` | 16 | Maximum concurrent isolated drivers |
| `MAX_DRIVER_SIZE` | `0x10_0000` | Maximum driver binary size (1 MB) |

### Initialization

`init_isolated_region()` (PHASE 3.80) splits 2 MB huge pages in the isolated region into 4 KB page tables, then strips all identity mapping. Pages are allocated on demand via `alloc_isolated_page()` during driver loading.

### API

```rust
fn allocate_driver_slot(driver_id: u32, size: u64) -> Option<u64>
fn free_driver_slot(driver_id: u32)
fn driver_base(driver_id: u32) -> Option<u64>
fn set_driver_layout(driver_id: u32, text_size: u32, rodata_size: u32, data_size: u32, bss_size: u32)
fn driver_id_for_address(virt_addr: *const u8) -> Option<u32>
fn alloc_isolated_page(virt: u64, flags: u64) -> Option<u64>
fn free_isolated_page(virt: u64) -> bool
fn free_isolated_range(start: u64, end: u64)
fn is_in_isolated_region(virt: u64) -> bool
fn is_in_driver_region(addr: u64, driver_id: u32) -> bool
fn validate_driver_ptr(ptr: *const u8, size: usize, driver_id: u32, writable: bool) -> Result<(), &'static str>
fn validate_driver_str_ptr(ptr: *const u8, driver_id: u32) -> Result<usize, &'static str>
fn handle_isolated_page_fault(virt: u64) -> bool
fn isolation_mode_str(mode: IsolationMode) -> &'static str
```

### Isolation Modes

| Mode | Value | Applied to | Behavior on page fault |
|------|-------|-----------|----------------------|
| `None` | 0 | — | No isolation |
| `Basic` | 1 | BOOT, SYSTEM drivers | Ignore (no check) |
| `Sandbox` | 2 | DEMAND drivers | Mark driver FAULTED |

### Pointer Validation Rules

`validate_driver_ptr()` accepts addresses in these ranges:
- Driver's own isolated slot (0x30000000 base per slot)
- Kernel heap (0x01000000–0x02000000)
- Kernel .rodata/.text (0x00100000–0x01000000)
- User heap (0x10000000–0x12000000)
- mmap region (0x20000000–0x22000000)
- User code (0x400000–0x800000)
- Kernel image (0x200000–PHYS_MEM_END default)

All other addresses are rejected.

### Integration

- `v3loader.rs` — `alloc_driver_memory` uses isolated region with heap fallback; `bind_isolated_driver` links driver to slot after registration
- `driver_runtime.rs` — `DriverInstance` stores `isolation_mode`, `isolated_base`, `isolated_size`; `set_isolation_region()` method
- `boot_loader/mod.rs` — calls `bind_isolated_driver` after each `register_driver_ext`
- `caps.rs` — `CAP_ISOLATION = 2048` (bit 11)
- `ndreg.rs` — SHOW and RUNTIME display isolation info

### Tests

12 tests: constants sanity, region bounds, alloc/free, driver_id lookup, layout, pointer validation (in/out-of-region, writable/read-only), overflow, max slots, str ptr, mode for category, mode string.

## Boot Driver Loader System

`src/drivers/boot_loader/mod.rs` — Automatic boot-time driver loading subsystem (v2 with dependency resolver). Runs as PHASE 3.85 in `main.rs` boot sequence.

### Boot Order

1. **BOOT drivers** — scanned from `C:\System\Drivers\` (required for system init)
2. **SYSTEM drivers** — scanned from `C:\System\Drivers\` (standard kernel extension)

Within each category, drivers are **dependency-sorted**: the boot loader scans `.nem` files, extracts `__dep_` symbol dependencies, builds a `DependencyGraph`, and loads drivers in topological order (dependencies before dependents).

If any BOOT driver fails to load/initialize, the boot continues (no panic) the driver is marked FAULTED.

### API

```rust
fn driver_scan(category: DriverCategory) -> Result<Vec<DriverScanResult>, &'static str>
fn driver_load(path: &str) -> Result<DriverId, &'static str>
fn driver_init(id: DriverId) -> Result<(), &'static str>
fn driver_activate(id: DriverId) -> Result<(), &'static str>
fn driver_unload(id: DriverId) -> Result<(), &'static str>
fn boot_load_all() -> BootSummary  // orchestrator: returns counts per category
```

### BootSummary

```rust
struct BootSummary {
    boot_total: usize, boot_ok: usize, boot_fail: usize,
    system_total: usize, system_ok: usize, system_fail: usize,
    total: usize, ok: usize, fail: usize,
}
```

### Driver Categories

Defined in `crate::nem::DriverCategory`:
- `Boot = 0` — loaded first, required for hardware init
- `System = 1` — loaded second, standard system drivers
- `Demand = 2` — on-demand loading only

### Implementation Notes

- `driver_load` reads file content from NeoFS via `read_whole_file`, then calls `loader::load_nem`.
- `driver_init` calls `driver_runtime::transition` to advance state.
- `driver_activate` marks driver Active in the runtime.
- `driver_unload` transitions to Unloaded.
- Boot loader has 8 kernel tests (scan/load/unload/init/activate, category ordering, empty categories).

## NEM v2 Format

`src/nem/mod.rs` — Extended NeoDOS Driver Format with ABI validation.

### Header v2 (48 bytes)

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 4 | magic | "NEM\0" |
| 4 | 4 | version | 2 |
| 8 | 2 | header_size | 48 |
| 10 | 2 | driver_type | NemDriverType |
| 12 | 4 | entry_offset | Offset to code |
| 16 | 4 | code_size | Raw x86-64 code size |
| 20 | 2 | compat_flags | Compatibility flags |
| 22 | 2 | abi_min | Min ABI version (0.0.1 → 1) |
| 24 | 2 | abi_target | Target ABI version |
| 26 | 2 | abi_max | Max ABI version |
| 28 | 1 | category | DriverCategory (0=Boot,1=System,2=Demand) |
| 29 | 3 | reserved | Padding |
| 32 | 16 | name | ASCII driver name (null-padded) |

### ABI Constants

- `ABI_MIN_VALID = 1` — kernel minimum supported ABI
- `ABI_TARGET = 1` — kernel target ABI (v0.3 encoded as 1.1.2 → 1)
- `ABI_MAX_VALID = 2` — kernel max supported ABI

### ABI Validation Rule

A driver is ABI-compatible iff:
- `driver.abi_min ≤ ABI_MAX_VALID` (driver doesn't require newer kernel)
- `driver.abi_max ≥ ABI_MIN_VALID` (driver isn't too old)
- `ABI_MIN_VALID ≤ driver.abi_target ≤ ABI_MAX_VALID` (target in range)

## NDREG Command

`src/shell/commands/ndreg.rs` — NeoDOS Driver Registry CLI (regedit-like).

| Subcommand | Description |
|-----------|-------------|
| `NDREG LIST [path]` | List .nem drivers with pipeline state + error + visual progress |
| `NDREG SHOW <name>` | Show full driver details + certification check + error diagnostics |
| `NDREG QUERY` | Summarize driver registry + per-state breakdown |
| `NDREG RUNTIME` | Runtime snapshot: loaded/active/faulted counts + per-driver pipeline |
| `NDREG HEALTH` | Validate driver metadata integrity (NEM header validity) |
| `NDREG DEBUG <name>` | Diagnose why a driver is NOT active (5-stage checklist) |
| `NDREG LOAD <path>` | Load driver through certification pipeline (→ Active if all pass) |
| `NDREG UNLOAD <name> [/F]` | Gracefully unload a driver (sends EVENT_DRIVER_UNLOAD, waits for ACK, cleanup resources). `/F` forces unload without waiting |
| `NDREG RELOAD <path>` | Reload a driver from disk with ABI version check (unload + load + re-initialize) |

All data is read-only from NeoFS + runtime registry. No driver execution.

## MCP Server — Kernel Introspection & VFS Analysis

`scripts/mcp_server/` implements a Model Context Protocol (MCP) server for
AI-assisted kernel debugging, VFS inspection, and architectural validation.

### Launch
```bash
bash scripts/mcp-server.sh              # stdio mode (MCP protocol)
bash scripts/mcp-server.sh --tool vfs_list path='\' drive=C
```

### Tools (18 total)

| Tool | Description |
|------|-------------|
| `kernel_index` | Source file index with line counts, grouped by subsystem |
| `search_symbol` | Search for fn/struct/trait/const in kernel source |
| `get_kernel_architecture` | Memory layout, boot phases, subsystem boundaries |
| `get_build_errors` | Check for duplicate code, missing artifacts, ABI issues |
| `vfs_list` | List directory via NeoDOS FS image parser |
| `vfs_read` | Read file via VFS (text or hex dump) |
| `vfs_stat` | File/directory metadata (inode, size, perms) |
| `vfs_resolve` | Path resolution with fallback across drives |
| `vfs_tree` | Recursive directory tree |
| `vfs_dump_superblock` | Superblock details |
| `vfs_dump_inodes` | Inode table dump |
| `list_loaded_modules` | List NEM drivers and DLLs from build artifacts |
| `get_module_symbols` | Show symbols/exports for a NEM driver or DLL |
| `sys_loadlib_analyze` | Read-only analysis of what sys_loadlib would do |
| `analyze_libneodos_api` | libneodos ABI table, syscall wrappers, error codes |
| `check_abi_compatibility` | NEM/ELF ABI version check against kernel |
| `analyze_libneodos_coverage` | Syscall coverage: which have wrappers, which missing |
| `check_consistency` | Validate architecture: code, docs, artifacts, invariants |

### Resources (3)
| URI | Description |
|-----|-------------|
| `neodos://system/info` | Project structure, version, build artifacts |
| `neodos://kernel/architecture` | Memory layout, boot phases, subsystem map |
| `neodos://libneodos/api` | Full AbiTable reference, error constants, syscall map |

### Prompts (3)

| Prompt | Description |
|--------|-------------|
| `analyze_system_state` | Comprehensive system analysis (kernel/modules/VFS/API) |
| `debug_module_loading` | Debug why a NEM/DLL fails to load |
| `analyze_vfs_path` | Trace VFS path resolution through NeoDOS filesystem |

### Architecture

- `server.py` — MCP protocol engine (JSON-RPC 2.0 over stdio)
- `parsers/neodos_fs.py` — NeoDOS filesystem image parser (reads neodos_image.img)
- `parsers/nem_parser.py` — NEM v3 driver format parser (80-byte header, relocs, symbols)
- `parsers/elf_parser.py` — ELF64 parser for DLL/user binary analysis
- `tools/kernel_tools.py` — Kernel introspection (source index, symbol search, build check)
- `tools/vfs_tools.py` — VFS analysis (list, read, stat, tree, superblock, inodes)
- `tools/module_tools.py` — Module analysis (NEM/DLL parsing, sys_loadlib simulation)
- `tools/libneodos_tools.py` — libneodos API analysis (AbiTable, coverage, ABI check)
- `tools/system_tools.py` — Consistency checker, system resource provider

Reglas: toda operación de archivos pasa por los parsers VFS (nunca acceso directo
al disco). Los módulos dinámicos se analizan offline — no se cargan realmente.
El MCP es observador, no generador de sistema.

## Dependencias

```bash
python3 scripts/check_deps.py        # Validate subsystem dependency rules
```

Ver `docs/ARCHITECTURE_SOURCE_OF_TRUTH.md` para la arquitectura completa de subsistemas.

## Arquitectura (subsystem boundaries)

La kernel está organizada en 16 subsistemas explícitos. Cada subsistema:
- Tiene responsabilidades definidas y prohibidas
- Expone APIs públicas e internas
- Tiene dependencias controladas
- Tiene reglas de sincronización

### Reglas de acoplamiento (forbidden dependencies)

| Subsistema | No puede depender de |
|-----------|---------------------|
| Scheduler | VFS, drivers de bloque, AHCI/ATA |
| IRQ handler | `schedule()`, VFS, heap allocation |
| ATA driver | AHCI, RAM disk, scheduler |
| BlockDevice trait | scheduler, filesystems |
| Shell | AHCI, ATA, syscall dispatch |
| Console | scheduler, filesystems, drivers |
| Memory/frame allocator | scheduler, filesystems, drivers |

Ver `docs/ARCHITECTURE_SOURCE_OF_TRUTH.md` para la especificación completa.

## Mejoras pendientes

Ver `docs/IMPROVEMENTS.md` para la lista completa de items pendientes por prioridad.

## Changelog

Cada feature completada debe añadir entrada en `CHANGELOG.md` con formato:
```markdown
## [v0.12.0] - YYYY-MM-DD
### Added
- sys_brk/ sys_mmap: ...
### Changed
- ...
```

## HAL v0.4 (Hardware Abstraction Layer, raw/safe split)

### Architecture

`src/hal/` implements ABI v0.4. All inline assembly is confined to this directory tree.

| Module | Description |
|--------|-------------|
| `hal/raw/` | Bare asm primitives: MSR, CPUID, TSC, I/O ports, control registers, segment regs, GS-segment, interrupt flags, pause, TLB |
| `hal/safe/` | Type-safe wrappers: `Msr` trait with `read_msr<T: Msr>()` / `write_msr<T: Msr>()`, MSR constants with `IsSafe` flag, `read_cr2()` |
| `hal/pci/` | PCIe ECAM MMIO config space: `ecam_read_config_*`, `ecam_write_config_*`, `set_ecam_base` |
| `hal/x64/` | Extern "C" ABI surface for bootloader/kernel, delegates to `hal/raw` |

### Audit constraint

```bash
grep -rn 'asm!(' src/ --exclude-dir=hal/    # MUST return 0
grep -rn 'asm!(' src/hal/ --exclude-dir=target  # All 55 asm calls here
```

### Msr trait example

```rust
use crate::hal::safe::{read_msr, write_msr, Msr, GsBase, GS_BASE};

let gs_base: u64 = read_msr(&GS_BASE);
unsafe { write_msr(&GS_BASE, new_base); }
```

## PCIe ECAM (A2.1)

`src/hal/pci.rs` — Enhanced Configuration Access Mechanism (MMIO).

| Function | Description |
|----------|-------------|
| `set_ecam_base(base)` | Set ECAM base from MCFG, activate ECAM mode |
| `ecam_is_active()` | Check if ECAM mode is active |
| `ecam_read_config_dword/bus/dev/func/offset)` | MMIO read PCI config (unsafe) |
| `ecam_read_config_word/byte` | 16/8-bit MMIO reads |
| `ecam_write_config_dword/word/byte` | MMIO PCI config writes |

`src/drivers/pci.rs` — auto-selects ECAM or legacy PIO:
- `pci_config_read_dword/write_dword/word/byte` — ECAM if active, else 0xCF8/0xCFC
- `find_capability(bus, dev, func, cap_id)` — scan capability list (MSI 0x05, MSI-X 0x11)
- `read_bar/bus/dev/func/bar_index)` — read BAR value
- `map_bar_mmio(bus, dev, func, bar_index)` — map BAR MMIO region with size detection
- `init_ecam()` — called at Phase 2.3, reads MCFG, maps ECAM region as UC-

MCFG parsing in `src/timers/hpet.rs`:
- `get_ecam_info()` returns `(base_addr, segment_group, start_bus, end_bus)` from ACPI MCFG

## I/O APIC (A2.2)

`src/interrupts/ioapic.rs` — I/O APIC interrupt controller replacing legacy 8259A PIC.

| Function | Description |
|----------|-------------|
| `init()` | Find IOAPIC via MADT, mask PIC, route ISA IRQs 0/1/4/12 to vectors 32/33/36/44 |
| `is_active()` | IOAPIC initialized and active |
| `mask_irq(irq)` / `unmask_irq(irq)` | Mask/unmask IOAPIC redirection entry |
| `ioapic_addr()` | MMIO base address (0 if not found) |
| `ioapic_pin_count()` | Number of redirection entries |
| `eoi_irq(vector)` | No-op (LAPIC handles EOI for edge-triggered) |

MADT parsing in `src/timers/hpet.rs`:
- `find_ioapic()` returns `(ioapic_addr, gsi_base)` from MADT
- `get_isa_overrides()` returns `Vec<(source, gsi, flags)>` from MADT ISA override entries

## MSI-X (A2.2 extension)

`src/interrupts/msi.rs` — per-entry MSI-X table programming:

| Function | Description |
|----------|-------------|
| `configure_msix_entry(bus, dev, func, entry_index, vector)` | Map BAR, write 16-byte table entry (addr+data+ctrl), enable MSI-X |
| `configure_msix_entries(bus, dev, func, num_entries, handler)` | Configure N entries with auto-vector allocation |

## HAL v0.3 (Hardware Abstraction Layer)

`src/hal/` implements ABI v0.3 — a minimal, pure hardware abstraction. HAL is the lowest layer; kernel depends on HAL, never the reverse.

**26 primitives** (extern "C"):

### CPU Control
| Function | Description |
|----------|-------------|
| `enable_interrupts()` / `disable_interrupts()` | STI / CLI (x86) |
| `halt()` | HLT loop (`-> !`) |
| `poweroff()` | QEMU debug port + PS/2 reset (`-> !`) |
| `read_cr2()` | Page-fault linear address |
| `read_cr3()` / `write_cr3(val)` | Page table base register |
| `flush_tlb(virt)` | `invlpg` instruction |
| `interrupts_enabled()` | Read RFLAGS.IF |
| `hlt_once()` | Single HLT (returns after next IRQ) |

### Port I/O
| Function | Description |
|----------|-------------|
| `inb(port)` / `outb(port, val)` | 8-bit port I/O |
| `inw(port)` / `outw(port, val)` | 16-bit port I/O |
| `inl(port)` / `outl(port, val)` | 32-bit port I/O |

### Page Memory
| Function | Description |
|----------|-------------|
| `alloc_page()` / `free_page(ptr)` | Physical frame alloc/free |
| `map_page(phys, virt, flags)` / `unmap_page(virt)` | 4K page table manipulation |
| `memory_barrier()` | SeqCst fence |

### Interrupt Management
| Function | Description |
|----------|-------------|
| `register_irq(vector, handler)` | IDT entry setup (stub — not yet dynamic) |
| `ack_irq(vector)` | PIC EOI via port I/O |

### Timing
| Function | Description |
|----------|-------------|
| `get_ticks()` | Read global timer tick counter |
| `increment_ticks()` | Atomic increment (timer IRQ) |
| `sleep_hint(us)` | Busy-wait delay |

### Non-ABI helpers (Rust ABI, not extern "C")
| Function | Description |
|----------|-------------|
| `without_interrupts(|| { ... })` | Save+disable+run+restore interrupts |
| `walk_ptes_4k(virt)` | Walk active page tables to find 4K PTE |
| `cpu_info()` | CPU brand string / features |

**Backend**: `hal/x64/` implements all primitives for x86_64. A future `hal/aarch64/` would provide the same API for ARM.
**Init code** stays in `arch/x64/` (GDT, IDT, PIC, paging init, entry point, serial) — these are architecture-specific and not part of the HAL contract.

## IRQL Framework (A2.4)

`src/hal/x64/irql.rs` — Per-CPU interrupt request level mechanism replacing blanket `cli`/`sti`.

### IRQL Levels

| Level | Constant | Value | Description |
|-------|----------|-------|-------------|
| PASSIVE | `PASSIVE_LEVEL` | 0 | Normal kernel/user code, all interrupts enabled |
| APC | `APC_LEVEL` | 1 | APC delivery, most device interrupts enabled |
| DISPATCH | `DISPATCH_LEVEL` | 2 | DPC delivery + scheduler, timer/device IRQs masked |
| DIRQL | `DIRQL_BASE` | 3 | Device interrupt handler level |
| HIGH | `HIGH_LEVEL` | 15 | NMI, machine check |

### Per-CPU Storage

`current_irql` field in KPRCB at GS offset `0x016` (replaces former `in_dispatch_level` bool). Access via `this_cpu_irql()` / `this_cpu_set_irql()` in `cpu_local.rs`.

### API

```rust
raise_irql(new_level) -> old_level   // CLI if new >= DISPATCH
lower_irql(old_level)                // STI if dropping below DISPATCH
current_irql() -> u8                 // read-only, no side effects
at_or_above_dispatch() -> bool       // current_irql() >= 2
at_dispatch(|| { ... })              // closure at DISPATCH_LEVEL
```

### IrqMutex

`IrqMutex<T>` wraps `spin::Mutex<T>` with automatic IRQL raise/lower:
- `lock()` → raises to DISPATCH, returns `IrqMutexGuard`
- Guard `Drop` → lowers IRQL back
- Satisfies invariant: holding a spinlock implies IRQL >= DISPATCH

### INV-14: Page Fault at DISPATCH

Page fault handler (`idt.rs`) checks IRQL at entry. If `current_irql() >= DISPATCH_LEVEL`, panics with `BUGCHECK KI_EXCEPTION_ACCESS_VIOLATION`.

### Migration from `without_interrupts`

Key paths migrated:
- `work_queue.rs` — `process_high_safe()` / `process_low_safe()`
- `scheduler/mod.rs` — all global helpers (`current_pid`, `current_tid`, `get_current_cwd`, etc.)
- `pipe.rs` — `wake_pipe_readers()`, `block_current_for_pipe()`

### Tests (5)

| Test | Description |
|------|-------------|
| `irql_raise_lower_passive_dispatch` | Raise PASSIVE→DISPATCH and back |
| `irql_page_fault_at_dispatch_panics` | Verify invariant enforcement |
| `irql_spinlock_implicit_raise` | IrqMutex raises/lowers IRQL correctly |
| `irql_nesting_stack` | Nested raise/lower preserves correct old levels |
| `irql_preemption_threshold` | Raising to same level is no-op, threshold semantics |

## DPC Engine (A2.5)

`src/dpc/mod.rs` — Deferred Procedure Call engine for executing callbacks at DISPATCH_LEVEL, offloaded from DIRQL interrupt handlers.

### Design

| Concept | Description |
|---------|-------------|
| **Per-CPU queues** | 128-entry SPSC ring buffer stored in `DPC_QUEUES[16]` static array (not in KPRCB to keep it ≤4096) |
| **Enqueue** | `insert_queue_dpc(fn, ctx)` — SPSC, no locks (producer runs at DIRQL with interrupts off) |
| **Dispatch** | `dpc_dispatch_pending()` — drains queue at DISPATCH_LEVEL (consumer) |
| **Nesting limit** | `MAX_DPC_DEPTH=10` prevents infinite recursion when DPCs enqueue other DPCs |
| **Integration** | Called from timer handler exit (`idt.rs`) and syscall return (`clear_need_resched()` in `syscall/mod.rs`) |

### Per-CPU Queue Access

```rust
static mut DPC_QUEUES: [DpcQueue; MAX_CPUS] = [...];
fn this_cpu_dpc_queue() -> &'static mut DpcQueue { DPC_QUEUES[cpu_id] }
```

### Tests (5)

| Test | Description |
|------|-------------|
| `dpc_enqueue_dispatch_level` | Basic enqueue + dispatch + callback execution |
| `dpc_irq_to_dispatch_transition` | Simulate IRQ enqueue followed by DIRQL→DISPATCH dispatch |
| `dpc_nesting_depth_limit` | Verify MAX_DPC_DEPTH prevents infinite recursion |
| `dpc_callback_execution_order` | Verify FIFO order of 3 callbacks |
| `dpc_stress_100_irqs` | 100 IRQs generating DPCs each, all executed, no leaks |

## NT6 Security Reference Monitor

`src/security/` — Security identity and access control for processes and objects.

| Module | File | Contents |
|--------|------|----------|
| SID | `src/security/sid.rs` | `Sid` struct (S-R-I-S* format), `sid_builtin_admin()` (S-1-5-18), `sid_builtin_user()` (S-1-5-21-0-0-0-1000), `format_string()` |
| Token | `src/security/token.rs` | `Token` struct (sid, is_admin, groups, privileges bitmap, session_id), `new_admin()`/`new_user()`, `is_admin_token()`, `has_privilege()`, `inherit_from()`, 12 privilege constants (SE_*_PRIVILEGE) |
| SAM | `src/security/sam.rs` | `SamDatabase` + `SamEntry` con nombre, SID, flags (admin/disabled/locked), full_name, comment. Binary format: header (16B) + variable-length entries. `parse_sam()`/`serialize_sam()` |
| ACL | `src/security/acl.rs` | `Ace` (allow/deny, access_mask, Sid), `Acl` (revision + ACE vec), `SecurityDescriptor` (owner, group, dacl). Access constants: `ACCESS_READ`, `ACCESS_WRITE`, `ACCESS_EXECUTE`, `ACCESS_ALL` |
| Access | `src/security/access.rs` | `se_access_check()` — token vs SD check with admin bypass, deny-by-default, ACL iteration |
| Initialization | `src/security/mod.rs` | `init_security()` in Phase 2.77. `DEFAULT_ADMIN_TOKEN`/`DEFAULT_USER_TOKEN` lazy_static globals |

### Token Lifecycle
- Boot: idle process (PID 0) gets admin token with `SE_ADMIN_PRIVILEGES`
- PID 1 (NeoInit): inherits admin token via `add_ring3_process()`
- Child processes: inherit parent's token at spawn via `Token::inherit_from()` (clones SID, groups, privileges, session_id)
- `is_current_admin()` uses `eprocess.token.is_admin_token()` replacing old PID-based check
- Token privileges: 12 `SE_*` flags (admin = 0xFFFF, user = SE_CHANGE_NOTIFY only)
- Group membership via `add_group()`/`is_in_group()` with `Vec<Sid>`
- Session tracking via `session_id: u32` (admin=0, users=1+)

### SAM Database
- `SamDatabase` — collection of `SamEntry` users, max 64 entries
- Binary serialisation via `serialize_sam()` / `parse_sam()`
- Format: 16B header (magic `SAM\0`, version, count) + variable-length entries (username, SID, flags, full_name, comment)
- Case-insensitive username lookup
- Flags: `SAM_FLAG_ADMIN`, `SAM_FLAG_DISABLED`, `SAM_FLAG_LOCKED`

### Tests (23)

| Test | Description |
|------|-------------|
| `sid_format` | Admin SID format: S-1-5-18, revision=1, sub_authority_count=1 |
| `token_admin_boot_default` | Admin token has admin=true, user token has admin=false |
| `token_inherit` | Child token matches parent SID and admin status |
| `acl_allow_access` | Allow ACE grants read access, denies write |
| `acl_deny_access` | Deny ACE blocks user, admin bypasses |
| `acl_inherit_parent` | SecurityDescriptor clone preserves structure |
| `se_access_check_deny` | Deny ACE correctly blocks access |
| `se_access_check_allow` | Allow ACE correctly grants access |
| `se_access_check_admin_override` | Admin token bypasses restrictive ACL |
| `se_admin_required` | Syscall 50 requires admin permission |
| `se_user_denied_admin_syscall` | User token cannot call admin syscalls |
| `se_admin_token_isolation` | Admin and user tokens have different SIDs, admin bypasses ACL |
| `sam_create_database` | SAM database init |
| `sam_add_user` | Add user with admin flag |
| `sam_find_by_username` | Case-insensitive username lookup |
| `sam_find_by_sid` | Find user by SID |
| `sam_remove_user` | Remove user from database |
| `sam_flags_disabled_locked` | SAM_FLAG_DISABLED / SAM_FLAG_LOCKED bits |
| `sam_parse_roundtrip` | Serialize + parse binary roundtrip with full_name/comment |
| `sam_parse_magic_error` | Invalid magic rejected |
| `sam_parse_truncated` | Truncated data rejected |
| `sam_max_entries_enforced` | Max 64 entries enforced |
| `sam_case_insensitive_lookup` | find_by_username case-insensitive |

## Artifacts generados

| Archivo | Path | Descripción |
|---------|------|-------------|
| Bootloader UEFI | `neodos/bootloader.efi` | v0.10.5 |
| Kernel ELF | `neodos/kernel.elf` | v0.14.0 |
| Disco GPT unificado | `neodos/disk_image.img` | 112 MB (ESP + NeoDOS FS) |
| NeoDOS FS image (temp) | `neodos/scripts/neodos_image.img` | 10 MB, regenerado en build |
| GPT builder | `neodos/scripts/create_gpt_image.py` | Combina ESP + NeoDOS en GPT |
| HAL ABI v0.3 | `neodos/neodos-kernel/src/hal/` | 7 módulos: cpu, io, mem, irq, time + x64 backend |
| PCI NEM driver | `neodos/drivers/pci/pci.nem` | NEM v3 standalone PCI bus enumerator (SYSTEM, full bus scan via bridge traversal) |
| ATA NEM driver | `neodos/drivers/ata/ata.nem` | NEM v3 standalone ATA driver with DMA+PIO, primary+secondary channels (SYSTEM) |
| AHCI NEM driver | `neodos/drivers/ahci/ahci.nem` | NEM v3 standalone AHCI driver (SYSTEM, DMA polling, ATA+ATAPI) |
| Driver Isolation | `neodos-kernel/src/drivers/isolation.rs` | X4 driver isolation layer (16 MB region, 16 × 1 MB slots, pointer validation, sandbox mode) |
| libmath NXL | `neodos/libmath.nxl` | Math library NXL (slot 1, 0x1e040000) — abs, min, max, pow, sqrt, sin, cos, log, exp |
| libconsole NXL | `neodos/console.nxl` | Console library NXL (slot 2, 0x1e080000) — readline, history, completion, progress bars |
| Serial log | `neodos/qemu_output.log` | Última sesión QEMU |

## NeoDOS LSP

`neodos-lsp/` — Language Server Protocol implementation for NeoDOS development.
Provides IDE features (completion, goto-def, hover, diagnostics, rename) for
kernel, driver, user-mode, and library code.

### LSP Features

| Feature | Handler | Description |
|---------|---------|-------------|
| Completion | `textDocument/completion` | Symbols + syscalls + shell commands + capabilities |
| Go to Definition | `textDocument/definition` | Navigate to symbol declarations |
| Find References | `textDocument/references` | All references to a symbol |
| Hover | `textDocument/hover` | Type signatures, docs, NeoDOS annotations |
| Diagnostics | `textDocument/publishDiagnostics` | Unbalanced delimiters, missing semicolons |
| Rename | `textDocument/rename` | Safe renaming with workspace edit |
| Document Symbols | `textDocument/documentSymbol` | Hierarchical file outline |

### MCP Tools (LSP bridge)

8 tools added to `neodos-mcp` for AI-level code analysis:

| Tool | Description |
|------|-------------|
| `lsp_list_symbols` | List symbols in a file/directory |
| `lsp_search_symbol` | Search symbols by name across the codebase |
| `lsp_get_syscalls` | List all syscalls with numbers and dispatch locations |
| `lsp_get_shell_commands` | List all shell commands with categories |
| `lsp_get_capabilities` | List all capability flags with bit values |
| `lsp_get_diagnostics` | Run basic source diagnostics on a .rs file |
| `lsp_get_driver_states` | List driver lifecycle states and transitions |
| `lsp_get_kernel_modules` | List kernel subsystems with file counts |

### Usage

Build:
```bash
cd neodos-lsp && cargo build --release
```

The binary runs as a stdio LSP server. Configure in your editor or use the
opencode LSP config (see opencode.json).

The MCP tools are available via `scripts/mcp-server.sh` — no LSP server needed
for AI-level queries.

### Integration

- `opencode.json` registers `neodos-lsp` for `.rs` files and `neodos-mcp`
  for AI-level analysis tools (includes all LSP-style tools: lsp_list_symbols,
  lsp_search_symbol, lsp_get_syscalls, etc.).
- The existing `neodos-mcp` tools (kernel, VFS, modules, ABI, LSP) complement
  the LSP with filesystem, build artifact, and architectural analysis.

---

## Networking Subsystem (B3.1/B3.2)

`src/net/` implements the TCP/IP networking stack for NeoDOS. Architecture follows
the NT model: the kernel exposes `\Device\Tcp` and `\Device\Udp` as device objects
in the Ob namespace. Sockets are ObType::Socket (18) objects with operations via
ob_set_info/ob_query_info.

### Module Structure

| Module | File | Description |
|--------|------|-------------|
| Types | `src/net/types.rs` | Core types: MacAddr, Ipv4Addr, SocketAddrV4, TcpState, SocketType, SocketDirection |
| Ethernet | `src/net/ethernet.rs` | Ethernet frame header (14 bytes), FCS computation |
| ARP | `src/net/arp.rs` | ARP protocol (64-entry cache, 300s timeout, static entries, request/reply) |
| IPv4 | `src/net/ipv4.rs` | IPv4 header (20 bytes), header checksum, packet building |
| ICMP | `src/net/icmp.rs` | ICMP echo request/reply (ping), checksum computation |
| UDP | `src/net/udp.rs` | UDP header (8 bytes), pseudo-header checksum |
| TCP | `src/net/tcp.rs` | TCP state machine (11 states), connection lifecycle, send/recv buffers |
| Socket | `src/net/socket.rs` | Socket manager, bind/connect/listen/send/recv/close, KWait wake |
| NIC | `src/net/nic.rs` | NetworkInterface trait, NicRegistry (4 slots), IP/next-hop management |
| e1000 | `src/net/e1000.rs` | Intel e1000 NIC driver (82540EM, 82543GC, 82545EM, 82574L) |

### ObType::Socket (18)

Added to the Object Manager. Created via `ob_create(ObType::Socket, path)` with attrs:
- bits 0-7: socket_type (1=TCP, 2=UDP, 3=Raw)
- bits 8-23: port number

### ObInfoClass additions

| Value | Class | Description |
|-------|-------|-------------|
| 17 | SocketInfo | Socket type, direction, local/remote addr |
| 18 | SocketAddr | Local and remote IP:port |
| 19 | TcpStatus | TCP state (0=Closed, 4=Established, etc.) |
| 20 | NicInfo | NIC list (id, MAC, IP, link status) |

### ObSetInfoClass additions

| Value | Class | Description |
|-------|-------|-------------|
| 18 | SocketConnect | Connect to remote (buf=ip[4]+port[2] big-endian) |
| 19 | SocketBind | Bind to local address |
| 20 | SocketListen | Start listening |
| 21 | SocketSend | Send data payload |
| 22 | SocketClose | Close socket |

### KWait integration

Network wait reasons added to KWait (ABI extended):
- `SocketRead { socket_id }` — blocked on socket data
- `SocketConnect { socket_id }` — waiting for connection completion
- `SocketAccept { socket_id }` — waiting for incoming connection

### NIC Initialization

At Phase 3.88 in the boot sequence, `net::init_networking()`:
1. Creates `\Device\Tcp` and `\Device\Udp` namespace entries
2. Probes PCI for e1000 NIC (bus scan for 8086:100E/1004/100F/10D3)
3. Initializes found NIC (MAC, ring buffers, RX/TX descriptors, MMIO)
4. Sets default IP (10.0.2.15) on first NIC
5. ARP cache starts accepting entries

### Network Packet Flow

```
NIC (e1000)
  → poll_packet() → 2048 byte buffer
    → ethernet::EthernetHeader parse
      → ARP (0x0806): cache insert (reply) or respond (request)
      → IPv4 (0x0800): Ipv4Header parse
        → ICMP (proto 1): echo request → build reply → send
        → TCP (proto 6): future
        → UDP (proto 17): future
```

### Tests (17)

| Test | Description |
|------|-------------|
| `net_mac_addr_basics` | MAC address format, broadcast detection |
| `net_ipv4_addr_basics` | IPv4 addr format, u32 conversion, network prefix |
| `net_ipv4_checksum` | Verify computed checksum = 0 for valid header |
| `net_arp_cache_insert_lookup` | ARP cache insert + lookup |
| `net_arp_cache_eviction` | Max 64 entries enforced |
| `net_arp_cache_static_survives_eviction` | Static entries survive LRU eviction |
| `net_tcp_state_machine_simple` | TCP state enum, is_connected() |
| `net_tcp_connection_lifecycle` | TCP alloc→bind→listen→close |
| `net_tcp_connect_and_close` | TCP connect to remote, close |
| `net_icmp_echo_reply_build` | Build ICMP echo reply, verify fields |
| `net_socket_manager_lifecycle` | Socket alloc/socket_count/free |
| `net_socket_bind_connect` | Socket local/remote addr, direction |
| `net_udp_header_checksum` | UDP header src/dst port, length |
| `net_socket_addr_fmt` | SocketAddrV4 Display format |
| `net_ipv4_classification` | Loopback, multicast, link-local detection |
| `net_nic_registry_empty` | Empty NIC registry state |

### ObSetInfoClass / ObInfoClass additions in AGENTS.md tables

For `src/object/types.rs`:
- ObInfoClass: SocketInfo(17), SocketAddr(18), TcpStatus(19), NicInfo(20)
- ObSetInfoClass: SocketConnect(18), SocketBind(19), SocketListen(20), SocketSend(21), SocketClose(22)
