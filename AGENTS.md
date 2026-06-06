# NeoDOS — AGENTS.md
## Versión Actual

v0.28.0

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

**Note**: KVM works with `-machine pc` (PIIX3). Q35 machine type produces
`KVM: entry failed, hardware error 0x0` with OVMF on some hosts. Use TCG for Q35.

## Git workflow (testing primero)

**IMPORTANTE: nunca subir código sin testear antes.**

1. `cargo build` en `neodos-kernel/` — comprueba que compila
2. `python3 scripts/auto_test.py` — 320 kernel tests + 5 user-mode binaries
3. Solo si todo pasa: `git commit && git push`

**Cada vez que se complete una tarea:**
- Actualizar `docs/IMPROVEMENTS.md` (mover item a Completado con descripción)
- Actualizar `AGENTS.md` si es necesario (nuevas secciones, tablas de syscalls, comandos, etc.)
- Actualizar `docs/ARCHITECTURE.md`, `docs/KERNEL.md` u otros doc si la feature afecta al diseño
- Si se añade una syscall nueva: actualizar tabla de syscalls en `AGENTS.md` y `src/syscall.rs`
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
| Syscall | `src/syscall.rs` | Raw `int 0x80` wrappers (exit, write, read, open, readfile, writefile, close, yield, getpid, brk, mmap, munmap). Error constants (`EINVAL`, `ENOENT`, etc.). All return `Result<T, i64>` |
| IO | `src/io.rs` | `Stdout`/`Stdin`/`Stderr` structs with `write()`/`read().` `core::fmt::Write` impls. Stack-buffered `_print()`/`_eprint()` (1024 bytes) |
| FS | `src/fs.rs` | `File::open(path)` → handle, `File::read(buf)`, `File::write(buf)` |
| Mem | `src/mem.rs` | `brk()`, `sbrk()`, `mmap()`, `munmap()`. Constants: `PROT_READ`, `PROT_WRITE`, `MAP_ANONYMOUS` |
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
| `init_default()` | Replicates legacy layout: kernel_image, user_window, kernel_heap, user_heap, dll_region, mmap_region, driver_iso |
| Overlap detection | `panic!()` on any overlapping reservation |

Boot-time `validate_layout_consistency()` asserts layout-registered bases match hardcoded constants.

## Boot ABI

Bootloader loads ELF segments manually, calls `ExitBootServices` (memory map leaked via `forget`), jumps to kernel. `BootInfo` has: framebuffer info, raw memory map pointer/metadata, and ACPI RSDP address (`acpi_rsdp_addr`).

## Code generation

`neodos/drivers/ps2kbd/build.rs` parses `KBDUS.klc`/`KBDSP.klc` (UTF-16LE keyboard layouts) at build time into `$OUT_DIR/kbd_layout.rs` with scan code → ASCII tables. Copied to `neodos-kernel/src/drivers/nem/drivers/kbd_layout.rs` for reference. Two layouts: US (index 0), SP (index 1, default). Layout switching at runtime via Event Bus (`EVENT_KEYB_LAYOUT` type 9) sent from the `KEYB US|SP` shell command to the NEM ps2kbd driver.

## Input system

Solo **PS/2** (IRQ1). `input.rs` tiene un ring-buffer lock-free de 1024 bytes, productor = IRQ1, consumidor = shell loop. Driver UHCI para USB no funcional en PIIX3.

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

`src/slab.rs` — Efficient fixed-size allocation for kernel objects.

| Concept | Description |
|---------|-------------|
| **Size classes** | 9 power-of-2 caches: 8, 16, 32, 64, 128, 256, 512, 1024, 2048 bytes |
| **Slab pages** | 4 KB pages from `hal::alloc_page()` (physical frames) |
| **Page header** | 32-byte `#[repr(C, align(16))]` header at offset 0: magic "SLAB" (u32), slot_size (u16), capacity (u16), allocated (u16), free_head (u16), next pointer |
| **Free list** | Inline `u16` indices stored in each free slot — O(1) alloc and free |
| **Alignment** | Minimum 16 bytes per slot (from header alignment) |
| **Fallback** | `linked_list_allocator::LockedHeap` for objects >2048 bytes or alignment >16 |
| **Isolation** | Heap region (0x01000000..0x02000000) reserved in frame bitmap to prevent slab/heap overlap |
| **Global allocator** | `SlabAllocator` implements `GlobalAlloc`, set as `#[global_allocator]` in `allocator.rs` |
| **Locking** | Single `spin::Mutex` protects all 9 caches; `LockedHeap` has its own internal Mutex |
| **Tests** | 9 tests: per-size alloc/free, multi-page stress, mix sizes, large fallback, free-reuse |

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

