use lazy_static::lazy_static;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use crate::serial_println;
use crate::scheduler::{current_scheduler, ThreadState};
use crate::panic_classification::PanicClass;
use crate::trace::TraceEvent;
use crate::exception::{
    EXCEPTION_DIVIDE_ERROR, EXCEPTION_GPF, EXCEPTION_PAGE_FAULT,
    EXCEPTION_INVALID_OPCODE, EXCEPTION_OVERFLOW, EXCEPTION_BOUND_RANGE,
    EXCEPTION_DEVICE_NOT_AVAILABLE,
    DispatchResult,
    exception_dispatch,
};

// ── Lock-free timer diagnostic ring buffer ──
// Written from timer IRQ context (single producer), dumped to serial
// after boot tests pass (single consumer). Avoids serial_println!
// deadlock risk (spin::Mutex in IRQ context).
const TD_RING_SIZE: usize = 256;

#[derive(Clone, Copy)]
#[repr(C)]
struct TimerDiagEntry {
    tick: u64,
    cur_tid: u32,
    cur_rsp: u64,
    cur_cs: u64,
    flags: u8,       // bit0=is_user, bit1=should_preempt, bit2=has_non_idle
    path: u8,        // 0=ring3, 1=idle, 2=kernel, 3=no_preempt
    next_tid: u32,
    next_rsp: u64,
    next_ks_top: u64,
    returned_rsp: u64,
    frame_rip: u64,
    frame_cs: u64,
    frame_rflags: u64,
    phase: u8,       // 0=entry, 1=after_sched, 2=pre_return, 3=pre_iretq
}

static mut TD_RING: [TimerDiagEntry; TD_RING_SIZE] = [TimerDiagEntry {
    tick: 0, cur_tid: 0, cur_rsp: 0, cur_cs: 0, flags: 0, path: 0,
    next_tid: 0, next_rsp: 0, next_ks_top: 0, returned_rsp: 0,
    frame_rip: 0, frame_cs: 0, frame_rflags: 0, phase: 0,
}; TD_RING_SIZE];
static TD_HEAD: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

#[inline(always)]
fn td_push(entry: TimerDiagEntry) {
    let idx = unsafe {
        TD_HEAD.fetch_add(1, core::sync::atomic::Ordering::Relaxed) % TD_RING_SIZE
    };
    unsafe {
        *TD_RING.get_unchecked_mut(idx) = entry;
    }
}

/// Called by `timer_handler_asm` after it has restored all GPRs and immediately
/// before `iretq`.  `frame_rsp` is therefore the frame the CPU will consume,
/// not a value inferred from a KTHREAD.  This must remain lock-free and
/// allocation-free: it runs while handling the timer IRQ.
#[no_mangle]
pub extern "C" fn timer_trace_iretq_frame(frame_rsp: u64) {
    let (rip, cs, rflags) = unsafe {
        let frame = frame_rsp as *const u64;
        (frame.read(), frame.add(1).read(), frame.add(2).read())
    };
    td_push(TimerDiagEntry {
        tick: crate::hal::get_ticks(), cur_tid: 0, cur_rsp: frame_rsp,
        cur_cs: cs, flags: 0, path: 3, next_tid: 0, next_rsp: 0,
        next_ks_top: 0, returned_rsp: frame_rsp,
        frame_rip: rip, frame_cs: cs, frame_rflags: rflags, phase: 3,
    });
}

pub fn timer_diag_dump() {
    unsafe { core::arch::asm!("cli"); }
    crate::println!("[TIMER_DIAG] DUMP ENTERED");
    use core::sync::atomic::Ordering;
    let head = TD_HEAD.load(Ordering::Acquire);
    let count = head.min(TD_RING_SIZE);
    if count == 0 {
        crate::println!("[TIMER_DIAG] No entries recorded (head={})", head);
        unsafe { core::arch::asm!("sti"); }
        return;
    }
    crate::println!("[TIMER_DIAG] === {} entries (head={}) ===", count, head);
    let start = if head < TD_RING_SIZE { 0 } else { head % TD_RING_SIZE };
    for i in 0..count {
        let idx = (start + i) % TD_RING_SIZE;
        let e = unsafe { *TD_RING.get_unchecked(idx) };
        if e.tick == 0 && e.cur_rsp == 0 && e.returned_rsp == 0 { continue; }
        let path_str = match e.path { 0 => "R3", 1 => "IDLE", 2 => "KERN", _ => "NOPRE" };
        let phase_str = match e.phase { 0 => "ENTRY", 1 => "SCHED", 2 => "RET", _ => "IRETQ" };
        crate::println!(
            "[TD][{}] {} {} tick={} tid={} cs=0x{:x} rsp=0x{:x} f={:02x} nxt_tid={} nxt_rsp=0x{:x} ks=0x{:x} ret=0x{:x} frame=[rip=0x{:x} cs=0x{:x} rflags=0x{:x}]",
            i, path_str, phase_str, e.tick, e.cur_tid, e.cur_cs, e.cur_rsp, e.flags,
            e.next_tid, e.next_rsp, e.next_ks_top, e.returned_rsp,
            e.frame_rip, e.frame_cs, e.frame_rflags);
    }
    unsafe { core::arch::asm!("sti"); }
}

pub fn timer_diag_dump_secondary() {
    unsafe { core::arch::asm!("cli"); }
    crate::println!("[TIMER_DIAG_SECONDARY] DUMP ENTERED");
    use core::sync::atomic::Ordering;
    let head = TD_HEAD.load(Ordering::Acquire);
    let count = head.min(TD_RING_SIZE);
    if count == 0 {
        crate::println!("[TIMER_DIAG_SECONDARY] No entries (head={})", head);
        unsafe { core::arch::asm!("sti"); }
        return;
    }
    crate::println!("[TIMER_DIAG_SECONDARY] === {} entries (head={}) ===", count, head);
    let start = if head < TD_RING_SIZE { 0 } else { head % TD_RING_SIZE };
    let mut netd = 0;
    for i in 0..count {
        let idx = (start + i) % TD_RING_SIZE;
        let e = unsafe { *TD_RING.get_unchecked(idx) };
        if e.tick == 0 && e.cur_rsp == 0 && e.returned_rsp == 0 { continue; }
        if e.next_tid == 2 || e.phase == 3 { netd += 1; }
        let path_str = match e.path { 0 => "R3", 1 => "IDLE", 2 => "KERN", _ => "NOPRE" };
        let phase_str = match e.phase { 0 => "ENTRY", 1 => "SCHED", 2 => "RET", _ => "IRETQ" };
        crate::println!(
            "[TD2][{}] {} {} tick={} tid={} cs=0x{:x} rsp=0x{:x} f={:02x} nxt_tid={} nxt_rsp=0x{:x} ks=0x{:x} ret=0x{:x} frame=[rip=0x{:x} cs=0x{:x} rflags=0x{:x}]",
            i, path_str, phase_str, e.tick, e.cur_tid, e.cur_cs, e.cur_rsp, e.flags,
            e.next_tid, e.next_rsp, e.next_ks_top, e.returned_rsp,
            e.frame_rip, e.frame_cs, e.frame_rflags);
    }
    crate::println!("[TD2] netd/iretq relevant: {}", netd);
    unsafe { core::arch::asm!("sti"); }
}

// ── Fase 3 P1-P7: Netd pointer/stack/frame hexdump ──
static NETD_KPTR: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static NETD_BASE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static NETD_TOP: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static NETD_INIT_RSP: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static NETD_ENTRY: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

