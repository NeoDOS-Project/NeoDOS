# NeoDOS Syscall ABI

NeoDOS user-mode applications execute in Ring 3. To request a kernel service,
applications trigger software interrupt `INT 0x80`.

## Calling Convention

| Register | Purpose |
|----------|---------|
| `RAX` | Syscall number |
| `RBX` | Argument 0 |
| `RCX` | Argument 1 |
| `RDX` | Argument 2 |
| `R8` | Argument 3 |
| `R9` | Argument 4 |

Return in `RAX`: `>= 0` success, `< 0` error. All other GPRs preserved.

### Return Convention

- Success: non-negative value (fd, PID, bytes written, etc.)
- Error: negative value encoded via `err_to_u64()`:
  ```rust
  pub fn err_to_u64(e: SyscallError) -> u64 {
      (-(e as i64)) as u64
  }
  ```
- User-mode checks: `cmp rax, -1` / `jg success` (positive or zero = OK)

### SyscallNum Enum

```rust
pub enum SyscallNum {
    Exit = 0, Write = 1, Yield = 2, GetPid = 3, Read = 4,
    Dup2 = 6, ReadDir = 8, WaitPid = 9, WriteFile = 12, Close = 13,
    ChDir = 16, Brk = 18, Mmap = 19, Munmap = 20, LoadLib = 21,
    ThreadCreate = 22, ThreadJoin = 23, SetExceptionHandler = 29,
    WaitAlertable = 40, SleepEx = 41, Poweroff = 42,
    GetVolumeLabel = 46, ChDirParent = 47, KObjEnum = 48,
    SetPriority = 51, KillProcess = 52, CursorBlink = 53,
    SetVolumeLabel = 54, Fsck = 55, DriverLoad = 57, DriverUnload = 58,
    Poll = 59,
    ObOpen = 60, ObCreate = 61, ObQueryInfo = 62, ObSetInfo = 63,
    ObEnum = 64, ObWait = 65, ObDestroy = 66,
    CmOpenKey = 67, CmCreateKey = 68, CmQueryValue = 69,
    CmSetValue = 70, CmEnumKey = 71, CmEnumValue = 72,
    CmDeleteKey = 73, CmFlushKey = 74, CmLoadHive = 75, CmUnloadHive = 76,
    ObService = 77,
}

impl SyscallNum {
    pub const MAX_VALID: u64 = 77;
    pub fn from_u64(n: u64) -> Option<Self>;
}
```

### SyscallError Enum

```rust
pub enum SyscallError {
    Inval = 1,    // Invalid argument
    NoEnt = 2,    // No such entry
    NoMem = 3,    // Out of memory
    Acces = 4,    // Permission denied
    BadF  = 5,    // Bad file descriptor
    Fault = 6,    // Bad user pointer
    NoSys = 7,    // No such syscall
    Again = 8,    // Resource temporarily unavailable
    Pipe  = 9,    // Pipe error
    Exist = 10,   // Already exists
    NotDir= 11,   // Not a directory
    IsDir = 12,   // Is a directory
    Io    = 13,   // I/O error
    NoDev = 14,   // No such device
    Busy  = 15,   // Resource busy
    Perm  = 16,   // Operation not permitted (admin required)
}
```

### Error Codes (Returned as Negative u64)

Error values are the negation of `SyscallError` enum variants defined in `src/syscall/mod.rs`.

| Value | Name | Meaning |
|-------|------|---------|
| -1 | Inval | Invalid argument |
| -2 | NoEnt | File/path not found |
| -3 | NoMem | Memory allocation failure |
| -4 | Acces | Permission denied |
| -5 | BadF | Invalid file descriptor |
| -6 | Fault | Bad user-mode pointer |
| -7 | NoSys | No such syscall/function |
| -8 | Again | Try again (non-blocking) |
| -9 | Pipe | Pipe error |
| -10 | Exist | Object already exists |
| -11 | NotDir | Component not a directory |
| -12 | IsDir | Is a directory, expected file |
| -13 | Io | I/O error |
| -14 | NoDev | No such device |
| -15 | Busy | Resource busy |
| -16 | Perm | Permission denied (security) |

### SSDT (Syscall Service Dispatch Table)

Dispatch uses a fixed-size array `[Option<SyscallFn>; 256]` indexed by RAX.
Each handler has type `fn(Registers) -> u64`. The table is initialized in
a `lazy_static!` block at `syscall/mod.rs:491`.

