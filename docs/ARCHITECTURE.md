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

## Kernel Subsystems (High-Level)

- **arch/x64**: GDT, IDT, PIC, paging, interrupt handlers
- **drivers**: ATA + keyboard input
- **buffer**: block cache
- **fs**: NeoDOS filesystem + minimal VFS helpers + drive letter manager
- **shell**: DOS-like shell and built-in commands (`HELP`, `DIR`, `TYPE`, `COPY`, `MD`, `CD`, `CPUINFO`, `MEM`, …)
- **scheduler**: round-robin scheduler used by the timer ISR when processes exist; idle process is always available

## Debug Interfaces

The provided script `scripts/qemu-debug.sh` runs QEMU with:

- Serial output to stdout (saved to `neodos/qemu_output.log`)
- QEMU monitor on `telnet 127.0.0.1:4444`
- GDB server on `tcp::1234`

See `docs/DEBUG.md` for a walkthrough.

