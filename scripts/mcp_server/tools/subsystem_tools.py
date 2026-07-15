"""
tools/subsystem_tools.py — Subsystem analysis tools for Security, Scheduler, IPC, Boot, Memory.

Provides read-only introspection of NeoDOS kernel subsystems, extracting
architecture and design info from documentation and source code.
"""

import re
from pathlib import Path

NEODOS_ROOT: Path = None
KERNEL_SRC: Path = None


def configure(root_dir: str):
    global NEODOS_ROOT, KERNEL_SRC
    NEODOS_ROOT = Path(root_dir)
    KERNEL_SRC = NEODOS_ROOT / "neodos-kernel" / "src"


def _read_doc(name: str) -> str:
    path = NEODOS_ROOT / "docs" / name
    if path.exists():
        return path.read_text()
    return ""


# ── Security ──

SECURITY_SOURCE_FILES = [
    "src/security/sid.rs", "src/security/token.rs", "src/security/acl.rs",
    "src/security/access.rs", "src/security/sam.rs", "src/security/mod.rs",
]

SECURITY_SUMMARY = """Security Reference Monitor — NT6-style

SID (src/security/sid.rs):
  Format: S-R-I-S* (revision=1, authority=5 for NT Authority)
  Builtins: sid_builtin_admin() → S-1-5-18, sid_builtin_user() → S-1-5-21-0-0-0-1000

Token (src/security/token.rs):
  Attached to every EPROCESS. Factory: new_admin() (0xFFFF privs), new_user() (SE_CHANGE_NOTIFY)
  Inherited at process spawn via inherit_from(parent)
  12 privilege flags (SE_CREATE_TOKEN through SE_MANAGE_VOLUME)

ACL (src/security/acl.rs):
  Canonical ACE order: all Deny before Allow. Use insert_ace_canonical().
  Access: ACCESS_READ(1), WRITE(2), EXECUTE(4), DELETE(8), ALL(0xFFFF)

SeAccessCheck (src/security/access.rs):
  1. Admin bypass → grant  2. Empty DACL → deny  3. Deny ACEs first
  4. Allow ACEs → grant if all bits covered  5. Fallback → deny

SAM (src/security/sam.rs):
  Flat-file database, max 64 entries. Binary format with "SAM\\0" magic.
  Flags: SAM_FLAG_ADMIN(1), DISABLED(2), LOCKED(4)
"""


def security_info() -> str:
    """Show security subsystem: SIDs, tokens, ACL, SAM, access checks."""
    lines = [SECURITY_SUMMARY, "Source files:"]
    for sf in SECURITY_SOURCE_FILES:
        path = KERNEL_SRC.parent / sf if KERNEL_SRC else None
        if path and path.exists():
            flines = sum(1 for _ in open(path))
            lines.append(f"  {sf} ({flines} lines)")
        else:
            lines.append(f"  {sf} (not found)")
    lines.append("")

    doc = _read_doc("security.md")
    if doc:
        test_section = ""
        for line in doc.splitlines():
            if "Tests" in line or "test" in line.lower():
                test_section += "  " + line + "\n"
        if test_section:
            lines.append("Tests (from docs):")
            lines.append(test_section)

    return "\n".join(lines)


# ── Scheduler ──

SCHEDULER_SUMMARY = """Scheduler — Preemptive priority scheduler with SMP support

Priorities: 0 (Idle) to 255 (Real-time), default 16 (Normal)
Aging: priority boosted every 50ms if starved
Timeslice: 4 ticks (each tick = 1ms at 1KHz APIC timer)
Run queues: per-priority linked list of KTHREADs
SMP: work stealing when a CPU's run queue is empty

Allowed schedule() call sites:
  - Syscall return (NEED_RESCHED set)
  - Timer tick preempting Ring 3 (CS=0x1B)
  - sys_yield from Ring 3
  - sys_exit (final reschedule)

EPROCESS: PID, token, handle table, VT number, memory context
KTHREAD: state (Ready/Running/Blocked/Zombie), priority, TEB, kernel stack

Key files:
  src/scheduler/mod.rs — run queues, schedule(), process lifecycle
  src/scheduler/kthread.rs — thread state, context switch
  src/globals.rs — CURRENT_PROCESS, NEED_RESCHED
"""


