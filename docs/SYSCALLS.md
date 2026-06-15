# NeoDOS 1.0 Stable Syscall ABI

This document defines the System Call Application Binary Interface (ABI) for NeoDOS.

## Calling Convention

NeoDOS user-mode applications execute in Ring 3. To request a kernel service, applications must trigger the software interrupt `INT 0x80`.

The registers used to pass arguments and receive the return value are:
- `RAX`: System call number (specifies which operation to perform).
- `RBX`: Argument 0
- `RCX`: Argument 1
- `RDX`: Argument 2
- `R8`:  Argument 3
- `R9`:  Argument 4

Upon return from `INT 0x80`, the kernel places the return value in `RAX`. Return convention: `≥ 0` success, `< 0` error (user checks `cmp rax, -1`).

All other general-purpose registers are preserved across system calls.

---

## Syscall Index

### 0 — `sys_exit`
Terminates the calling process and frees all allocated resources (heap, mmap, pipes, handles, kernel stack).
- **Arg0 (`RBX`)**: Exit code (`u64`). By convention, 0 means success.
- **Returns (`RAX`)**: Does not return to the caller.

### 1 — `sys_write`
Writes a buffer of bytes to a file descriptor (stdout or pipe writer).
- **Arg0 (`RBX`)**: File descriptor (`u64`, 1=stdout, 2=stderr, pipe writer fd).
- **Arg1 (`RCX`)**: Pointer to the output buffer (`*const u8`).
- **Arg2 (`RDX`)**: Number of bytes to write (`usize`).
- **Returns (`RAX`)**: Number of bytes written, or error code on failure.

### 2 — `sys_yield`
Voluntarily yields the CPU to the next ready process. The current process transitions Running→Ready, resets its time slice, and forces a reschedule.
- **Args**: None.
- **Returns (`RAX`)**: `0`.

### 3 — `sys_getpid`
Retrieves the Process ID (PID) of the calling process.
- **Args**: None.
- **Returns (`RAX`)**: The current PID (`u64`).

### 4 — `sys_read`
Reads data from a file descriptor (stdin or pipe reader).
- **Arg0 (`RBX`)**: File descriptor (`u64`, 0=stdin, pipe reader fd).
- **Arg1 (`RCX`)**: Pointer to the output buffer (`*mut u8`).
- **Arg2 (`RDX`)**: Maximum number of bytes to read (`usize`).
- **Returns (`RAX`)**: Number of bytes read, `-EAGAIN` if pipe empty (caller retries), or error.

### 5 — `sys_pipe`
Creates a unidirectional data pipe for inter-process communication.
- **Arg0 (`RBX`)**: Pointer to a `[u64; 2]` array to receive `[read_fd, write_fd]`.
- **Returns (`RAX`)**: `0` on success, or error code.

### 6 — `sys_dup2`
Duplicates an existing file descriptor to a target slot (used for redirection).
- **Arg0 (`RBX`)**: Source file descriptor to duplicate.
- **Arg1 (`RCX`)**: Target file descriptor number.
- **Returns (`RAX`)**: `0` on success, or error code.

