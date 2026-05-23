# NeoDOS — AGENTS.md
## Versión Actual

v0.16.0

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

## Git workflow (testing primero)

**IMPORTANTE: nunca subir código sin testear antes.**

1. `cargo build` en `neodos-kernel/` — comprueba que compila
2. `python3 scripts/auto_test.py` — 99 kernel tests + 4 user-mode binaries
3. Solo si todo pasa: `git commit && git push`

**Cada vez que se complete una tarea:**
- Actualizar `docs/IMPROVEMENTS.md` (mover item a Completado con descripción)
- Actualizar `AGENTS.md` si es necesario (nuevas secciones, tablas de syscalls, comandos, etc.)
- Actualizar `docs/ARCHITECTURE.md`, `docs/KERNEL.md` u otros doc si la feature afecta al diseño
- Si se añade una syscall nueva: actualizar tabla de syscalls en `AGENTS.md` y `src/syscall.rs`
- Si se añade un comando del shell: actualizar `AGENTS.md` en la sección de comandos
- `git add -A && git commit -m "feat: ..." && git push`

## Two packages, no workspace

- `neodos-bootloader/` — UEFI app, target `x86_64-unknown-uefi`, produces `bootloader.efi`
- `neodos-kernel/` — freestanding kernel, target `x86_64-unknown-none`, produces `kernel.elf`

Each has its own `Cargo.toml`, `Cargo.lock`, `.gitignore`. No root workspace.

## Kernel quirks

- **Nightly** pinned in `rust-toolchain.toml` (needs `abi_x86_interrupt`).
- **Custom linker** via `kernel.ld` + `.cargo/config.toml`: `-Tkernel.ld`, `-melf_x86_64`, `-no-pie`, `relocation-model=static`, `rust-lld`.
- **Entry**: `_start` in `.text.entry` at `0x200000`, called `extern "sysv64" fn(&BootInfo) -> !` (RDI = `&BootInfo`).
- **Heap**: 16 MB @ `0x1000000`, uses `linked_list_allocator`. `Box`, `Vec`, `String` disponibles.
- **Profiles**: release with `opt-level=3`, `lto=true`, `debug=true`, `panic="abort"`.
- A shared `.cargo/config.toml` at `neodos/` adds extra linker flags (`-melf_x86_64`, `rust-lld`) for the kernel target only.

## Boot ABI

Bootloader loads ELF segments manually, calls `ExitBootServices` (memory map leaked via `forget`), jumps to kernel. `BootInfo` has: framebuffer info + raw memory map pointer/metadata.

## Code generation

`neodos-kernel/build.rs` parses `KBDUS.klc`/`KBDSP.klc` (UTF-16LE keyboard layouts) at build time into `$OUT_DIR/kbd_layout.rs` with scan code → ASCII tables. `src/drivers/keyboard.rs` includes it via `include!`. Two layouts: US (index 0), SP (index 1, default).

## Input system

Solo **PS/2** (IRQ1). `input.rs` tiene un ring-buffer lock-free de 1024 bytes, productor = IRQ1, consumidor = shell loop. Driver UHCI para USB no funcional en PIIX3.

## Adding a shell command

1. Create `src/shell/commands/<name>.rs` with an `impl DosShell` method.
2. Add `mod <name>;` to `src/shell/commands/mod.rs`.
3. Add a `CommandEntry` to `handler::COMMANDS` in `handler.rs`. Help text is automatic.

## AHCI Driver

- **DMA polling** por puerto, buffers estáticos separados por puerto lógico
- **ATA**: READ/WRITE DMA EXT (0x25/0x35), multi-sector hasta 8 sectores (4KB)
- **ATAPI**: PACKET command (0xA0) con DMA, READ_10 CDB, sectores de 2048 bytes
- **Por puerto**: DeviceType::Ata / DeviceType::Atapi
- **Port reset**: ciclo DET vía SCTL para recuperación de errores
- **PRDT**: hasta 8 entradas scatter-gather
- Per-port buffers: `PORT_CMD_LIST[]`, `PORT_RECV_FIS[]`, `PORT_CMD_TABLE[]`, `PORT_DMA_BUF[]`

## Un disco GPT unificado

El sistema usa una **sola imagen de disco con tabla GPT** que contiene dos particiones:

| Partición | Tipo | LBA | Contenido |
|-----------|------|-----|-----------|
| 1 | ESP (FAT32) | 2048–206847 | bootloader.efi + kernel.elf |
| 2 | NeoDOS FS | 206848–227327 | Sistema de archivos NeoDOS |

El kernel parsea la GPT al arrancar mediante `drivers/gpt.rs`, busca la partición de tipo
`EBD0A0A2-B9E5-4433-87C0-68B6B72699C7`, y ajusta `base_lba` en el driver ATA para que
el FS vea el superbloque en LBA 0 relativo a la partición.

