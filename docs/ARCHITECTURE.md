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
  - calls ExitBootServices and captures the final UEFI memory map
  - calls the kernel entry point as: extern "sysv64" fn(&BootInfo) -> !
       ↓
NeoDOS Kernel (x86_64-unknown-none)
  - initializes graphics + serial + VGA fallback
  - initializes CPU structures (GDT/IDT) and PIC
  - initializes physical memory stats from the UEFI memory map (first 4 GiB)
  - parses GPT → finds NeoDOS partition → sets base_lba on ATA driver
  - initializes block cache + mounts NeoDOS FS
  - loads custom page tables (4 GiB identity map)
  - starts the DOS-like shell
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

## ATA y base_lba

El driver ATA (`drivers/ata.rs`) expone dos familias de lecturas:

- **`read_sector` / `write_sector` / `read_dma` / etc.** — usan `base_lba` (offset de partición).
  El NeoDOS FS las invoca con LBAs relativos a la partición, y el driver suma `base_lba`
  antes de enviar el comando al disco.
- **`read_sector_master`** — lee LBAs absolutos (sin `base_lba`). El driver FAT32 la usa para
  leer el sector de arranque en LBA 0 o 2048.

`base_lba` se configura en `main.rs` después de parsear la GPT, antes de montar el FS.

## Boot ABI (`BootInfo`)

The bootloader passes a pointer to a `BootInfo` struct using the System V AMD64 ABI:

- `RDI` = `&BootInfo` (first argument)

`BootInfo` contains:

- GOP framebuffer info (base, size, width/height/stride)
- a raw pointer to the final UEFI memory map buffer plus its metadata (`size`, `desc_size`, `desc_version`)

The memory map buffer is intentionally leaked by the bootloader after `ExitBootServices` so the kernel can read it.

## Memory Model (Current)

- Kernel link/entry address starts at `0x200000` (see `neodos-kernel/kernel.ld`).
- Custom paging currently identity-maps the first 4 GiB.
- The `MEM` shell command reports totals derived from the UEFI memory map, clamped to the first 4 GiB and with some reservations applied:
  - first 1 MiB
  - kernel image (`__kernel_start..__kernel_end`)
  - framebuffer range

## Driver Infrastructure (v0.10.0)

NeoDOS supports modular drivers loaded as user-mode processes:

### Driver Registry
- `DEVICE_HANDLERS` array in `syscall.rs` (max 8 devices)
- Each entry: `{ device_id, owner_pid, registered }`

### Device Events
- `DEVICE_EVENTS` array in `drivers/mod.rs` (atomic flags)
- `signal_device_event(id)` sets pending flag
- Drivers poll with `sys_ioctl(buf=0)` to check for events

### Syscalls
| # | Name | Args | Description |
|---|------|------|-------------|
| 14 | sys_ioctl | RBX=device_id, RCX=cmd, RDX=buf | Device I/O control; buf=0 for poll mode |
| 15 | sys_register_device | RBX=device_id | Register current PID as device handler |

### Shell Commands
- `LOAD <filename>` — Load .ndm driver module and spawn as process
- `DEVICESEND <device_id> <cmd>` — Signal device event from shell

### Driver Format (.ndm)
- Flat binary loaded at `0x400000` (user window)
- Uses INT 0x80 for syscalls
- Calls `sys_register_device(device_id)` to become handler
- Polls with `sys_ioctl(device_id, 0, 0, 0)` for events

## Kernel Subsystems (High-Level)

- **arch/x64**: GDT, IDT, PIC, paging, interrupt handlers
- **drivers**: ATA + keyboard + device event infrastructure
- **buffer**: block cache
- **fs**: NeoDOS filesystem + minimal VFS helpers + drive letter manager
- **shell**: DOS-like shell and built-in commands (`HELP`, `DIR`, `TYPE`, `COPY`, `MD`, `CD`, `CPUINFO`, `MEM`, `LOAD`, `DEVICESEND`, …)
- **scheduler**: round-robin scheduler used by the timer ISR when processes exist; idle process is always available
- **usermode**: Ring 3 execution support for drivers and user binaries

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

## Debug Interfaces

The provided script `scripts/qemu-debug.sh` runs QEMU with:

- Serial output to stdout (saved to `neodos/qemu_output.log`)
- QEMU monitor on `telnet 127.0.0.1:4444`
- GDB server on `tcp::1234`

See `docs/DEBUG.md` for a walkthrough.

