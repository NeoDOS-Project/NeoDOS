# NeoDOS Kernel Subsystem Architecture

## 1. Overview

NeoDOS is a monolithic kernel organized into explicit subsystems with stable internal contracts. Each subsystem owns its state, exposes narrow APIs, and has forbidden responsibilities.

### Layer Diagram

```
┌─────────────────────────────────────────────────────────┐
│                     USER MODE                           │
│  (Ring 3: .BIN binaries, INT 0x80 syscall interface)   │
└────────────────────┬────────────────────────────────────┘
                     │ INT 0x80
┌────────────────────▼────────────────────────────────────┐
│  2. SYSCALL SUBSYSTEM                                   │
│  Dispatch, argument validation, user-space ABI          │
├──────────┬──────────┬──────────┬──────────┬─────────────┤
│  3. VFS  │ 4. EXEC  │ 5. SHELL │ 6. INPUT │ 7. CONSOLE  │
│          │          │          │          │             │
├──────────┴──────────┴──────────┴──────────┴─────────────┤
│  8. MEMORY SUBSYSTEM (frame allocator + paging)         │
├─────────────────────────────────────────────────────────┤
│  9. BLOCK DEVICE SUBSYSTEM (BlockDevice trait)          │
├──────────┬──────────┬───────────────────────────────────┤
│ 10. ATA  │11. AHCI  │ 12. STORAGE (GPT, ISO9660)        │
├──────────┴──────────┴───────────────────────────────────┤
│ 13. SCHEDULER SUBSYSTEM (processes, context switching)   │
├─────────────────────────────────────────────────────────┤
│ 14. INTERRUPT SUBSYSTEM (IDT, PIC, IRQ dispatch)        │
├─────────────────────────────────────────────────────────┤
│ 15. ARCHITECTURE ABSTRACTION LAYER (Platform trait)     │
├─────────────────────────────────────────────────────────┤
│ 16. MODULE SUBSYSTEM (TSR, loadable modules)            │
└─────────────────────────────────────────────────────────┘
```

## 2. Syscall Subsystem

**Purpose**: Handle INT 0x80 from user space: validate arguments, dispatch to kernel services, return results.

**Ownership**: `syscall::` — `syscall_dispatch()`, `NEED_RESCHED`, `DEVICE_HANDLERS`, `syscall_try_resched()`.

**Responsibilities**:
- Validate user-space pointers and lengths
- Dispatch to VFS, scheduler, input, paging as needed
- Set `NEED_RESCHED` after `sys_yield` 
- Handle `sys_exit` → mark process Terminated → jump to `exit_to_kernel`
- Handle `sys_brk` and `sys_mmap` → allocate heap pages

**Forbidden**:
- Direct hardware access (no port I/O, no MMIO)
- Direct interrupt management (no PIC/APIC access)
- Scheduler policy decisions (only reads current PID, never calls `schedule()`)
- Context switching (only sets `NEED_RESCHED`)

**Public API**:
- `syscall_dispatch(rax, rbx, rcx, rdx) -> u64` — called from `syscall_handler_asm`
- `syscall_try_resched(current_rsp) -> u64` — called from syscall handler asm after dispatch
- `clear_need_resched() -> bool` — check and clear `NEED_RESCHED` flag
- `set_need_resched()` — set `NEED_RESCHED` from kernel code
- `wake_blocked_readers()` — wake processes blocked on stdin
- `register_device(device_id, owner_pid) -> bool`
- `get_device_handler(device_id) -> Option<DeviceHandler>`

**Internal API**:
- `is_user_ptr_valid(ptr, len) -> bool`
- `copy_user_string(ptr) -> Result<String, ()>`
- `normalize_dos_path(path) -> String`

**Dependencies**: scheduler (read PID, heap range, CWD), VFS (file operations), input (read), paging (heap pages), console (write), serial (logging).

**Allowed Callers**: Interrupt handler (`syscall_handler_asm` in IDT).

**Synchronization**: Interrupts disabled during scheduler/VFS access (`without_interrupts`).

**Failure Model**: Returns `u64::MAX` on invalid args; `sys_exit` marks process Terminated and returns control to kernel. Memory allocation failures propagate as `u64::MAX`.

## 3. VFS Subsystem

**Purpose**: Unified filesystem interface with drive letters, path resolution, mount points.

