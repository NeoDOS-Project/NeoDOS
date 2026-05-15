# NeoDOS 1.0 Stable Syscall ABI

This document defines the stable System Call Application Binary Interface (ABI) for NeoDOS version 1.0. This ABI is guaranteed not to change in breaking ways across 1.x releases, allowing developers to build long-term compatible user-mode software.

## Calling Convention

NeoDOS user-mode applications execute in Ring 3. To request a kernel service, applications must trigger the software interrupt `INT 0x80`.

The registers used to pass arguments and receive the return value are:
- `RAX`: System call number (specifies which operation to perform).
- `RBX`: Argument 0
- `RCX`: Argument 1
- `RDX`: Argument 2

Upon return from `INT 0x80`, the kernel places the return value in `RAX`. Error conditions are typically indicated by `u64::MAX` (or `-1` cast to unsigned) in `RAX`.

All other general-purpose registers are preserved across system calls.

---

## Syscall Index

### 0 - `sys_exit`
Terminates the calling process and frees its allocated resources.
- **Arg0 (`RBX`)**: Exit code (`u64`). By convention, 0 means success.
- **Returns (`RAX`)**: Does not return to the caller.

### 1 - `sys_write`
Writes a buffer of bytes to the standard output (console).
- **Arg0 (`RBX`)**: Pointer to the string/buffer (`*const u8`).
- **Arg1 (`RCX`)**: Length of the buffer in bytes (`usize`).
- **Returns (`RAX`)**: Number of bytes written on success, or `u64::MAX` on error (e.g., bad memory address or length > 4096).

### 2 - `sys_yield`
Voluntarily yields the CPU to the next ready process in the scheduler.
- **Args**: None.
- **Returns (`RAX`)**: `0`.

### 3 - `sys_getpid`
Retrieves the Process ID (PID) of the current process.
- **Args**: None.
- **Returns (`RAX`)**: The current PID (`u64`).

### 4 - `sys_read`
Reads data from standard input (keyboard buffer).
- **Arg0 (`RBX`)**: File descriptor (must be `0` for stdin).
- **Arg1 (`RCX`)**: Pointer to the output buffer (`*mut u8`).
- **Arg2 (`RDX`)**: Maximum number of bytes to read (`usize`).
- **Returns (`RAX`)**: Number of bytes successfully read, or `u64::MAX` on error.

### 9 - `sys_waitpid`
Blocks the calling process until the specified child process terminates.
- **Arg0 (`RBX`)**: PID of the process to wait for (`u32`).
- **Returns (`RAX`)**: The exit code of the child process.

### 10 - `sys_open`
Opens a file on the Virtual File System (VFS).
- **Arg0 (`RBX`)**: Pointer to a null-terminated UTF-8 string containing the file path (`*const u8`).
- **Arg1 (`RCX`)**: Open flags (`u64` - reserved/unused currently).
- **Returns (`RAX`)**: File handle (`u64`) on success, or `u64::MAX` if the file could not be found or opened.

### 11 - `sys_readfile`
Reads data from an open file handle.
- **Arg0 (`RBX`)**: File handle (`u64` returned by `sys_open`).
- **Arg1 (`RCX`)**: Pointer to the output buffer (`*mut u8`).
- **Arg2 (`RDX`)**: Number of bytes to read (`usize`).
- **Returns (`RAX`)**: Number of bytes successfully read, or `u64::MAX` on error.

### 12 - `sys_writefile`
Writes data to an open file handle.
- **Arg0 (`RBX`)**: File handle (`u64` returned by `sys_open`).
- **Arg1 (`RCX`)**: Pointer to the input buffer (`*const u8`).
- **Arg2 (`RDX`)**: Number of bytes to write (`usize`).
- **Returns (`RAX`)**: Number of bytes successfully written, or `u64::MAX` on error.

### 13 - `sys_close`
Closes an open file descriptor or file handle.
- **Arg0 (`RBX`)**: File descriptor / File handle to close (`u64`).
- **Returns (`RAX`)**: `0` on success.

### 14 - `sys_ioctl`
Performs device-specific I/O control operations.
- **Arg0 (`RBX`)**: Device ID (`u32`).
- **Arg1 (`RCX`)**: Command code (`u32`).
- **Arg2 (`RDX`)**: Pointer to data buffer (`*mut u8`). If `0`, checks for pending events (poll mode).
- **Returns (`RAX`)**: Device-specific response or byte count, `1` if polling has pending events, `0` if no events, or `u64::MAX` on error.

### 15 - `sys_register_device`
Registers the current process as the handler for a specific hardware device.
- **Arg0 (`RBX`)**: Device ID to register (`u32`).
- **Returns (`RAX`)**: `0` on success, or `u64::MAX` if registration fails.