pub fn netd_record_create(kptr: u64, base: u64, top: u64, init_rsp: u64, entry: u64) {
    if crate::hal::get_ticks() < 100 { return; } // skip early unit tests
    NETD_KPTR.store(kptr, core::sync::atomic::Ordering::Relaxed);
    NETD_BASE.store(base, core::sync::atomic::Ordering::Relaxed);
    NETD_TOP.store(top, core::sync::atomic::Ordering::Relaxed);
    NETD_INIT_RSP.store(init_rsp, core::sync::atomic::Ordering::Relaxed);
    NETD_ENTRY.store(entry, core::sync::atomic::Ordering::Relaxed);
    // Hexdump initial 18-slot frame for comparison with PRE-IRETQ
    if init_rsp >= 0x1000 {
        let mut slots = [0u64; 18];
        unsafe {
            let p = init_rsp as *const u64;
            for i in 0..18 { slots[i] = p.add(i).read_volatile(); }
        }
        frame_push(0, kptr, init_rsp, init_rsp, slots);
    }
}

const FRAME_SLOTS: usize = 18;
const FRAME_DUMP_CAP: usize = 8;
#[derive(Clone, Copy)]
#[repr(C)]
struct FrameDump {
    tick: u64,
    kptr: u64,
    init_rsp: u64,
    ret_rsp: u64,
    slots: [u64; 18],
}
static mut FRAME_DUMPS: [FrameDump; 8] = [FrameDump { tick: 0, kptr: 0, init_rsp: 0, ret_rsp: 0, slots: [0; 18] }; 8];
static FRAME_HEAD: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

fn frame_push(tick: u64, kptr: u64, init_rsp: u64, ret_rsp: u64, slots: [u64; 18]) {
    let idx = FRAME_HEAD.fetch_add(1, core::sync::atomic::Ordering::Relaxed) % FRAME_DUMP_CAP;
    unsafe { FRAME_DUMPS[idx] = FrameDump { tick, kptr, init_rsp, ret_rsp, slots }; }
}

pub fn netd_record_select(next_kptr: u64, next_rsp: u64, next_ks_top: u64) {
    if crate::hal::get_ticks() < 100 { return; }
    let init = NETD_INIT_RSP.load(core::sync::atomic::Ordering::Relaxed);
    let base = NETD_BASE.load(core::sync::atomic::Ordering::Relaxed);
    let top = NETD_TOP.load(core::sync::atomic::Ordering::Relaxed);
    let kptr = NETD_KPTR.load(core::sync::atomic::Ordering::Relaxed);
    // Log pointer equality and rsp equality via TD_RING as well (reuse flags)
    // For hexdump, capture the 18-slot frame at ret_rsp
    if next_rsp < 0x1000 { return; }
    let mut slots = [0u64; 18];
    unsafe {
        let base_ptr = next_rsp as *const u64;
        for i in 0..18 { slots[i] = base_ptr.add(i).read_volatile(); }
    }
    frame_push(crate::hal::get_ticks(), next_kptr, init, next_rsp, slots);
    let _ = (base, top, kptr);
}

pub fn frame_dump() {
    unsafe { core::arch::asm!("cli"); }
    crate::println!("[FRAME_DUMP] {} entries", FRAME_HEAD.load(core::sync::atomic::Ordering::Relaxed).min(FRAME_DUMP_CAP));
    let head = FRAME_HEAD.load(core::sync::atomic::Ordering::Relaxed);
    let n = head.min(FRAME_DUMP_CAP);
    let start = if head < FRAME_DUMP_CAP { 0 } else { head % FRAME_DUMP_CAP };
    for i in 0..n {
        let idx = (start + i) % FRAME_DUMP_CAP;
        let e = unsafe { FRAME_DUMPS[idx] };
        if e.tick == 0 { continue; }
        crate::println!("[FD][{}] tick={} kptr=0x{:x} init=0x{:x} ret=0x{:x}", i, e.tick, e.kptr, e.init_rsp, e.ret_rsp);
        for s in 0..18 {
            let exp = if s < 15 { 0 } else if s == 15 { NETD_ENTRY.load(core::sync::atomic::Ordering::Relaxed) } else if s == 16 { 0x08 } else { 0x202 };
            let mark = if e.slots[s] == exp { " " } else { "*" };
            crate::println!("  slot{:02} {} 0x{:016x} exp 0x{:016x}", s, mark, e.slots[s], exp);
        }
        // Search for displaced values within ±256 bytes
        let entry = NETD_ENTRY.load(core::sync::atomic::Ordering::Relaxed);
        for off in -64..64 {
            let addr = (e.ret_rsp as i64 + off*8) as u64;
            if addr < 0x1000 { continue; }
            let v = unsafe { (addr as *const u64).read_volatile() };
            if v == entry { crate::println!("  displaced RIP 0x{:x} at off {}", v, off); }
            if v == 0x08 { crate::println!("  displaced CS 0x08 at off {}", off); }
        }
    }
    unsafe { core::arch::asm!("sti"); }
}

pub fn netd_diag_dump() {
    unsafe { core::arch::asm!("cli"); }
    crate::println!("[NETD_DIAG] kptr=0x{:x} base=0x{:x} top=0x{:x} init=0x{:x} entry=0x{:x}",
        NETD_KPTR.load(core::sync::atomic::Ordering::Relaxed),
        NETD_BASE.load(core::sync::atomic::Ordering::Relaxed),
        NETD_TOP.load(core::sync::atomic::Ordering::Relaxed),
        NETD_INIT_RSP.load(core::sync::atomic::Ordering::Relaxed),
        NETD_ENTRY.load(core::sync::atomic::Ordering::Relaxed));
    // Canary check
    let base = NETD_BASE.load(core::sync::atomic::Ordering::Relaxed);
    if base != 0 {
        let canary = unsafe { (base as *const u64).read_volatile() };
        crate::println!("[CANARY] base 0x{:x} canary 0x{:x} exp 0x{:x} {}", base, canary, crate::scheduler::STACK_CANARY, if canary==crate::scheduler::STACK_CANARY { "OK" } else { "CORRUPT" });
    }
    unsafe { core::arch::asm!("sti"); }
}

// ── FINAL_IRETQ capture (H1) ──
#[no_mangle] static FINAL_RSP: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
#[no_mangle] static FINAL_RIP: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
#[no_mangle] static FINAL_CS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
#[no_mangle] static FINAL_RFLAGS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

pub fn final_iretq_dump() {
    unsafe { core::arch::asm!("cli"); }
    let rsp = FINAL_RSP.load(core::sync::atomic::Ordering::Relaxed);
    let rip = FINAL_RIP.load(core::sync::atomic::Ordering::Relaxed);
    let cs = FINAL_CS.load(core::sync::atomic::Ordering::Relaxed);
    let rflags = FINAL_RFLAGS.load(core::sync::atomic::Ordering::Relaxed);
    let ret = NETD_INIT_RSP.load(core::sync::atomic::Ordering::Relaxed);
    crate::println!("[FINAL_IRETQ] rsp=0x{:x} ret+120=0x{:x} delta={} rip=0x{:x} cs=0x{:x} rflags=0x{:x} exp_rip=0x{:x} exp_cs=0x08 exp_rflags=0x202",
        rsp, ret.wrapping_add(120), (rsp as i64 - ret.wrapping_add(120) as i64), rip, cs, rflags, NETD_ENTRY.load(core::sync::atomic::Ordering::Relaxed));
    if rsp == ret.wrapping_add(120) && rip == NETD_ENTRY.load(core::sync::atomic::Ordering::Relaxed) && cs == 0x08 {
        crate::println!("[FINAL_IRETQ] FRAME CORRECT");
    } else {
        crate::println!("[FINAL_IRETQ] FRAME MISMATCH");
    }
    unsafe { core::arch::asm!("sti"); }
}

