use core::sync::atomic::{AtomicUsize, AtomicU64, Ordering};

pub const VT_COUNT: usize = 4;
pub const VT_QUEUE_SIZE: usize = 4096;
pub const VT_CONSOLE_COLS: usize = 160;
pub const VT_CONSOLE_ROWS: usize = 50;
pub const VT_DIAG_RING_SIZE: usize = 64;

pub use crate::console::ConsoleState;

// ── Fase 1: Ring buffer diagnóstico sin serial por push ──
#[derive(Clone, Copy)]
pub struct VtDiagEntry {
    pub seq: u64,
    pub op: u8, // 0=push ok, 1=push full/drop, 2=pop ok, 3=pop empty
    pub byte: u8,
    pub head: usize,
    pub tail: usize,
    pub occ: usize,
    pub cpu: u32,
    pub tid: u32,
}
static VT_DIAG_SEQ: AtomicU64 = AtomicU64::new(0);
static VT_DIAG_HEAD: AtomicUsize = AtomicUsize::new(0);
static mut VT_DIAG_RING: [VtDiagEntry; VT_DIAG_RING_SIZE] = [VtDiagEntry { seq: 0, op: 0xFF, byte: 0, head: 0, tail: 0, occ: 0, cpu: 0, tid: 0 }; VT_DIAG_RING_SIZE];
static VT_PUSH_CNT: AtomicU64 = AtomicU64::new(0);
static VT_POP_CNT: AtomicU64 = AtomicU64::new(0);
static VT_DROP_CNT: AtomicU64 = AtomicU64::new(0);
static VT_MAX_OCC: AtomicUsize = AtomicUsize::new(0);

fn vt_diag_record(op: u8, byte: u8, head: usize, tail: usize) {
    let occ = if tail >= head { tail - head } else { VT_QUEUE_SIZE - head + tail };
    let max = VT_MAX_OCC.load(Ordering::Relaxed);
    if occ > max { VT_MAX_OCC.store(occ, Ordering::Relaxed); }
    let seq = VT_DIAG_SEQ.fetch_add(1, Ordering::Relaxed);
    let idx = VT_DIAG_HEAD.fetch_add(1, Ordering::Relaxed) % VT_DIAG_RING_SIZE;
    let cpu = if crate::hal::safe::GsBase::read() == 0 { 0 } else { unsafe { crate::arch::x64::cpu_local::this_cpu_id() } };
    let tid = crate::scheduler::current_tid();
    let e = VtDiagEntry { seq, op, byte, head, tail, occ, cpu, tid };
    unsafe { VT_DIAG_RING[idx] = e; }
    match op {
        0 => { VT_PUSH_CNT.fetch_add(1, Ordering::Relaxed); },
        1 => { VT_DROP_CNT.fetch_add(1, Ordering::Relaxed); },
        2 => { VT_POP_CNT.fetch_add(1, Ordering::Relaxed); },
        _ => {}
    }
}

pub fn vt_diag_dump() {
    unsafe { core::arch::asm!("cli"); }
    crate::println!("[VT_DIAG] push={} pop={} drop={} max_occ={} capacity={} (usable={})",
        VT_PUSH_CNT.load(Ordering::Relaxed),
        VT_POP_CNT.load(Ordering::Relaxed),
        VT_DROP_CNT.load(Ordering::Relaxed),
        VT_MAX_OCC.load(Ordering::Relaxed),
        VT_QUEUE_SIZE, VT_QUEUE_SIZE-1);
    let head = VT_DIAG_HEAD.load(Ordering::Relaxed);
    let n = head.min(VT_DIAG_RING_SIZE);
    let start = if head < VT_DIAG_RING_SIZE { 0 } else { head % VT_DIAG_RING_SIZE };
    for i in 0..n {
        let idx = (start + i) % VT_DIAG_RING_SIZE;
        let e = unsafe { VT_DIAG_RING[idx] };
        if e.op == 0xFF { continue; }
        let op_s = match e.op { 0 => "PUSH", 1 => "DROP", 2 => "POP ", 3 => "EMPTY", _ => "?" };
        crate::println!("[VT_DIAG][{}] seq={} {} byte=0x{:02x} '{}' h={} t={} occ={} cpu={} tid={}",
            i, e.seq, op_s, e.byte, if e.byte>=0x20 && e.byte<0x7f {e.byte as char} else {'.'}, e.head, e.tail, e.occ, e.cpu, e.tid);
    }
    let (h, t) = crate::input::manager::vt_active_head_tail();
    let occ = crate::input::manager::vt_active_occupancy();
    crate::println!("[VT_DIAG] head={} tail={} occ={} (live)", h, t, occ);
    unsafe { core::arch::asm!("sti"); }
}