**Archivos:** `arch/x64/paging.rs` (mmap_alloc_page, mmap_free_range, handle_mmap_page_fault, load_file_mmap_page), `scheduler.rs` (MmapRegion, VMA list per-process), `syscall.rs` (sys_mmap/sys_munmap dispatch), `arch/x64/idt.rs` (page_fault_handler → handle_mmap_page_fault)

Región dedicada: `0x20000000..0x22000000` (32 MB), dividida en páginas 4 KB durante el arranque (`init_mmap_demand_paging`).

- **MmapRegion**: base, len, prot (1=R, 2=W), flags (bit0=1 anonymous, 0=file-backed), drive, inode, file_size
- **sys_mmap (RAX=19)**: RBX=hint, RCX=len, RDX=prot, R8=flags, R9=file_handle — solo registra VMA, no aloca páginas
- **sys_munmap (RAX=20)**: RBX=addr, RCX=len — libera páginas y elimina VMA
- **Anonymous**: page fault → allocate_frame() + map USER_ACCESSIBLE
- **File-backed**: page fault → with_vfs → vfs.read() a frame identity-mapped → map USER_ACCESSIBLE
- **is_user_ptr_valid()**: extendido para cubrir regiones mmap
- **sys_exit**: libera todas las regiones mmap del proceso

## User-mode process lifecycle

`cmd_run` in `shell/commands/run.rs` loads a flat binary to `USER_BASE` (0x400000) and calls `execute_usermode()`.

`execute_usermode()` in `usermode.rs` saves the kernel RSP/RIP into `EXIT_RSP`/`EXIT_RIP` statics, then IRETQs to Ring 3. The function is **not** `options(noreturn)` — it can return.

On `sys_exit` (INT 0x80, RAX=0): `syscall_dispatch` frees all external resources (user slot, heap pages, mmap regions, pipe refcounts), calls `request_exit_to_kernel()` (sets `EXIT_NOW=1`) inside `without_interrupts`, and marks the process `Terminated` in the scheduler, then the `syscall_handler_asm` trampoline detects RAX==0 and jumps to `exit_to_kernel`, which restores `EXIT_RSP`/`EXIT_RIP` plus all callee-saved registers (rbx, r12-r15, rbp) — preventing user-mode register clobber from corrupting shell local variables. Control returns to `execute_usermode`'s caller (`cmd_run`), which calls `scheduler::cleanup_terminated_process(pid)` to recycle the scheduler slot and free the kernel stack (`Box<AlignedKStack>`). The `KILL` command (`kill_pid()`) does full cleanup (heap, mmap, pipes, user slot, kernel stack) and recycles the slot immediately. The `sys_waitpid` syscall recycles the waited-for process's slot after detecting it is `Terminated`.

Key files: `usermode.rs` (trampoline & context save/restore), `idt.rs` (syscall_handler_asm exit path), `syscall.rs` (dispatch & Terminated marking), `scheduler.rs` (recycle_terminated, cleanup_terminated_process, kill_pid).

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

## Default File Permissions by Context

Al crear un archivo o directorio en NeoFS, se asignan permisos RWXSD según el tipo de archivo (extensión). Los permisos se almacenan en el campo `mode` del inodo y coexisten con `MODE_FILE`/`MODE_DIR`.

| Tipo | Extensiones | Permisos (RWXSD) | Uso |
|------|-------------|------------------|-----|
| Ejecutable | `.BIN`, `.COM`, `.EXE` | `R-X--` | Leer + ejecutar, protección contra modificación accidental |
| Driver | `.NEM` | `R----` | Solo lectura, archivos críticos del sistema |
| Librería | `.DLL` | `R-X--` | Cargar desde user-mode, no modificar |
| Script | `.BAT`, `.CMD` | `R-X--` | Leer + ejecutar por el shell |
| Sistema | `.SYS` | `R----` | Configuración crítica del sistema |
| Configuración | `.CFG`, `.INI` | `RW---` | Leer y modificar |
| Texto | `.TXT`, `.MD`, `.LOG`, `.ASC` | `RW---` | Edición normal |
| Otros | cualquier otra extensión | `RW---` | Por defecto |
| Directorios | — | `RWXD-` | Permisos completos sobre el directorio |