### ATA bus-master DMA

Kernel scans PCI bus 0 at boot for the IDE controller (class 0x01, subclass 0x01) with bus-master capability (prog-if bit 7). `drivers/pci.rs` uses I/O ports 0xCF8/0xCFC.

BAR4 gives the bus-master I/O base. Bus-master bit enabled in PCI command register. Two page-aligned (4KB) static buffers for PRDT + DMA data. Polling-based (no IRQ). Methods `read_dma()`/`write_dma()` support up to 8 sectors (4 KB) per call. Existing PIO methods unchanged.

The ATA driver adds `base_lba` to all logical LBAs before sending them to the disk, so the
NeoDOS FS code never needs to know about partition offsets. The FAT32 driver reads from
the master drive using absolute LBAs (no `base_lba`).

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
```

## User-mode process lifecycle

`cmd_run` in `shell/commands/run.rs` loads a flat binary to `USER_BASE` (0x400000) and calls `execute_usermode()`.

`execute_usermode()` in `usermode.rs` saves the kernel RSP/RIP into `EXIT_RSP`/`EXIT_RIP` statics, then IRETQs to Ring 3. The function is **not** `options(noreturn)` — it can return.

On `sys_exit` (INT 0x80, RAX=0): `syscall_dispatch` marks the process `Terminated` in the scheduler, then the `syscall_handler_asm` trampoline detects RAX==0 and jumps to `exit_to_kernel`, which restores `EXIT_RSP`/`EXIT_RIP`. Control returns to `execute_usermode`'s caller (`cmd_run`), which prints "Process exited." and shows the shell prompt.

Key files: `usermode.rs` (trampoline & context save/restore), `idt.rs` (syscall_handler_asm exit path), `syscall.rs` (dispatch & Terminated marking).

## Shell: TAB autocomplete + history

El shell tiene autocompletado con **TAB** (`shell.rs:try_complete`):
- **Primera palabra**: completa comandos built-in (HELP, DIR, etc.) y `.BIN` del PATH
- **Argumentos**: completa nombres de archivo/directorio desde el directorio actual
- **Rutas**: soporta rutas con separador (`DIR \\BIN\\TE` → `\\BIN\\TEST`)
- Match único: reemplaza y añade espacio (comandos)
- Múltiples matches: lista todos y redibuja prompt + línea

El shell tiene historial de comandos con **↑/↓** (`shell.rs`, `keyboard.rs`):
- Buffer circular de 32 entradas
- Las flechas se emiten como bytes sentinela 0x01 (up) / 0x02 (down) desde el driver PS/2
- `history` se almacena como `Vec<String>` en `DosShell`, se inicializa en `new()`

## Shell: DEL, REN, RD

Comandos de gestión de archivos que operan via VFS (`vfs.rs`):

| Comando | Descripción | VFS method |
|---------|-------------|------------|
| `DEL file` | Elimina archivo (libera bloques, inodo, marca entry 0xE5) | `vfs.remove_file()` |
| `REN old new` | Renombra archivo en el mismo directorio | `vfs.rename()` |
| `RD dir` | Elimina directorio vacío | `vfs.remove_dir()` |

Métodos del trait `FileSystem`: `remove_file()`, `remove_dir()`, `rename()` — con default `NotImplemented`.

## Syscall Table (INT 0x80)

### Architecture

- `SyscallNum` enum (`from_u64()`) — maps RAX values to typed dispatch arms
- `SyscallError` enum (16 codes) — returned as negative `u64` via `err_to_u64()` (e.g., `NoEnt=2` → `0xFFFF_FFFF_FFFF_FFFE`)
- `syserr!` macro — `syserr!(NoEnt)` expands to `return err_to_u64(SyscallError::NoEnt)`
- `validate_abi()` — called at boot from `main.rs`, asserts all syscall numbers have handlers and error encoding is correct
- Return convention: `≥ 0` success, `< 0` error (user checks `cmp rax, -1`)

Calling convention: RAX = syscall number, RBX = arg0, RCX = arg1, RDX = arg2. Return in RAX.

| RAX | Syscall | Args | Descripción |
|-----|---------|------|-------------|
| 0 | `sys_exit` | RBX=code | Termina proceso |
| 1 | `sys_write` | RBX=ptr, RCX=len | Escribe a consola |
| 2 | `sys_yield` | — | Cede CPU |
| 3 | `sys_getpid` | — | Retorna PID actual |
| 4 | `sys_read` | RBX=fd, RCX=buf, RDX=count | Lee de stdin |
| 9 | `sys_waitpid` | RBX=pid | Espera proceso hijo |
| 10 | `sys_open` | RBX=path_ptr, RCX=flags | Abre archivo → inode |
| 11 | `sys_readfile` | RBX=inode, RCX=buf, RDX=count | Lee desde archivo |
| 12 | `sys_writefile` | RBX=inode, RCX=buf, RDX=count | Escribe a archivo |
| 13 | `sys_close` | RBX=fd | No-op (placeholder) |
| 18 | `sys_brk` | RBX=new_break | Ajusta program break (paginación bajo demanda) |
| 19 | `sys_mmap` | RBX=size | Asigna memoria contigua zero-filled en heap |

## ELF64 Loader

`src/elf.rs` — Minimal ELF64 loader for user-mode binaries.

- Validates ELF magic (`\x7fELF`), class (64-bit), endianness (LSB), machine (x86-64), type (EXEC or DYN)
- Parses program headers; loads `PT_LOAD` segments at their specified virtual addresses
- Zero-fills `.bss` (`p_memsz - p_filesz`)
- Entry point returned via `ElfLoadResult { entry: u64 }`
- Backward compatible: `cmd_run` detects ELF vs flat binary by checking the first 4 bytes
- `hello.elf` test binary generated by `userbin/generate_hello_elf.py`
- 7 kernel tests registered in `testing.rs` via `register_elf_tests()`

## User-mode binaries

Ubicados en `userbin/`. Generados por scripts Python (no requieren NASM).

| Binario | Generador | Tamaño | Prueba |
|---------|-----------|--------|--------|
| `hello.bin` | `generate_hello.py` | 232 B | sys_write, sys_getpid, sys_yield, sys_exit |
| `systest.bin` | `generate_systest.py` | 247 B | Misma estructura que hello.bin + mensajes v0.10.4 |

User window (code+stack): `0x400000` .. `0x800000` (4 MB, 32 slots de 128 KB)
User heap (demand-paged 4 KB): `0x10000000` .. `0x12000000` (32 MB, 16 slots de 2 MB)
Binarios flat cargados en `0x400000`.

## In-Kernel Test Framework

171 tests en 16 suites. Registrados en `testing.rs`, ejecutados por el comando `test` del shell.

| Suite | Tests | Descripción |
|-------|-------|-------------|
| Environment | 6 | Variables de entorno |
| Input | 5 | Input buffer (ring buffer) |
| Keyboard | 5 | UTF-8 encoding, compose keys |
| Process | 3 | Process struct, state transitions |
| UTF-8 | 6 | Validación UTF-8 |
| Allocator | 8 | Box, Vec, String |
| Sync | 4 | Atomic flags (NEED_RESCHED) |
| NeoFS | 75 | Inode metadata, permissions, timestamps, block count, DOS attrs, serialization, stress, corruption, rendering |
| NEM | 14 | NEM test driver format parsing (header, types, edge cases) |
| ELF | 7 | ELF64 loader: header validation, segment loading, edge cases |
| Event Bus | 9 | Event: creation, push/pop, ordering, overflow, IDs, handler register/dispatch, type filter, unregister, empty queue |
| Driver State | 21 | Driver certification pipeline: 7-state lifecycle, transition matrix, certify_and_activate(), last_error tracking, inactive_reason debug |
| Stress | 8 | Stress: sched, syscall, mem |

Comando `test`:
1. Ejecuta `testing::run_all()` (171 tests kernel)
2. Si pasan, ejecuta `run SYSTEST.BIN`, `run FILETEST.BIN`, `run ALLTEST.BIN` (user-mode)

## NEM Module

`src/nem/mod.rs` — NeoDOS Test Driver Format v1 parser. Minimal 32-byte header + raw x86-64 code section.

- Types: `NemDriverType::Null|Echo|Lifecycle|Mutation|Fault|Burst` (0-5)
- Function `parse_nem(data: &[u8]) -> Option<ParsedNem>` — zero-copy, no alloc
- `build_valid_nem()` — generates valid NEM binaries for testing
- 14 parse tests registered in `testing.rs` via `register_nem_tests()`

## Event Bus v1

`src/eventbus/mod.rs` — Centralized event routing layer.

| Concept | Description |
|---------|-------------|
| **Event** | `#[repr(C)]` struct (56 bytes): `event_id`, `event_type`, `source`, `timestamp`, `device_id`, `data0`, `data1`, `flags` |
| **Event types** | 11 named constants: TIMER_TICK, KEYBOARD_INPUT, SERIAL_DATA, DISK_IO_COMPLETE, PROCESS_EXIT, DRIVER_LOADED, DRIVER_CRASH, POLICY_VIOLATION, FS_MOUNTED, USER(0x1000+) |
| **Event sources** | SOURCE_HAL, SOURCE_DRIVER, SOURCE_KERNEL, SOURCE_USERLAND |
| **Queue** | Lock-free SPSC ring buffer (64 slots). Pushed from IRQ context, popped from scheduler context |
| **Callbacks** | `register_handler(event_type, callback, name)` — max 32 handlers |
| **Dispatch** | `dispatch_one()`/`dispatch_pending()` — outside IRQ context, controlled by scheduler |
| **IRQ integration** | TimerTick pushed from PIT IRQ0, KeyboardInput pushed from PS/2 IRQ1 |
| **Scheduler integration** | `EVENT_BUS.dispatch_pending()` in idle loop |
| **Isolation** | No driver execution in IRQ context. No recursive dispatch. Events immutable after enqueue |

