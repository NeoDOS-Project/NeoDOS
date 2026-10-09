# libneodos — User-Mode Library

## Overview

`no_std` library for Ring 3 user-mode binaries. Target: `x86_64-unknown-none`. Located in `libneodos/`. Provides syscall wrappers, I/O primitives, filesystem access, memory management, and console features for `.NXE` binaries.

## Three Packages (No Workspace)

The project uses three independent Cargo packages with separate `Cargo.toml` files, not a Cargo workspace:

| Package | Location | Target | Description |
| --------- | ---------- | -------- | ------------- |
| neodos-bootloader | `neodos-bootloader/` | UEFI | UEFI bootloader |
| neodos-kernel | `neodos-kernel/` | freestanding | Kernel binary |
| libneodos | `libneodos/` | `x86_64-unknown-none` | User-mode library |

Each has independent dependencies and build configuration.

## Full Module Table

### Syscall (`src/syscall.rs`)

SSDT (System Service Dispatch Table): a 256-slot array mapping the RAX number to a handler function pointer. A permission table maps each syscall number to its required privilege level. There are **37 assigned syscalls** (RAX 0-99); the authoritative table is [`docs/kernel/syscalls.md`](../kernel/syscalls.md).

All wrappers return `Result<T, i64>` where `T` is the return type and `i64` is the negative errno on failure.

**Assigned syscalls** (libneodos wrappers map 1:1):

| RAX | Name | Description |
| ----- | ------ | ------------- |
| 0 | `exit` | Terminate process |
| 1 | `yield` | Yield CPU |
| 2 | `wait_alertable` | Alertable wait (dispatch pending APC) |
| 3 | `sleep_ex` | Alertable sleep |
| 4 | `set_exception_handler` | Set SEH handler |
| 10 | `brk` | Set program break |
| 11 | `mmap` | Map memory (lazy) |
| 12 | `munmap` | Unmap memory |
| 20 | `write` | Write to file/pipe |
| 21 | `read` | Read from file/pipe |
| 22 | `dup2` | Duplicate file descriptor |
| 23 | `close` | Close file descriptor |
| 24 | `poll` | Poll multiple fds |
| 25 | `loadlib` | Load NXL library |
| 30 | `cursor_blink` | Toggle cursor blink |
| 35 | `driver_unload` (admin) | Unload a NEM driver |
| 36 | `icmp_ping` | ICMP echo request → RTT (µs) |
| 40-48 | `ob_*` | Object Manager (see below) |
| 50-59 | `cm_*` | Registry (Cm) (see below) |
| 99 | `debug_dump` | Diagnostic dump |

Legacy wrappers `getpid`, `pipe`, `spawn`, `readdir`, `waitpid`, `open`, `chdir`,
`thread_create` and `thread_join` were removed and migrated to the Ob API
(e.g. `getpid` → `ob_query_info(ProcessId = 34)`; file/dir operations →
`ob_open`/`ob_create`/`ob_query_info`/`ob_set_info`). Power control is exposed as
`ob_power_shutdown()` / `ob_power_reboot()`:

| Name | Mechanism | Description |
| ------ | ----------- | ------------- |
| `ob_power_shutdown` | Ob API | Open `\System\PowerManager` + `ob_set_info(PowerShutdown=37)` |
| `ob_power_reboot` | Ob API | Open `\System\PowerManager` + `ob_set_info(PowerReboot=38)` |

**Object Manager syscalls** (RAX 40-48, the Ob API):

| Name | RAX | Description |
| ------ | ----- | ------------- |
| `ob_open` | 40 | Open Ob object by path → handle |
| `ob_create` | 41 | Create Ob object (File, Directory, Pipe, etc.) |
| `ob_query_info` | 42 | Query object info (ReadContent, FsckStatus, etc.) |
| `ob_set_info` | 43 | Set object info (WriteContent, VfsRename, etc.) |
| `ob_enum` | 44 | Enumerate objects (directory listing) |
| `ob_wait` | 45 | Wait on object for signal/event |
| `ob_destroy` | 46 | Destroy Ob object |
| `ob_service` | 47 | Service control (admin) |
| `ob_snapshot` | 48 | Filesystem snapshot operations (admin) |

**Registry syscalls** (RAX 50-59, the Cm API):

| Name | RAX | Description |
| ------ | ----- | ------------- |
| `cm_open_key` | 50 | Open registry key |
| `cm_create_key` | 51 | Create registry key |
| `cm_query_value` | 52 | Read registry value |
| `cm_set_value` | 53 | Write registry value |
| `cm_enum_key` | 54 | Enumerate subkeys |
| `cm_enum_value` | 55 | Enumerate values |
| `cm_delete_key` | 56 | Delete registry key |
| `cm_flush_key` | 57 | Flush key to disk |
| `cm_load_hive` | 58 | Load registry hive (admin) |
| `cm_unload_hive` | 59 | Unload registry hive (admin) |