La asignación se realiza en `NeoDosFs::create_file_at()` (vía `default_perms_for_filename()`) y `create_directory_at()`. El script `create_neodos_image.py` aplica los mismos criterios al generar la imagen inicial del FS.

## Shell: FSCK

Comando de verificación de integridad del sistema de archivos NeoDOS:

| Comando | Descripción |
|---------|-------------|
| `FSCK [drive:] [/F]` | Verifica integridad del FS. Sin /F: solo comprueba. Con /F: repara errores |

Checks: superblock (magic, block_size, num_blocks, label), inode table (mode, inode_num mismatch, block pointers, cross-links), directory tree walk (orphans, dangling entries, entry-type vs mode mismatches). 6 unit tests.

## Shell: LOADLIB

Comando para cargar shared libraries (DLLs) desde el filesystem:

| Comando | Descripción |
|---------|-------------|
| `LOADLIB <path>` | Carga un DLL en un slot libre de la región de DLLs |

El DLL se carga en la región `0x1e000000..0x1e200000` (8 slots de 256 KB cada uno). El kernel parsea el ELF, marca las páginas como USER_ACCESSIBLE (read-only), y devuelve la dirección base. La tabla de exportaciones del DLL queda accessible en la dirección base.

DLLs disponibles:
- `libneodos.dll` — Librería estándar (slot 0, `0x1e000000`), cargada automáticamente en boot
- `libmath.dll` — Librería de matemáticas (slot 1, `0x1e040000`), carga manual con `LOADLIB C:\SYSTEM\LIB\LIBMATH.DLL`

Para usar desde user-mode: llamar a `libneodos::loadlib(path)` que invoca `sys_loadlib` (RAX=21) y devuelve la dirección base del DLL.

## Syscall Table (INT 0x80)

### Architecture

- `SyscallNum` enum (`from_u64()`) — maps RAX values to typed dispatch arms
- `SyscallError` enum (16 codes) — returned as negative `u64` via `err_to_u64()` (e.g., `NoEnt=2` → `0xFFFF_FFFF_FFFF_FFFE`)
- `syserr!` macro — `syserr!(NoEnt)` expands to `return err_to_u64(SyscallError::NoEnt)`
- `validate_abi()` — called at boot from `main.rs`, asserts all syscall numbers have handlers and error encoding is correct
- Return convention: `≥ 0` success, `< 0` error (user checks `cmp rax, -1`)

Calling convention: RAX = syscall number, RBX = arg0, RCX = arg1, RDX = arg2, R8 = arg3, R9 = arg4. Return in RAX.

| RAX | Syscall | Args | Descripción |
|-----|---------|------|-------------|
| 0 | `sys_exit` | RBX=code | Termina proceso |
| 1 | `sys_write` | RBX=fd, RCX=ptr, RDX=len | Escribe a fd (1=consola, pipe writer) |
| 2 | `sys_yield` | — | Cede CPU |
| 3 | `sys_getpid` | — | Retorna PID actual |
| 4 | `sys_read` | RBX=fd, RCX=buf, RDX=count | Lee de fd (0=stdin, pipe reader); bloquea con -EAGAIN |
| 5 | `sys_pipe` | RBX=fds_ptr | Crea pipe, escribe [read_fd, write_fd] en fds_ptr |
| 6 | `sys_dup2` | RBX=old_fd, RCX=new_fd | Duplica old_fd a new_fd (redirección) |
| 9 | `sys_waitpid` | RBX=pid | Espera proceso hijo |
| 10 | `sys_open` | RBX=path_ptr, RCX=flags | Abre archivo → fd (handle index 0-15) |
| 11 | `sys_readfile` | RBX=fd, RCX=buf, RDX=count | Lee desde archivo (usa offset del handle) |
| 12 | `sys_writefile` | RBX=fd, RCX=buf, RDX=count | Escribe a archivo (usa offset del handle) |
| 13 | `sys_close` | RBX=fd | Cierra handle (pipe, file, device, event) |
| 16 | `sys_chdir` | RBX=path_ptr | Cambia directorio actual |
| 17 | `sys_getcwd` | RBX=buf, RCX=len | Obtiene directorio actual |
| 18 | `sys_brk` | RBX=new_break | Ajusta program break (paginación bajo demanda) |
| 19 | `sys_mmap` | RBX=hint, RCX=len, RDX=prot, R8=flags, R9=fd | Mapeo lazy: anónimo (flags=1) o file-backed (flags=0, R9=fd) |
| 20 | `sys_munmap` | RBX=addr, RCX=len | Libera mapeo mmap |
| 21 | `sys_loadlib` | RBX=path_ptr | Carga un DLL desde NeoFS en un slot libre de la región de DLLs |

