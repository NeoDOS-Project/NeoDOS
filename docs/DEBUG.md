# NeoDOS Debug Guide

## Overview

Debug NeoDOS using:
- **GDB** - CPU-level debugging (breakpoints, registers, memory)
- **QEMU Monitor** - Machine state inspection (memory, interrupts, TLB)
- **VNC Viewer** - GUI output visualization
- **Serial Console** - Bootloader output

## Prerequisites

```bash
# Install required packages
sudo apt install gdb qemu-system-x86 ovmf mtools dosfstools

# Verify installation
which gdb qemu-system-x86_64
ls /usr/share/OVMF/OVMF_CODE.fd
```

## Quick Start

### Terminal 1: Start QEMU with debug server

```bash
cd neodos
bash scripts/qemu-debug.sh
```

**Expected output:**
```
[*] NeoDOS QEMU Debug Session
[+] Created temporary OVMF_VARS: /tmp/OVMF_VARS_12345.fd
==========================================
Launching QEMU...
==========================================
VNC:           localhost:5900
QEMU Monitor:  localhost:4444
GDB:           localhost:1234
Serial:        this terminal
```

You should see bootloader messages:
```
========================================
NeoDOS Bootloader v0.1
========================================

[+] Fetching memory map...
    Total memory: 512.00 MB
    ...
```

### Terminal 2: Connect with GDB

```bash
cd neodos
gdb -x .gdbinit
```

**Expected output:**
```
GNU gdb (GNU Toolchain ...) ...
Reading symbols from neodos-kernel/target/x86_64-unknown-none/release/neodos_kernel...
Reading symbols from /home/.../neodos/.gdbinit...
...
Kernel entry point reached!
RAX: 0x0000000000000000
...
```

GDB will:
1. Connect to QEMU's GDB server (port 1234)
2. Load kernel symbols
3. Set breakpoint at kernel entry (0x200000)
4. Print kernel state automatically
5. Wait for commands

### Terminal 3: View VNC Output

```bash
vncviewer localhost:5900
```

You should see kernel output on the virtual screen:
```
========================================
NeoDOS Kernel v0.1
========================================

[+] Kernel Entry Point
    ...
```

## GDB Workflow

### Connect Without .gdbinit

```bash
gdb
(gdb) target remote localhost:1234
(gdb) file neodos-kernel/target/x86_64-unknown-none/release/neodos_kernel
(gdb) break *0x200000
(gdb) continue
```

### Common GDB Commands

| Command | Purpose |
|---------|---------|
| `c` or `continue` | Resume execution |
| `n` or `next` | Execute next instruction |
| `s` or `step` | Step into call |
| `p $rax` | Print RAX register |
| `p /x $cr3` | Print CR3 in hex |
| `p /i $rip` | Print instruction at RIP |
| `x /16x 0x200000` | Dump memory (16 hex qwords) |
| `x /32i 0x200000` | Disassemble (32 instructions) |
| `info registers` | Show all registers |
| `info break` | List breakpoints |
| `break *0x200100` | Set breakpoint at address |
| `watch $rsp` | Set watchpoint on RSP |
| `info watchpoints` | List watchpoints |
| `bt` | Print stack backtrace |
| `quit` | Exit GDB |

### Inspect Kernel State

```gdb
(gdb) p /x $rax                  # RAX register
(gdb) p /x $rbx                  # RBX (kernel load addr = 0x200000)
(gdb) p /x $rsi                  # RSI (RuntimeServices ptr)
(gdb) p /x $rsp                  # RSP (stack pointer)
(gdb) p /x $cr0                  # CR0 (paging enabled?)
(gdb) p /x $cr3                  # CR3 (page table root)
(gdb) p /x $cr4                  # CR4 (paging features)
(gdb) p /x $rip                  # RIP (instruction pointer)
```

### Disassemble Kernel

```gdb
(gdb) x /32i 0x200000            # First 32 instructions
(gdb) x /32i $rip                # Around current RIP
(gdb) disassemble /m _start      # Disassemble _start function
```

### Memory Inspection

```gdb
(gdb) x /16x 0x200000            # 16 qwords (128 bytes) at kernel entry
(gdb) x /16x 0xB8000             # VGA text buffer
(gdb) x /32b 0x200000            # 32 bytes
(gdb) x /32s 0xB8000             # String dump of VGA buffer
```

### Breakpoints

```gdb
(gdb) break *0x200000            # At kernel entry
(gdb) break *0x200100            # At specific address
(gdb) info break                 # List all breakpoints
(gdb) delete 1                   # Delete breakpoint 1
(gdb) disable 1                  # Disable breakpoint 1
(gdb) enable 1                   # Re-enable breakpoint 1
```

### Conditional Breakpoints

```gdb
(gdb) break *0x200000 if $rax == 0
(gdb) break *0x200000 if $rsp > 0x80000000
```

### Custom Commands (in .gdbinit)

```gdb
(gdb) kernel_state              # Show all CPU state
```

This is defined in `.gdbinit` to display:
- RIP, RSP, RBP
- CR3, CR4

## QEMU Monitor

Connect with telnet:

```bash
telnet localhost 4444
```

You'll see a prompt:
```
QEMU 7.2.0 monitor - type 'help' for more information
(qemu)
```

### Common Monitor Commands

| Command | Purpose |
|---------|---------|
| `info registers` | All CPU registers |
| `info tlb` | TLB entries (paging) |
| `info mem` | Memory regions |
| `info irq` | Interrupt status |
| `x /16x 0x200000` | Memory dump |
| `p $rax` | Print RAX |
| `p $cr3` | Print CR3 |
| `help` | List all commands |
| `quit` | Disconnect monitor |