pub fn vt_diag_reset() {
    VT_PUSH_CNT.store(0, Ordering::Relaxed);
    VT_POP_CNT.store(0, Ordering::Relaxed);
    VT_DROP_CNT.store(0, Ordering::Relaxed);
    VT_MAX_OCC.store(0, Ordering::Relaxed);
    VT_DIAG_SEQ.store(0, Ordering::Relaxed);
    VT_DIAG_HEAD.store(0, Ordering::Relaxed);
    unsafe { for i in 0..VT_DIAG_RING_SIZE { VT_DIAG_RING[i].op = 0xFF; } }
}

pub struct VtInputQueue {
    buffer: [u8; VT_QUEUE_SIZE],
    pub(crate) head: AtomicUsize,
    pub(crate) tail: AtomicUsize,
}

impl VtInputQueue {
    pub const fn new() -> Self {
        VtInputQueue {
            buffer: [0; VT_QUEUE_SIZE],
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    pub fn push(&self, byte: u8) -> Result<(), ()> {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed);
        let next = (tail + 1) % VT_QUEUE_SIZE;
        if next == head {
            vt_diag_record(1, byte, head, tail);
            return Err(());
        }
        unsafe {
            (self.buffer.as_ptr() as *mut u8).add(tail).write(byte);
        }
        self.tail.store(next, Ordering::Release);
        vt_diag_record(0, byte, head, next);
        Ok(())
    }

    pub fn pop(&self) -> Option<u8> {
        let tail = self.tail.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Relaxed);
        if head == tail {
            vt_diag_record(3, 0, head, tail);
            return None;
        }
        let byte = unsafe { self.buffer.as_ptr().add(head).read() };
        let next = (head + 1) % VT_QUEUE_SIZE;
        self.head.store(next, Ordering::Release);
        vt_diag_record(2, byte, next, tail);
        Some(byte)
    }

    pub fn has_data(&self) -> bool {
        self.head.load(Ordering::Relaxed) != self.tail.load(Ordering::Acquire)
    }
}

pub struct VtShadowBuffer {
    pub chars: [[u8; VT_CONSOLE_COLS]; VT_CONSOLE_ROWS],
}

impl VtShadowBuffer {
    pub const fn new() -> Self {
        VtShadowBuffer {
            chars: [[0u8; VT_CONSOLE_COLS]; VT_CONSOLE_ROWS],
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────

pub fn register_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;

    test_case!("vt_queue_create", {
        let q = VtInputQueue::new();
        test_eq!(q.pop(), None);
    });

    test_case!("vt_queue_push_pop", {
        let q = VtInputQueue::new();
        test_eq!(q.push(0x41), Ok(()));
        test_eq!(q.pop(), Some(0x41));
        test_eq!(q.pop(), None);
    });

    test_case!("vt_queue_capacity", {
        let q = VtInputQueue::new();
        let mut count = 0;
        while q.push(count as u8).is_ok() {
            count += 1;
        }
        test_true!(count > 0);
        test_eq!(count, VT_QUEUE_SIZE - 1);
    });

    test_case!("vt_queue_wrap_around", {
        let q = VtInputQueue::new();
        for i in 0..(VT_QUEUE_SIZE - 1) { let _ = q.push(i as u8); }
        for i in 0..50 { test_eq!(q.pop(), Some(i as u8)); }
        for i in (VT_QUEUE_SIZE - 1)..(VT_QUEUE_SIZE - 1 + 50) { let _ = q.push(i as u8); }
        for i in 50..(VT_QUEUE_SIZE - 1) { test_eq!(q.pop(), Some(i as u8)); }
        for i in (VT_QUEUE_SIZE - 1)..(VT_QUEUE_SIZE - 1 + 50) { test_eq!(q.pop(), Some(i as u8)); }
        test_eq!(q.pop(), None);
    });

    test_case!("vt_push_to_all_queues", {
        use crate::input::manager::{push_byte, pop_byte_from_vt};
        for _vt in 0..VT_COUNT {
            let _ = push_byte(b'X');
            let active = crate::input::active_vt();
            test_eq!(pop_byte_from_vt(active), Some(b'X'));
        }
    });

    test_case!("vt_count_at_least_2", {
        test_true!(VT_COUNT >= 2);
    });
}