pub fn gdt_dump() {
    unsafe { core::arch::asm!("cli"); }
    let mut gdtr: [u8; 10] = [0; 10];
    unsafe { core::arch::asm!("sgdt [{0}]", in(reg) gdtr.as_mut_ptr(), options(nostack)); }
    let limit = u16::from_le_bytes([gdtr[0], gdtr[1]]);
    let base = u64::from_le_bytes([gdtr[2], gdtr[3], gdtr[4], gdtr[5], gdtr[6], gdtr[7], gdtr[8], gdtr[9]]);
    crate::println!("[GDT] base=0x{:x} limit=0x{:x}", base, limit);
    if base != 0 {
        let entry08 = unsafe { ((base + 8) as *const u64).read_volatile() };
        crate::println!("[GDT] entry 0x08 raw=0x{:016x}", entry08);
        let present = (entry08 >> 47) & 1;
        let typ = (entry08 >> 40) & 0xF;
        let s = (entry08 >> 44) & 1;
        let dpl = (entry08 >> 45) & 0x3;
        let l = (entry08 >> 53) & 1;
        crate::println!("[GDT] 0x08 P={} S={} DPL={} L={} type=0x{:x}", present, s, dpl, l, typ);
        if present != 1 || s != 1 || typ != 0xA {
            crate::println!("[GDT] 0x08 INVALID kernel code descriptor");
        }
    }
    let tr: u16;
    unsafe { core::arch::asm!("str {0:x}", out(reg) tr, options(nostack)); }
    crate::println!("[TR] selector=0x{:x}", tr);
    let mut tss_base: u64 = 0;
    // TSS base is in GDT entry for TR, hard to decode without base, just log TR
    unsafe { core::arch::asm!("cli"); }
    crate::println!("[TSS] RSP0 will be checked in timer path");
    unsafe { core::arch::asm!("sti"); }
}

pub fn decode_gpf_error(err: u16) {
    let ext = (err >> 0) & 1;
    let idt = (err >> 1) & 1;
    let ti = (err >> 2) & 1;
    let index = (err >> 3) & 0x1FFF;
    crate::println!("[GPF_DECODE] err=0x{:x} EXT={} IDT={} TI={} index=0x{:x} ({})", err, ext, idt, ti, index, index*8);
    if ti == 1 {
        crate::println!("[GPF_DECODE] TI=1 → LDT, not GDT");
    } else {
        crate::println!("[GPF_DECODE] TI=0 → GDT selector 0x{:x}", (index<<3) | (err & 0x3));
    }
    if err == 0x8ae0 || err == 0x3ae0 {
        crate::println!("[GPF_DECODE] err matches 0x8ae0/0x3ae0 pattern from earlier GPFs (displaced CS?)");
    }
}

core::arch::global_asm!(
    ".extern timer_handler_inner",
    ".extern timer_trace_iretq_frame",
    ".global timer_handler_asm",
    "timer_handler_asm:",
    "push rbp",
    "push r15",
    "push r14",
    "push r13",
    "push r12",
    "push r11",
    "push r10",
    "push r9",
    "push r8",
    "push rdi",
    "push rsi",
    "push rdx",
    "push rcx",
    "push rbx",
    "push rax",
    "mov rdi, rsp",
    "call timer_handler_inner",
    // Fase 3: trace iretq frame on old stack (stack-based, no r12 dependency)
    "push rax",
    "mov rdi, rax",
    "add rdi, 120",
    "call timer_trace_iretq_frame",
    "pop rax",
    "mov rsp, rax",
    "pop rax",
    "pop rbx",
    "pop rcx",
    "pop rdx",
    "pop rsi",
    "pop rdi",
    "pop r8",
    "pop r9",
    "pop r10",
    "pop r11",
    "pop r12",
    "pop r13",
    "pop r14",
    "pop r15",
    "pop rbp",
    "iretq"
);

core::arch::global_asm!(
    ".extern syscall_dispatch",
    ".extern syscall_try_resched",
    ".extern syscall_resched_enabled",
    ".extern apc_dispatch_on_syscall_return",
    ".extern is_thread_terminated",
    ".global syscall_handler_asm",
    "syscall_handler_asm:",
    "push rbp",
    "push r15",
    "push r14",
    "push r13",
    "push r12",
    "push r11",
    "push r10",
    "push r9",
    "push r8",
    "push rdi",
    "push rsi",
    "push rdx",
    "push rcx",
    "push rbx",
    "push rax",
    "mov r15, [rsp]",
    // The 15 saved GPRs occupy 120 bytes.  The CPU's INT 0x80 return
    // frame starts at rsp+120 (RIP, CS, RFLAGS, RSP, SS).
    ".extern syscall_trace_frame",
    "mov rdi, rsp",
    "xor rsi, rsi",                       // phase 0 = handler entry
    "call syscall_trace_frame",
    "mov rdi, [rsp + 0]",
    "mov rsi, [rsp + 8]",
    "mov rdx, [rsp + 16]",
    "mov rcx, [rsp + 24]",
    "mov r8,  [rsp + 48]",
    "mov r9,  [rsp + 56]",
    "call syscall_dispatch",
    "mov [rsp + 0], rax",
    // Check if syscall number was 0 (exit) — if so, check per-CPU exit_now flag
    "test r15, r15",
    "jnz 1f",
    // Read per-CPU exit_now from KPRCB via GS segment
    "xor rax, rax",
    "mov al, gs:[0xB98]",                  // OFFSET_EXIT_NOW in KPRCB
    "test al, al",
    "jz 4f",
    // Clear exit_now and jump to exit_to_kernel
    "mov byte ptr gs:[0xB98], 0",          // OFFSET_EXIT_NOW
    ".extern exit_to_kernel",
    "jmp exit_to_kernel",
    // Non-last thread exit: check if thread was terminated
    "4:",
    "push rsp",
    "call is_thread_terminated",
    "add rsp, 8",
    "test rax, rax",
    "jz 1f",
    // Thread terminated but not last → reschedule (switch to next thread)
    "mov rdi, rsp",
    "call syscall_try_resched",
    "mov rsp, rax",
    "jmp 3f",
    "1:",
    // Check per-CPU NEED_RESCHED via GS segment (offset 0x015 in KPRCB)
    "xor rax, rax",
    "mov al, gs:[0x015]",                  // OFFSET_NEED_RESCHED in KPRCB
    "test al, al",
    "jz 2f",
    // Clear per-CPU NEED_RESCHED
    "mov byte ptr gs:[0x015], 0",          // OFFSET_NEED_RESCHED
    // Also clear the global NEED_RESCHED (backward compat) and do work
    "call clear_need_resched",
    "test al, al",
    "jz 2f",
    "call syscall_resched_enabled",
    "test rax, rax",
    "jz 2f",
    "mov rdi, rsp",
    "call syscall_try_resched",
    "mov rsp, rax",
    "2:",
    // A4.5: Dispatch pending APCs before returning to Ring 3
    "call apc_dispatch_on_syscall_return",
    "3:",
    "mov rdi, rsp",
    "mov rsi, 1",                         // phase 1 = frame about to be iretq'd
    "call syscall_trace_frame",
    "pop rax",
    "pop rbx",
    "pop rcx",
    "pop rdx",
    "pop rsi",
    "pop rdi",
    "pop r8",
    "pop r9",
    "pop r10",
    "pop r11",
    "pop r12",
    "pop r13",
    "pop r14",
    "pop r15",
    "pop rbp",

    "iretq"
);

