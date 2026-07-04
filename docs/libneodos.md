# libneodos — User-Mode Library

## Overview

`no_std` library for Ring 3 user-mode binaries. Target: `x86_64-unknown-none`. Located in `libneodos/`. Provides syscall wrappers, I/O primitives, filesystem access, memory management, and console features for `.NXE` binaries.

## Three Packages (No Workspace)

The project uses three independent Cargo packages with separate `Cargo.toml` files, not a Cargo workspace:

| Package | Location | Target | Description |
|---------|----------|--------|-------------|
| neodos-bootloader | `neodos-bootloader/` | UEFI | UEFI bootloader |
| neodos-kernel | `neodos-kernel/` | freestanding | Kernel binary |
| libneodos | `libneodos/` | `x86_64-unknown-none` | User-mode library |

Each has independent dependencies and build configuration.

## Full Module Table

### Syscall (`src/syscall/mod.rs`)

SSDT (System Service Dispatch Table): 256-slot `lazy_static` array mapping RAX number to handler function pointer. Permission table maps each syscall number to allowed privilege level. 32+ handlers for RAX 0-76.

All wrappers return `Result<T, i64>` where `T` is the return type and `i64` is the negative errno on failure.

**Foundation syscalls** (RAX 0-29):
| Name | RAX | Signature | Description |
|------|-----|-----------|-------------|
| `exit` | 0 | `(code: i32) -> !` | Terminate process |
| `write` | 1 | `(fd: u64, buf: &[u8]) -> usize` | Write to file/pipe |
| `yield` | 2 | `() -> ()` | Yield CPU |
| `getpid` | 3 | `() -> u64` | Get process ID |
| `read` | 4 | `(fd: u64, buf: &mut [u8]) -> usize` | Read from file/pipe |
| `pipe` | 5 | `() -> [u64; 2]` | Create pipe → [read_fd, write_fd] |
| `dup2` | 6 | `(old_fd: u64, new_fd: u64) -> u64` | Duplicate file descriptor |
| `spawn` | 7 | `(path: &str, args: &[&str]) -> u64` | Spawn process, returns PID |
| `readdir` | 8 | `(fd: u64, buf: &mut [u8]) -> usize` | Read directory entries |
| `waitpid` | 9 | `(pid: u64) -> i32` | Wait for child to exit |
| `open` | 10 | `(path: &str) -> u64` | Open file → fd |
| `close` | 13 | `(fd: u64) -> ()` | Close file descriptor |
| `chdir` | 16 | `(path: &str) -> ()` | Change working directory |
| `brk` | 18 | `(addr: u64) -> u64` | Set program break |
| `mmap` | 19 | `(addr: u64, size: u64, prot: i32, flags: i32) -> u64` | Memory map |
| `munmap` | 20 | `(addr: u64, size: u64) -> ()` | Unmap memory |
| `loadlib` | 21 | `(path: &str, slot: u32) -> ()` | Load NXL library |
| `thread_create` | 22 | `(entry: usize, arg: u64) -> u64` | Create thread |
| `thread_join` | 23 | `(tid: u64) -> u64` | Join thread |
| `set_exception_handler` | 29 | `(handler: usize) -> ()` | Set exception handler |

**Extended syscalls** (RAX 40-76):
| Name | RAX | Description |
|------|-----|-------------|
| `wait_alertable` | 40 | Wait with alertable flag |
| `sleep_ex` | 41 | Sleep with microsecond resolution |
| `poweroff` | 42 | System poweroff |
| `chdir_parent` | 47 | Change to parent directory |
| `cursor_blink` | 53 | Toggle cursor blink |
| `fsck` | 55 | Filesystem check |
| `driver_unload` | 58 | Unload a NEM driver |
| `poll` | 59 | Poll multiple fds for readiness |

**Object Manager syscalls** (RAX 60-66, the Ob API):
| Name | RAX | Description |
|------|-----|-------------|
| `ob_open` | 60 | Open Ob object by path → handle |
| `ob_create` | 61 | Create Ob object (File, Directory, Pipe, etc.) |
| `ob_query_info` | 62 | Query object info (ReadContent, VfsDirEnum, etc.) |
| `ob_set_info` | 63 | Set object info (WriteContent, VfsRename, etc.) |
| `ob_enum` | 64 | Enumerate objects (directory listing) |
| `ob_wait` | 65 | Wait on object for signal/event |
| `ob_destroy` | 66 | Destroy Ob object |

**Registry syscalls** (RAX 67-76, the Cm API):
| Name | RAX | Description |
|------|-----|-------------|
| `cm_open_key` | 67 | Open registry key |
| `cm_create_key` | 68 | Create registry key |
| `cm_query_value` | 69 | Read registry value |
| `cm_set_value` | 70 | Write registry value |
| `cm_enum_key` | 71 | Enumerate subkeys |
| `cm_enum_value` | 72 | Enumerate values |
| `cm_delete_key` | 73 | Delete registry key |
| `cm_flush_key` | 74 | Flush key to disk |
| `cm_load_hive` | 75 | Load registry hive |
| `cm_unload_hive` | 76 | Unload registry hive |

All use `inline assembly` via `syscall` instruction with `x86_64::syscall()`.

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
pub fn progress_init(current: u64, total: u64);
pub fn progress_update(current: u64);
pub fn progress_finish();
```

The console module is lazy-loaded via `sys_loadlib` on first use from `console.nxl` (NXL slot 2). All history and completion callbacks are provided by the NXL.

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

3. **Linker script**: Use `user.ld` placing code at `0x400000`:
   ```
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
|------|-----|-------------|-------------|
| 0 | `libneodos.nxl` | Auto-loaded at boot | Core user library routines |
| 1 | `libmath.nxl` | Manual (`sys_loadlib`) | Math library |
| 2 | `console.nxl` | Lazy-loaded by console module | Terminal I/O, history, completion, progress bar |

Slot allocation is static; each NXL has a fixed slot that cannot be changed at runtime.

## ABI Table (Version 7)

Version 7 cleaned up legacy dead entries from the struct definitions. Key ABI structs:

- `ObBasicInfo` — base info for any Ob object (type, flags, access mask)
- `ObEnumEntry` — directory listing entry (name, type, size)
- `ObProcessInfo` — process query result (pid, name, state, priority)
- `ObPipeInfo` — pipe query result (pipe_id, refcount, bytes_available)

Struct layout is `#[repr(C)]` with explicit padding where needed. All structs defined in `src/abi.rs` and re-exported from `libneodos`. NEM drivers and NXL libraries that interact with these structs must match v7 layout exactly.