## IPC / Pipes

`src/pipe.rs` — Pipe IPC implementation for inter-process communication.

### Pipe Manager
- **16 static pipe buffers** of 4 KB each, protected by `spin::Mutex`
- Reference-counted: auto-freed when all reader/writer fds are closed
- `sys_pipe` allocates a pipe, returns two fds (reader + writer)
- `sys_close` on a pipe fd decrements refcount; pipe freed when refs reach 0
- `sys_dup2` copies an fd to another slot (increments refcount for pipe fds)

### Per-Process Handle Table
- `Process.handle_table: HandleTable` — `Vec<HandleEntry>`-backed, grows dynamically, unlimited capacity
- `HandleEntry` types: `Closed`, `Stdin`, `Stdout`, `Stderr`, `PipeReader(id)`, `PipeWriter(id)`, `File(drive, inode, offset)`, `Device(id)`, `Event(type)`
- File handles carry a per-open `offset` cursor for independent read/write positioning
- fd 0 = stdin (keyboard), fd 1 = stdout (console), fd 2 = stderr (console)
- fds 3+ available for pipes/files/devices/events (unlimited, grows dynamically)
- Default table for Ring 3 processes; `closed_handle_table()` for Ring 0
- `sys_exit` iterates handle table and cleans up all resource types (pipes decrement refcount, files closed cleanly)

### Blocking Reads
- When a process reads from an empty pipe with write end open:
  1. Process state set to `Blocked { waiting_for: 0xFFFF_0000 | pipe_id }`
  2. `NEED_RESCHED` flag set
  3. `syscall_dispatch` returns `-EAGAIN` to user space
  4. Assembly resched picks a different process
- On pipe write: `wake_pipe_readers()` scans scheduler processes, sets Blocked→Ready
- When woken process runs: user code retries `read()` syscall (handles -EAGAIN)

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
- `wake_pipe_readers()` in `pipe.rs` iterates scheduler processes via `Scheduler::processes`
- `block_current_for_pipe()` sets current process to `Blocked` + sets `NEED_RESCHED`

## Priority Scheduler (A2)

`src/scheduler.rs` — Planificador prioritario con time-slicing dinámico y aging.

### Priority Levels
| Nivel | Constante | Time Slice | Descripción |
|-------|-----------|-----------|-------------|
| 0 | `PRIORITY_HIGH` | 400 ticks | Procesos críticos del sistema |
| 1 | `PRIORITY_ABOVE_NORMAL` | 200 ticks | Procesos importantes de usuario |
| 2 | `PRIORITY_NORMAL` | 100 ticks | Prioridad por defecto (nuevos procesos) |
| 3 | `PRIORITY_IDLE` | 50 ticks | Background, solo se ejecuta si no hay nada más |

### Algorithm
- **schedule()**: escanea por nivel de prioridad (HIGH→IDLE), round-robin dentro del mismo nivel
- **on_timer_tick()**: decrementa `time_slice_remaining` cada tick; al expirar marca Ready + `NEED_RESCHED`
- **sys_yield**: Running→Ready + resetea time slice + fuerza re-schedule
- **Preemption from Ring 3**: timer handler detecta CS=0x1B (user mode), guarda RSP, llama schedule(), cambia TSS.RSP0
- **Aging** (cada 100 ticks): boostea prioridad si un proceso Ready no se ha ejecutado en >= 1000 ticks

