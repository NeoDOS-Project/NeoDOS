# NeoDOS Syscall ABI

NeoDOS user-mode applications execute in Ring 3. To request a kernel service, applications trigger software interrupt `INT 0x80`.

## Calling Convention

| Register | Purpose |
|----------|---------|
| `RAX` | Syscall number |
| `RBX` | Argument 0 |
| `RCX` | Argument 1 |
| `RDX` | Argument 2 |
| `R8` | Argument 3 |
| `R9` | Argument 4 |

Return in `RAX`: `≥ 0` success, `< 0` error. All other GPRs preserved.

---

## Syscall Index

### 0 — `sys_exit`
Terminates the calling process and frees all allocated resources.
- **Args**: `RBX` = exit code (`u64`). 0 = success.
- **Returns**: Does not return.

### 1 — `sys_write`
Writes a buffer to a file descriptor (stdout, stderr, pipe writer).
- **Args**: `RBX`=fd, `RCX`=buf, `RDX`=len.
- **Returns**: Bytes written, or error code.

### 2 — `sys_yield`
Voluntarily yields the CPU. Running→Ready, resets time slice, forces reschedule.
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

### 5 — `sys_pipe`
Creates a unidirectional data pipe.
- **Args**: `RBX` = pointer to `[read_fd, write_fd]` (`*mut u64`).
- **Returns**: `0` on success, or error code.

### 6 — `sys_dup2`
Duplicates an fd to a target slot (redirection).
- **Args**: `RBX`=old_fd, `RCX`=new_fd.
- **Returns**: `0` on success, or error code.

### 7 — `sys_spawn`
Loads and executes a user-mode binary in a new child process.
- **Args**: `RBX`=path_ptr, `RCX`=stdin_fd(0xFF=inherit), `RDX`=stdout_fd, `R8`=stderr_fd.
- **Returns**: PID on success, or error code.

### 8 — `sys_readdir`
Reads one directory entry from a HANDLE_DIR fd.
- **Args**: `RBX`=fd, `RCX`=buf_ptr (`*mut DirEntryRaw`).
- **Returns**: `1` = entry read, `0` = end of dir, or error.

### 9 — `sys_waitpid`
Blocks until the specified child process terminates.
- **Args**: `RBX` = PID (`u32`).
- **Returns**: Exit code on success, or error.

### 10 — `sys_open`
Opens a file on the VFS and returns an fd.
- **Args**: `RBX`=path_ptr, `RCX`=flags (reserved).
- **Returns**: fd (`u8`) on success, or error code.

### 11 — `sys_readfile`
Reads from an open file at the current offset.
- **Args**: `RBX`=fd, `RCX`=buf, `RDX`=count.
- **Returns**: Bytes read, or error code.

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
- **Returns**: fd (≥3), or error code.

### 61 — `sys_ob_create`
Create an Ob object. Types: 1=Process, 2=Driver, 4=Pipe, 11=Directory, 13=Event, 14=Semaphore, 15=Timer, 16=Thread, 17=Section.
- **Args**: `RBX`=path_ptr, `RCX`=type, `RDX`=fds_out, `R8`=attrs.
  - `type=14 (Semaphore)`: `attrs[0:15]=initial_count`, `attrs[16:31]=max_count`
  - `type=15 (Timer)`: `attrs[0:30]=period_ms`, `attrs[31]=1 (periodic) / 0 (oneshot)`
  - `type=17 (Section)`: `attrs[0:31]=size`, `attrs[32:39]=prot`
- **Returns**: fd, or error code.

### 62 — `sys_ob_query_info`
Query object info. Classes: 15=ReadContent, 16=VolumeLabel.
- **Args**: `RBX`=fd, `RCX`=class, `RDX`=buf, `R8`=size.
- **Returns**: Bytes written, or error code.

### 63 — `sys_ob_set_info`
Set object info. Classes: 4=ProcessTerminate, 5=KeyboardLayout, 6=VfsRename, 7=WriteContent, 8=SetCwd, 9=SetVolumeLabel, 10=TimerStart, 11=TimerCancel, 12=SemaphoreRelease, 13=MapView, 14=UnmapView.
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
