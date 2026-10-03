//! Netd pointer/stack/frame hexdump and FINAL_IRETQ capture.

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