extern "C" {
    fn timer_handler_asm();
    fn syscall_handler_asm();
}

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();

        idt.divide_error.set_handler_fn(divide_error_handler);
        idt.debug.set_handler_fn(debug_handler);
        idt.non_maskable_interrupt.set_handler_fn(nmi_handler);
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.overflow.set_handler_fn(overflow_handler);
        idt.bound_range_exceeded.set_handler_fn(bounds_handler);
        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
        idt.device_not_available.set_handler_fn(device_not_available_handler);

        unsafe {
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(crate::arch::x64::gdt::DOUBLE_FAULT_IST_INDEX);
        }

        idt.invalid_tss.set_handler_fn(invalid_tss_handler);
        idt.segment_not_present.set_handler_fn(segment_not_present_handler);
        idt.stack_segment_fault.set_handler_fn(stack_segment_fault_handler);
        idt.general_protection_fault.set_handler_fn(gpf_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.x87_floating_point.set_handler_fn(x87_handler);
        idt.alignment_check.set_handler_fn(alignment_check_handler);
        idt.machine_check.set_handler_fn(machine_check_handler);
        idt.simd_floating_point.set_handler_fn(simd_handler);
        idt.virtualization.set_handler_fn(virtualization_handler);

        unsafe {
            idt[32].set_handler_addr(x86_64::VirtAddr::new(timer_handler_asm as *const () as u64));
        }
        idt[33].set_handler_fn(keyboard_handler);
        idt[36].set_handler_fn(serial_handler);
        idt[44].set_handler_fn(mouse_handler);

        unsafe {
            idt[0x80]
                .set_handler_addr(x86_64::VirtAddr::new(syscall_handler_asm as *const () as u64))
                .set_privilege_level(x86_64::PrivilegeLevel::Ring3)
                .disable_interrupts(true);
        }

        // IPI handler for per-CPU reschedule (vector 0xF0)
        unsafe {
            idt[0xF0]
                .set_handler_addr(x86_64::VirtAddr::new(ipi_reschedule_handler as *const () as u64));
        }

        // IPI handler for TLB shootdown (vector 0xF1)
        unsafe {
            idt[0xF1]
                .set_handler_addr(x86_64::VirtAddr::new(ipi_tlb_shootdown_handler as *const () as u64));
        }

        // IPI handler for cross-CPU function call (vector 0xF2)
        unsafe {
            idt[0xF2]
                .set_handler_addr(x86_64::VirtAddr::new(ipi_call_function_handler as *const () as u64));
        }

        idt
    };
}

macro_rules! panic_classified {
    ($class:expr, $($arg:tt)*) => {{
        crate::panic_classification::set_panic_class($class);
        panic!($($arg)*);
    }};
}

/// Helper: check if the exception was from user mode (Ring 3).
fn is_user_exception(frame: &InterruptStackFrame) -> bool {
    frame.code_segment == 0x1B
}

/// Helper: terminate the current user process (Ring 3 exception unhandled).
fn terminate_user_process() {
    use crate::scheduler::{current_scheduler, current_tid};
    use crate::syscall::set_need_resched;
    let tid = current_tid();
    if tid > 0 {
        let mut s = current_scheduler().lock();
        if let Some(k) = s.find_kthread_mut(tid) {
            k.state = ThreadState::Terminated;
        }
    }
    set_need_resched();
}

extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();

    if is_user_exception(&stack_frame) {
        let result = exception_dispatch(
            EXCEPTION_DIVIDE_ERROR, rip, rsp, 0, true, 0, 0,
        );
        match result {
            DispatchResult::Handled => return,
            DispatchResult::Terminated => {
                terminate_user_process();
                return;
            }
            DispatchResult::Panic => {} // fall through to kernel panic
        }
    }

    crate::trace_event!(TraceEvent::Panic, 0, 0, 0, 0);
    panic_classified!(PanicClass::UnknownCpuException,
        "Divide error: rip={:#x}", rip);
}

extern "x86-interrupt" fn debug_handler(frame: InterruptStackFrame) {
    ktrace!(crate::log::LogSubsys::Exception, "Debug exception @ {:#x}", frame.instruction_pointer.as_u64());
}

extern "x86-interrupt" fn nmi_handler(stack_frame: InterruptStackFrame) {
    crate::trace_event!(TraceEvent::Panic, 1, 0, 0, 0);
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    crate::crash::dump_nmi(rip, rsp);
    panic_classified!(PanicClass::UnknownCpuException, "Non-maskable interrupt");
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    ktrace!(crate::log::LogSubsys::Exception, "Breakpoint: rip={:#x}", stack_frame.instruction_pointer.as_u64());
}

extern "x86-interrupt" fn overflow_handler(stack_frame: InterruptStackFrame) {
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    if is_user_exception(&stack_frame) {
        let result = exception_dispatch(EXCEPTION_OVERFLOW, rip, rsp, 0, true, 0, 0);
        match result {
            DispatchResult::Handled => return,
            DispatchResult::Terminated => { terminate_user_process(); return; }
            DispatchResult::Panic => {}
        }
    }
    panic_classified!(PanicClass::UnknownCpuException, "Overflow: rip={:#x}", rip);
}

extern "x86-interrupt" fn bounds_handler(stack_frame: InterruptStackFrame) {
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    if is_user_exception(&stack_frame) {
        let result = exception_dispatch(EXCEPTION_BOUND_RANGE, rip, rsp, 0, true, 0, 0);
        match result {
            DispatchResult::Handled => return,
            DispatchResult::Terminated => { terminate_user_process(); return; }
            DispatchResult::Panic => {}
        }
    }
    panic_classified!(PanicClass::UnknownCpuException, "Bound range: rip={:#x}", rip);
}

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    if is_user_exception(&stack_frame) {
        let result = exception_dispatch(EXCEPTION_INVALID_OPCODE, rip, rsp, 0, true, 0, 0);
        match result {
            DispatchResult::Handled => return,
            DispatchResult::Terminated => { terminate_user_process(); return; }
            DispatchResult::Panic => {}
        }
    }
    panic_classified!(PanicClass::UnknownCpuException, "Invalid opcode: rip={:#x}", rip);
}

extern "x86-interrupt" fn device_not_available_handler(stack_frame: InterruptStackFrame) {
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    if is_user_exception(&stack_frame) {
        let result = exception_dispatch(EXCEPTION_DEVICE_NOT_AVAILABLE, rip, rsp, 0, true, 0, 0);
        match result {
            DispatchResult::Handled => return,
            DispatchResult::Terminated => { terminate_user_process(); return; }
            DispatchResult::Panic => {}
        }
    }
    panic_classified!(PanicClass::UnknownCpuException, "Device not available: rip={:#x}", rip);
}

extern "x86-interrupt" fn double_fault_handler(stack_frame: InterruptStackFrame, error_code: u64) -> ! {
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    crate::crash::dump_double_fault(rip, rsp, error_code);
    panic_classified!(PanicClass::DoubleFault,
        "Double fault: rip={:#x} rsp={:#x} error={:#x}",
        rip, rsp, error_code);
}

extern "x86-interrupt" fn invalid_tss_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    panic_classified!(PanicClass::InvalidContextSwitch,
        "Invalid TSS: rip={:#x} rsp={:#x} error={:#x}",
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.stack_pointer.as_u64(),
        error_code);
}

extern "x86-interrupt" fn segment_not_present_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    panic_classified!(PanicClass::MemoryCorruption,
        "Segment not present: rip={:#x} error={:#x}",
        stack_frame.instruction_pointer.as_u64(), error_code);
}

extern "x86-interrupt" fn stack_segment_fault_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    panic_classified!(PanicClass::StackCorruption,
        "Stack segment fault: rip={:#x} rsp={:#x} error={:#x}",
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.stack_pointer.as_u64(),
        error_code);
}

