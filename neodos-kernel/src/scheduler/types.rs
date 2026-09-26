//! Scheduler types — extracted from mod.rs (mechanical split, no behavior change)
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use crate::object::ObId;
use crate::security::token::Token;

// Constants (moved verbatim)
pub const KERNEL_STACK_SIZE: usize = 16384;
const IDLE_STACK_SIZE: usize = 4096;
pub const IDLE_TIME_SLICE: u16 = 10;

pub const PRIORITY_HIGH: u8 = 0;
pub const PRIORITY_ABOVE_NORMAL: u8 = 1;
pub const PRIORITY_NORMAL: u8 = 2;
pub const PRIORITY_IDLE: u8 = 3;
pub const PRIORITY_COUNT: u8 = 4;

pub const TIME_SLICES: [u16; PRIORITY_COUNT as usize] = [400, 200, 100, 50];

pub const BOOT_TID: u32 = 0;
pub const IDLE_TID: u32 = 1;

pub const AGING_INTERVAL_TICKS: u64 = 500;
pub const MAX_STARVATION_TICKS: u64 = 5000;

pub const TEB_SIZE: u64 = 0x1000;

pub const STACK_CANARY: u64 = 0xDEAD_BEEF_CAFE_BABE;

/// Phase 14-A: hard maximum byte length of a kernel process/thread name.
///
/// Names are bounded fixed-size metadata; they never cause heap allocation and
/// are immutable after the owning object is published. Over-long input is
/// truncated at `NAME_MAX` bytes; non-ASCII bytes are replaced with `?` so the
/// stored value is always valid ASCII/UTF-8.
pub const NAME_MAX: usize = 32;

/// Bounded, allocation-free kernel object name (process/thread).
///
/// This is *metadata*: PID/TID remain the authoritative numeric identity and
/// the name never participates in lookups, scheduling or security decisions.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct KernelName {
    bytes: [u8; NAME_MAX],
    len: u8,
}

impl KernelName {
    pub const fn empty() -> Self {
        KernelName { bytes: [0u8; NAME_MAX], len: 0 }
    }

    /// Bounded copy of `s` (ASCII; non-ASCII bytes become `?`), truncated.
    pub fn from_str(s: &str) -> Self {
        let mut n = Self::empty();
        n.push_str(s);
        n
    }

    /// Derive a short name from a filesystem/object path: take the substring
    /// after the last `\` or `/` and strip a trailing `.nxe` (any case).
    /// A plain name with no separators/suffix is returned unchanged.
    pub fn from_path(path: &str) -> Self {
        let base = path.rsplit(|c| c == '\\' || c == '/').next().unwrap_or(path);
        let base = base
            .strip_suffix(".nxe")
            .or_else(|| base.strip_suffix(".NXE"))
            .unwrap_or(base);
        Self::from_str(base)
    }

    /// Bounded append; stops at `NAME_MAX` (deterministic truncation).
    pub fn push_str(&mut self, s: &str) {
        for &b in s.as_bytes() {
            if (self.len as usize) >= NAME_MAX {
                break;
            }
            self.bytes[self.len as usize] = if b.is_ascii() { b } else { b'?' };
            self.len += 1;
        }
    }

    /// Bounded append of a decimal `u32` (used for `idle/<cpu>`).
    pub fn push_u32(&mut self, mut v: u32) {
        if v == 0 {
            if (self.len as usize) < NAME_MAX {
                self.bytes[self.len as usize] = b'0';
                self.len += 1;
            }
            return;
        }
        let mut tmp = [0u8; 10];
        let mut i = 0usize;
        while v > 0 && i < tmp.len() {
            tmp[i] = b'0' + (v % 10) as u8;
            v /= 10;
            i += 1;
        }
        while i > 0 {
            i -= 1;
            if (self.len as usize) >= NAME_MAX {
                break;
            }
            self.bytes[self.len as usize] = tmp[i];
            self.len += 1;
        }
    }

    /// Borrow the stored name. Always valid UTF-8 (ASCII by construction).
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }

    pub fn len(&self) -> usize { self.len as usize }
    pub fn is_empty(&self) -> bool { self.len == 0 }
}

impl Default for KernelName {
    fn default() -> Self { Self::empty() }
}

impl fmt::Debug for KernelName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for KernelName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// MmapRegion
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MmapRegion {
    pub base: u64,
    pub len: u64,
    pub prot: u16,
    pub flags: u16,
    pub drive: u8,
    pub inode: u32,
    pub file_size: u32,
}

