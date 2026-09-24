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

// ThreadState
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThreadState {
    Ready,
    Running,
    Blocked { waiting_for: u32 },
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
    pub waiting_for: Option<u32>,
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
}

impl fmt::Debug for Kthread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Kthread")
            .field("tid", &self.tid)
            .field("pid", &self.pid)
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
}

// Eprocess
pub struct Eprocess {
    pub pid: u32,
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
}