`validate_abi()` is called at boot (Phase 3.9) to assert that every assigned
syscall number has a registered handler:

```rust
pub fn validate_abi() {
    const ASSIGNED: &[u64] = &[
        0, 1, 2, 3, 4, 6, 13, 16, 18, 19, 20, 21, 29,
        40, 41, 42, 47, 53, 55, 58, 59,
        60, 61, 62, 63, 64, 65, 66,
        67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77,
    ];
    for &n in ASSIGNED {
        assert!(SYSCALL_TABLE[n as usize].is_some(),
                "SSDT missing handler for assigned syscall {}", n);
    }
}
```

### Permission System

Each syscall slot has a `SyscallPermission` with an `admin` flag. Admin
syscalls (e.g., `DriverUnload=58`, `CmLoadHive=75`) require the calling
process to hold an admin token (`token.is_admin_token()`). Non-admin calls
return `-Perm`.

---

## Syscall Index

### 0 — `sys_exit`
Terminates the calling process and frees all allocated resources.
- **Args**: `RBX` = exit code (`u64`). 0 = success.
- **Returns**: Does not return.

### 1 — `sys_write`
Writes a buffer to a file descriptor (stdout, stderr, pipe writer, Filesystem handle).
- **Args**: `RBX`=fd, `RCX`=buf, `RDX`=len.
- **Returns**: Bytes written, or error code.
- **Filesystem**: When fd refers to an open file handle, writes at current offset and advances it.

### 2 — `sys_yield`
Voluntarily yields the CPU. Running->Ready, resets time slice, forces reschedule.
- **Args**: None.
- **Returns**: `0`.

### 3 — `sys_getpid`
Retrieves the Process ID (PID) of the calling process.
- **Args**: None.
- **Returns**: PID (`u32`).

### 4 — `sys_read`
Reads data from a file descriptor (stdin, pipe reader). Blocks with `-EAGAIN` if pipe empty.
- **Args**: `RBX`=fd, `RCX`=buf, `RDX`=count.
- **Returns**: Bytes read, or error.

### 5 — `sys_pipe` **(REMOVED — use `ObCreate` + pipe fd)**
Was: Creates a unidirectional data pipe. Now handled via Object Manager.

### 6 — `sys_dup2`
Duplicates an fd to a target slot (redirection).
- **Args**: `RBX`=old_fd, `RCX`=new_fd.
- **Returns**: `0` on success, or error code.

### 7 — `sys_spawn` **(REMOVED — process creation via Ob)**
Was: Loads and executes a user-mode binary. Now handled via Object Manager.

### 8 — `sys_readdir` **(REMOVED — use `ObEnum` instead)**
Was: Reads directory entries. Now handled via `ob_enum` (RAX 64).

### 9 — `sys_waitpid` **(REMOVED — use `ob_wait` on process)**
Was: Waits for child process. Now handled via `ob_wait` (RAX 65).

### 10 — `sys_open` **(REMOVED — use `ob_open` instead)**
Was: Opens files on VFS. Now handled via `ob_open` (RAX 60).

### 11 — `sys_readfile` **(REMOVED — use `ob_query_info` instead)**
Was: Reads from open file. Now handled via Object Manager.

### 13 — `sys_close`
Closes an fd (file, pipe, device, event). For pipes, decrements refcount.
- **Args**: `RBX`=fd.
- **Returns**: `0` on success, or error code.

### 16 — `sys_chdir` (LEGACY)
Changes CWD of the calling process. Prefer `ob_set_info(SetCwd=8)`.
- **Args**: `RBX` = path_ptr.
- **Returns**: `0` on success, or error code.

### 18 — `sys_brk`
Adjusts program break. Pages allocated on demand via page fault handler.
- **Args**: `RBX` = new break (0 = query).
- **Returns**: New/current break, or error code.

### 19 — `sys_mmap`
Lazy mapping: registers VMA without pages. Allocated on first access.
- **Args**: `RBX`=hint, `RCX`=len, `RDX`=prot, `R8`=flags(1=anon), `R9`=fd.
- **Returns**: Base address, or error code.

### 20 — `sys_munmap`
Unmaps a previously mmap'd region.
- **Args**: `RBX`=addr, `RCX`=len.
- **Returns**: `0` on success, or error code.

### 21 — `sys_loadlib`
Loads an NXL shared library from filesystem into a free slot.
- **Args**: `RBX`=path_ptr.
- **Returns**: Base address, or error code.