extern "x86-interrupt" fn gpf_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    // Read actual GS selector directly from the CPU register to
    // determine if the fault is from a bad GS load.
    let gs: u16;
    unsafe { core::arch::asm!("mov {0:x}, gs", out(reg) gs, options(nomem, nostack)); }
    kerror!(crate::log::LogSubsys::Exception,
        "GPF: error={:#x} rip={:#x} cs={:#x} rflags={:#x} rsp={:#x} tick={} GS={:#x}",
        error_code, rip,
        stack_frame.code_segment,
        stack_frame.cpu_flags, rsp,
        crate::hal::get_ticks(), gs,
    );

    // Try user-mode dispatch first
    let in_user_window = (crate::arch::x64::paging::USER_BASE..crate::arch::x64::paging::USER_LIMIT).contains(&rip);
    if is_user_exception(&stack_frame) || in_user_window {
        // For GPF, the fault_addr is typically RIP (null deref) or a selector
        let fault_addr = rip;
        let result = exception_dispatch(EXCEPTION_GPF, rip, rsp, error_code, true, fault_addr, error_code);
        match result {
            DispatchResult::Handled => return,
            DispatchResult::Terminated => {
                terminate_user_process();
                return;
            }
            DispatchResult::Panic => {} // fall through
        }
    }

    // Fase 3 P2-P6: dump diagnostico FRAME/TD/NETD/FINAL/GDT antes de panic para capturar first invalid
    crate::serial_println!("[GPF_DIAG] tick={} tid={} rip=0x{:x} rsp=0x{:x} err=0x{:x}", crate::hal::get_ticks(), crate::scheduler::current_tid(), rip, rsp, error_code);
    frame_dump();
    netd_diag_dump();
    final_iretq_dump();
    gdt_dump();
    decode_gpf_error(error_code as u16);
    // TD_RING dump via secondary (no cli/sti double)
    crate::arch::x64::idt::timer_diag_dump();

    let class = if error_code == 0x15c {
        PanicClass::InvalidIretq
    } else if rsp & 0xFFF < 0x100 || rsp & 0xFFF > 0xF00 {
        // RSP near page boundary — possible stack overflow
        PanicClass::StackCorruption
    } else {
        PanicClass::Gpf
    };
    crate::trace_event!(TraceEvent::Panic, 3, error_code, rip, rsp);
    panic_classified!(class,
        "GPF: error={:#x} rip={:#x}", error_code, rip);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    // INV-14: Page fault at IRQL >= DISPATCH is fatal (bugcheck).
    let irql = unsafe { crate::arch::x64::cpu_local::this_cpu_irql() };
    if irql >= crate::hal::irql::DISPATCH_LEVEL {
        let rip = stack_frame.instruction_pointer.as_u64();
        let virt = crate::hal::read_cr2();
        panic_classified!(PanicClass::PageFault,
            "BUGCHECK KI_EXCEPTION_ACCESS_VIOLATION: page fault at IRQL {} (>= DISPATCH) \
             @ {:#x} virt={:#x}",
            irql, rip, virt);
    }

    let virt = crate::hal::read_cr2();
    let is_user = error_code.contains(PageFaultErrorCode::USER_MODE);
    let is_write = error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE);
    let is_not_present = !error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION);

    if is_user && is_not_present {
        if crate::arch::x64::paging::handle_heap_page_fault(virt, true, is_write) {
            return;
        }
        if crate::arch::x64::paging::handle_mmap_page_fault(virt, true, is_write) {
            return;
        }
        // Also handle TEB demand paging (TEB at 0x7000)
        if crate::arch::x64::paging::handle_teb_page_fault(virt) {
            return;
        }
    }

    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();

    // A3.4: Try user-mode SEH dispatch before panic
    if is_user {
        let fault_code = if is_not_present { 0u64 } else { 1u64 }; // 0=not-present, 1=protection
        let result = exception_dispatch(
            EXCEPTION_PAGE_FAULT, rip, rsp, 0, true, virt, fault_code,
        );
        match result {
            DispatchResult::Handled => return,
            DispatchResult::Terminated => {
                terminate_user_process();
                return;
            }
            DispatchResult::Panic => {} // fall through
        }
    }

    let class = if !is_not_present {
        PanicClass::PageTableCorruption
    } else {
        PanicClass::PageFault
    };
    crate::trace_event!(TraceEvent::Panic, 4, virt, rip, is_write as u64);
    panic_classified!(class,
        "Page fault @ {:#x} (user={}, write={}, np={}) rip={:#x}",
        virt, is_user, is_write, is_not_present, rip);
}

extern "x86-interrupt" fn x87_handler(stack_frame: InterruptStackFrame) {
    panic_classified!(PanicClass::UnknownCpuException,
        "x87 FP: rip={:#x}", stack_frame.instruction_pointer.as_u64());
}

extern "x86-interrupt" fn alignment_check_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    panic_classified!(PanicClass::MemoryCorruption,
        "Alignment check: rip={:#x} error={:#x}",
        stack_frame.instruction_pointer.as_u64(), error_code);
}

extern "x86-interrupt" fn machine_check_handler(stack_frame: InterruptStackFrame) -> ! {
    panic_classified!(PanicClass::UnknownCpuException,
        "Machine check: rip={:#x}", stack_frame.instruction_pointer.as_u64());
}

extern "x86-interrupt" fn simd_handler(stack_frame: InterruptStackFrame) {
    panic_classified!(PanicClass::UnknownCpuException,
        "SIMD FP: rip={:#x}", stack_frame.instruction_pointer.as_u64());
}

extern "x86-interrupt" fn virtualization_handler(stack_frame: InterruptStackFrame) {
    panic_classified!(PanicClass::UnknownCpuException,
        "Virtualization: rip={:#x}", stack_frame.instruction_pointer.as_u64());
}

/// Read CS selector from the interrupt stack frame.
/// The timer_handler_asm pushes 15 GPRs, then the iretq frame starts
/// at current_rsp + 120 (= 15 * 8).  CS is at +128.
/// 0x08 = Ring 0 (kernel), 0x1B = Ring 3 (user).
unsafe fn read_cs_from_stack(current_rsp: u64) -> u16 {
    ((current_rsp + 128) as *const u16).read()
}

/// Return true if the interrupted context was Ring 3 (user mode),
/// meaning it is safe to preempt and context-switch.
unsafe fn is_user_mode_interrupt(current_rsp: u64) -> bool {
    read_cs_from_stack(current_rsp) == 0x1B
}

unsafe fn prepare_timer_return(next: *mut crate::scheduler::Kthread) {
    let next_rsp = (*next).rsp;
    let next_cs = *((next_rsp + 128) as *const u64);
    if next_cs & 3 == 3 {
        crate::arch::x64::gdt::prepare_ring3_return(
            (*next).kernel_stack_top, (*next).tid, (*next).pid);
    }
}

