# NeoDOS — AGENTS.md

## Versión Actual
v0.15.2

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

Bootloader loads ELF segments manually, calls `ExitBootServices`, jumps to kernel. `BootInfo` has: framebuffer info + raw memory map pointer/metadata.

## Code generation

`neodos-kernel/build.rs` parses `KBDUS.klc`/`KBDSP.klc` (UTF-16LE keyboard layouts) at build time into `$OUT_DIR/kbd_layout.rs` with scan code → ASCII tables. Two layouts: US (index 0), SP (index 1, default).

## Input system

Solo **PS/2** (IRQ1). `input.rs` tiene un ring-buffer lock-free de 1024 bytes, productor = IRQ1, consumidor = shell loop. Driver UHCI para USB no funcional en PIIX3.

## AHCI Driver

- **DMA polling** por puerto, buffers estáticos separados por puerto lógico
- **ATA**: READ/WRITE DMA EXT (0x25/0x35), multi-sector hasta 8 sectores (4KB)
- **ATAPI**: PACKET command (0xA0) con DMA, READ_10 CDB, sectores de 2048 bytes
- **Por puerto**: DeviceType::Ata / DeviceType::Atapi
- **Port reset**: ciclo DET vía SCTL para recuperación de errores
- **PRDT**: hasta 8 entradas scatter-gather
- Per-port buffers: `PORT_CMD_LIST[]`, `PORT_RECV_FIS[]`, `PORT_CMD_TABLE[]`, `PORT_DMA_BUF[]`

## Un disco GPT unificado

Single GPT disk image: ESP (FAT32) + NeoDOS FS. Kernel parsea GPT y ajusta `base_lba` para el FS. FAT32 lee con LBAs absolutos.

## ATA bus-master DMA

Kernel scans PCI bus 0 for IDE controller (class 0x01, subclass 0x01) with bus-master capability. BAR4 gives bus-master I/O base. Two page-aligned 4KB static buffers for PRDT + DMA data. Polling-based. Methods `read_dma()`/`write_dma()` support up to 8 sectors per call.

## User-mode process lifecycle

`cmd_run` loads flat binary to `USER_BASE` (0x400000) and calls `execute_usermode()` via IRETQ to Ring 3. On `sys_exit` (INT 0x80, RAX=0): marks process Terminated in scheduler, then `syscall_handler_asm` jumps to `exit_to_kernel`, restoring `EXIT_RSP`/`EXIT_RIP` and returning to caller.

## Syscall Table (INT 0x80)

RAX = syscall number, RBX = arg0, RCX = arg1, RDX = arg2. Return in RAX.

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

## In-Kernel Test Framework

99 tests en 11 suites. Registrados en `testing.rs`, ejecutados por el comando `test` del shell.

| Suite | Tests | Descripción |
|-------|-------|-------------|
| Environment | 6 | Variables de entorno |
| Input | 5 | Input buffer (ring buffer) |
| Keyboard | 5 | UTF-8 encoding, compose keys |
| Process | 3 | Process struct, state transitions |
| UTF-8 | 6 | Validación UTF-8 |
| Allocator | 8 | Box, Vec, String |
| Sync | 4 | Atomic flags (NEED_RESCHED) |
| NeoFS | 36 | Inode metadata, timestamps, block count, attrs, serialización |
| Stress | 8 | Stress: sched, syscall, mem |

Comando `test`:
1. Ejecuta `testing::run_all()` (99 tests kernel)
2. Si pasan, ejecuta `run SYSTEST.BIN`, `run FILETEST.BIN`, `run ALLTEST.BIN` (user-mode)

## Artifacts generados

| Archivo | Path | Descripción |
|---------|------|-------------|
| Bootloader UEFI | `neodos/bootloader.efi` | v0.10.5 |
| Kernel ELF | `neodos/kernel.elf` | v0.15.2 |
| Disco GPT unificado | `neodos/disk_image.img` | 112 MB (ESP + NeoDOS FS) |
| NeoDOS FS image | `neodos/scripts/neodos_image.img` | 10 MB |
| Serial log | `neodos/qemu_output.log` | Última sesión QEMU |

## Mejoras pendientes

Ver `docs/IMPROVEMENTS.md` para la lista completa.
