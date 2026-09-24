# NeoDOS Syscall ABI (v8 — SSDT Reorganized)

NeoDOS user-mode applications execute in Ring 3. To request a kernel service,
applications trigger software interrupt `INT 0x80`.

## Calling Convention

| Register | Purpose |
| ---------- | --------- |
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
    // Process (0-9)
    Exit = 0, Yield = 1, WaitAlertable = 2, SleepEx = 3,
    SetExceptionHandler = 4,
    // Memory (10-19)
    Brk = 10, Mmap = 11, Munmap = 12,
    // I/O (20-29)
    Write = 20, Read = 21, Dup2 = 22, Close = 23,
    Poll = 24, LoadLib = 25,
    // Console (30-34)
    CursorBlink = 30,
    // Driver (35-39)
    DriverUnload = 35,
    // Object Manager (40-49)
    ObOpen = 40, ObCreate = 41, ObQueryInfo = 42, ObSetInfo = 43,
    ObEnum = 44, ObWait = 45, ObDestroy = 46, ObService = 47,
    // Registry Cm (50-59)
    CmOpenKey = 50, CmCreateKey = 51, CmQueryValue = 52,
    CmSetValue = 53, CmEnumKey = 54, CmEnumValue = 55,
    CmDeleteKey = 56, CmFlushKey = 57, CmLoadHive = 58, CmUnloadHive = 59,
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

### SSDT (Syscall Service Dispatch Table)

Dispatch uses a fixed-size array `[Option<SyscallFn>; 256]` indexed by RAX.
Each handler has type `fn(Registers) -> u64`. The table is initialized in
a `lazy_static!` block at `syscall/mod.rs`.

`validate_abi()` is called at boot (Phase 3.9) to assert that every assigned
syscall number has a registered handler:

```rust
pub fn validate_abi() {
    const ASSIGNED: &[u64] = &[
        0, 1, 2, 3, 4,
        10, 11, 12,
        20, 21, 22, 23, 24, 25,
        30, 35,
        40, 41, 42, 43, 44, 45, 46, 47,
        50, 51, 52, 53, 54, 55, 56, 57, 58, 59,
    ];
    for &n in ASSIGNED {
        assert!(SYSCALL_TABLE[n as usize].is_some(),
                "SSDT missing handler for assigned syscall {}", n);
    }
}
```

### Permission System

Each syscall slot has a `SyscallPermission` with an `admin` flag. Admin
syscalls (e.g., `DriverUnload=35`, `CmLoadHive=58`) require the calling
process to hold an admin token (`token.is_admin_token()`). Non-admin calls
return `-Perm`.

---

## Syscall Index

### Process (RAX 0-4)

#### 0 — `sys_exit`

Terminates the calling process and frees all allocated resources.

- **Args**: `RBX` = exit code (`u64`). 0 = success.
- **Returns**: Does not return.

#### 1 — `sys_yield`

Voluntarily yields the CPU. Running->Ready, resets time slice, forces reschedule.

- **Args**: None.
- **Returns**: `0`.

#### 2 — `sys_wait_alertable`

Alertable wait. If APC pending, dispatches and returns `APC_ALERTED` (1).

- **Args**: None.
- **Returns**: `1` if APC delivered, `0` otherwise.

#### 3 — `sys_sleep_ex`

Alertable yield. Checks APCs before/after yield.

- **Args**: None.
- **Returns**: `1` if APC delivered, `0` otherwise.

#### 4 — `sys_set_exception_handler`

Sets SEH handler for current thread. `handler_fn=0` clears chain.

- **Args**: `RBX`=handler_fn.
- **Returns**: 0 success, -1 TEB not ready.

### Memory (RAX 10-12)

#### 10 — `sys_brk`

Adjusts program break. Pages allocated on demand via page fault handler.

- **Args**: `RBX` = new break (0 = query).
- **Returns**: New/current break, or error code.

#### 11 — `sys_mmap`

Lazy mapping: registers VMA without pages. Allocated on first access.

- **Args**: `RBX`=hint, `RCX`=len, `RDX`=prot, `R8`=flags(1=anon), `R9`=fd.
- **Returns**: Base address, or error code.

#### 12 — `sys_munmap`

Unmaps a previously mmap'd region.

- **Args**: `RBX`=addr, `RCX`=len.
- **Returns**: `0` on success, or error code.