#[no_mangle]
pub extern "C" fn timer_handler_inner(current_rsp: u64) -> u64 {
    crate::invariants::timer_irq_enter();
    crate::invariants::irq_enter_check(32);

    crate::hal::increment_ticks();
    crate::console::cursor_timer_tick();
    let current_tick = crate::hal::get_ticks();

    // Increment per-CPU timer tick count
    unsafe { crate::arch::x64::cpu_local::this_cpu_inc_timer_tick_count(); }

    crate::trace_event!(TraceEvent::IrqTimerTick, current_tick, current_rsp, 0, 0);

    // A3.3: Watchdog pet + check on every timer tick
    crate::watchdog::watchdog_pet();

    // v0.46: Timer Object tick — decrement running timers
    crate::object::timer::tick();
    if crate::watchdog::watchdog_check() {
        crate::watchdog::watchdog_trigger();
    }

    {
        use core::sync::atomic::Ordering;
        let last_flush = crate::globals::LAST_FLUSH_TICK.load(Ordering::Relaxed);
        if current_tick.saturating_sub(last_flush) >= crate::globals::FLUSH_INTERVAL_TICKS {
            crate::globals::NEED_CACHE_FLUSH.store(true, Ordering::Relaxed);
        }
    }

    let scheduler_mutex = current_scheduler();
    let mut scheduler = scheduler_mutex.lock();

    scheduler.on_timer_tick(current_rsp);

    let tid = scheduler.current_tid;
    let interrupted_cs = unsafe { *((current_rsp + 128) as *const u64) };
    let is_user_mode = (interrupted_cs & 3) == 3;

    // ── Preemptive context switch ──
    // Rule: Ring 3 threads may be preempted on timeslice expiry.
    // Ring 0 kernel threads (netd, boot) that are in Ready state (via yield
    // or timeslice expiry) are also preempted if another non-idle thread
    // is available — the "idle preemption" path below handles this.

    let has_non_idle = scheduler.has_non_idle_threads();
    let current_state = scheduler.current_kthread_mut().map(|k| k.state);

    // Diagnostic entry: capture state before preemption decision
    td_push(TimerDiagEntry {
        tick: current_tick, cur_tid: tid, cur_rsp: current_rsp,
        cur_cs: interrupted_cs,
        flags: (is_user_mode as u8)
            | (((current_state == Some(ThreadState::Ready)) as u8) << 1)
            | ((has_non_idle as u8) << 2),
        path: 3, next_tid: 0, next_rsp: 0, next_ks_top: 0, returned_rsp: 0,
        frame_rip: 0, frame_cs: 0, frame_rflags: 0, phase: 0,
    });

    if is_user_mode && tid != crate::scheduler::IDLE_TID {
        let should_preempt = current_state == Some(ThreadState::Ready);

        crate::trace_timer_irq!(
            if should_preempt { 1u8 } else { 3u8 },
            tid, interrupted_cs, has_non_idle as u8);

        if should_preempt {
            kdebug!(crate::log::LogSubsys::Sched, "[SCHED] PREEMPT tid={} reason=timeslice_expired", tid);
            // Save the current thread's RSP
            if let Some(k) = scheduler.current_kthread_mut() {
                k.rsp = current_rsp;
            }

            // Pick next thread
            let next = scheduler.schedule();
            let next_tid = unsafe { (*next).tid };

            // Safety: if the next thread has rsp==0, proceeding would
            // cause a triple fault (push at address 0).  Log and panic
            // instead so we can identify the root cause.
            let next_rsp = unsafe { (*next).rsp };
            let next_ks = unsafe { (*next).kernel_stack_top };
            td_push(TimerDiagEntry {
                tick: current_tick, cur_tid: tid, cur_rsp: current_rsp,
                cur_cs: interrupted_cs,
                flags: 0x01, path: 0,
                next_tid, next_rsp, next_ks_top: next_ks, returned_rsp: 0,
                frame_rip: 0, frame_cs: 0, frame_rflags: 0, phase: 1,
            });
            netd_record_select(next as *const _ as u64, next_rsp, next_ks);
            if next_rsp == 0 {
                if next_tid == crate::scheduler::IDLE_TID {
                    crate::hal::ack_irq(32);
                    crate::invariants::timer_irq_exit();
                    crate::invariants::irq_exit_clear();
                    return current_rsp;
                }
                panic!("timer_handler: next TID={} has rsp=0 (prev={}, cpl=0, kernel_stack_top=0x{:x})",
                    next_tid, tid, unsafe { (*next).kernel_stack_top });
            }

            // A Ring 3 interrupt may dequeue a Ready kernel thread. Its Ring
            // 0 frame cannot be returned through this Ring 3 interrupt frame.
            let next_cs = unsafe { *((next_rsp + 128) as *const u64) };
            if next_cs & 3 != 3 {
                unsafe {
                    (*next).state = ThreadState::Ready;
                    crate::scheduler::Scheduler::enqueue_to_cpu_run_queue(&*next);
                }
                if let Some(current) = scheduler.find_kthread_mut(tid) {
                    crate::scheduler::Scheduler::remove_from_run_queue(current);
                    current.state = ThreadState::Running;
                }
                scheduler.current_tid = tid;
                crate::hal::ack_irq(32);
                crate::invariants::timer_irq_exit();
                crate::invariants::irq_exit_clear();
                return current_rsp;
            }

            if next_tid == tid {
                unsafe {
                    (*next).state = ThreadState::Running;
                    (*next).time_slice_remaining =
                        crate::scheduler::TIME_SLICES[((*next).priority as usize).min(
                            crate::scheduler::PRIORITY_COUNT as usize - 1,
                        )];
                    (*next).ticks_since_scheduled = 0;
                    // INVARIANT: TSS.RSP0 must always match the current thread.
                    // Even when schedule() returns the same thread, we must
                    // update RSP0 because a PREVIOUS context switch may have
                    // left it pointing to another thread's kernel stack.
                    let ks_top = (*next).kernel_stack_top;
                    prepare_timer_return(next);
                }
                crate::hal::ack_irq(32);
                crate::invariants::timer_irq_exit();
                crate::invariants::irq_exit_clear();
                return current_rsp;
            }

            // Reset the NEXT thread's time slice
            unsafe {
                let nt = &mut *next;
                let idx = (nt.priority as usize).min(crate::scheduler::PRIORITY_COUNT as usize - 1);
                nt.time_slice_remaining = if nt.tid == crate::scheduler::IDLE_TID {
                    crate::scheduler::IDLE_TIME_SLICE
                } else {
                    crate::scheduler::TIME_SLICES[idx]
                };
                nt.ticks_since_scheduled = 0;
            }

            // Switch TSS.RSP0 to the new thread's kernel stack
            let next_ks_top = unsafe { (*next).kernel_stack_top };
            if next_ks_top == 0 {
                panic!("timer IRQ: next TID={} has kernel_stack_top=0 (triple fault on Ring 3 entry)",
                    unsafe { (*next).tid });
            }
            unsafe {
                prepare_timer_return(next);
            }

            // Update per-CPU current thread and PID
            unsafe {
                crate::arch::x64::cpu_local::this_cpu_set_current_thread(next);
                crate::arch::x64::cpu_local::this_cpu_set_current_pid((*next).pid);
                crate::arch::x64::cpu_local::this_cpu_inc_context_switch_count();
            }

            let next_rsp = unsafe { (*next).rsp };
            td_push(TimerDiagEntry {
                tick: current_tick, cur_tid: tid, cur_rsp: current_rsp,
                cur_cs: interrupted_cs,
                flags: 0x01, path: 0,
                next_tid: unsafe { (*next).tid }, next_rsp,
                next_ks_top: unsafe { (*next).kernel_stack_top },
                returned_rsp: next_rsp,
                frame_rip: 0, frame_cs: 0, frame_rflags: 0, phase: 2,
            });
            crate::hal::ack_irq(32);
            crate::invariants::timer_irq_exit();
            crate::invariants::irq_exit_clear();

            let _ = crate::eventbus::EVENT_BUS.push_event(
                crate::eventbus::EVENT_TIMER_TICK,
                crate::eventbus::SOURCE_HAL,
                1,
                current_tick,
                0,
                0,
            );

            crate::trace_cswitch!(tid as u64, unsafe { (*next).tid } as u64);
            return next_rsp;
        }

        // Thread alive and time slice not expired — just set per-CPU NEED_RESCHED
        let alive = scheduler.current_kthread_mut()
            .is_some_and(|k| k.state != ThreadState::Terminated);
        if alive {
            unsafe { crate::arch::x64::cpu_local::this_cpu_set_need_resched(true); }
            crate::hal::ack_irq(32);
            crate::invariants::timer_irq_exit();
            crate::invariants::irq_exit_clear();
            let _ = crate::eventbus::EVENT_BUS.push_event(
                crate::eventbus::EVENT_TIMER_TICK,
                crate::eventbus::SOURCE_HAL,
                1,
                current_tick,
                0,
                0,
            );
            return current_rsp;
        }
    } else if tid == crate::scheduler::IDLE_TID {
        // ── Idle thread preemption ──────────────────────
        // Preempt idle (TID 1) if any other thread is ready
        let should_preempt = current_state == Some(ThreadState::Ready);

        crate::trace_timer_irq!(if should_preempt { 2u8 } else { 3u8 },
            tid, interrupted_cs, has_non_idle as u8);

        if should_preempt && has_non_idle {
            kdebug!(crate::log::LogSubsys::Sched, "[SCHED] PREEMPT idle tid={} has_non_idle={}",
                tid, has_non_idle);
            if let Some(k) = scheduler.current_kthread_mut() {
                k.rsp = current_rsp;
            }
            let next = scheduler.schedule();
            let next_ks = unsafe { (*next).kernel_stack_top };
            let next_rsp_tmp = unsafe { (*next).rsp };
            td_push(TimerDiagEntry {
                tick: current_tick, cur_tid: tid, cur_rsp: current_rsp,
                cur_cs: interrupted_cs, flags: 0x02, path: 1,
                next_tid: unsafe { (*next).tid }, next_rsp: next_rsp_tmp,
                next_ks_top: next_ks, returned_rsp: 0,
                frame_rip: 0, frame_cs: 0, frame_rflags: 0, phase: 1,
            });
            netd_record_select(next as *const _ as u64, next_rsp_tmp, next_ks);
            unsafe {
                let nt = &mut *next;
                let idx = (nt.priority as usize).min(crate::scheduler::PRIORITY_COUNT as usize - 1);
                nt.time_slice_remaining = crate::scheduler::TIME_SLICES[idx];
                nt.ticks_since_scheduled = 0;
            }
            let next_ks_top = unsafe { (*next).kernel_stack_top };
            if next_ks_top == 0 {
                panic!("timer idle-preempt: next TID={} has kernel_stack_top=0",
                    unsafe { (*next).tid });
            }
            unsafe {
                prepare_timer_return(next);
                crate::arch::x64::cpu_local::this_cpu_set_current_thread(next);
                crate::arch::x64::cpu_local::this_cpu_set_current_pid((*next).pid);
                crate::arch::x64::cpu_local::this_cpu_inc_context_switch_count();
            }
            let next_rsp = unsafe { (*next).rsp };
            td_push(TimerDiagEntry {
                tick: current_tick, cur_tid: tid, cur_rsp: current_rsp,
                cur_cs: interrupted_cs, flags: 0x02, path: 1,
                next_tid: unsafe { (*next).tid }, next_rsp,
                next_ks_top: unsafe { (*next).kernel_stack_top }, returned_rsp: next_rsp,
                frame_rip: 0, frame_cs: 0, frame_rflags: 0, phase: 2,
            });
            crate::hal::ack_irq(32);
            crate::invariants::timer_irq_exit();
            crate::invariants::irq_exit_clear();
            let _ = crate::eventbus::EVENT_BUS.push_event(
                crate::eventbus::EVENT_TIMER_TICK,
                crate::eventbus::SOURCE_HAL,
                1,
                current_tick,
                0,
                0,
            );

            crate::trace_cswitch!(tid as u64, unsafe { (*next).tid } as u64);
            return next_rsp;
        }
    } else {
        // ── Kernel thread (Ring 0, non-idle) preemption ──
        // Kernel threads are NOT preempted on every tick even if their
        // timeslice expired, because they may hold kernel locks.
        // However, if the thread yielded (state=Ready), we DO preempt
        // if another thread can run.
        let should_preempt = current_state == Some(ThreadState::Ready);

        crate::trace_timer_irq!(if should_preempt { 2u8 } else { 0u8 },
            tid, interrupted_cs, has_non_idle as u8);

        if should_preempt && has_non_idle {
            kdebug!(crate::log::LogSubsys::Sched, "[SCHED] PREEMPT kernel tid={} reason=yield_or_expired has_non_idle={}",
                tid, has_non_idle);
            if let Some(k) = scheduler.current_kthread_mut() {
                k.rsp = current_rsp;
            }
            let next = scheduler.schedule();
            let next_tid = unsafe { (*next).tid };
            let next_rsp = unsafe { (*next).rsp };
            let next_ks = unsafe { (*next).kernel_stack_top };
            td_push(TimerDiagEntry {
                tick: current_tick, cur_tid: tid, cur_rsp: current_rsp,
                cur_cs: interrupted_cs, flags: 0x04, path: 2,
                next_tid, next_rsp, next_ks_top: next_ks, returned_rsp: 0,
                frame_rip: 0, frame_cs: 0, frame_rflags: 0, phase: 1,
            });
            netd_record_select(next as *const _ as u64, next_rsp, next_ks);
            if next_rsp == 0 {
                if next_tid == crate::scheduler::IDLE_TID {
                    crate::hal::ack_irq(32);
                    crate::invariants::timer_irq_exit();
                    crate::invariants::irq_exit_clear();
                    return current_rsp;
                }
                panic!("timer_handler: kernel preempt next TID={} has rsp=0", next_tid);
            }
            if next_tid == tid {
                // Same thread: no context switch needed
                if let Some(k) = scheduler.current_kthread_mut() {
                    k.state = ThreadState::Running;
                    let idx = (k.priority as usize).min(crate::scheduler::PRIORITY_COUNT as usize - 1);
                    k.time_slice_remaining = crate::scheduler::TIME_SLICES[idx];
                    k.ticks_since_scheduled = 0;
                }
                crate::hal::ack_irq(32);
                crate::invariants::timer_irq_exit();
                crate::invariants::irq_exit_clear();
                return current_rsp;
            }
            unsafe {
                let nt = &mut *next;
                let idx = (nt.priority as usize).min(crate::scheduler::PRIORITY_COUNT as usize - 1);
                nt.time_slice_remaining = crate::scheduler::TIME_SLICES[idx];
                nt.ticks_since_scheduled = 0;
            }
            let next_ks_top = unsafe { (*next).kernel_stack_top };
            if next_ks_top == 0 {
                panic!("timer kernel-preempt: next TID={} has kernel_stack_top=0",
                    unsafe { (*next).tid });
            }
            unsafe {
                crate::arch::x64::gdt::prepare_ring3_return(
                    next_ks_top, (*next).tid, (*next).pid);
                crate::arch::x64::cpu_local::this_cpu_set_current_thread(next);
                crate::arch::x64::cpu_local::this_cpu_set_current_pid((*next).pid);
                crate::arch::x64::cpu_local::this_cpu_inc_context_switch_count();
            }
            let next_rsp = unsafe { (*next).rsp };
            td_push(TimerDiagEntry {
                tick: current_tick, cur_tid: tid, cur_rsp: current_rsp,
                cur_cs: interrupted_cs, flags: 0x04, path: 2,
                next_tid: unsafe { (*next).tid }, next_rsp,
                next_ks_top: unsafe { (*next).kernel_stack_top }, returned_rsp: next_rsp,
                frame_rip: 0, frame_cs: 0, frame_rflags: 0, phase: 2,
            });
            crate::hal::ack_irq(32);
            crate::invariants::timer_irq_exit();
            crate::invariants::irq_exit_clear();
            let _ = crate::eventbus::EVENT_BUS.push_event(
                crate::eventbus::EVENT_TIMER_TICK,
                crate::eventbus::SOURCE_HAL,
                1,
                current_tick,
                0,
                0,
            );
            crate::trace_cswitch!(tid as u64, unsafe { (*next).tid } as u64);
            return next_rsp;
        }
    }

    // ── Kernel mode interrupt (no preemption) ──
    // Per Source of Truth §6.2: Ring 0 code runs to completion.
    // If on_timer_tick() set state to Ready (timeslice expired),
    // restore it to Running because no context switch occurred.
    if tid > 0 {
        let alive = scheduler.current_kthread_mut()
            .is_some_and(|k| k.state != ThreadState::Terminated);
        if alive {
            if let Some(k) = scheduler.current_kthread_mut() {
                if k.state == ThreadState::Ready && k.tid == tid {
                    crate::scheduler::Scheduler::remove_from_run_queue(k);
                    k.state = ThreadState::Running;
                }
            }
            unsafe { crate::arch::x64::cpu_local::this_cpu_set_need_resched(true); }
            crate::hal::ack_irq(32);
            crate::invariants::timer_irq_exit();
            crate::invariants::irq_exit_clear();
            return current_rsp;
        }
        unsafe { crate::arch::x64::cpu_local::this_cpu_set_need_resched(true); }
    }

    // ── Idle thread (TID 0/1) — no timeslice to manage ──
    if has_non_idle {
        unsafe { crate::arch::x64::cpu_local::this_cpu_set_need_resched(true); }
    }
    crate::hal::ack_irq(32);
    crate::invariants::timer_irq_exit();
    crate::invariants::irq_exit_clear();

    // Push TimerTick event (lock‑free, IRQ‑safe)
    let _ = crate::eventbus::EVENT_BUS.push_event(
        crate::eventbus::EVENT_TIMER_TICK,
        crate::eventbus::SOURCE_HAL,
        1,
        current_tick,
        0,
        0,
    );

    // A2.5: DPC dispatch — process deferred procedures at DISPATCH_LEVEL
    // after device IRQ handling. This is the DIRQL→DISPATCH transition point.
    crate::dpc::dpc_dispatch_pending();

    current_rsp
}