**Ownership**: `fs::vfs::Vfs` — mount table, path walk, drive letter abstraction.

**Responsibilities**:
- Maintain mount table (26 drive letters, nested mount points)
- Path resolution (`C:\\foo\\bar` → drive + inode)
- Dispatch read/write/lookup/create to mounted `FileSystem` implementations
- `split_drive()` / `resolve_path()` / `walk_components()`

**Forbidden**:
- Block device access (no `read_sector()` calls)
- Memory allocation in hot paths
- Inter-process communication
- Scheduling decisions

**Public API** (on `Vfs` struct):
- `resolve_path(path) -> Result<(usize, VfsNode), VfsError>`
- `read(drive_idx, inode, offset, buf) -> Result<usize, VfsError>`
- `write(drive_idx, inode, offset, buf) -> Result<usize, VfsError>`
- `readdir(drive_idx, inode, index) -> Result<Option<DirEntry>, VfsError>`
- `mkdir(path) / create(path) / remove_file(path) / remove_dir(path) / rename(path, new)`
- `mount(letter, fs) / unmount(letter)`
- `mount_at_path(path, mounted_drive) / unmount_path(path)`

**Dependencies**: `FileSystem` trait implementations (NeoDOS FS, FAT32).

**Allowed Callers**: Syscall subsystem, shell commands.

**Synchronization**: Protected by `globals::VFS` Mutex; accessed via `globals::with_vfs()`.

**Trait: `FileSystem`**:
```rust
pub trait FileSystem: Send {
    fn read(&mut self, inode: u32, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError>;
    fn write(&mut self, inode: u32, offset: u64, buf: &[u8]) -> Result<usize, VfsError>;
    fn lookup(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError>;
    fn readdir(&mut self, dir_inode: u32, index: usize) -> Result<Option<DirEntry>, VfsError>;
    fn mkdir(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError>;
    fn create(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError>;
    fn stat(&mut self, inode: u32) -> Result<VfsNode, VfsError>;
    fn remove_file(...) -> Result<(), VfsError>;
    fn remove_dir(...) -> Result<(), VfsError>;
    fn rename(...) -> Result<(), VfsError>;
    fn volume_label() -> Result<String, VfsError>;
}
```

## 4. Exec Subsystem

**Purpose**: Load, spawn, and manage user-mode process lifecycle.

**Ownership**: `usermode::` — `execute_usermode()`, `spawn_usermode()`, `wait_for_process()`, `EXIT_RSP`/`EXIT_RIP`.

**Responsibilities**:
- Load flat binaries from VFS into user memory slots
- Create Ring 3 initial process state (entry point, stack, heap)
- Save/restore kernel stack (`EXIT_RSP`/`EXIT_RIP`) for Ring 3 → Ring 0 transitions
- Handle process exit (`exit_to_kernel` asm trampoline → return to caller)

**Forbidden**:
- Filesystem operations (reads binaries via VFS only)
- Scheduler policy decisions
- Direct interrupt management

**Public API**:
- `spawn_usermode(entry, stack_top, slot_idx, cwd_drive, cwd_path) -> u32` (PID)
- `wait_for_process(pid)` — blocks until process exits
- `execute_usermode(entry_point, stack_pointer)` — IRETQ to Ring 3
- `request_exit_to_kernel()` — set `EXIT_NOW` flag so syscall exits
- `current_wait_pid() -> u32`
- `clear_wait_pid()`

**Dependencies**: scheduler (add process, read process state), paging (alloc/free user slots, heap slots), `arch::x64::gdt` (set TSS.RSP0).

**Allowed Callers**: Shell `RUN` command, `test` command.

**Synchronization**: `without_interrupts` when accessing scheduler.

## 5. Shell Subsystem

**Purpose**: Interactive DOS-style command interpreter.

**Ownership**: `shell::` — `DosShell`, command handlers, environment variables, batch processing.

**Responsibilities**:
- Command parsing and dispatch
- TAB autocompletion
- Environment variable management
- Batch file execution
- Current working directory tracking

**Forbidden**:
- Direct syscall dispatch (uses VFS/scheduler public APIs)
- Hardware access
- Interrupt management

**Public API**: `DosShell::new()`, `DosShell::run()`, per-command `cmd_*` methods.