### Inspect Physical Memory

```
(qemu) x /16x 0x200000      # First 128 bytes of kernel
(qemu) x /16x 0xB8000       # VGA text buffer
(qemu) x /32b 0x100000      # Bootloader code
```

### Inspect CPU State

```
(qemu) info registers        # All registers + control regs
(qemu) p $cr0                # Paging control
(qemu) p $cr3                # Page table root
(qemu) p $cr4                # Paging features
(qemu) p $rip                # Instruction pointer
(qemu) p $rsp                # Stack pointer
```

### TLB and Paging

```
(qemu) info tlb              # TLB entries
(qemu) info mem              # Memory layout
```

## VNC Viewer

### Connect

```bash
vncviewer localhost:5900
```

Or use a VNC client:
- **Linux:** `vncviewer` (TigerVNC), `krdc` (KDE)
- **macOS:** `open vnc://localhost:5900`
- **Windows:** VNC Viewer, TightVNC

### View Kernel Output

You should see:
```
========================================
NeoDOS Kernel v0.1
========================================

[+] Kernel Entry Point
    Load address:     0x200000
    Magic:            0xneodkrn

[+] CPU Information
    Vendor:           Intel
    Brand:            Intel(R) Core(TM) i7-8700K CPU @ 3.70GHz

[+] Paging & CPU State
    CR3 (Page root):  0x0000000000000000
    CR4 (Features):   0x00000000000b0671

[+] Stack Information
    RSP:              0x0000000080000000

========================================
NeoDOS Kernel Ready
========================================
```

## Serial Console

The terminal running `qemu-debug.sh` shows serial output:
- Bootloader messages from `ConOut`
- QEMU debug messages
- Any serial output from kernel (if implemented)

**Example:**
```
========================================
NeoDOS Bootloader v0.1
========================================

[+] Fetching memory map...
    Total memory: 512.00 MB
    Available:    491.00 MB

[+] Loading kernel binary...
    Kernel loaded: 4096 bytes
    Kernel magic: 0xneodkrn
    Magic verified ✓

[+] Copying kernel to 0x200000...
    Kernel copied ✓

[+] Exiting boot services...
[+] Jumping to kernel @ 0x200000...
```

## Troubleshooting

### GDB Won't Connect

```bash
# Check if QEMU is running
ps aux | grep qemu

# Check port 1234
lsof -i :1234

# Try manual connection
gdb
(gdb) target remote localhost:1234
(gdb) continue
```

### QEMU Starts but No Output

1. **Check serial console** (Terminal 1)
   - Should show bootloader messages
   
2. **Check VNC connection** (Terminal 3)
   - Should show kernel VGA output
   
3. **Check QEMU Monitor** (Terminal 2)
   ```
   telnet localhost 4444
   (qemu) info registers    # Verify CPU is running
   ```

### Kernel Doesn't Print to VGA

1. **Check kernel magic**
   ```gdb
   (gdb) x /4x 0x200000
   ```
   Should show `0xNEODKRN` (magic header)

2. **Check CR3 and CR4**
   ```gdb
   (gdb) p /x $cr3      # Should be non-zero
   (gdb) p /x $cr4      # Should have PG bit set
   ```

3. **Verify VGA buffer is being written**
   ```gdb
   (gdb) x /16x 0xB8000     # Should see ASCII characters
   ```

### Build Errors

**"target not found"**
```bash
rustup target add x86_64-unknown-uefi
rustup target add x86_64-unknown-none
```

**"ovmf not found"**
```bash
sudo apt install ovmf
ls /usr/share/OVMF/
```

**Linker errors**
```bash
# Clean build
cargo clean
bash scripts/build.sh
```

## Advanced Debugging

### Trace Instructions

Enable instruction tracing in QEMU (slower):
```bash
# Modify qemu-debug.sh to add:
# -d in_asm,cpu,int
```

### Dump Page Tables

```gdb
(gdb) x /512x 0x...       # Depends on CR3 value
```

### Monitor CPU Cycles

```gdb
(gdb) watch $rip
(gdb) continue  # Stops at every instruction
```

This is very slow but shows exact execution flow.

### Kernel Panic Debugging

If kernel panics:
1. GDB will break at panic location
2. Inspect stack: `(gdb) x /32x $rsp`
3. Check panic message in VGA output
4. Look at `panic()` in `src/main.rs`

## Performance Profiling

### Enable QEMU Profiling

```bash
# In qemu-debug.sh, add:
# -d trace:...
# -trace events=events.txt
```

Then analyze performance bottlenecks.

## References

- [GDB Manual](https://sourceware.org/gdb/documentation/)
- [QEMU Monitor](https://qemu-project.gitlab.io/qemu/system/monitor.html)
- [VNC Protocol](https://en.wikipedia.org/wiki/Virtual_Network_Computing)
- [x86-64 Debugging](https://wiki.osdev.org/X86-64)

## Summary

**Typical debug workflow:**
1. Terminal 1: `bash scripts/qemu-debug.sh` (QEMU + serial output)
2. Terminal 2: `gdb -x .gdbinit` (GDB debugging)
3. Terminal 3: `vncviewer localhost:5900` (VGA output)
4. Inspect state via GDB commands or Monitor
5. Use Ctrl+A, X to exit QEMU

Happy debugging! 🐛