### I/O (RAX 20-25)

#### 20 — `sys_write`

Writes a buffer to a file descriptor (stdout, stderr, pipe writer, Filesystem handle).

- **Args**: `RBX`=fd, `RCX`=buf, `RDX`=len.
- **Returns**: Bytes written, or error code.

#### 21 — `sys_read`

Reads data from a file descriptor (stdin, pipe reader). Blocks if pipe empty.

- **Args**: `RBX`=fd, `RCX`=buf, `RDX`=count.
- **Returns**: Bytes read, or error.

#### 22 — `sys_dup2`

Duplicates an fd to a target slot (redirection).

- **Args**: `RBX`=old_fd, `RCX`=new_fd.
- **Returns**: `0` on success, or error code.

#### 23 — `sys_close`

Closes an fd (file, pipe, device, event). For pipes, decrements refcount.

- **Args**: `RBX`=fd.
- **Returns**: `0` on success, or error code.

#### 24 — `sys_poll`

Poll fds for ready I/O.

- **Args**: `RBX`=pfds_ptr, `RCX`=nfds, `RDX`=timeout_ms.
- **Returns**: Ready count, or error code.

#### 25 — `sys_loadlib`

Loads an NXL shared library from filesystem into a free slot.

- **Args**: `RBX`=path_ptr.
- **Returns**: Base address, or error code.

### Console (RAX 30)

#### 30 — `sys_cursor_blink`

Enable/disable automatic cursor blinking.

- **Args**: `RBX`=0 (disable), 1 (enable).
- **Returns**: `0` on success, or error code.

### Driver (RAX 35)

#### 35 — `sys_driver_unload` (admin)

Unload a NEM driver by name.

- **Args**: `RBX`=name_ptr, `RCX`=force_flag.
- **Returns**: `0` on success, or error code.

### Object Manager (RAX 40-48)

#### 40 — `sys_ob_open`

Open an Ob namespace object by path. Security access check, allocates handle.

- **Args**: `RBX`=path_ptr, `RCX`=desired_access.
- **Returns**: fd (>=3), or error code.

#### 41 — `sys_ob_create`

Create an Ob object. Types: 1=Process, 2=Driver, 4=Pipe, 11=Directory,
13=Event, 14=Semaphore, 15=Timer, 16=Thread, 17=Section, 18=Socket,
21=PowerManager, 22=KeyboardDevice.

- Kernel-created (NOT user-creatable): `PowerManager(21)`, `KeyboardDevice(22)`.
- **Args**: `RBX`=path_ptr, `RCX`=type, `RDX`=fds_out, `R8`=attrs.
- **Returns**: fd, or error code.

#### 42 — `sys_ob_query_info`

Query object info by fd and info class (see `ObInfoClass`).

- **Args**: `RBX`=fd, `RCX`=class, `RDX`=buf, `R8`=size.
- **Returns**: Bytes written, or error code.

#### 43 — `sys_ob_set_info`

Set object info by fd and set-info class (see `ObSetInfoClass`).

- **Args**: `RBX`=fd, `RCX`=class, `RDX`=buf, `R8`=size.
- **Returns**: `0` on success, or error code.

#### 44 — `sys_ob_enum`

Enumerate contents of an Ob directory by fd.

- **Args**: `RBX`=dir_fd, `RCX`=buf, `RDX`=max_entries.
- **Returns**: Entries written, or error code.

#### 45 — `sys_ob_wait`

Wait on one or more Ob objects to become signaled.

- **Args**: `RBX`=count, `RCX`=handles, `RDX`=type, `R8`=timeout.
- **Supported**: Process (ChildExit), Pipe, Event, Timer, Thread, Semaphore.
- **Returns**: `0` on success, or error code.

#### 46 — `sys_ob_destroy`

Destroy/delete an Ob object by fd (file, directory, namespace object).

- **Args**: `RBX`=fd.
- **Returns**: `0` on success, or error code.

#### 47 — `sys_ob_service` (admin)

Service control: START(0), STOP(1), RESTART(2), QUERY_STATUS(3), SET_CONFIG(4).

- **Args**: `RBX`=fd, `RCX`=control, `RDX`=buf, `R8`=size.
- **Returns**: Bytes written (for QUERY_STATUS), or error code.