**Allowed Callers**: Kernel initialization (`main.rs`).

## 6. Input Subsystem

**Purpose**: Lock-free ring buffer for PS/2 keyboard input.

**Ownership**: `input::` — `InputBuffer`, `push_byte()`, `pop_byte()`.

**Responsibilities**:
- Provide lock-free single-producer (IRQ1) / single-consumer (shell) FIFO
- Push bytes from keyboard handler
- Pop bytes for shell / `sys_read`

**Forbidden**:
- Hardware access (keyboard driver owns scan code → ASCII conversion)
- Scheduling
- Memory allocation

**Public API**: `push_byte(u8)`, `pop_byte() -> Option<u8>`.

**Allowed Callers**: Interrupt handler (keyboard IRQ1), shell, syscall (`sys_read`).

**Synchronization**: Lock-free (atomic head/tail indices).

## 7. Console Subsystem

**Purpose**: Display output to framebuffer (GOP) and serial.

**Ownership**: `console::`, `graphics::`, `font::`, `arch::x64::serial::`.

**Responsibilities**:
- Framebuffer pixel rendering (font bitmap, scrolling, cursor)
- Serial port output (COM1)
- `println!` / `print!` macros for kernel logging

**Forbidden**:
- Disk/block I/O
- Process management
- Memory allocation in hot output paths

**Public API**:
- `print_str(s)` — write string to framebuffer
- `clear_screen()`, `draw_cursor(visible)`
- `serial_print!` / `serial_println!` macros
- `println!` / `print!` macros

**Allowed Callers**: All kernel code.

## 8. Memory Subsystem

**Purpose**: Physical frame allocation, virtual memory management, heap demand paging.

**Ownership**: `memory::` (frame allocator), `arch::x64::paging::` (page tables).

### Frame Allocator (`memory::`)
**Responsibilities**:
- Bitmap-based allocator tracking 4 GiB physical address space
- `allocate_frame()` / `free_frame()`
- Parse UEFI memory map during init

**Public API**:
- `init(boot_info)` — parse memory map, mark free/used regions
- `allocate_frame() -> Option<u64>` — returns physical address
- `free_frame(phys)` — return frame to pool
- `stats() -> MemoryStats` — free/used/reserved statistics

**Forbidden**: Virtual memory operations, process management.

### Page Table Manager (`arch::x64::paging::`)
**Responsibilities**:
- Custom identity-mapped page tables (4 GiB, 2 MB huge pages)
- 2 MB → 4 KB page splitting for demand paging
- User slot allocation (32 slots, code + stack)
- Heap slot allocation (16 slots, 2 MB each)
- Demand paging: `heap_alloc_page()`, `heap_free_page()`, `heap_free_range()`
- TLB management (INVLPG, CR3 reload)

**Public API**:
- `init_custom_page_tables()`, `init_heap_demand_paging()`
- `alloc_user_slot() -> Option<UserSlot>`, `free_user_slot(slot_idx)`
- `alloc_heap_slot() -> Option<HeapSlot>`, `free_heap_slot(index)`
- `heap_alloc_page(virt) -> Option<u64>`, `heap_free_page(virt)`
- `heap_free_range(start, end)`
- `handle_heap_page_fault(virt, user, write) -> bool`
- `split_2mb_page(virt) -> Result<(), ()>`
- `is_heap_virtual_addr(virt) -> bool`
- `map_user_range(base, size)`, `unmap_user_range(base, size)`
- Constants: `USER_BASE`, `USER_LIMIT`, `PROCESS_HEAP_BASE`, `PROCESS_HEAP_SIZE`, `HUGE_PAGE_SIZE`, `PAGE_4K`, `USER_SLOT_COUNT`

**Allowed Callers**: Syscall subsystem (brk, mmap), scheduler (kill_pid → free_user_slot, heap_free_range), usermode (spawn), page fault handler.

## 9. Block Device Subsystem

**Purpose**: Abstract unified interface for block-level storage access.

**Ownership**: `drivers::block::` — `BlockDevice` trait + implementations.

**Responsibilities**:
- Define `BlockDevice` interface
- Implement for `AtaDriver` and `AhciDriver`
- Provide `read_sector`/`write_sector` convenience methods
- Optional `num_sectors()`, `sector_size()`, `flush()`