/// IPI handler for per-CPU reschedule (vector 0xF0).
/// Called when a remote CPU wakes a thread on this CPU.
extern "x86-interrupt" fn ipi_reschedule_handler(_: InterruptStackFrame) {
    unsafe {
        crate::arch::x64::cpu_local::this_cpu_set_need_resched(true);
    }
    crate::hal::ack_irq(crate::arch::x64::ipi::IPI_RESCHEDULE);
}

/// IPI handler for TLB shootdown (vector 0xF1).
/// Invalidates TLB entries for a virtual address range and sends ACK.
extern "x86-interrupt" fn ipi_tlb_shootdown_handler(_: InterruptStackFrame) {
    crate::arch::x64::ipi::ipi_tlb_shootdown_handler_impl();
    crate::hal::ack_irq(crate::arch::x64::ipi::IPI_TLB_SHOOTDOWN);
}

/// IPI handler for cross-CPU function call (vector 0xF2).
/// Executes a registered function on the receiving CPU and sends ACK.
extern "x86-interrupt" fn ipi_call_function_handler(_: InterruptStackFrame) {
    crate::arch::x64::ipi::ipi_call_function_handler_impl();
    crate::hal::ack_irq(crate::arch::x64::ipi::IPI_CALL_FUNCTION);
}