All use `int 0x80` via inline assembly.

### IO (`src/io.rs`)

`Stdout`, `Stdin`, `Stderr` structs wrapping FDs 1, 0, 2 respectively. Each provides:

```rust
impl Stdout {
    pub fn write(&self, buf: &[u8]) -> usize;
}
impl Stdin {
    pub fn read(&self, buf: &mut [u8]) -> usize;
}
impl Stderr {
    pub fn write(&self, buf: &[u8]) -> usize;
}
```

All three implement `core::fmt::Write` for use with `write!`/`writeln!`. Stack-buffered `_print()` and `_eprint()` functions use a 1024-byte stack buffer before calling `sys_write`.

### FS (`src/fs.rs`)

```rust
pub struct File { fd: u64 }

impl File {
    pub fn open(path: &str) -> Result<File, i64>;   // ob_open → fd
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, i64>;  // ob_query_info ReadContent
    pub fn write(&self, buf: &[u8]) -> Result<usize, i64>;     // ob_set_info WriteContent
    pub fn close(&self);                                       // ob_close
}
```

Wraps the Ob API for file access. `File::open` calls `ob_open` to get a handle, then stores the fd. Read/write call `ob_query_info(ReadContent)` / `ob_set_info(WriteContent)` respectively.

### Mem (`src/mem.rs`)

```rust
pub fn brk(addr: u64) -> u64;          // sys_brk RAX 18
pub fn sbrk(increment: i64) -> u64;    // brk(current + increment)
pub fn mmap(addr: u64, size: u64, prot: i32, flags: i32) -> u64;  // sys_mmap RAX 19
pub fn munmap(addr: u64, size: u64);                                // sys_munmap RAX 20
```

Constants:

- `PROT_READ: i32` = 1
- `PROT_WRITE: i32` = 2
- `MAP_ANONYMOUS: i32` = 0x20

`sbrk` is implemented as `brk(current_break + increment)` with the current break tracked via static variable.

### Args (`src/args.rs`)

```rust
pub fn read_args() -> [u8; 256];
pub fn is_help_flag(args: &[u8]) -> bool;
pub fn trim_ascii(s: &[u8]) -> &[u8];
```

`read_args()` returns the current process command-line args. Since v0.50.3 it first tries the kernel per-process store via `sys_ob_open("\\Global\\Info\\Process")` + `sys_ob_query_info(ProcessArgs=39)` (populated atomically by `sys_ob_create(PROCESS)` from the legacy `0x41F000` buffer, fixing the pipeline data race). If that query fails (old kernel) it falls back to the legacy shared buffer at `0x41F000`.

### Console (`src/console.rs`)

```rust
pub fn read_byte() -> u8;             // blocking read from stdin
pub fn history_add_raw(line: &str);
pub fn history_prev() -> Option<&str>;
pub fn history_next() -> Option<&str>;
pub fn history_reset();
pub fn history_get_count() -> u32;
pub fn history_get_entry(index: u32) -> Option<&str>;
pub fn register_completion(callback: CompletionFn);
pub fn progress_begin(title: &str, total: u64) -> i32;
pub fn progress_update(id: i32, current: u64);
pub fn progress_set_message(id: i32, text: &str);
pub fn progress_finish(id: i32);
pub fn spinner_begin(title: &str);
pub fn spinner_update();
pub fn spinner_finish();
```

The console module is lazy-loaded via `sys_loadlib` on first use from `console.nxl` (NXL slot 2). All history and completion callbacks are provided by the NXL.