**Trait**:
```rust
pub trait BlockDevice: Send {
    fn num_sectors(&self) -> Option<u64>;
    fn sector_size(&self) -> u32;
    fn read_blocks(&mut self, lba: u64, count: u8, buf: &mut [u8]) -> Result<(), ()>;
    fn write_blocks(&mut self, lba: u64, count: u8, buf: &[u8]) -> Result<(), ()>;
    fn flush(&mut self) -> Result<(), ()>;
    fn set_base_lba(&mut self, lba: u64);
    fn base_lba(&self) -> u64;
    fn read_sector(&mut self, lba: u64) -> Result<[u8; 512], ()>;
    fn write_sector(&mut self, lba: u64, data: &[u8; 512]) -> Result<(), ()>;
}
```

**Forbidden (for `BlockDevice` implementors)**:
- Assume specific hardware (AHCI/ATA/PIO)
- Access global state directly
- Implement storage stack logic (partition parsing, filesystems)

## 10. ATA Driver

**Purpose**: ATA PIO and bus-master DMA on primary/secondary IDE channels.

**Ownership**: `drivers::ata::AtaDriver` — I/O port operations for ATA commands.

**Responsibilities**:
- ATA PIO read/write (sector, multiple sectors)
- Bus-master DMA read/write (via PCI BAR4)
- Channel selection (Primary/Secondary, Master/Slave)
- `base_lba` offset for partition-relative addressing

**Forbidden**:
- AHCI fallback logic (removed — use `BlockDevice` abstraction instead)
- Global state access (`globals::AHCI_DRIVER`)

**Public API**: 
- `new(channel)`, `set_base_lba(lba)`, `base_lba()`
- `read_sector(lba)`, `write_sector(lba, data)`
- `read_sector_master(lba)` — absolute LBA, used by FAT32
- `read_sectors(lba, count, buf)`, `write_sectors(lba, count, data)`
- `read_dma(lba, count, buf)`, `write_dma(lba, count, data)`
- `init_dma(bmba)`

**Allowed Callers**: Block device subsystem, initialization code.

## 11. AHCI Driver

**Purpose**: AHCI SATA controller driver with DMA.

**Ownership**: `drivers::ahci::AhciDriver` — HBA MMIO, port management.

**Responsibilities**:
- Probe PCI for AHCI controllers
- Port reset and initialization
- ATA READ/WRITE DMA EXT (0x25/0x35)
- ATAPI PACKET command (0xA0)
- PRDT-based scatter-gather DMA
- Per-port static buffers

**Forbidden**: Filesystem logic, partition parsing.

## 12. Storage Subsystem

**Purpose**: Partition table parsing and filesystem detection.

**Ownership**: `drivers::gpt::`, `drivers::iso9660::`.

**Responsibilities**:
- GPT partition table parsing
- NeoDOS partition GUID detection
- ISO 9660 (CD-ROM) basic support

## 13. Scheduler Subsystem

**Purpose**: Process lifecycle management, context-switch orchestration, CPU time distribution.

**Ownership**: `scheduler::` — `Scheduler` struct, `Process` struct, `ProcessState`.

**Responsibilities**:
- Process table management (create, kill, wait)
- Process state machine (Ready → Running → Blocked/Terminated)
- Round-robin scheduling (`schedule()`)
- CPU time accounting (`on_timer_tick()`)
- Idle process management
- Current-working-directory per process
- Heap tracking per process

**Forbidden**:
- Filesystem operations
- Direct page table manipulation
- Syscall argument parsing
- Hardware/interrupt management

**Public API**:
- `current_scheduler() -> &'static Mutex<Scheduler>`
- `has_non_idle_processes() -> bool`
- `add_ring3_process(entry, stack_top, slot_idx, cwd_drive, cwd_path, heap_base) -> u32`
- `kill_pid(pid) -> bool`, `wake_waiters(pid)`
- `current_process_mut() -> Option<&mut Process>`
- `schedule() -> *mut Process`
- `on_timer_tick()` — called from timer IRQ
- `get_current_cwd() -> (u8, String)`, `set_current_cwd(drive, path)`
- `current_process_heap_range() -> (u64, u64)`, `set_current_heap_break(new_break)`
- `TIMER_TICKS: AtomicU64`