### 7 — `sys_spawn`
Loads a user-mode binary from the NeoDOS filesystem and executes it in a new child process. Supports file descriptor redirection via `stdin_fd`, `stdout_fd`, `stderr_fd` (pass `0xFF` to inherit the parent's fd).
- **Arg0 (`RBX`)**: Pointer to a null-terminated UTF-8 file path (`*const u8`).
- **Arg1 (`RCX`)**: Stdin fd to redirect (`u8`, `0xFF` = inherit).
- **Arg2 (`RDX`)**: Stdout fd to redirect (`u8`, `0xFF` = inherit).
- **Arg3 (`R8`)**: Stderr fd to redirect (`u8`, `0xFF` = inherit).
- **Returns (`RAX`)**: PID of the child process on success, or error code.

### 8 — `sys_readdir`
Reads one directory entry from a HANDLE_DIR file descriptor. Returns a `DirEntryRaw` struct (inode, mode, size, name[260]) into the user buffer.
- **Arg0 (`RBX`)**: Directory fd (`u8`) obtained from `sys_open`.
- **Arg1 (`RCX`)**: Pointer to a `DirEntryRaw` output buffer (`*mut u8`).
- **Returns (`RAX`)**: `1` if an entry was read, `0` if end of directory, or error code.

### 9 — `sys_waitpid`
Blocks the calling process until the specified child process terminates.
- **Arg0 (`RBX`)**: PID of the process to wait for (`u32`).
- **Returns (`RAX`)**: The exit code of the child process.

### 10 — `sys_open`
Opens a file on the Virtual File System (VFS) and returns a file descriptor (handle index 0–15).
- **Arg0 (`RBX`)**: Pointer to a null-terminated UTF-8 string containing the file path (`*const u8`).
- **Arg1 (`RCX`)**: Open flags (`u64`, reserved).
- **Returns (`RAX`)**: File descriptor (`u8`) on success, or error code.

### 11 — `sys_readfile`
Reads data from an open file descriptor at the current offset.
- **Arg0 (`RBX`)**: File descriptor (`u8` returned by `sys_open`).
- **Arg1 (`RCX`)**: Pointer to the output buffer (`*mut u8`).
- **Arg2 (`RDX`)**: Number of bytes to read (`usize`).
- **Returns (`RAX`)**: Number of bytes successfully read, or error code.

### 12 — `sys_writefile`
Writes data to an open file descriptor at the current offset.
- **Arg0 (`RBX`)**: File descriptor (`u8` returned by `sys_open`).
- **Arg1 (`RCX`)**: Pointer to the input buffer (`*const u8`).
- **Arg2 (`RDX`)**: Number of bytes to write (`usize`).
- **Returns (`RAX`)**: Number of bytes written, or error code.

### 13 — `sys_close`
Closes a file descriptor (file, pipe, device, event). For pipes, decrements the reference count; the pipe buffer is freed when all references reach 0.
- **Arg0 (`RBX`)**: File descriptor to close (`u8`).
- **Returns (`RAX`)**: `0` on success, or error code.

### 14 — `sys_ioctl`
Performs device-specific I/O control (legacy, may be deprecated).
- **Arg0 (`RBX`)**: Device ID (`u32`).
- **Arg1 (`RCX`)**: Command code (`u32`).
- **Arg2 (`RDX`)**: Pointer to data buffer (`*mut u8`).
- **Returns (`RAX`)**: Device-specific response, or error code.

### 15 — `sys_register_device`
Registers the current process as the handler for a hardware device (legacy, may be deprecated).
- **Arg0 (`RBX`)**: Device ID to register (`u32`).
- **Returns (`RAX`)**: `0` on success, or error code.

### 16 — `sys_chdir`
Changes the current working directory of the calling process.
- **Arg0 (`RBX`)**: Pointer to a null-terminated UTF-8 path string (`*const u8`).
- **Returns (`RAX`)**: `0` on success, or error code.

### 17 — `sys_getcwd`
Returns the absolute path of the current working directory.
- **Arg0 (`RBX`)**: Pointer to the output buffer (`*mut u8`).
- **Arg1 (`RCX`)**: Buffer size (`usize`).
- **Returns (`RAX`)**: Number of bytes written (including null terminator), or error code.

### 18 — `sys_brk`
Adjusts the program break (end of the data segment). Physical pages are allocated on demand by the page fault handler — this syscall only moves the break pointer. Pages are touched (read+write) to trigger fault-based allocation.
- **Arg0 (`RBX`)**: New program break address. Pass `0` to query the current break.
- **Returns (`RAX`)**: The new (or current) program break on success, or error code.

### 19 — `sys_mmap`
Lazy memory mapping — registers a Virtual Memory Area (VMA) but does not allocate pages immediately. Pages are allocated on first access via the page fault handler. Supports anonymous mappings (flags=1) and file-backed mappings (flags=0, R9=fd). Region: `0x20000000..0x22000000` (32 MB).
- **Arg0 (`RBX`)**: Hint address (unused, reserved).
- **Arg1 (`RCX`)**: Length in bytes (`usize`).
- **Arg2 (`RDX`)**: Protection flags (1=R, 2=W).
- **Arg3 (`R8`)**: Flags (1=anonymous, 0=file-backed).
- **Arg4 (`R9`)**: File descriptor (for file-backed mappings).
- **Returns (`RAX`)**: Base address of the mapped region, or error code.

### 20 — `sys_munmap`
Unmaps a previously mmap'd region, freeing all physical pages and removing the VMA entry.
- **Arg0 (`RBX`)**: Base address of the region to unmap.
- **Arg1 (`RCX`)**: Length in bytes.
- **Returns (`RAX`)**: `0` on success, or error code.

### 21 — `sys_loadlib`
Loads a NeoDOS shared library (NXL) from the filesystem into a free slot in the NXL region (`0x1e000000..0x1e200000`, 8 × 256 KB slots). The ELF is parsed, sections mapped as read-only USER_ACCESSIBLE, and the export table becomes accessible at the returned base address.
- **Arg0 (`RBX`)**: Pointer to a null-terminated UTF-8 file path (`*const u8`).
- **Returns (`RAX`)**: Base address of the loaded NXL, or error code.

### 22 — `sys_thread_create`
Creates a new thread within the current process. The new thread begins executing at the given entry point with the provided stack.
- **Arg0 (`RBX`)**: Entry point address (`*const fn()`).
- **Arg1 (`RCX`)**: Stack base address.
- **Returns (`RAX`)**: Thread ID (TID), or error code.

### 23 — `sys_thread_join`
Blocks the calling thread until the specified thread terminates.
- **Arg0 (`RBX`)**: Thread ID (TID) to wait for.
- **Returns (`RAX`)**: `0` on success, or error code.

### 24 — `sys_getcpuinfo`
Copies a `CpuInfoFull` structure (CPUID vendor, brand, features, SMP topology, timer info) to the user buffer.
- **Arg0 (`RBX`)**: Pointer to output buffer (`*mut u8`).
- **Arg1 (`RCX`)**: Buffer size (`usize`).
- **Returns (`RAX`)**: Number of bytes written, or error code.

### 25 — `sys_mkdir`
Creates a new directory via the Virtual File System (VFS).
- **Arg0 (`RBX`)**: Pointer to a null-terminated UTF-8 path string (`*const u8`).
- **Returns (`RAX`)**: `0` on success, or error code.

### 26 — `sys_unlink`
Deletes a file via the Virtual File System (VFS). The file is removed from the directory tree and its data blocks are freed.
- **Arg0 (`RBX`)**: Pointer to a null-terminated UTF-8 path string (`*const u8`).
- **Returns (`RAX`)**: `0` on success, or error code.

### 27 — `sys_rmdir`
Removes an empty directory via the Virtual File System (VFS).
- **Arg0 (`RBX`)**: Pointer to a null-terminated UTF-8 path string (`*const u8`).
- **Returns (`RAX`)**: `0` on success, or error code.

### 28 — `sys_rename`
Renames a file or directory via the Virtual File System (VFS). The new name (leaf) is extracted from the second path argument.
- **Arg0 (`RBX`)**: Pointer to the current path (`*const u8`).
- **Arg1 (`RCX`)**: Pointer to the new path (leaf name extracted automatically) (`*const u8`).
- **Returns (`RAX`)**: `0` on success, or error code.

### 40 — `sys_wait_alertable`
Alertable wait. If a user APC is pending, dispatches it immediately and returns `APC_ALERTED` (1). Otherwise, blocks the calling thread in an alertable state — it can be woken by a queued APC.
- **Args**: None.
- **Returns (`RAX`)**: `1` (`APC_ALERTED`) if an APC was delivered, `0` if wait completed normally.

### 41 — `sys_sleep_ex`
Alertable yield. Yields the CPU, checking for pending APCs before and after the yield. If an APC was received, returns `APC_ALERTED` (1).
- **Args**: None.
- **Returns (`RAX`)**: `1` (`APC_ALERTED`) if an APC was delivered, `0` otherwise.

### 42 — `sys_poweroff`
Powers off the machine. Flushes caches, sends EVENT_SHUTDOWN, and attempts hardware poweroff via QEMU debug port, ACPI S5, and PS/2 reset.
- **Args**: None.
- **Returns (`RAX`)**: Does not return.

### 50 — `sys_ndreg`
Admin-only syscall for NDREG operations (kernel driver registry). Requires admin token.
- **Args**: Reserved.
- **Returns (`RAX`)**: Reserved.