### Implementation
- `Process` struct: `priority` (u8), `time_slice_remaining` (u16), `ticks_since_scheduled` (u64)
- `timer_handler_inner`: lee CS del stack frame, solo preemptea si interrumpió Ring 3
- Afecta solo procesos user-mode (Ring 3); el shell corre en Ring 0 y no pasa por schedule()
- 7 nuevos tests de scheduler: prioridad, round-robin, time-slice, aging

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
| `test.bin` | Rust `userbin/test/` | ~21 KB | libmath.dll self-test: load, symbol resolution, arithmetic, edge cases, stress (1M iter), determinism |

User window (code+stack): `0x400000` .. `0x800000` (4 MB, 32 slots de 128 KB)
User heap (demand-paged 4 KB): `0x10000000` .. `0x12000000` (32 MB, 16 slots de 2 MB)
Binarios flat cargados en `0x400000`.

## Async I/O (IRP System, X6)

`src/irp/mod.rs` — Unified I/O Request Packet model for all kernel block operations.

| Concept | Description |
|---------|-------------|
| **IRP struct** | `#[repr(C)]` with `IrpOp` (Read/Write/Flush/IoCtl), buffer ptr + len, LBA + count, `IrpStatus` (Pending/Completed/Error), callback + ctx, chain_next, waiting_pid |
| **Global pool** | 64 slots protected by `Spin::Mutex`, sequential IDs via `AtomicU32`. `irp_alloc()`/`irp_free()`/`irp_get_params()` — last returns a snapshot to avoid double-lock deadlock |
| **IrpQueue** | Per-device FIFO ring buffer (32 entries) for queuing async operations. `push()`, `pop()`, `peek()`, `len()` |
| **Completion** | `irp_complete(id, status)` — sets status, wakes waiter (`irp_wake_waiter` via `IRP_WAIT_MAGIC`), handles chaining, dispatches callback via `WORK_QUEUE.push_high()` using `Box<IrpCbDispatch>` |
| **Scheduler** | `irp_block_current(id)` sets `ProcessState::Blocked { waiting_for: IRP_WAIT_MAGIC \| id }`. `irp_complete` wakes via `irp_wake_waiter()` — same pattern as pipe blocking |
| **Chaining** | `chain_next: Option<IrpId>` — auto-cleared on complete. Device driver responsible for submitting chained IRPs |
| **Sync helpers** | `irp_sync_read()`/`irp_sync_write()` — allocate IRP, submit, block, free. For code that wants synchronous IRP path |
| **BlockDevice** | Trait extended with `submit_irp(irp_id)` and `poll_irp(irp_id)`. All 5 implementors (RamDisk, BootAta, AhciDriver, NvmeDriver, NemBlockDevice) implement `submit_irp` via `irp_get_params()` → sync I/O → `irp_complete_result()` |
| **Tests** | 11 tests: alloc/free, status transitions, error codes, unique IDs, slot reuse, queue FIFO/wraparound, callback dispatch via work queue, flush/ioctl ops, params extraction |

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

329 tests en 38 suites. Registrados en `testing.rs`, ejecutados por el comando `test` del shell.