**Process State Machine**:
```
  ┌─────┐  schedule()  ┌─────────┐
  │Ready│─────────────→│ Running │
  └─────┘              └─────────┘
     ↑                     │
     │                     │ on_timer_tick (time slice)
     │                     │ or sys_yield
     │                     ▼
     │                  ┌─────┐
     └──────────────────│Ready│ (state changed by scheduler)
                        └─────┘
     Running → Terminated (sys_exit)
     Running → Blocked (waiting for child)
     Blocked → Ready (child exits)
```

**Allowed Callers**: Syscall subsystem (read PID, heap), usermode (spawn), timer handler (on_timer_tick), shell (CWD).

**Synchronization**: `Mutex<Scheduler>` wrapped in `lazy_static`. All access inside `without_interrupts`.

## 14. Interrupt Subsystem

**Purpose**: CPU exception/interrupt handlers, PIC management, IRQ dispatch.

**Ownership**: `arch::x64::idt::` — IDT data structure, `*_handler` functions, timer/keyboard/syscall asm stubs.

**Responsibilities**:
- Set up IDT with all 256 entries
- CPU exception handlers (GPF, page fault, double fault, etc.)
- Timer IRQ0 handler (increment tick, set NEED_RESCHED, acknowledge PIC)
- Keyboard IRQ1 handler (read scan code, push to input buffer)
- INT 0x80 syscall handler (call syscall_dispatch, handle re-scheduling)
- PIC (8259A) initialization and EOI

**Forbidden**:
- Context switching (timer handler NEVER calls schedule())
- Blocking operations
- Heap allocation
- Filesystem operations

**Public API**:
- `init()` — load IDT
- `timer_handler_inner(current_rsp) -> u64` — called from timer_handler_asm
- `keyboard_handler(frame)` — IRQ1 handler
- `gpf_handler(frame, error)`, `page_fault_handler(frame, error)`, etc.
- Constants for PIC offsets, IST indices

**Allowed Callers**: Hardware (CPU exceptions, IRQs, INT).

**Synchronization**: Interrupt gates (IF cleared on entry). Timer handler uses `Mutex::lock()` for scheduler and PIC.

## 15. Architecture Abstraction Layer

**Purpose**: Isolate platform-dependent code behind traits, enabling portability.

**Ownership**: `arch::` — `Platform` trait, x86_64 implementation.

**Responsibilities**:
- Define `Platform` trait with required operations
- Implement for x86_64
- Re-export platform items for generic kernel code

**Trait**:
```rust
pub trait Platform {
    fn halt() -> !;
    fn poweroff() -> !;
    fn enable_interrupts();
    fn disable_interrupts();
    fn init_gdt();
    fn init_idt();
    fn init_pic();
    fn init_serial();
    fn read_cpu_info() -> CpuInfo;
}
```

**Forbidden**: Application logic, filesystem operations.

**Allowed Callers**: Kernel initialization, any subsystem needing platform services.

## 16. Module Subsystem (TSR / Loadable Modules)

**Purpose**: Terminate-and-Stay-Resident programs, loadable module loading, and the kernel service table for Ring-0 modules.

**Ownership**: `tsr::`, `module_abi::`, `driver.ndm` (module descriptor).

**Responsibilities**:
- TSR dispatch from timer interrupt (`tsr::dispatch_interrupt`)
- NDM v1 header parsing and validation (`module_abi::NdModuleHeader`)
- Kernel service table for Ring-0 modules (`module_abi::KernelServiceTableV1` at `0x4FFFF00`)
- Module versioning and compatibility checks (`NDM_ABI_VERSION`)
- LOAD command integration (load path in `shell::commands::load`)

**Public API**:
- `NdModuleHeader::from_bytes(data) -> Option<ParsedModule>` — parse and validate
- `init_kernel_service_table()` — install service pointers at `0x4FFFF00`
- `KERNEL_SERVICE_TABLE_ADDR` — well-known address for Ring-0 modules
- `ModuleType::{Driver, FileSystem, ShellExtension}`
- `ParsedModule` — code_slice, data_slice, entry_point_offset, name

**Files**: `src/module_abi.rs`, `src/tsr/mod.rs`, `shell/commands/load.rs`, `docs/MODULE_ABI.md`

## Dependency Map

