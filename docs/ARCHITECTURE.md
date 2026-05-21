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

## Driver Infrastructure (v0.10.0+)

NeoDOS supports modular drivers loaded as user-mode processes (`.BIN` flat binaries via `LOAD`).

### Kernel-side Drivers (built-in)
- **ATA** (`drivers/ata.rs`) — PIO + bus-master DMA + AHCI fallback, primary + secondary channel
- **AHCI** (`drivers/ahci.rs`) — DMA polling, per-port buffers, ATA + ATAPI
- **PS/2 Keyboard** (`drivers/keyboard.rs`) — IRQ1, scancode → ASCII via KLC layouts
- **USB HID** (`drivers/usb_hid.rs`) — UHCI, non-functional on PIIX3
- **PCI** (`drivers/pci.rs`) — I/O port config space access (0xCF8/0xCFC)
- **GPT** (`drivers/gpt.rs`) — parses GUID partition table
- **FAT32** (`drivers/fat32.rs`) — ESP boot partition filesystem
- **RTC** (`drivers/rtc.rs`) — CMOS RTC read
- **ACPI** (`drivers/acpi.rs`) — RSDP/XSDT parsing, poweroff via PM1a

### Device Registry
- `DEVICE_HANDLERS` array in `syscall.rs` (max 8 devices, indexed by device_id)
- Each entry: `{ device_id: u32, owner_pid: u32 }`

### Device Events
- `DEVICE_EVENTS` array in `drivers/mod.rs`: `[DeviceEvent; 8]`, each with `pending: AtomicBool`
- `signal_device_event(id)` — sets `pending` flag (from ISR or kernel code)
- Drivers poll with `sys_ioctl(dev, 0, buf=0)` = 0 → returns 1 if pending, 0 if none
- **New**: `src/eventbus/mod.rs` — centralized Event Bus v1 replaces ad-hoc device events for IRQ normalization and driver callback dispatch (lock-free SPSC queue, monotonic IDs, 11 event types, scheduler-controlled dispatch)

### Syscalls
| # | Name | Args | Description |
|---|------|------|-------------|
| 14 | sys_ioctl | RBX=device_id, RCX=cmd, RDX=buf | Device I/O control; buf=0 for poll mode |
| 15 | sys_register_device | RBX=device_id | Register current PID as device handler |

### Shell Commands
- `LOAD <file.bin>` — spawn binary as Ring 3 driver process
- `DEVICESEND <device_id> <cmd>` — Signal device event from shell
- `DEVICES` — list registered device handlers

### Process / TSR Infrastructure
- `TSR <FILE INT>` — load terminate-and-stay-resident handler (hooks INT 0x1C timer tick)
- `PS` — list all processes with PID, state, RIP, ticks
- `KILL <PID>` — terminate process by PID

## Kernel Subsystems (High-Level)
- **arch/x64**: GDT, IDT, PIC, paging (4-level, 2 MB huge pages + 4 KB demand-paging), interrupt handlers (timer IRQ0, keyboard IRQ1, syscall INT 0x80)
- **drivers**: ATA (PIO + bus-master DMA + AHCI fallback), AHCI, PS/2 keyboard, USB HID, PCI scanner, device event infrastructure
- **buffer**: block cache (periodic flush via timer)
- **fs**: **VFS layer** (`fs/vfs.rs`) — `Vfs` struct with 26 drive slots (A-Z), `FileSystem` trait (`read`/`write`/`lookup`/`readdir`/`mkdir`/`create`/`stat`/`remove_file`/`remove_dir`/`rename`), `VfsNode { inode, mode, size }`, path resolution with `walk_components`, mount point support. Implementations: `NeoDosFs` (native format, mounted on C:), `Fat32Driver` (ESP, mounted on A:)
- **memory**: frame allocator (bitmap, 4 GiB max), external heap allocator (`linked_list_allocator` 16 MB @ 0x1000000), user heap demand-paging (0x10000000..0x12000000, 32 MB, 16 × 2 MB slots → 4 KB PTs)
- **process**: `Process` struct with PID, state, registers, `user_slot`, `cwd_drive`/`cwd_path`, `heap_base`/`heap_break`, `waiting_for`
- **scheduler**: round-robin (`schedule()`), timer-driven (`on_timer_tick` every 100 ticks ≈ 5.5 Hz), max 16 processes, idle process (PID 0) always present
- **usermode**: Ring 3 execution via `execute_usermode_asm` (IRETQ), process lifecycle in `spawn_usermode`/`wait_for_process`/`sys_exit` → `exit_to_kernel`
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