def scheduler_info() -> str:
    """Show scheduler: processes, threads, priorities, run queue, SMP."""
    lines = [SCHEDULER_SUMMARY, "Source files:"]
    sched_dir = KERNEL_SRC / "scheduler" if KERNEL_SRC else None
    if sched_dir and sched_dir.exists():
        for f in sorted(sched_dir.rglob("*.rs")):
            flines = sum(1 for _ in open(f))
            rel = f.relative_to(KERNEL_SRC)
            lines.append(f"  src/{rel} ({flines} lines)")
    lines.append("")
    lines.append("Architectural invariants (from ARCHITECTURE_SOURCE_OF_TRUTH.md):")
    lines.append("  INV-4: No scheduler from Ring 0 except 4 allowed sites")
    lines.append("  INV-6: Every process slot is free or valid")
    lines.append("  INV-7: No interrupt-stack execution of scheduler code")
    return "\n".join(lines)


# ── IPC ──

IPC_SUMMARY = """IPC Subsystem — Pipes, IRP, Work Queue, Event Bus

Pipes (src/object/pipe.rs):
  Max 16 pipes, 4KB ring buffer each. Reference-counted, auto-freed.
  Blocking reads: ThreadState::Blocked { waiting_for: 0xFFFF_0000 | pipe_id }
  pipe_write returns -EPIPE if no readers. pipe_read returns 0 (EOF) if no writers.

Handle Table (src/handle.rs):
  Per-EPROCESS Vec<HandleEntry>. FD 0=Stdin, 1=Stdout, 2=Stderr, 3+=allocated.
  HandleEntry: { object_id: ObId, offset: u64 }
  Cleanup on exit: close pipes + ob_close on Ob handles.

IRP (src/irp/mod.rs):
  64-slot global pool, sequential IDs. Per-device FIFO queue (32 entries).
  ops: Read(0), Write(1), Flush(2), Discard(3), Ioctl(4)
  BlockDevice trait: 5 implementors (RamDisk, BootAta, AhciDriver, NvmeDriver, NemBlockDevice)

Work Queue (src/work_queue.rs):
  Two-level lock-free SPSC: high (syscall return) + low (idle loop), 64 slots each.

Event Bus v2 (src/eventbus/mod.rs):
  32+ event types (0=TimerTick...31=KeyRepeat), 0x1000+=user.
  High queue 16 slots, normal 64 slots. Max 64 handlers.
"""


def ipc_info() -> str:
    """Show IPC subsystem: pipes, IRP, work queue, event bus."""
    lines = [IPC_SUMMARY, "Source files:"]
    for path, label in [
        (KERNEL_SRC / "object" / "pipe.rs" if KERNEL_SRC else None, "src/object/pipe.rs"),
        (KERNEL_SRC / "handle.rs" if KERNEL_SRC else None, "src/handle.rs"),
        (KERNEL_SRC / "irp" / "mod.rs" if KERNEL_SRC else None, "src/irp/mod.rs"),
        (KERNEL_SRC / "work_queue.rs" if KERNEL_SRC else None, "src/work_queue.rs"),
        (KERNEL_SRC / "eventbus" / "mod.rs" if KERNEL_SRC else None, "src/eventbus/mod.rs"),
    ]:
        if path and path.exists():
            flines = sum(1 for _ in open(path))
            lines.append(f"  {label} ({flines} lines)")
        else:
            lines.append(f"  {label} (not found)")
    return "\n".join(lines)


# ── Boot ──

BOOT_PHASES = """Kernel Boot Phases (from src/main.rs rust_start())

Phase  Description
─────  ──────────────────────────────────────────────────────
0      Verify boot info magic + version
1      Framebuffer, RAM disk, serial init, benchmark
2      GDT (5+TSS), IDT (exceptions+IRQs+INT 0x80), MSI, PIC
3      HPET, APIC timer calibration, PS/2 + USB HID
2.5    Physical memory: UEFI mem map, buddy allocator, crash dump
2.75   Kernel heap: slab + linked_list_allocator
2.759  Object Manager init (ObObjectTable)
2.7595 Timer Manager init (64 slots)
2.76   Ob namespace root + dirs (\\Global, \\Device, \\Registry...)
2.77   Security subsystem init (default tokens)
2.8    SMP: per-CPU data + INIT-SIPI-SIPI
2.9    IPI: cross-CPU resched, TLB shootdown, call-function
2.91   I/O APIC: MADT detect, disable PIC, route ISA IRQs
3      STI, 4GiB identity map (2MB huge pages)
3.0    Demand paging: heap+mmap → 4KB PTEs
3.1    TEB page at 0x7000 (USER_ACCESSIBLE) for SEH
3.2    PCIe ECAM: MCFG, MMIO, activate
3.3    Storage: ATA→AHCI→NVMe→VirtIO probe
3.4    GPT scan, IoStack, Block Cache, Page Cache
3.4b   NeoDOS FS mount → C:
3.4c   FAT32 ESP mount → A:
3.5    Input manager (VT, keyboard)
3.80   Driver Isolation Layer (16×1MB @ 0x30000000)
3.85   Boot driver loader (BOOT→SYSTEM, dep-sorted)
3.86   AHCI port reclaim
3.87   NEM bridges (RTC), NXL region, hot-reload
3.88   Networking: e1000, ARP, TCP/UDP namespace
3.881  Registry init (Cm): SYSTEM hive
3.9    ABI freeze validation
4      Self-tests, cmdtest.nxe, NeoInit launch (PID 1)
"""