// ThreadState — F-03: waiting_for is now u64 full-width (no 16-bit truncation)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThreadState {
    Ready,
    Running,
    Blocked { waiting_for: u64 },
    Suspended,
    Terminated,
}

impl ThreadState {
    pub fn to_u8(&self) -> u8 {
        match self {
            ThreadState::Ready => 0,
            ThreadState::Running => 1,
            ThreadState::Blocked { .. } => 2,
            ThreadState::Suspended => 3,
            ThreadState::Terminated => 4,
        }
    }
}

// Kthread — kernel thread control block
#[repr(C)]
pub struct Kthread {
    pub rax: u64,  pub rbx: u64,  pub rcx: u64,  pub rdx: u64,
    pub rsi: u64,  pub rdi: u64,  pub r8: u64,   pub r9: u64,
    pub r10: u64,  pub r11: u64,  pub r12: u64,  pub r13: u64,
    pub r14: u64,  pub r15: u64,  pub rbp: u64,
    pub rsp: u64,  pub rip: u64,  pub rflags: u64,
    pub tid: u32,
    pub pid: u32,
    pub state: ThreadState,
    pub cpu_ticks: u64,
    pub waiting_for: Option<u64>,
    pub priority: u8,
    pub time_slice_remaining: u16,
    pub ticks_since_scheduled: u64,
    pub kernel_stack_top: u64,
    pub kernel_stack: Option<Box<crate::scheduler::stack::AlignedKStack>>,
    pub teb_base: u64,
    pub cpu: u32,
    pub obj_id: Option<ObId>,
    pub kernel_apc_queue: VecDeque<crate::apc::ApcEntry>,
    pub user_apc_queue: VecDeque<crate::apc::ApcEntry>,
    pub apc_pending: bool,
    /// True for the per-CPU idle thread. Idle threads are never enqueued and
    /// are only selected when no other thread is Ready. Replaces the old
    /// `tid == IDLE_TID` checks so AP idle threads (distinct TIDs) are handled.
    pub is_idle: bool,
    /// Set when a *running* thread asks to yield (`yield_current_thread` /
    /// `sys_yield`). The thread is NOT marked Ready nor enqueued at that point:
    /// doing so would expose a live context with a stale `rsp` to other CPUs,
    /// which could dispatch the same KTHREAD concurrently on two CPUs sharing
    /// one kernel stack (Phase 13-A: AP iretq GPF). The flag is consumed by the
    /// timer/syscall switch-out path, which saves `rsp` first and only then
    /// publishes the thread as Ready.
    pub yield_requested: bool,
    /// Phase 14-A: human-readable thread name (bounded, immutable after
    /// publication). Metadata only — never used for scheduling/identity.
    pub name: KernelName,
}

impl fmt::Debug for Kthread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Kthread")
            .field("tid", &self.tid)
            .field("pid", &self.pid)
            .field("name", &self.name)
            .field("rip", &self.rip)
            .field("rsp", &self.rsp)
            .field("state", &self.state)
            .field("cpu_ticks", &self.cpu_ticks)
            .field("priority", &self.priority)
            .field("time_slice_remaining", &self.time_slice_remaining)
            .field("kernel_stack_top", &self.kernel_stack_top)
            .field("obj_id", &self.obj_id)
            .finish()
    }
}

impl Kthread {
    pub fn take_kernel_stack(&mut self) -> Option<Box<crate::scheduler::stack::AlignedKStack>> {
        self.kernel_stack.take()
    }

    /// Phase 14-A: read-only accessor. Allocation-free, no locks, no mutation.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }
}

// Eprocess
pub struct Eprocess {
    pub pid: u32,
    /// Phase 14-A: human-readable process name (bounded, immutable after
    /// publication). Metadata only.
    pub name: KernelName,
    pub parent_pid: u32,
    pub handle_table: crate::handle::HandleTable,
    pub cwd_drive: u8,
    pub cwd_path: String,
    pub heap_base: u64,
    pub heap_break: u64,
    pub user_slot: Option<u8>,
    pub mmap_regions: Vec<MmapRegion>,
    pub mmap_next: u64,
    pub thread_count: u32,
    pub exit_code: i64,
    pub obj_id: Option<ObId>,
    pub ob_id: Option<ObId>,
    pub address_space: crate::scheduler::address_space::AddressSpace,
    pub token: Token,
    pub vt_num: u8,
    pub args: [u8; 256],
}