extern "x86-interrupt" fn keyboard_handler(_: InterruptStackFrame) {
    // Read scancode directly from PS/2 controller
    let status: u8 = crate::hal::inb(0x64);
    let scancode = if (status & 0x01) != 0 {
        Some(crate::hal::inb(0x60))
    } else {
        None
    };

    if let Some(scancode) = scancode {
        // FIX v2: single-path direct — process synchronously in IRQ.
        // Previous dual-path (direct + queued) caused double-char.
        // Queued-only (push_event) left keyboard dead because dispatch_pending
        // is not guaranteed to run before the blocked READB waiter is checked
        // (idle dispatch delayed by IDLE_TIME_SLICE / scheduler state).
        // Direct path does push_byte+wake_blocked_readers immediately, matching
        // pre-P0 working behavior. Keep one log for forensics.
        let seq = crate::kbd::event::kbd_event_handler_direct(scancode);
        crate::serial_println!("[KBD_IRQ] seq={} scancode=0x{:02x} direct-only", seq, scancode);
    }
    crate::hal::ack_irq(33);
}

extern "x86-interrupt" fn serial_handler(_: InterruptStackFrame) {
    while crate::hal::inb(0x3FD) & 1 != 0 {
        let byte = crate::hal::inb(0x3F8);
        let _ = crate::eventbus::EVENT_BUS.push_event(
            crate::eventbus::EVENT_SERIAL_DATA,
            crate::eventbus::SOURCE_HAL,
            2,
            byte as u64,
            0,
            0,
        );
    }
    crate::hal::ack_irq(36);
}

extern "x86-interrupt" fn mouse_handler(_: InterruptStackFrame) {
    let status: u8 = crate::hal::inb(0x64);
    if (status & 0x01) != 0 {
        let byte = crate::hal::inb(0x60);
        let _ = crate::eventbus::EVENT_BUS.push_event(
            crate::eventbus::EVENT_MOUSE_INPUT,
            crate::eventbus::SOURCE_HAL,
            4,
            byte as u64,
            0,
            0,
        );
    }
    crate::hal::ack_irq(44);
}

pub fn init() {
    IDT.load();
}

// ─────────────────────────────────────────────────────────────────────────────
// Dynamic MSI handler registration
// ─────────────────────────────────────────────────────────────────────────────
//
// The IDT is a lazy_static — its entries cannot be changed after it is loaded.
// To support dynamic MSI vector allocation without rebuilding the IDT, we use
// a secondary dispatch table: every MSI-capable vector (48..=255) points to
// the same `msi_generic_handler` entry in the IDT, which then calls the
// per-vector function stored in MSI_HANDLER_TABLE.

/// Maximum number of IDT entries (vectors 0-255).
const IDT_SIZE: usize = 256;
/// First vector available for MSI allocation (after legacy IRQ remaps 32-47).
const MSI_VECTOR_BASE: usize = 48;

type MsiHandlerFn = fn(vector: u8);

/// Per-vector handler table.  Index == vector number.  `None` means the
/// vector is not yet claimed.
static MSI_HANDLER_TABLE: spin::Mutex<[Option<MsiHandlerFn>; IDT_SIZE]> =
    spin::Mutex::new([None; IDT_SIZE]);

/// Register a handler function for an already-allocated MSI vector.
/// Panics if the vector is out of the MSI range or already registered.
pub fn msi_register_handler(vector: u8, handler: MsiHandlerFn) {
    assert!(
        (vector as usize) >= MSI_VECTOR_BASE,
        "msi_register_handler: vector {} is in the legacy range",
        vector
    );
    let mut table = MSI_HANDLER_TABLE.lock();
    assert!(
        table[vector as usize].is_none(),
        "msi_register_handler: vector {} already has a handler",
        vector
    );
    table[vector as usize] = Some(handler);
}

/// Unregister the handler for an MSI vector (call before freeing the vector).
pub fn msi_unregister_handler(vector: u8) {
    if (vector as usize) < MSI_VECTOR_BASE {
        return;
    }
    MSI_HANDLER_TABLE.lock()[vector as usize] = None;
}

/// Generic MSI dispatch — called from the IDT stub for every MSI vector.
/// It looks up the per-vector handler in MSI_HANDLER_TABLE and calls it.
/// If no handler is registered the spurious interrupt is silently discarded.
#[no_mangle]
pub extern "C" fn msi_dispatch(vector: u8) {
    let handler = {
        let table = MSI_HANDLER_TABLE.lock();
        table[vector as usize]
    };
    if let Some(f) = handler {
        f(vector);
    } else {
        ktrace!(crate::log::LogSubsys::Interrupts, "Spurious interrupt on vector {}", vector);
    }
    // Send EOI via the HAL (no-op if APIC is not configured yet).
    crate::hal::ack_irq(vector);
}