def boot_phases() -> str:
    """Show kernel boot phases and initialization sequence."""
    lines = [BOOT_PHASES]
    lines.append("")
    lines.append("Boot ABI (neodos-bootloader/ + src/main.rs):")
    lines.append("  BootInfo: magic(0x4E444F53), version, fb_info, mem map, fs_image, ACPI RSDP")
    lines.append("  Constants: BOOTINFO_MAGIC, KERNEL_VERSION_CODE, BOOT_VERSION")
    lines.append("")
    lines.append("Source files:")
    for path, label in [
        (NEODOS_ROOT / "neodos-bootloader", "neodos-bootloader/ (UEFI bootloader)"),
        (KERNEL_SRC / "main.rs" if KERNEL_SRC else None, "neodos-kernel/src/main.rs (rust_start)"),
    ]:
        if label.startswith("neodos-bootloader"):
            entries = list(path.rglob("*.rs")) if path.exists() else []
            lines.append(f"  {label} ({len(entries)} source files)")
        elif path and path.exists():
            flines = sum(1 for _ in open(path))
            lines.append(f"  {label} ({flines} lines)")
        else:
            lines.append(f"  {label} (not found)")
    return "\n".join(lines)


# ── Memory ──

MEMORY_SUMMARY = """Kernel Memory Layout

Region              Start       Size       Description
──────────────────  ──────────  ─────────  ─────────────────────────────
Kernel image        0x00400000  ~1 MB      ELF loaded by bootloader
Kernel .rodata      varies      ~1 MB      Read-only data
Kernel heap         0x01000000  16 MB      Slab + linked_list_allocator
User window         0x00400000   4 MB      32 × 128KB process slots
User heap           0x10000000  32 MB      16 × 2MB demand-paged
DLL region          0x1E000000   2 MB      8 × 256KB NXL slots
mmap region         0x20000000  32 MB      Lazy allocation
Driver isolation    0x30000000  16 MB      16 × 1MB NEM driver slots
TEB                 0x00007000  4 KB       USER_ACCESSIBLE SEH page

Allocators:
  Buddy (physical): power-of-2, max order 10 (4KB → 4MB)
  Slab (kernel heap): for small allocations (< 4KB)
  linked_list_allocator: fallback for larger kernel allocations
  Page cache: 128 × 4KB = 512 KB, hash + LRU

INV-5: Every physical frame has exactly one owner
INV-8: Kernel heap must not be identity-mapped as user-accessible
"""


def memory_layout() -> str:
    """Show kernel memory layout: regions, allocators, page tables."""
    lines = [MEMORY_SUMMARY, "Source files:"]
    for path, label in [
        (KERNEL_SRC / "memory" / "mod.rs" if KERNEL_SRC else None, "src/memory/mod.rs"),
        (KERNEL_SRC / "memory" / "buddy.rs" if KERNEL_SRC else None, "src/memory/buddy.rs"),
        (KERNEL_SRC / "memory" / "frame.rs" if KERNEL_SRC else None, "src/memory/frame.rs"),
        (KERNEL_SRC / "paging.rs" if KERNEL_SRC else None, "src/paging.rs"),
        (KERNEL_SRC / "buffer" / "page_cache.rs" if KERNEL_SRC else None, "src/buffer/page_cache.rs"),
    ]:
        if path and path.exists():
            flines = sum(1 for _ in open(path))
            lines.append(f"  {label} ({flines} lines)")
        else:
            lines.append(f"  {label} (not found)")
    return "\n".join(lines)