```
main.rs
  ├── memory.rs          ← arch::x64::paging
  ├── allocator.rs       ← (no deps on subsystems)
  ├── console/           ← graphics, font, arch::x64::serial
  ├── drivers/
  │   ├── block.rs       ← AtaDriver, AhciDriver (implements BlockDevice)
  │   ├── ata.rs         ← globals::AHCI_DRIVER (REMOVE: use BlockDevice)
  │   ├── ahci.rs
  │   ├── pci.rs
  │   ├── gpt.rs         ← BlockDevice
  │   ├── iso9660.rs
  │   ├── keyboard.rs
  │   └── fat32.rs       ← BlockDevice
  ├── fs/
  │   ├── vfs.rs         ← FileSystem trait
  │   └── neodos_fs.rs   ← BlockDevice, BlockCache
  ├── scheduler.rs       ← (no direct hardware deps)
  ├── syscall.rs         ← scheduler, VFS, paging, input, console
  ├── usermode.rs        ← scheduler, paging, arch::x64::gdt
  ├── module_abi.rs      ← globals::ATA_DRIVER, console, memory, BlockDevice
  └── shell/             ← VFS, scheduler (CWD), module_abi (LOAD command)
```

### Forbidden Dependencies (MUST NEVER happen)

| From | To | Reason |
|------|----|--------|
| Scheduler | VFS | Process scheduling must not depend on filesystem |
| Scheduler | BlockDevice | Scheduler should be independent of storage |
| IRQ handlers | VFS | Interrupt handlers must be non-blocking |
| IRQ handlers | Scheduler (schedule) | Timer handler must not context-switch |
| BlockDevice impls | Scheduler | Storage drivers must not depend on process state |
| Console | VFS | Console output must work without filesystem |
| Scheduler | Console output (println) | Creates subtle deadlocks (CONSOLE == SERIAL) |

## Lifecycle & Initialization Order

```
1. graphics::init()         — Framebuffer (needed early for panic output)
2. arch::x64::init_serial() — Serial port
3. console::init()           — Console subsystem
4. arch::x64::init_gdt()    — GDT + TSS + segment selectors
5. arch::x64::init_idt()    — IDT + interrupt handlers
6. arch::x64::init_pic()    — PIC (8259A)
7. keyboard::init_ps2()     — PS/2 controller
8. usb_hid::init()          — USB HID (best-effort)
9. memory::init()           — Frame allocator (from UEFI memory map)
10. allocator::init()        — Heap allocator (linked_list_allocator)
11. arch::enable_interrupts()— STI
12. AtaDriver::new()         — ATA controllers
13. pci::find_ide() + DMA    — PCI bus-master DMA init
14. AhciDriver::probe()      — AHCI controller
15. gpt::scan_partitions()   — GPT partition table
16. BlockCache::new()        — Block cache singleton
17. NeoDosFs::new() + mount  — NeoDOS FS mount on C:
18. Fat32Driver::new() + mount— FAT32 ESP mount on A:
19. paging::init()            — Custom page tables, user slots, heap paging
20. shell::new() + shell.run()— Interactive shell
```

## Synchronization Rules

### Interrupt-safe APIs
- `input::push_byte()` / `pop_byte()` — lock-free atomics
- `NEED_RESCHED` — `AtomicBool`
- `TIMER_TICKS` — `AtomicU64`
- `DEVICE_EVENTS` — atomic flags accessed with `SeqCst`

### Must be inside `without_interrupts`
- Scheduler lock acquisition
- Process state transitions
- VFS operations (via `globals::with_vfs`)
- TSS.RSP0 updates
- User slot / heap slot allocation

### MUST NOT be called from IRQ context
- `schedule()` — only from syscall return path
- `heap_alloc_page()` — can trigger frame allocation
- `println!` — can deadlock if serial mutex is held
- VFS path resolution
- Process creation / destruction

## Audit of Global Mutable State

