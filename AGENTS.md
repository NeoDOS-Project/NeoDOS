# NeoDOS — AGENTS.md

## Versión Actual
v0.10.3

## Build & Run

All commands from `neodos/`. Dependencies: `rustup`, `qemu-system-x86`, `ovmf`, `gdb`, `mtools`, `dosfstools`, `util-linux` (sfdisk).

```bash
bash scripts/build.sh                  # bootloader + kernel + GPT disk image
bash scripts/build.sh --neodos-image   # + NeoDOS FS image + user binaries
bash scripts/qemu-debug.sh             # QEMU + OVMF, serial to stdout, GDB :1234
gdb -x .gdbinit                         # from neodos/, connects to QEMU
python3 scripts/auto_test.py            # Automated headless test runner
```

El acelerador de QEMU se configura con la variable `QEMU_ACCEL`. Por defecto usa **TCG** (software). Si el host soporta KVM, se puede habilitar con:

```bash
QEMU_ACCEL=kvm bash scripts/qemu-debug.sh
QEMU_ACCEL=kvm python3 scripts/auto_test.py
```

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

Solo **PS/2** (IRQ1). `input.rs` tiene un ring-buffer lock-free de 1024 bytes, productor = IRQ1, consumidor = shell loop.

Hay un driver UHCI para teclado USB (`drivers/usb_hid/`) pero **no funcional en PIIX3**:
el registro FLBASEADD no acepta escrituras (se queda en `0xFF7F:FF7F`), por lo que el
controlador nunca inicia. Pendiente de debug (posiblemente requiere MMIO en vez de I/O).

No se añaden dispositivos USB a QEMU (`piix3-usb-uhci`, `usb-kbd`) — solo PS/2.

## Adding a shell command

1. Create `src/shell/commands/<name>.rs` with an `impl DosShell` method.
2. Add `mod <name>;` to `src/shell/commands/mod.rs`.
3. Add a `CommandEntry` to `handler::COMMANDS` in `handler.rs`. Help text is automatic.

## Disco unificado GPT

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

## User-mode process lifecycle

`cmd_run` in `shell/commands/run.rs` loads a flat binary to `USER_BASE` (0x400000) and calls `execute_usermode()`.

`execute_usermode()` in `usermode.rs` saves the kernel RSP/RIP into `EXIT_RSP`/`EXIT_RIP` statics, then IRETQs to Ring 3. The function is **not** `options(noreturn)` — it can return.

On `sys_exit` (INT 0x80, RAX=0): `syscall_dispatch` marks the process `Terminated` in the scheduler, then the `syscall_handler_asm` trampoline detects RAX==0 and jumps to `exit_to_kernel`, which restores `EXIT_RSP`/`EXIT_RIP`. Control returns to `execute_usermode`'s caller (`cmd_run`), which prints "Process exited." and shows the shell prompt.

Key files: `usermode.rs` (trampoline & context save/restore), `idt.rs` (syscall_handler_asm exit path), `syscall.rs` (dispatch & Terminated marking).

## Syscall Table (INT 0x80)

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

## User-mode binaries

Ubicados en `userbin/`. Generados por scripts Python (no requieren NASM).

| Binario | Generador | Tamaño | Prueba |
|---------|-----------|--------|--------|
| `hello.bin` | `generate_hello.py` | 232 B | sys_write, sys_getpid, sys_yield, sys_exit |
| `systest.bin` | `generate_systest.py` | 247 B | Misma estructura que hello.bin + mensajes v0.10.3 |

User window: `0x400000` .. `0x800000`. Binarios flat cargados en `0x400000`.

## In-Kernel Test Framework

52 tests en 8 suites. Registrados en `testing.rs`, ejecutados por el comando `test` del shell.

| Suite | Tests | Descripción |
|-------|-------|-------------|
| Environment | 7 | Variables de entorno |
| Input | 6 | Input buffer (ring buffer) |
| Keyboard | 5 | UTF-8 encoding, compose keys |
| Drive | 14 | Drive manager, path resolution |
| Process | 3 | Process struct, state transitions |
| UTF-8 | 6 | Validación UTF-8 |
| Allocator | 8 | Box, Vec, String |
| Sync | 4 | Atomic flags (NEED_RESCHED) |

Comando `test`:
1. Ejecuta `testing::run_all()` (52 tests kernel)
2. Si pasan, automáticamente ejecuta `run SYSTEST.BIN` (user-mode)

## Mejoras pendientes

Ver `docs/IMPROVEMENTS.md` para la lista completa de items pendientes por prioridad.

## Artifacts generados

| Archivo | Path | Descripción |
|---------|------|-------------|
| Bootloader UEFI | `neodos/bootloader.efi` | v0.10.3 |
| Kernel ELF | `neodos/kernel.elf` | v0.10.3 |
| Disco GPT unificado | `neodos/disk_image.img` | 112 MB (ESP + NeoDOS FS) |
| NeoDOS FS image (temp) | `neodos/scripts/neodos_image.img` | 10 MB, regenerado en build |
| GPT builder | `neodos/scripts/create_gpt_image.py` | Combina ESP + NeoDOS en GPT |
| Serial log | `neodos/qemu_output.log` | Última sesión QEMU |