| Suite | Tests | Descripción |
|-------|-------|-------------|
| Environment | 6 | Variables de entorno |
| Input | 5 | Input buffer (ring buffer) |
| Keyboard | 5 | UTF-8 encoding, compose keys |
| Scheduler | 7 | Priority scheduling, time-slice, round-robin, aging |
| UTF-8 | 6 | Validación UTF-8 |
| Allocator | 8 | Box, Vec, String |
| Sync | 4 | Atomic flags (NEED_RESCHED) |
| NeoFS | 75 | Inode metadata, permissions, timestamps, block count, DOS attrs, serialization, stress, corruption, rendering |
| NEM | 23 | NEM v1+v2 driver format parsing (header, types, v2 ABI fields, categories) |
| ELF | 7 | ELF64 loader: header validation, segment loading, edge cases |
| Capability | 11 | X3 Capability flags, CapabilitySet, category defaults, check/enforce, escalation policy |
| Event Bus | 17 | Unified v2: priority queues, subscription filters (type/source/device), dynamic payload, backpressure, 17 tests |
| Slab | 9 | Slab allocator: per-size alloc/free, multi-page, realloc fallback, reuse |
| Driver State | 21 | Driver certification pipeline: 7-state lifecycle, transition matrix, certify_and_activate(), last_error tracking, inactive_reason debug |
| Pipe | 13 | IPC pipes: alloc/free, write/read, EOF, EPIPE, blocking, fd table |
| Mmap | 6 | MmapRegion struct, flags, address bounds, VMA add/remove |
| FSCK | 6 | Inode validation helpers, block pointer logic, mode checks, range checks |
| Isolation | 12 | X4 Driver Isolation Layer: constants, bounds, alloc/free, driver_id lookup, layout, pointer validation, overflow, max slots, str ptr, mode for category, mode string |
| Boot Loader | 8 | Boot driver loader: scan, load, init, activate, unload, category ordering |
| ABI Negotiation | 10 | ABI version negotiation, window overlap, compatibility warnings, edge cases |
| Dependency | 13 | Dependency graph, topological sort, cycle detection, symbol extraction, case-insensitive |
| Storage Ref | 14 | Reference storage driver: entrypoints, lifecycle, R/W, geometry, error handling |
| IRP | 11 | Async I/O: IRP alloc/free, completion/error, pool reuse, queue FIFO/wraparound, callback dispatch, flush/ioctl ops, get_params |
| PS/2 Kbd Ref | 10 | Reference PS/2 keyboard driver: entrypoints, lifecycle, key events, error handling |
| Framebuffer Ref | 8 | Reference framebuffer driver: entrypoints, lifecycle, clear/pixel/scroll, error handling |
| KOBJ | 8 | Kernel Object Manager: register/unregister, refcount, type enum, name, full registry, lookup, unregister edge cases, count |
| Page Cache | 13 | Page cache (advanced): hash map O(1), LRU doubly-linked, create, peek, dirty, invalidate, capacity, stats, hit_rate, pending_writes |
| PCI Enumeration | 3 | PCI bus 0 devices, bus 1 empty, bridge detection algorithm |
| Stress | 14 | Stress: sched, syscall, mem, buddy allocator, handle table |
| Hot Reload | 11 | Hot reload: resource tracking, registry, state transitions, unload/reload, error codes |

Comando `test`:
1. Ejecuta `testing::run_all()` (329 tests kernel)
2. Si pasan, ejecuta `run SYSTEST.BIN`, `run FILETEST.BIN`, `run ALLTEST.BIN`, `run CPUTEST.BIN`, `run TEST.BIN` (user-mode)

## Kernel Object Manager (KOBJ) v1

`src/kobj/mod.rs` — Unified kernel object system with reference counting and common metadata.

| Concept | Description |
|---------|-------------|
| **KObjType** | Enum (u32 repr): Unknown, Process, Driver, Device, Pipe, EventBus, BlockDevice, Filesystem, MemoryRegion |
| **KObjEntry** | Per-object metadata: KObjId (u64), refcount (u32), type, 24-byte name, flags, creation_tick, native_id |
| **KObjRegistry** | 64-slot fixed-size registry protected by `spin::Mutex`. Global via `lazy_static!` |
| **API** | `kobj_register()`, `kobj_unregister()`, `kobj_ref()`, `kobj_unref()`, `kobj_lookup()`, `kobj_count()`, `kobj_iter_snapshot()` |
| **Integration** | Processes (scheduler.rs), drivers (driver_runtime.rs), pipes (pipe.rs) — auto-register on create, auto-unregister on destroy |
| **Shell** | `KOBJ` command — list all kernel objects with ID, type, name, refcount, native ID |

### KOBJ Command

| Subcommand | Description |
|-----------|-------------|
| `KOBJ` | List all kernel objects tracked by KOBJ. Shows ID, type, name, reference count, and native ID |

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

See `docs/NEM_SPEC.md` for full NEM format spec.

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

1. **BOOT drivers** — scanned from `C:\SYSTEM\DRIVERS\BOOT\` (required for system init)
2. **SYSTEM drivers** — scanned from `C:\SYSTEM\DRIVERS\SYSTEM\` (standard kernel extension)

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
| libmath DLL | `neodos/libmath.dll` | Math library DLL (slot 1, 0x1e040000) — abs, min, max, pow, sqrt, sin, cos, log, exp |
| Serial log | `neodos/qemu_output.log` | Última sesión QEMU |