| Variable | Type | Location | Plan |
|----------|------|----------|------|
| `SCHEDULER` | `Mutex<Scheduler>` | `scheduler.rs` | Keep (subsystem-owned) |
| `ALLOCATOR` | `Mutex<FrameAllocator>` | `memory.rs` | Keep (subsystem-owned) |
| `STATS` | `Mutex<MemoryStats>` | `memory.rs` | Keep (subsystem-owned) |
| `RENDERER` | `Mutex<Option<Renderer>>` | `graphics.rs` | Keep (subsystem-owned) |
| `SERIAL1` | `Mutex<SerialPort>` | `arch/x64/serial.rs` | Move to `console/` |
| `VFS` | `Mutex<Vfs>` | `globals.rs` | Keep in `fs/vfs.rs` |
| `ATA_DRIVER` | `Mutex<Option<AtaDriver>>` | `globals.rs` | Move to `drivers/` |
| `ATA_DRIVER_SECONDARY` | `Mutex<Option<AtaDriver>>` | `globals.rs` | Move to `drivers/` |
| `AHCI_DRIVER` | `Mutex<Option<AhciDriver>>` | `globals.rs` | Move to `drivers/` |
| `BLOCK_CACHE` | `Mutex<Option<BlockCache>>` | `globals.rs` | Move to `buffer/` |
| `RAM_DISK_BASE/SIZE` | `AtomicU64` | `globals.rs` | Move to `drivers/block.rs` |
| `NEED_CACHE_FLUSH` | `AtomicBool` | `globals.rs` | Move to `buffer/` |
| `LAST_FLUSH_TICK` | `AtomicU64` | `globals.rs` | Move to `buffer/` |
| `DEVICE_HANDLERS` | `[Option; 8]` (static mut) | `syscall.rs` | Keep (owned by syscall) |
| `DEVICE_EVENTS` | `[DeviceEvent; 8]` (static mut) | `drivers/mod.rs` | Keep (owned by drivers) |
| `EXIT_RSP / EXIT_RIP` | `u64` (static mut) | `usermode.rs` | Keep (owned by usermode) |
| `EXIT_NOW` | `AtomicU8` | `usermode.rs` | Keep |
| `WAIT_PID` | `u32` (static mut) | `usermode.rs` | Keep |
| `SLOT_USED` | `[bool; 32]` (static mut) | `arch/x64/paging.rs` | Keep (owned by paging) |
| `HEAP_SLOT_USED` | `[bool; 16]` (static mut) | `arch/x64/paging.rs` | Keep |
| `PML4 / PDPT / PD` | `AlignedPageTable` (static mut) | `arch/x64/paging.rs` | Keep |
| `PRDT / DMA_DATA` | `DmaAligned` (static mut) | `drivers/ata.rs` | Keep (owned by ATA) |
| `TSS` | `TaskStateSegment` (static mut) | `arch/x64/gdt.rs` | Keep |
| `IDLE_STACK` | `[u8; 4096]` (static mut) | `scheduler.rs` | Keep |

## Current Coupling Issues (Technical Debt)

### Critical (blocking stability)
1. **`AtaDriver::ahci_fallback`** — ATA driver reaches into `globals::AHCI_DRIVER` at runtime. Should be a `BlockDevice` composite/wrapper, not baked into ATA.
2. **`AtaDriver` reads RAM disk** — `read_sector()` checks `globals::ram_disk_buf()`. RAM disk should be a separate `BlockDevice` implementation.  
3. **`AtaDriver` reads globals::AHCI_DRIVER** — `read_sector_inner()`, `write_sector_inner()`, `read_sectors()`, `read_sector_master_inner()` all check `ahci_fallback` and lock AHCI. This couples ATA to AHCI at the implementation level.

### High (architectural decay risk)
4. **Scheduler knows about paging** — `kill_pid()` calls `paging::free_user_slot()` and `paging::heap_free_range()`. Should notify memory subsystem via callback or event.
5. **Syscall knows about everything** — `syscall_dispatch()` imports from scheduler, VFS, paging, memory, input, console. This is an inherent property of a syscall dispatcher, but could be reduced by moving per-syscall handlers into their respective subsystems.
6. **Architecture code bleeds into kernel** — `constants.rs`? No, but `paging.rs` lives in `arch/x64/` yet is called from many generic locations. This is correct (paging IS arch-specific), but the API boundary needs explicit documentation.

### Medium 
7. **Serial is in arch/x64** — It's really a platform service, not an architecture abstraction. Should move to `console/` or similar.
8. **NeoDOS FS directly accesses block device** — Should go through `BlockDevice` trait (it already does via `globals::with_block_device()`).
9. **FAT32 driver directly accesses block device** — Same pattern.
10. **`globals.rs` is unstructured** — Acts as a dumping ground for singletons. Each should be owned by its subsystem.
