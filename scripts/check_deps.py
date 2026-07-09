#!/usr/bin/env python3
"""
Dependency validation tool for NeoDOS kernel.

Scans all Rust source files and checks for forbidden
cross-subsystem dependencies.

Usage:
    python3 scripts/check_deps.py
"""

import os
import re
import sys
from pathlib import Path

SRC_DIR = Path(__file__).resolve().parent.parent / "neodos-kernel" / "src"

# ── Subsystem definitions ────────────────────────────────────────────
# Each entry: subsystem_name → { paths, allowed, forbidden_deps }
# - paths: file/directory patterns to assign files to this subsystem
# - allowed: patterns that override forbidden_deps (if an import matches
#   both an allowed and a forbidden pattern, it is permitted)
# - forbidden_deps: module-path patterns (slash → ::) that must NOT
#   appear in imports as complete path components

SUBSYSTEMS = {
    # ═══════════════ LAYER 0: HARDWARE ═══════════════
    "hal": {
        "paths": ["hal/"],
        "allowed": ["arch"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "drivers", "fs", "vfs",
            "buffer", "net", "cm", "object", "security",
            "irp", "eventbus", "interrupts", "timers",
            "nem", "apc", "dpc", "kwait", "crash",
            "debugger", "exception", "watchdog", "virtio",
            "urn", "handle", "elf", "nxl", "usermode",
            "globals", "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "arch": {
        "paths": ["arch/"],
        "allowed": ["hal", "memory", "scheduler", "syscall",
                     "panic_classification"],
        "forbidden_deps": [
            "input", "console", "graphics", "font",
            "drivers", "fs", "vfs", "buffer", "net", "cm",
            "object", "security", "irp", "eventbus",
            "interrupts", "timers", "nem", "apc", "dpc",
            "kwait", "crash", "debugger", "exception",
            "watchdog", "virtio", "urn", "handle", "elf",
            "nxl", "usermode", "globals", "work_queue",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "cpu": {
        "paths": ["cpu.rs"],
        "allowed": ["hal", "arch", "timers"],
        "forbidden_deps": [
            "scheduler", "syscall", "console", "graphics", "font",
            "drivers", "fs", "vfs", "buffer", "net", "cm",
            "object", "security", "irp", "eventbus", "interrupts",
            "nem", "apc", "dpc", "kwait", "crash",
            "debugger", "exception", "watchdog", "virtio", "urn",
            "handle", "elf", "nxl", "usermode", "globals",
            "work_queue", "panic_classification", "boot_benchmark",
            "abi_freeze", "input",
        ],
    },

    # ═══════════════ LAYER 1: CORE ═══════════════
    "memory": {
        "paths": ["memory/"],
        "allowed": ["arch", "hal"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "drivers", "fs", "vfs",
            "buffer", "net", "cm", "object", "security",
            "irp", "eventbus", "interrupts", "timers",
            "nem", "apc", "dpc", "kwait", "crash",
            "debugger", "exception", "watchdog", "virtio",
            "urn", "handle", "elf", "nxl", "usermode",
            "globals", "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "interrupts": {
        "paths": ["interrupts/"],
        "allowed": ["arch", "hal", "drivers/pci", "drivers/driver_runtime",
                     "drivers"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "fs", "vfs", "buffer",
            "net", "cm", "object", "security", "irp",
            "eventbus", "timers", "nem", "apc", "dpc",
            "kwait", "crash", "debugger", "exception",
            "watchdog", "virtio", "urn", "handle", "elf",
            "nxl", "usermode", "globals", "work_queue",
            "panic_classification", "boot_benchmark", "abi_freeze",
        ],
    },
    "timers": {
        "paths": ["timers/"],
        "allowed": ["arch", "hal", "interrupts"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "drivers", "fs", "vfs",
            "buffer", "net", "cm", "object", "security",
            "irp", "eventbus", "nem", "apc", "dpc",
            "kwait", "crash", "debugger", "exception",
            "watchdog", "virtio", "urn", "handle", "elf",
            "nxl", "usermode", "globals", "work_queue",
            "panic_classification", "boot_benchmark", "abi_freeze",
        ],
    },
    "trace": {
        "paths": ["trace.rs"],
        "allowed": ["arch", "hal"],
        "forbidden_deps": [
            "scheduler", "syscall", "console", "graphics", "font",
            "drivers", "fs", "vfs", "buffer", "net", "cm",
            "object", "security", "irp", "eventbus", "interrupts",
            "timers", "nem", "apc", "dpc", "kwait", "crash",
            "debugger", "exception", "watchdog", "virtio", "urn",
            "handle", "elf", "nxl", "usermode", "globals",
            "work_queue", "panic_classification", "boot_benchmark",
            "abi_freeze", "input",
        ],
    },
    "testing": {
        "paths": ["testing.rs"],
        "allowed": [],
        "forbidden_deps": [],
    },
    "panic_classification": {
        "paths": ["panic_classification.rs"],
        "allowed": ["hal", "arch"],
        "forbidden_deps": [
            "scheduler", "syscall", "console", "graphics", "font",
            "drivers", "fs", "vfs", "buffer", "net", "cm",
            "object", "security", "irp", "eventbus", "interrupts",
            "timers", "nem", "apc", "dpc", "kwait", "crash",
            "debugger", "exception", "watchdog", "virtio", "urn",
            "handle", "elf", "nxl", "usermode", "globals",
            "work_queue", "boot_benchmark", "abi_freeze", "input",
        ],
    },
    "allocator": {
        "paths": ["allocator.rs"],
        "allowed": [],
        "forbidden_deps": [
            "scheduler", "syscall", "console", "graphics", "font",
            "drivers", "fs", "vfs", "buffer", "net", "cm",
            "object", "security", "irp", "eventbus", "interrupts",
            "timers", "nem", "apc", "dpc", "kwait", "crash",
            "debugger", "exception", "watchdog", "virtio", "urn",
            "handle", "elf", "nxl", "usermode", "globals",
            "work_queue", "panic_classification", "boot_benchmark",
            "abi_freeze", "hal", "arch", "cpu", "memory", "input",
        ],
    },
    "slab": {
        "paths": ["slab.rs"],
        "allowed": ["memory", "arch", "hal"],
        "forbidden_deps": [
            "scheduler", "syscall", "console", "graphics", "font",
            "drivers", "fs", "vfs", "buffer", "net", "cm",
            "object", "security", "irp", "eventbus", "interrupts",
            "timers", "nem", "apc", "dpc", "kwait", "crash",
            "debugger", "exception", "watchdog", "virtio", "urn",
            "handle", "elf", "nxl", "usermode", "globals",
            "work_queue", "panic_classification", "boot_benchmark",
            "abi_freeze", "input",
        ],
    },

    # ═══════════════ LAYER 2: PRIMITIVES ═══════════════
    "dpc": {
        "paths": ["dpc/"],
        "allowed": ["interrupts", "arch", "hal"],
        "forbidden_deps": [
            "scheduler", "syscall", "console", "graphics", "font",
            "drivers", "fs", "vfs", "buffer", "net", "cm",
            "object", "security", "irp", "eventbus",
            "timers", "nem", "apc", "kwait", "crash",
            "debugger", "exception", "watchdog", "virtio", "urn",
            "handle", "elf", "nxl", "usermode", "globals",
            "work_queue", "panic_classification", "boot_benchmark",
            "abi_freeze", "input",
        ],
    },
    "kwait": {
        "paths": ["kwait/"],
        "allowed": ["scheduler", "arch", "hal", "object"],
        "forbidden_deps": [
            "syscall", "console", "graphics", "font",
            "drivers", "fs", "vfs", "buffer", "net", "cm",
            "security", "irp", "eventbus",
            "interrupts", "timers", "nem", "apc", "dpc",
            "crash", "debugger", "exception", "watchdog",
            "virtio", "urn", "handle", "elf", "nxl",
            "usermode", "globals", "work_queue",
            "panic_classification", "boot_benchmark",
            "abi_freeze", "input",
        ],
    },
    "apc": {
        "paths": ["apc/"],
        "allowed": ["scheduler", "arch", "hal", "memory", "irp", "object"],
        "forbidden_deps": [
            "syscall", "console", "graphics", "font",
            "drivers", "fs", "vfs", "buffer", "net", "cm",
            "security", "eventbus",
            "interrupts", "timers", "nem", "dpc",
            "crash", "debugger", "exception", "watchdog",
            "virtio", "urn", "handle", "elf", "nxl",
            "usermode", "globals", "work_queue",
            "panic_classification", "boot_benchmark",
            "abi_freeze", "input",
        ],
    },

    # ═══════════════ LAYER 3: SCHEDULER ═══════════════
    "scheduler": {
        "paths": ["scheduler/"],
        "allowed": [
            "arch", "hal", "memory", "interrupts",
            "timers", "dpc", "kwait", "apc", "cpu",
            "object", "trace", "security",
        ],
        "forbidden_deps": [
            "drivers", "fs", "vfs", "buffer", "net", "cm",
            "irp", "eventbus",
            "nem", "crash", "debugger", "exception",
            "watchdog", "virtio", "urn", "handle",
            "elf", "nxl", "usermode", "globals",
            "console", "graphics", "font", "input",
            "syscall", "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },

    # ═══════════════ LAYER 4: OBJECT & SECURITY ═══════════════
    "object": {
        "paths": ["object/"],
        "allowed": [
            "memory", "security", "scheduler",
            "arch", "hal", "handle", "trace", "kwait",
        ],
        "forbidden_deps": [
            "syscall", "console", "graphics", "font",
            "drivers", "fs", "vfs", "buffer", "net", "cm",
            "irp", "eventbus", "interrupts", "timers",
            "nem", "apc", "dpc",
            "crash", "debugger", "exception", "watchdog",
            "virtio", "urn",
            "elf", "nxl",
            "usermode", "globals", "work_queue",
            "panic_classification", "boot_benchmark",
            "abi_freeze", "input",
        ],
    },
    "security": {
        "paths": ["security/"],
        "allowed": ["memory", "object", "scheduler", "arch", "hal"],
        "forbidden_deps": [
            "syscall", "console", "graphics", "font",
            "drivers", "fs", "vfs", "buffer", "net", "cm",
            "irp", "eventbus", "interrupts", "timers",
            "nem", "apc", "dpc", "kwait",
            "crash", "debugger", "exception", "watchdog",
            "virtio", "urn", "handle",
            "elf", "nxl",
            "usermode", "globals", "work_queue",
            "panic_classification", "boot_benchmark",
            "abi_freeze", "input",
        ],
    },
    "handle": {
        "paths": ["handle.rs"],
        "allowed": ["object", "security", "scheduler", "memory"],
        "forbidden_deps": [
            "syscall", "console", "graphics", "font",
            "drivers", "fs", "vfs", "buffer", "net", "cm",
            "irp", "eventbus", "interrupts", "timers",
            "nem", "apc", "dpc", "kwait",
            "crash", "debugger", "exception", "watchdog",
            "virtio", "urn",
            "elf", "nxl",
            "usermode", "globals", "work_queue",
            "panic_classification", "boot_benchmark",
            "abi_freeze", "input",
        ],
    },

    # ═══════════════ LAYER 5: I/O ═══════════════
    "irp": {
        "paths": ["irp/"],
        "allowed": ["object", "memory", "scheduler", "arch", "hal", "work_queue"],
        "forbidden_deps": [
            "syscall", "console", "graphics", "font",
            "drivers", "fs", "vfs", "buffer", "net", "cm",
            "security", "eventbus", "interrupts", "timers",
            "nem", "apc", "dpc", "kwait",
            "crash", "debugger", "exception", "watchdog",
            "virtio", "urn", "handle",
            "elf", "nxl",
            "usermode", "globals",
            "panic_classification", "boot_benchmark",
            "abi_freeze", "input",
        ],
    },
    "drivers_pci": {
        "paths": ["drivers/pci.rs"],
        "allowed": ["hal", "interrupts", "memory", "arch", "timers"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "fs", "vfs", "buffer",
            "net", "cm", "object", "security", "irp",
            "eventbus",
            "nem", "apc", "dpc",
            "kwait", "crash", "debugger", "exception",
            "watchdog", "virtio", "urn", "handle",
            "elf", "nxl", "usermode", "globals",
            "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "drivers_block": {
        "paths": ["drivers/block.rs"],
        "allowed": ["irp", "memory", "object", "scheduler", "arch", "hal"],
        "forbidden_deps": [
            "syscall", "input", "console", "graphics", "font",
            "fs", "vfs", "buffer", "net", "cm",
            "security", "eventbus", "interrupts", "timers",
            "nem", "apc", "dpc", "kwait",
            "crash", "debugger", "exception", "watchdog",
            "virtio", "urn", "handle",
            "elf", "nxl",
            "usermode", "globals", "work_queue",
            "panic_classification", "boot_benchmark",
            "abi_freeze",
        ],
    },
    "drivers_ata": {
        "paths": ["drivers/ata.rs"],
        "allowed": ["drivers/block", "irp", "hal", "memory", "interrupts"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "fs", "vfs", "buffer",
            "net", "cm", "object", "security",
            "eventbus", "timers", "nem", "apc", "dpc",
            "kwait", "crash", "debugger", "exception",
            "watchdog", "virtio", "urn", "handle",
            "elf", "nxl", "usermode", "globals",
            "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "drivers_boot_ahci": {
        "paths": ["drivers/boot_ahci.rs"],
        "allowed": ["drivers/block", "memory", "hal", "interrupts", "irp", "arch"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "fs", "vfs", "buffer",
            "net", "cm", "object", "security",
            "eventbus", "timers", "nem", "apc", "dpc",
            "kwait", "crash", "debugger", "exception",
            "watchdog", "virtio", "urn", "handle",
            "elf", "nxl", "usermode", "globals",
            "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "drivers_nvme": {
        "paths": ["drivers/nvme.rs"],
        "allowed": ["drivers/block", "irp", "memory", "hal", "interrupts", "arch"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "fs", "vfs", "buffer",
            "net", "cm", "object", "security",
            "eventbus", "timers", "nem", "apc", "dpc",
            "kwait", "crash", "debugger", "exception",
            "watchdog", "virtio", "urn", "handle",
            "elf", "nxl", "usermode", "globals",
            "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "drivers_virtio_blk": {
        "paths": ["drivers/virtio_blk.rs"],
        "allowed": ["drivers/block", "virtio", "memory", "drivers/pci", "hal", "irp"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "fs", "vfs", "buffer",
            "net", "cm", "object", "security",
            "eventbus", "interrupts", "timers",
            "nem", "apc", "dpc", "kwait",
            "crash", "debugger", "exception", "watchdog",
            "urn", "handle",
            "elf", "nxl",
            "usermode", "globals", "work_queue",
            "panic_classification", "boot_benchmark",
            "abi_freeze",
        ],
    },
    "drivers_fat32": {
        "paths": ["drivers/fat32.rs"],
        "allowed": ["drivers/block", "irp", "memory", "globals", "hal",
                     "vfs", "fs"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font",
            "buffer", "net", "cm", "object", "security",
            "eventbus", "interrupts", "timers",
            "nem", "apc", "dpc", "kwait",
            "crash", "debugger", "exception", "watchdog",
            "virtio", "urn", "handle",
            "elf", "nxl",
            "usermode",
            "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "drivers_gpt": {
        "paths": ["drivers/gpt.rs"],
        "allowed": ["drivers/block", "memory"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "fs", "vfs", "buffer",
            "net", "cm", "object", "security", "irp",
            "eventbus", "interrupts", "timers",
            "nem", "apc", "dpc", "kwait",
            "crash", "debugger", "exception", "watchdog",
            "virtio", "urn", "handle",
            "elf", "nxl",
            "usermode", "globals", "work_queue",
            "panic_classification", "boot_benchmark",
            "abi_freeze",
        ],
    },
    "drivers_iso9660": {
        "paths": ["drivers/iso9660.rs"],
        "allowed": ["drivers/block", "memory", "fs", "vfs"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "buffer", "net", "cm",
            "object", "security", "irp", "eventbus",
            "interrupts", "timers", "nem", "apc", "dpc",
            "kwait", "crash", "debugger", "exception",
            "watchdog", "virtio", "urn", "handle",
            "elf", "nxl", "usermode", "globals",
            "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "drivers_ps2": {
        "paths": ["drivers/ps2.rs"],
        "allowed": ["hal", "input", "interrupts", "arch", "eventbus"],
        "forbidden_deps": [
            "scheduler", "syscall", "console", "graphics", "font",
            "drivers", "fs", "vfs", "buffer",
            "net", "cm", "object", "security", "irp",
            "timers", "nem", "apc", "dpc", "kwait",
            "crash", "debugger", "exception", "watchdog",
            "virtio", "urn", "handle",
            "elf", "nxl",
            "usermode", "globals", "work_queue",
            "panic_classification", "boot_benchmark",
            "abi_freeze",
        ],
    },
    "drivers_rtc": {
        "paths": ["drivers/rtc_bridge.rs"],
        "allowed": ["hal", "timers", "interrupts", "arch", "eventbus"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "drivers", "fs", "vfs",
            "buffer", "net", "cm", "object", "security",
            "irp", "nem", "apc", "dpc", "kwait",
            "crash", "debugger", "exception", "watchdog",
            "virtio", "urn", "handle",
            "elf", "nxl",
            "usermode", "globals", "work_queue",
            "panic_classification", "boot_benchmark",
            "abi_freeze",
        ],
    },
    "drivers_storage_manager": {
        "paths": ["drivers/storage_manager.rs"],
        "allowed": ["drivers/block", "drivers/nem", "memory", "irp", "globals",
                     "virtio"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "fs", "vfs", "buffer",
            "net", "cm", "object", "security",
            "eventbus", "interrupts", "timers",
            "nem", "apc", "dpc", "kwait",
            "crash", "debugger", "exception", "watchdog",
            "urn", "handle",
            "elf", "nxl",
            "usermode", "work_queue",
            "panic_classification", "boot_benchmark",
            "abi_freeze",
        ],
    },
    "drivers_nem": {
        "paths": ["drivers/nem/", "drivers/nem.rs"],
        "allowed": [
            "memory", "irp", "object", "security", "hal",
            "arch", "drivers/caps", "drivers/abi",
            "scheduler", "eventbus", "interrupts",
            "net", "input",
        ],
        "forbidden_deps": [
            "syscall", "input", "console", "graphics", "font",
            "fs", "vfs", "buffer", "cm",
            "timers", "apc", "dpc", "kwait",
            "crash", "debugger", "exception", "watchdog",
            "virtio", "urn", "handle",
            "elf", "nxl",
            "usermode", "globals", "work_queue",
            "panic_classification", "boot_benchmark",
            "abi_freeze",
        ],
    },
    "drivers_driver_runtime": {
        "paths": ["drivers/driver_runtime.rs"],
        "allowed": ["drivers/nem", "memory", "object", "irp",
                     "scheduler", "arch", "eventbus"],
        "forbidden_deps": [
            "syscall", "input", "console", "graphics", "font",
            "fs", "vfs", "buffer", "net", "cm",
            "security", "interrupts", "timers",
            "apc", "dpc", "kwait",
            "crash", "debugger", "exception", "watchdog",
            "virtio", "urn", "handle",
            "elf", "nxl",
            "usermode", "globals", "work_queue",
            "panic_classification", "boot_benchmark",
            "abi_freeze",
        ],
    },
    "drivers_hotreload": {
        "paths": ["drivers/hotreload.rs"],
        "allowed": ["drivers/nem", "drivers/caps", "drivers/driver_runtime",
                     "memory", "object", "eventbus"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "drivers", "fs", "vfs",
            "buffer", "net", "cm", "security", "irp",
            "interrupts", "timers",
            "apc", "dpc", "kwait",
            "crash", "debugger", "exception", "watchdog",
            "virtio", "urn", "handle",
            "elf", "nxl",
            "usermode", "globals", "work_queue",
            "panic_classification", "boot_benchmark",
            "abi_freeze",
        ],
    },
    "drivers_isolation": {
        "paths": ["drivers/isolation.rs"],
        "allowed": ["memory", "hal", "arch", "drivers/nem", "object"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "drivers", "fs", "vfs",
            "buffer", "net", "cm", "security", "irp",
            "eventbus", "interrupts", "timers",
            "apc", "dpc", "kwait",
            "crash", "debugger", "exception", "watchdog",
            "virtio", "urn", "handle",
            "elf", "nxl",
            "usermode", "globals", "work_queue",
            "panic_classification", "boot_benchmark",
            "abi_freeze",
        ],
    },
    "drivers_caps": {
        "paths": ["drivers/caps.rs"],
        "allowed": ["nem", "drivers", "drivers/driver_runtime"],
        "forbidden_deps": [
            "scheduler", "syscall", "console", "graphics", "font",
            "fs", "vfs", "buffer", "net", "cm",
            "object", "security", "irp", "eventbus",
            "interrupts", "timers",
            "apc", "dpc", "kwait", "crash", "debugger",
            "exception", "watchdog", "virtio", "urn",
            "handle",
            "elf", "nxl", "usermode", "globals",
            "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze", "input",
        ],
    },
    "drivers_abi": {
        "paths": ["drivers/abi/"],
        "allowed": ["nem"],
        "forbidden_deps": [
            "scheduler", "syscall", "console", "graphics", "font",
            "fs", "vfs", "buffer", "net", "cm",
            "object", "security", "irp", "eventbus",
            "interrupts", "timers",
            "apc", "dpc", "kwait", "crash", "debugger",
            "exception", "watchdog", "virtio", "urn",
            "handle",
            "elf", "nxl", "usermode", "globals",
            "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze", "input",
        ],
    },
    "drivers_boot_loader": {
        "paths": ["drivers/boot_loader/"],
        "allowed": ["drivers/block", "memory", "nem", "drivers/nem",
                     "drivers/driver_runtime", "eventbus", "fs"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "vfs", "buffer",
            "net", "cm", "object", "security", "irp",
            "interrupts", "timers",
            "apc", "dpc", "kwait",
            "crash", "debugger", "exception", "watchdog",
            "virtio", "urn", "handle",
            "nxl",
            "usermode", "globals", "work_queue",
            "panic_classification", "boot_benchmark",
            "abi_freeze",
        ],
    },
    "drivers_dependency": {
        "paths": ["drivers/dependency/"],
        "allowed": ["drivers/caps", "drivers/nem", "memory"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "drivers", "fs", "vfs",
            "buffer", "net", "cm", "object", "security",
            "irp", "eventbus", "interrupts", "timers",
            "apc", "dpc", "kwait",
            "crash", "debugger", "exception", "watchdog",
            "virtio", "urn", "handle",
            "elf", "nxl",
            "usermode", "globals", "work_queue",
            "panic_classification", "boot_benchmark",
            "abi_freeze",
        ],
    },

    # ═══════════════ LAYER 6: FILESYSTEM ═══════════════
    "fs": {
        "paths": ["fs/"],
        "allowed": ["drivers/block", "memory", "vfs", "buffer", "globals", "hal"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "net", "cm",
            "object", "security", "irp", "eventbus",
            "interrupts", "timers", "nem", "apc", "dpc",
            "kwait", "crash", "debugger", "exception",
            "watchdog", "virtio", "urn", "handle",
            "elf", "nxl", "usermode",
            "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "vfs": {
        "paths": ["vfs/"],
        "allowed": ["drivers/block", "memory", "globals", "hal",
                     "object", "fs"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "buffer",
            "net", "cm",
            "security", "irp",
            "eventbus", "interrupts", "timers", "nem",
            "apc", "dpc", "kwait", "crash", "debugger",
            "exception", "watchdog", "virtio", "urn",
            "handle",
            "elf", "nxl", "usermode",
            "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "buffer": {
        "paths": ["buffer/"],
        "allowed": ["memory", "fs", "drivers/block", "globals", "hal"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "vfs", "net", "cm",
            "object", "security", "irp", "eventbus",
            "interrupts", "timers", "nem", "apc", "dpc",
            "kwait", "crash", "debugger", "exception",
            "watchdog", "virtio", "urn", "handle",
            "elf", "nxl", "usermode",
            "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "globals": {
        "paths": ["globals.rs"],
        "allowed": [
            "vfs", "buffer", "drivers/block", "drivers/nem",
            "memory", "fs", "hal",
        ],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "net", "cm",
            "object", "security", "irp", "eventbus",
            "interrupts", "timers", "nem",
            "apc", "dpc", "kwait", "crash", "debugger",
            "exception", "watchdog", "virtio", "urn",
            "handle",
            "elf", "nxl", "usermode",
            "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },

    # ═══════════════ LAYER 7: REGISTRY ═══════════════
    "cm": {
        "paths": ["cm/"],
        "allowed": [
            "vfs", "memory", "object", "fs", "globals",
            "arch", "hal", "buffer",
        ],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "drivers", "net",
            "security", "irp", "eventbus",
            "interrupts", "timers", "nem", "apc", "dpc",
            "kwait", "crash", "debugger", "exception",
            "watchdog", "virtio", "urn", "handle",
            "elf", "nxl", "usermode",
            "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },

    # ═══════════════ LAYER 8: NETWORK ═══════════════
    "net": {
        "paths": ["net/"],
        "allowed": [
            "drivers/pci", "irp", "memory", "scheduler",
            "hal", "object", "eventbus", "kwait",
            "interrupts", "arch", "trace",
        ],
        "forbidden_deps": [
            "syscall", "input", "console", "graphics", "font",
            "drivers", "fs", "vfs", "buffer", "cm",
            "security", "nem", "apc", "dpc",
            "crash", "debugger", "exception", "watchdog",
            "virtio", "urn", "handle",
            "elf", "nxl",
            "usermode", "globals", "work_queue",
            "panic_classification", "boot_benchmark",
            "abi_freeze",
        ],
    },

    # ═══════════════ LAYER 9: USER INTERFACE ═══════════════
    "input": {
        "paths": ["input/"],
        "allowed": [
            "console", "graphics", "font", "hal",
            "eventbus", "scheduler", "arch",
        ],
        "forbidden_deps": [
            "syscall", "drivers", "fs", "vfs",
            "buffer", "net", "cm", "object", "security",
            "irp", "interrupts", "timers", "nem",
            "apc", "dpc", "kwait", "crash", "debugger",
            "exception", "watchdog", "virtio", "urn",
            "handle",
            "elf", "nxl", "usermode",
            "globals", "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "console": {
        "paths": ["console.rs"],
        "allowed": ["hal", "graphics", "font", "arch"],
        "forbidden_deps": [
            "scheduler", "syscall", "input",
            "drivers", "fs", "vfs", "buffer",
            "net", "cm", "object", "security", "irp",
            "eventbus", "interrupts", "timers", "nem",
            "apc", "dpc", "kwait", "crash", "debugger",
            "exception", "watchdog", "virtio", "urn",
            "handle",
            "elf", "nxl", "usermode",
            "globals", "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "graphics": {
        "paths": ["graphics.rs"],
        "allowed": ["hal", "font", "arch"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "drivers", "fs", "vfs", "buffer",
            "net", "cm", "object", "security", "irp",
            "eventbus", "interrupts", "timers", "nem",
            "apc", "dpc", "kwait", "crash", "debugger",
            "exception", "watchdog", "virtio", "urn",
            "handle",
            "elf", "nxl", "usermode",
            "globals", "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "font": {
        "paths": ["font.rs"],
        "allowed": ["graphics"],
        "forbidden_deps": [
            "scheduler", "syscall", "console",
            "hal", "arch", "cpu", "memory", "input",
            "drivers", "fs", "vfs", "buffer", "net", "cm",
            "object", "security", "irp", "eventbus",
            "interrupts", "timers", "nem", "apc", "dpc",
            "kwait", "crash", "debugger", "exception",
            "watchdog", "virtio", "urn", "handle",
            "elf", "nxl", "usermode", "globals",
            "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },

    # ═══════════════ LAYER 10: EVENT BUS ═══════════════
    "eventbus": {
        "paths": ["eventbus/"],
        "allowed": ["scheduler", "object", "memory", "kwait",
                     "arch", "hal", "trace"],
        "forbidden_deps": [
            "syscall", "input", "console", "graphics", "font",
            "drivers", "fs", "vfs", "buffer", "net", "cm",
            "security", "irp", "interrupts", "timers",
            "nem", "apc", "dpc", "crash", "debugger",
            "exception", "watchdog", "virtio", "urn",
            "handle",
            "elf", "nxl", "usermode", "globals",
            "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },

    # ═══════════════ LAYER 11: SYSCALL ═══════════════
    "syscall": {
        "paths": ["syscall/"],
        "allowed": [
            "scheduler", "input", "console", "hal", "arch",
            "vfs", "fs", "globals", "memory",
            "object", "security", "cm", "handle",
            "drivers/block", "irp", "eventbus",
            "graphics", "font", "usermode",
            "urn", "trace",
        ],
        "forbidden_deps": [
            "drivers/ata", "drivers/boot_ahci", "drivers/fat32",
            "drivers/pci", "drivers/rtc_bridge", "drivers/nvme",
            "drivers/virtio_blk", "drivers/nem",
            "drivers/ps2", "drivers/gpt", "drivers/iso9660",
            "drivers/storage_manager", "drivers/driver_runtime",
            "drivers/hotreload", "drivers/isolation",
            "drivers/dependency", "drivers/boot_loader",
            "drivers/caps", "drivers/abi",
            "virtio", "net", "crash", "debugger",
            "boot_benchmark", "abi_freeze", "invariants",
        ],
    },

    # ═══════════════ LAYER 12: USER MODE ═══════════════
    "usermode": {
        "paths": ["usermode.rs"],
        "allowed": ["arch", "scheduler", "memory", "hal"],
        "forbidden_deps": [
            "syscall", "input", "console", "graphics", "font",
            "drivers", "fs", "vfs", "buffer", "net", "cm",
            "object", "security", "irp", "eventbus",
            "interrupts", "timers", "nem", "apc", "dpc",
            "kwait", "crash", "debugger", "exception",
            "watchdog", "virtio", "urn", "handle",
            "elf", "nxl", "globals", "work_queue",
            "panic_classification", "boot_benchmark",
            "abi_freeze",
        ],
    },
    "elf": {
        "paths": ["elf.rs"],
        "allowed": ["memory", "arch", "hal", "scheduler",
                     "scheduler/address_space"],
        "forbidden_deps": [
            "syscall", "input", "console",
            "graphics", "font", "drivers", "fs", "vfs",
            "buffer", "net", "cm", "object", "security",
            "irp", "eventbus", "interrupts", "timers",
            "nem", "apc", "dpc", "kwait", "crash",
            "debugger", "exception", "watchdog", "virtio",
            "urn", "handle", "nxl", "usermode",
            "globals", "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "nxl": {
        "paths": ["nxl.rs"],
        "allowed": ["memory", "elf", "arch", "hal",
                     "globals", "fs", "vfs"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "drivers", "buffer",
            "net", "cm", "object", "security",
            "irp", "eventbus", "interrupts", "timers",
            "nem", "apc", "dpc", "kwait", "crash",
            "debugger", "exception", "watchdog", "virtio",
            "urn", "handle", "usermode",
            "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "work_queue": {
        "paths": ["work_queue.rs"],
        "allowed": ["scheduler", "dpc", "arch", "hal"],
        "forbidden_deps": [
            "syscall", "input", "console", "graphics", "font",
            "drivers", "fs", "vfs", "buffer", "net", "cm",
            "object", "security", "irp", "eventbus",
            "interrupts", "timers", "nem", "apc", "kwait",
            "crash", "debugger", "exception", "watchdog",
            "virtio", "urn", "handle",
            "elf", "nxl",
            "usermode", "globals",
            "panic_classification", "boot_benchmark",
            "abi_freeze",
        ],
    },

    # ═══════════════ LAYER 13: DEBUG & CRASH ═══════════════
    "crash": {
        "paths": ["crash/"],
        "allowed": [
            "console", "arch", "hal", "memory",
            "drivers/nem", "timers", "panic_classification",
            "interrupts",
        ],
        "forbidden_deps": [
            "scheduler", "syscall", "input",
            "graphics", "font", "fs", "vfs", "buffer",
            "net", "cm", "object", "security", "irp",
            "eventbus", "nem", "apc", "dpc", "kwait",
            "debugger", "exception", "watchdog",
            "virtio", "urn", "handle",
            "elf", "nxl",
            "usermode", "globals", "work_queue",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "debugger": {
        "paths": ["debugger/"],
        "allowed": ["arch", "hal", "interrupts"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "drivers", "fs", "vfs",
            "buffer", "net", "cm", "object", "security",
            "irp", "eventbus", "timers", "nem", "apc",
            "dpc", "kwait", "crash", "exception",
            "watchdog", "virtio", "urn", "handle",
            "elf", "nxl",
            "usermode", "globals", "work_queue",
            "panic_classification", "boot_benchmark",
            "abi_freeze",
        ],
    },
    "exception": {
        "paths": ["exception/"],
        "allowed": ["arch", "scheduler", "usermode", "memory",
                     "crash", "hal", "panic_classification"],
        "forbidden_deps": [
            "syscall", "input", "console", "graphics", "font",
            "drivers", "fs", "vfs", "buffer", "net", "cm",
            "object", "security", "irp", "eventbus",
            "interrupts", "timers", "nem", "apc", "dpc",
            "kwait", "debugger", "watchdog",
            "virtio", "urn", "handle",
            "elf", "nxl",
            "globals", "work_queue",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "watchdog": {
        "paths": ["watchdog/"],
        "allowed": [
            "timers", "crash", "hal", "interrupts",
            "panic_classification", "arch",
        ],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "drivers", "fs", "vfs",
            "buffer", "net", "cm", "object", "security",
            "irp", "eventbus", "nem", "apc", "dpc",
            "kwait", "debugger", "exception",
            "virtio", "urn", "handle",
            "elf", "nxl",
            "usermode", "globals", "work_queue",
            "boot_benchmark", "abi_freeze",
        ],
    },

    # ═══════════════ LAYER 14: OTHER ═══════════════
    "nem": {
        "paths": ["nem/"],
        "allowed": ["memory", "arch", "hal"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "drivers", "fs", "vfs",
            "buffer", "net", "cm", "object", "security",
            "irp", "eventbus", "interrupts", "timers",
            "apc", "dpc", "kwait", "crash", "debugger",
            "exception", "watchdog", "virtio", "urn",
            "handle",
            "elf", "nxl", "usermode", "globals",
            "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "urn": {
        "paths": ["urn/"],
        "allowed": ["object", "memory", "scheduler",
                     "globals", "handle"],
        "forbidden_deps": [
            "syscall", "input", "console", "graphics", "font",
            "drivers", "fs", "vfs", "buffer", "net", "cm",
            "security", "irp", "eventbus", "interrupts",
            "timers", "nem", "apc", "dpc", "kwait",
            "crash", "debugger", "exception", "watchdog",
            "virtio",
            "elf", "nxl",
            "usermode",
            "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "virtio": {
        "paths": ["virtio/"],
        "allowed": ["drivers/pci", "memory", "hal", "arch"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "drivers", "fs", "vfs",
            "buffer", "net", "cm", "object", "security",
            "irp", "eventbus", "interrupts", "timers",
            "nem", "apc", "dpc", "kwait", "crash",
            "debugger", "exception", "watchdog",
            "urn", "handle",
            "elf", "nxl", "usermode",
            "globals", "work_queue", "panic_classification",
            "boot_benchmark", "abi_freeze",
        ],
    },
    "invariants": {
        "paths": ["invariants.rs"],
        "allowed": [],
        "forbidden_deps": [],
    },
    "boot_benchmark": {
        "paths": ["boot_benchmark.rs"],
        "allowed": ["timers", "hal", "arch", "drivers/boot_ahci"],
        "forbidden_deps": [
            "scheduler", "syscall", "input", "console",
            "graphics", "font", "fs", "vfs", "buffer",
            "net", "cm", "object", "security", "irp",
            "eventbus", "interrupts", "nem", "apc", "dpc",
            "kwait", "crash", "debugger", "exception",
            "watchdog", "virtio", "urn", "handle",
            "elf", "nxl",
            "usermode", "globals", "work_queue",
            "panic_classification", "abi_freeze",
        ],
    },
    "abi_freeze": {
        "paths": ["abi_freeze.rs"],
        "allowed": [
            "eventbus", "drivers/caps", "drivers/abi",
            "drivers/nem", "scheduler", "irp", "kwait",
            "hal", "arch", "memory",
        ],
        "forbidden_deps": [
            "syscall", "input", "console", "graphics", "font",
            "fs", "vfs", "buffer", "net", "cm",
            "object", "security",
            "interrupts", "timers",
            "apc", "dpc",
            "crash", "debugger", "exception", "watchdog",
            "virtio", "urn", "handle",
            "elf", "nxl",
            "usermode", "globals", "work_queue",
            "panic_classification", "boot_benchmark",
        ],
    },
}


# ── Pattern matching ───────────────────────────────────────────────

def path_matches(pattern: str, module_path: str) -> bool:
    """Check if pattern appears as a complete component in module_path.

    Both pattern and module_path use :: as separator (slashes in
    pattern are converted automatically).  The match succeeds when
    *pattern* is found at a ``::`` boundary — this avoids false
    positives such as ``cm`` matching ``core::cmp`` or ``elf``
    matching ``self``.
    """
    pat = pattern.replace("/", "::")
    escaped = re.escape(pat)
    return bool(re.search(
        rf'(?:^|::){escaped}(?:::|\Z)',
        module_path,
    ))


# ── Dependency extraction ───────────────────────────────────────────

IMPORT_RE = re.compile(r'^\s*use\s+(?:crate::)?([^;]+);')


def get_owning_subsystem(file_path: str) -> str | None:
    rel = os.path.relpath(file_path, str(SRC_DIR)).replace("\\", "/")
    # Match most specific paths first (longer paths checked earlier)
    candidates = []
    for name, info in SUBSYSTEMS.items():
        for pat in info["paths"]:
            if rel.startswith(pat) or rel == pat:
                candidates.append((len(pat), name))
    if not candidates:
        return None
    candidates.sort(key=lambda x: -x[0])  # longest match wins
    return candidates[0][1]


def extract_crate_imports(file_path: str) -> list[str]:
    imports = []
    with open(file_path, "r") as f:
        for line in f:
            m = IMPORT_RE.match(line)
            if m:
                imports.append(m.group(1))
    return imports


def check_forbidden(from_subsystem: str, imported_path: str) -> list[str]:
    violations = []
    info = SUBSYSTEMS.get(from_subsystem)
    if not info:
        return violations

    for forbidden in info.get("forbidden_deps", []):
        if path_matches(forbidden, imported_path):
            allowed = False
            for allow in info.get("allowed", []):
                if path_matches(allow, imported_path):
                    allowed = True
                    break
            if not allowed:
                violations.append(forbidden)
    return violations


def main():
    src = str(SRC_DIR)
    violations = []

    for root, dirs, files in os.walk(src):
        dirs[:] = [d for d in dirs if not d.startswith('.')]
        for fname in files:
            if not fname.endswith(".rs"):
                continue
            fpath = os.path.join(root, fname)
            subsystem = get_owning_subsystem(fpath)
            if not subsystem:
                continue

            imports = extract_crate_imports(fpath)
            for imp in imports:
                forbidden = check_forbidden(subsystem, imp)
                if forbidden:
                    rel = os.path.relpath(fpath, src)
                    violations.append(
                        f"  {rel}: imports '{imp}' which contains "
                        f"forbidden dep(s): {', '.join(forbidden)}"
                    )

    print("=" * 60)
    print("NeoDOS Dependency Check")
    print("=" * 60)

    if violations:
        print(f"\n\u274c {len(violations)} violation(s) found:\n")
        for v in violations:
            print(v)
        sys.exit(1)
    else:
        print("\n\u2705 No dependency violations found.")
        sys.exit(0)


if __name__ == "__main__":
    main()
