use core::arch::global_asm;

global_asm!(
    ".section .text.entry, \"ax\"",
    ".global _start",
    "_start:",
    "mov esp, 0x1FFFF000",
    "and esp, 0xFFFFFFF0",
    "jmp rust_start",
);