Rules: events are queued deterministically, dispatched by scheduler, never executed in IRQ context.

See `docs/NEM_SPEC.md` for full format spec. Test driver binaries generated by `userbin/nem_builder.py`.

## Driver Certification Pipeline v1

`src/drivers/driver_runtime.rs` — Strict driver lifecycle state machine.

### Lifecycle States (7-state)

```rust
DriverState::Loaded      // binary loaded, not verified
DriverState::Initialized // driver_init() executed, process spawned
DriverState::Registered  // registry committed, Event Bus notified
DriverState::Bound       // bound to Event Bus / Device
DriverState::Active      // fully operational, certified
DriverState::Faulted     // runtime failure (recoverable? → Unloaded)
DriverState::Unloaded    // removed from system (terminal)
```

### Transition Rules

Only these transitions are valid:
```
Loaded → Initialized → Registered → Bound → Active
Any → Faulted
Any → Unloaded
All others → ERROR (TransitionError)
```

### Error Tracking

Each `DriverInstance` has:
- `last_error: u32` — error code from `ERR_*` constants
- `certification_step: u8` — which pipeline step failed (`PipelineStep`)

Error codes: `ERR_NONE=0`, `ERR_INIT_FAILED`, `ERR_REGISTRATION_FAILED`, `ERR_BIND_FAILED`, `ERR_SANDBOX_REJECTED`, `ERR_CERTIFICATION_FAILED`, `ERR_OUT_OF_MEMORY`, `ERR_POLICY_VIOLATION`, `ERR_LOAD_FAILED`.

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