Progress bars render with Unicode block characters (`▓` filled, `░` empty), adapt to terminal width, show percentage and `(current/total)` ratio, and only redraw when the percentage changes. The spinner cycles through `| / - \` for tasks with unknown duration.

### Keyboard (`src/keyboard.rs`)

User-mode keyboard management API wrapping `\Device\Keyboard` Ob object:

```rust
pub struct KbdState {
    pub modifiers: u8,
    pub leds: u8,
    pub active_layout_index: u32,
}
pub struct KbdLayoutInfo {
    pub index: u32,
    pub name: [u8; 32],
    pub lang_tag: [u8; 16],
    pub scancode_count: u16,
    pub compose_count: u16,
}
pub fn kbd_get_layout() -> Result<[u8; 32], i64>;       // active layout name
pub fn kbd_set_layout(name: &str) -> Result<(), i64>;    // switch by name
pub fn kbd_list_layouts() -> Result<LayoutList, i64>;    // all loaded layouts
pub fn kbd_get_state() -> Result<KbdState, i64>;         // modifiers, leds, layout
pub fn kbd_set_leds(leds: u8) -> Result<(), i64>;
pub fn kbd_get_repeat() -> Result<(u32, u32), i64>;      // (delay_ms, rate_cps)
pub fn kbd_set_repeat(delay: u32, rate: u32) -> Result<(), i64>;
```

Modifier constants: `KBD_SHIFT`, `KBD_CTRL`, `KBD_ALT`, `KBD_ALTGR`,
`KBD_CAPS`, `KBD_NUMLOCK`, `KBD_SCROLLLOCK`.

### i18n (`src/i18n.rs`)

Internationalization runtime. Loads NLTv2/v3 tables and looks up translated
strings. The binary format itself lives in the shared `libnlt` crate.

```rust
pub fn i18n_init();                              // read locale from Registry
pub fn i18n_language() -> &'static str;          // "es-ES"
pub fn i18n_load(app: &str) -> Result<(), ()>;   // NLTv2/v3, fallback chain
pub fn i18n_get_id(id: u32) -> &'static str;     // "?" on miss
pub fn i18n_plural(id: u32, n: u64) -> &'static str;
pub fn i18n_format(id: u32, args: &[&str]) -> &'static str;
pub fn i18n_region() -> Option<Region<'static>>;
pub fn i18n_available_locales() -> &'static str;
pub fn i18n_is_rtl() -> bool;
pub fn i18n_set_language(tag: &str);
pub fn i18n_reload_all();
```

Macros: `tr_id!(ID)`, `tr_fmt!(ID, args)`, `plural_id!(ID, n)`. See
[`nlt.md`](nlt.md) for the format, compiler (`nltc`) and tooling.

### Macros (`src/macros.rs`)

```rust
print!(fmt, args..);    // write formatted to stdout
println!(fmt, args..);  // write formatted + CRLF to stdout
eprint!(fmt, args..);   // write formatted to stderr
eprintln!(fmt, args..); // write formatted + CRLF to stderr
```

All output macros append `\r\n` for CRLF line endings (NT console convention). Implemented via `core::fmt::Write` on stdout/stderr.

## How to Create a User Binary

1. **Cargo.toml**: Add dependency:

   ```toml
   [dependencies]
   libneodos = { path = "../libneodos" }
   ```

2. **Target**: Configure `.cargo/config.toml`:

   ```toml
   [build]
   target = "x86_64-unknown-none"
   rustflags = ["-C", "relocation-model=static"]
   ```

3. **Linker script**: Use `user.ld` linking at address 0 (runtime loading places code starting at `0x400000` via ASLR slot):

   ```toml
   cargo:rustc-link-arg=-Tuser.ld
   ```

4. **Entry point**:

   ```rust
   #![no_std]
   #![no_main]

   #[no_mangle]
   pub extern "C" fn _start() -> ! {
       // application code
       libneodos::syscall::exit(0)
   }
   ```

5. **Build**: `cargo build --release`

The resulting ELF binary is the `.NXE` file placed in the disk image at `\Programs\<name>.NXE`.

## NXL Loading

NXL (NeoDOS eXecutable Library) files are loaded via `sys_loadlib` (RAX 21) into region `0x1e000000..0x1e200000` (2 MB total). Divided into 8 slots of 256 KB each.

| Slot | NXL | Load Policy | Description |
| ------ | ----- | ------------- | ------------- |
| 0 | `libneodos.nxl` | Auto-loaded at boot | Core user library routines |
| 1 | `libmath.nxl` | Manual (`sys_loadlib`) | Math library |
| 2 | `console.nxl` | Lazy-loaded by console module | Terminal I/O, history, completion, progress bar |

Slot allocation is static; each NXL has a fixed slot that cannot be changed at runtime.

## ABI Table (Version 8)

Current ABI is **v8**. Key ABI structs (all `#[repr(C)]`, defined in `libneodos/src/syscall.rs`):

- `ObBasicInfo` — base info for any Ob object (type, refcount, name)
- `ObEnumEntry` — directory listing entry (id, type, name, mode, size)
- `ObProcessInfo` — process query result (pid, parent pid, priority, thread count, state)
- `CpuStatsEntry` / `ThreadStatsEntry` — SMP observability snapshots (classes 24/25)
- `SmpStats` — global work-stealing counters (class 27, `\Global\Info\CpuInfo`)
- `ProcSnapshotHeader` / `ProcessInfoRaw` / `ThreadInfoRaw` — coherent process+thread snapshot (class 26)

NEM drivers and NXL libraries that interact with these structs must match the v8
layout exactly. The kernel-side definitions live in
`neodos-kernel/src/syscall/ob/types.rs`.