#### 48 — `sys_ob_snapshot` (admin)

Filesystem snapshot operations: CREATE(0), RESTORE(1), LIST(2), PURGE(3).

- **Args**: `RBX`=fd, `RCX`=op, `RDX`=buf, `R8`=size.
- **Returns**: snapshot_id (CREATE), count (LIST), or error code.

### Registry (RAX 50-59)

| RAX | Syscall | Purpose |
| ----- | --------- | --------- |
| 50 | `sys_cm_open_key` | Open existing registry key by path |
| 51 | `sys_cm_create_key` | Create or open registry key |
| 52 | `sys_cm_query_value` | Read a registry value |
| 53 | `sys_cm_set_value` | Write a registry value |
| 54 | `sys_cm_enum_key` | Enumerate subkeys |
| 55 | `sys_cm_enum_value` | Enumerate values in a key |
| 56 | `sys_cm_delete_key` | Delete a key and its subkeys |
| 57 | `sys_cm_flush_key` | Flush key to persistent storage |
| 58 | `sys_cm_load_hive` (admin) | Load a hive file |
| 59 | `sys_cm_unload_hive` (admin) | Unload a hive |

---

## SSDT Reorganization History

### v0.49 → v0.50 (Current)

Complete SSDT audit, cleanup, and reorganization:

**Syscalls removed** (dead enum variants + handler code):

- ReadDir (was 8), WaitPid (was 9), WriteFile (was 12)
- ThreadCreate (was 22), ThreadJoin (was 23)
- GetVolumeLabel (was 46), SetVolumeLabel (was 54)
- KObjEnum (was 48, never implemented)
- SetPriority (was 51), KillProcess (was 52)
- DriverLoad (was 57)

**Legacy handlers migrated to Object Manager**:

- ChDir → `ob_set_info(SetCwd)` (was 16)
- ChDirParent → `ob_set_info(SetCwd)` on parent (was 47)

**Legacy wrappers removed from libneodos**:

- sys_get_volume_label, sys_set_volume_label, sys_set_priority
- sys_kill, sys_driver_load, sys_kobj_enum

### New Numbering Scheme

| Category | Range | Count | Admin |
| ---------- | ------- | ------- | ------- |
| Process Control | 0-4 | 5 | — |
| Memory | 10-12 | 3 | — |
| I/O | 20-25 | 6 | — |
| Console | 30 | 1 | — |
| Driver | 35 | 1 | ✓ |
| Object Manager | 40-47 | 8 | 47 |
| Registry | 50-59 | 10 | 58-59 |

Total active: 34 syscalls | Reserved slots: 25 | Highest: 59

### Architecture Rule

**Every new syscall MUST be implemented as `sys_ob_*`** — it must operate on
Ob objects in the namespace, receive handles via `ob_open`/`ob_create`, and
return results via `ob_query_info`/`ob_set_info`.

## Power Manager Object (via ObSetInfoClass/ObQueryInfoClass)

Power management is handled entirely via the Object Manager — no dedicated syscall.

| Path | Operation | Class |
| ------ | ----------- | ------- |
| `\System\PowerManager` | Query power system state | `ObQueryInfoClass::PowerState = 32` |
| `\System\PowerManager` | Query active plan info | `ObQueryInfoClass::PowerPlanInfo = 33` (planned) |
| `\System\PowerManager` | Query power capabilities | `ObQueryInfoClass::PowerStatus = 34` (planned) |
| `\System\PowerManager` | Shutdown (power off) | `ObSetInfoClass::PowerShutdown = 37` |
| `\System\PowerManager` | Reboot | `ObSetInfoClass::PowerReboot = 38` |
| `\System\PowerManager` | Suspend to RAM | `ObSetInfoClass::PowerSuspend = 39` (planned) |
| `\System\PowerManager` | Hibernate to disk | `ObSetInfoClass::PowerHibernate = 40` (planned) |
| `\System\PowerManager` | Set active power plan | `ObSetInfoClass::PowerSetPlan = 41` (planned) |
| `\System\PowerManager` | Set power policy value | `ObSetInfoClass::PowerSetPolicy = 42` (planned) |

**Usage:**

```c
fd = sys_ob_open("\\System\\PowerManager", WRITE);
sys_ob_set_info(fd, PowerShutdown, NULL, 0);  // or PowerReboot
```
