//! Syscall ABI wrappers, split by subsystem.
//!
//! The public API is preserved: every item is re-exported at `libneodos::syscall::*`.

// ── Standard error codes (shared) ──

pub const EINVAL: i64 = -1;
pub const ENOENT: i64 = -2;
pub const ENOMEM: i64 = -3;
pub const EACCES: i64 = -4;
pub const EBADF: i64 = -5;
pub const EFAULT: i64 = -6;
pub const ENOSYS: i64 = -7;
pub const EAGAIN: i64 = -8;
pub const EPIPE: i64 = -9;
pub const EEXIST: i64 = -10;
pub const ENOTDIR: i64 = -11;
pub const EISDIR: i64 = -12;
pub const EIO: i64 = -13;
pub const ENODEV: i64 = -14;
pub const EBUSY: i64 = -15;

fn ret(val: i64) -> Result<u64, i64> {
    if val < 0 { Err(val) } else { Ok(val as u64) }
}

fn ret_unit(val: i64) -> Result<(), i64> {
    if val < 0 { Err(val) } else { Ok(()) }
}

// ── Inline asm helpers for Ob syscalls ──
// The kernel ABI: RAX=syscall, RBX=arg0, RCX=arg1, RDX=arg2, R8=arg3.
// We use push/pop to save rbx/rcx/rdx/r8 and write syscall registers
// in an order that prevents register overlap (read ptr/tmp registers
// first, then move to syscall registers).

// Safe syscall wrappers for Ob (RAX 60-64).
// Strategy: copy all args to temp registers (r8-r10) first, then
// move to the syscall arg registers (rbx/rcx/rdx/r8). This prevents
// the situation where reading an input register overwrites another
// input register due to register allocation overlap.

macro_rules! ob_syscall_2 {
    ($rax:literal, $rbx:expr, $rcx:expr) => {{
        let r: i64;
        core::arch::asm!(
            "push rbx",
            "push rcx",
            "mov r8, {a0}",
            "mov r9, {a1}",
            "mov rbx, r8",
            "mov rcx, r9",
            "mov rax, {n}",
            "int 0x80",
            "pop rcx",
            "pop rbx",
            a0 = in(reg) $rbx,
            a1 = in(reg) $rcx,
            n = const $rax,
            out("rax") r,
            out("r8") _, out("r9") _,
            options(nostack),
        );
        r
    }}
}

macro_rules! ob_syscall_3 {
    ($rax:literal, $rbx:expr, $rcx:expr, $rdx:expr) => {{
        let r: i64;
        core::arch::asm!(
            "push rbx",
            "push rcx",
            "push rdx",
            "mov r8, {a0}",
            "mov r9, {a1}",
            "mov r10, {a2}",
            "mov rbx, r8",
            "mov rcx, r9",
            "mov rdx, r10",
            "mov rax, {n}",
            "int 0x80",
            "pop rdx",
            "pop rcx",
            "pop rbx",
            a0 = in(reg) $rbx,
            a1 = in(reg) $rcx,
            a2 = in(reg) $rdx,
            n = const $rax,
            out("rax") r,
            out("r8") _, out("r9") _, out("r10") _,
            options(nostack),
        );
        r
    }}
}

macro_rules! ob_syscall_4 {
    ($rax:literal, $rbx:expr, $rcx:expr, $rdx:expr, $r8:expr) => {{
        let r: i64;
        core::arch::asm!(
            "push rbx",
            "push rcx",
            "push rdx",
            "push r8",
            "mov r9, {a0}",
            "mov r10, {a1}",
            "mov r11, {a2}",
            "mov r12, {a3}",
            "mov rbx, r9",
            "mov rcx, r10",
            "mov rdx, r11",
            "mov r8, r12",
            "mov rax, {n}",
            "int 0x80",
            "pop r8",
            "pop rdx",
            "pop rcx",
            "pop rbx",
            a0 = in(reg) $rbx,
            a1 = in(reg) $rcx,
            a2 = in(reg) $rdx,
            a3 = in(reg) $r8,
            n = const $rax,
            out("rax") r,
            out("r9") _, out("r10") _, out("r11") _, out("r12") _,
            options(nostack),
        );
        r
    }}
}

macro_rules! ob_syscall_5 {
    ($rax:literal, $rbx:expr, $rcx:expr, $rdx:expr, $r8:expr, $r9:expr) => {{
        let r: i64;
        core::arch::asm!(
            "push rbx", "push rcx", "push rdx", "push r8", "push r9",
            "mov r10, {a0}", "mov r11, {a1}", "mov r12, {a2}", "mov r13, {a3}", "mov r14, {a4}",
            "mov rbx, r10", "mov rcx, r11", "mov rdx, r12", "mov r8, r13", "mov r9, r14",
            "mov rax, {n}",
            "int 0x80",
            "pop r9", "pop r8", "pop rdx", "pop rcx", "pop rbx",
            a0 = in(reg) $rbx, a1 = in(reg) $rcx, a2 = in(reg) $rdx, a3 = in(reg) $r8, a4 = in(reg) $r9,
            n = const $rax,
            out("rax") r,
            out("r10") _, out("r11") _, out("r12") _, out("r13") _, out("r14") _,
            options(nostack),
        );
        r
    }}
}

mod process;
mod types;
mod ob;
mod fs;
mod cm;
mod net;
mod time;
mod drivers;

pub use process::*;
pub use types::*;
pub use ob::*;
pub use fs::*;
pub use cm::*;
pub use net::*;
pub use time::*;
pub use drivers::*;