All data is read-only from NeoFS + runtime registry. No driver execution.

## Dependencias

```bash
python3 scripts/check_deps.py        # Validate subsystem dependency rules
```

Ver `docs/KERNEL_SUBSYSTEMS.md` para la arquitectura completa de subsistemas.

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

Ver `docs/KERNEL_SUBSYSTEMS.md` para la especificación completa.

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

## Device Model + HAL Binding Layer v0.3

`src/devices/mod.rs` — Controlled hardware exposure layer. All driver hardware access is mediated through HAL ABI v0.3.

| Concept | Description |
|---------|-------------|
| **Device** | Logical hardware entity: `id`, `DeviceType`, `DeviceClass`, `DeviceState`, capabilities (R/W/I/D/M), optional IRQ vector |
| **DeviceRegistry** | 32-slot thread-safe registry with binding table. Boot-time populated. Locked via `spin::Mutex` |
| **DeviceHandle** | Opaque capability-limited handle given to drivers on `bind()` — no raw hardware access |
| **HAL Binding Layer** | `device_read/write/register_irq/ack_irq/query_status` — stubs ready for driver migration |
| **Boot-time devices** | 5 registered: `pit` (timer, IRQ32), `com1` (serial), `ps2kbd` (keyboard, IRQ33), `framebuffer`, `pci` (configuration space) |
| **DEVICES command** | Updated to show device model table (`ID`, `TYPE`, `CLASS`, `STATE`, `CAPS`, `BIND`, `NAME`) + TSR modules |

Rules: drivers never touch hardware directly. All access goes through `driver → HAL Binding Layer → HAL ABI v0.3 → hardware`. No raw port I/O, MMIO, or IRQ vector manipulation allowed.

## Artifacts generados

| Archivo | Path | Descripción |
|---------|------|-------------|
| Bootloader UEFI | `neodos/bootloader.efi` | v0.10.5 |
| Kernel ELF | `neodos/kernel.elf` | v0.14.0 |
| Disco GPT unificado | `neodos/disk_image.img` | 112 MB (ESP + NeoDOS FS) |
| NeoDOS FS image (temp) | `neodos/scripts/neodos_image.img` | 10 MB, regenerado en build |
| GPT builder | `neodos/scripts/create_gpt_image.py` | Combina ESP + NeoDOS en GPT |
| HAL ABI v0.3 | `neodos/neodos-kernel/src/hal/` | 7 módulos: cpu, io, mem, irq, time + x64 backend |
| Serial log | `neodos/qemu_output.log` | Última sesión QEMU |