### 22 — `sys_thread_create`
Creates a new thread in the current process.
- **Args**: `RBX`=entry, `RCX`=stack.
- **Returns**: TID, or error code.

### 23 — `sys_thread_join`
Blocks until the specified thread terminates.
- **Args**: `RBX`=tid.
- **Returns**: `0` on success, or error code.

### 29 — `sys_set_exception_handler`
Sets SEH handler for current thread (A3.4). handler_fn=0 clears chain.
- **Args**: `RBX`=handler_fn.
- **Returns**: 0 success, -1 TEB not ready.

### 40 — `sys_wait_alertable`
Alertable wait. If APC pending, dispatches and returns `APC_ALERTED` (1).
- **Args**: None.
- **Returns**: `1` if APC delivered, `0` otherwise.

### 41 — `sys_sleep_ex`
Alertable yield. Checks APCs before/after yield.
- **Args**: None.
- **Returns**: `1` if APC delivered, `0` otherwise.

### 42 — `sys_poweroff`
Powers off the machine (QEMU debug port + ACPI S5 + PS/2 reset).
- **Args**: None.
- **Returns**: Does not return.

### 47 — `sys_chdir_parent` (LEGACY)
Changes CWD of the parent process. Used by `CD.NXE` via ARGS_ADDR.
- **Args**: `RBX` = path_ptr.
- **Returns**: `0` on success, or error code.

### 53 — `sys_cursor_blink`
Enable/disable automatic cursor blinking.
- **Args**: `RBX`=0 (disable), 1 (enable).
- **Returns**: `0` on success, or error code.

### 55 — `sys_fsck`
Run filesystem integrity check. Returns `FsckStats`.
- **Args**: `RBX`=buf_ptr, `RCX`=drive_char, `RDX`=repair_flag.
- **Returns**: `FsckStats` filled on success, or error.

### 58 — `sys_driver_unload`
Unload a NEM driver by name (admin).
- **Args**: `RBX`=name_ptr, `RCX`=force_flag.
- **Returns**: `0` on success, or error code.

### 59 — `sys_poll`
Poll fds for ready I/O.
- **Args**: `RBX`=pfds_ptr, `RCX`=nfds, `RDX`=timeout_ms.
- **Returns**: Ready count, or error code.

### 60 — `sys_ob_open`
Open an Ob namespace object by path. Security access check, allocates handle.
- **Args**: `RBX`=path_ptr, `RCX`=desired_access.
- **Returns**: fd (>=3), or error code.

### 61 — `sys_ob_create`
Create an Ob object. Types: 1=Process, 2=Driver, 4=Pipe, 11=Directory, 13=Event, 14=Semaphore, 15=Timer, 16=Thread, 17=Section, 18=Socket.
- **Args**: `RBX`=path_ptr, `RCX`=type, `RDX`=fds_out, `R8`=attrs.
  - `type=14 (Semaphore)`: `attrs[0:15]=initial_count`, `attrs[16:31]=max_count`
  - `type=15 (Timer)`: `attrs[0:30]=period_ms`, `attrs[31]=1 (periodic) / 0 (oneshot)`
  - `type=17 (Section)`: `attrs[0:31]=size`, `attrs[32:39]=prot`
- **Returns**: fd, or error code.

### 62 — `sys_ob_query_info`
Query object info. Classes: 0=Basic, 1=Name, 2=File, 3=Process, 4=Thread, 5=Pipe, 6=Device, 7=CpuInfo, 8=Version, 9=DateTime, 10=Memory, 11=Drives, 12=Drivers, 13=Cwd, 14=KeyboardLayout, 15=ReadContent, 16=VolumeLabel, 17=SocketInfo, 18=SocketAddr, 19=TcpStatus, 20=NicInfo, 21=RegistryKey, 22=RegistryValue, 23=SocketRecv.
- **Args**: `RBX`=fd, `RCX`=class, `RDX`=buf, `R8`=size.
- **Returns**: Bytes written, or error code.

### 63 — `sys_ob_set_info`
Set object info. Classes: 0=ProcessPriority, 1=ThreadPriority, 2=ObjectName, 3=Security, 4=ProcessTerminate, 5=KeyboardLayout, 6=VfsRename, 7=WriteContent, 8=SetCwd, 9=SetVolumeLabel, 10=TimerStart, 11=TimerCancel, 12=SemaphoreRelease, 13=SectionMapView, 14=SectionUnmapView, 15=FileCreate, 16=FileDelete, 17=SetProcessVt, 18=SocketConnect, 19=SocketBind, 20=SocketListen, 21=SocketSend, 22=SocketClose, 23=RegistryCreateKey, 24=RegistryDeleteKey, 25=RegistrySetValue, 26=RegistryDeleteValue, 27=SetNicIp.
- **Args**: `RBX`=fd, `RCX`=class, `RDX`=buf, `R8`=size.
  - `class=10 (TimerStart)`: Starts the timer. `buf` unused. Returns 0.
  - `class=11 (TimerCancel)`: Cancels a running timer. `buf` unused. Returns 0.
  - `class=12 (SemaphoreRelease)`: Releases (increments) the semaphore. `buf`=release_count (u32, default 1). Returns 0.
  - `class=13 (MapView)`: Maps a Section view. `buf`=output base address (u64). Returns base address.
  - `class=14 (UnmapView)`: Unmaps a Section view. `buf`=base address to unmap (u64). Returns 0.
- **Returns**: `0` on success, or error code.

### 64 — `sys_ob_enum`
Enumerate contents of an Ob directory by fd.
- **Args**: `RBX`=dir_fd, `RCX`=buf, `RDX`=max_entries.
- **Returns**: Entries written, or error code.

### 65 — `sys_ob_wait`
Wait on one or more Ob objects to become signaled.
- **Args**: `RBX`=count, `RCX`=handles, `RDX`=type (0=ANY, 1=ALL), `R8`=timeout.
- **Supported waitable types**: Process (ChildExit), Pipe (PipeRead), Event, Timer (TimerExpired), Thread (ThreadJoin), Semaphore (SemaphoreAvailable).
- **Non-blocking check**: For Pipe, Semaphore, and Timer, performs an immediate non-blocking check before blocking.
- **Returns**: `0` on success, or error code.

### 66 — `sys_ob_destroy`
Destroy/delete an Ob object by fd (file, directory, namespace object).
- **Args**: `RBX`=fd.
- **Returns**: `0` on success, or error code.

---

## Objectification Status

| Status | Syscalls | Description |
|--------|----------|-------------|
| Already Objects | RAX 60-66 (ob_open/create/query_info/set_info/enum/wait/destroy) | Native Ob operations, operate on namespace handles |
| Removed from SSDT (use Ob) | pipe(5), spawn(7), open(10), close(13), readfile(11), mkdir, unlink, rmdir, rename | Replaced by ob_create/ob_open/ob_destroy + VFS object paths |
| Foundation (intact) | exit, write, read, yield, getpid, brk, mmap, munmap, loadlib, thread_create/join, poll, poweroff | Not object operations; remain as direct syscalls |
| Registry | RAX 67-76 (Cm*) | Configuration Manager — cell-based hive syscalls |

## Architecture Rule

**Every new syscall (RAX >= 77) MUST be implemented as `sys_ob_*`** — it must
operate on Ob objects in the namespace, receive handles obtained via
`ob_open`/`ob_create`, and return results via `ob_query_info`/`ob_set_info`.

This rule enforces the NT-like design where the Object Manager is the central
abstraction for all kernel resources. Foundation syscalls (memory, process
control, I/O) may remain unchanged for ABI stability, but any genuinely new
kernel functionality must enter through the Ob architecture.

## Registry Syscalls (RAX 67-76)

| RAX | Syscall | Purpose |
|-----|---------|---------|
| 67 | `sys_cm_open_key` | Open existing registry key by path |
| 68 | `sys_cm_create_key` | Create or open registry key |
| 69 | `sys_cm_query_value` | Read a registry value |
| 70 | `sys_cm_set_value` | Write a registry value |
| 71 | `sys_cm_enum_key` | Enumerate subkeys |
| 72 | `sys_cm_enum_value` | Enumerate values in a key |
| 73 | `sys_cm_delete_key` | Delete a key and its subkeys |
| 74 | `sys_cm_flush_key` | Flush key to persistent storage |
| 75 | `sys_cm_load_hive` | Load a hive file (admin) |
| 76 | `sys_cm_unload_hive` | Unload a hive (admin) |
| 77 | `sys_ob_service` | Service control: START(0), STOP(1), RESTART(2), QUERY_STATUS(3), SET_CONFIG(4). Admin only. |
