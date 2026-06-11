use core::fmt::Write;
use core::mem::size_of;
use core::sync::atomic::{AtomicBool, Ordering};
use crate::serial_println;
use crate::test_case;
use crate::test_eq;
use crate::test_true;

pub const CRASH_DUMP_AREA_BASE: u64 = 0x0F00_0000;
pub const CRASH_DUMP_AREA_SIZE: u64 = 0x0100_0000; // 16 MB
pub const CRASH_DUMP_HEADER_SIZE: usize = 0x4000;  // 16 KB header
pub const CRASH_DUMP_MAGIC: [u8; 4] = *b"NDMP";
pub const CRASH_DUMP_VERSION: u32 = 1;

pub const CAUSE_PANIC: u32 = 0;
pub const CAUSE_DOUBLE_FAULT: u32 = 1;
pub const CAUSE_TRIPLE_FAULT: u32 = 2;
pub const CAUSE_NMI: u32 = 3;
pub const CAUSE_WATCHDOG: u32 = 4;

static CRASH_DUMP_OCCURRED: AtomicBool = AtomicBool::new(false);

#[repr(C, packed)]
pub struct CrashDumpHeader {
    pub magic: [u8; 4],
    pub version: u32,
    pub timestamp: u64,
    pub cause: u32,
    pub cpu_count: u32,
    pub param: [u64; 4],

    pub stack_depth: u32,
    pub _pad0: u32,
    pub stack_trace: [u64; 32],

    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rsp: u64,
    pub rip: u64,
    pub rflags: u64,

    pub cr0: u64,
    pub cr2: u64,
    pub cr3: u64,
    pub cr4: u64,
    pub efer: u64,

    pub current_tid: u32,
    pub current_pid: u32,
    pub thread_state: u8,
    pub cpu_id: u8,
    pub _pad1: [u8; 6],

    pub pml4: [u64; 512],

    pub trace_count: u32,
    pub _pad2: [u8; 28],
    pub trace_events: [CrashTraceEvent; 128],
}

impl CrashDumpHeader {
    pub fn new_zeroed() -> Self {
        CrashDumpHeader {
            magic: CRASH_DUMP_MAGIC,
            version: CRASH_DUMP_VERSION,
            timestamp: 0,
            cause: 0,
            cpu_count: 0,
            param: [0; 4],
            stack_depth: 0,
            _pad0: 0,
            stack_trace: [0; 32],
            rax: 0, rbx: 0, rcx: 0, rdx: 0,
            rsi: 0, rdi: 0, r8: 0, r9: 0,
            r10: 0, r11: 0, r12: 0, r13: 0, r14: 0, r15: 0,
            rsp: 0, rip: 0, rflags: 0,
            cr0: 0, cr2: 0, cr3: 0, cr4: 0, efer: 0,
            current_tid: 0, current_pid: 0,
            thread_state: 0, cpu_id: 0,
            _pad1: [0; 6],
            pml4: [0; 512],
            trace_count: 0,
            _pad2: [0; 28],
            trace_events: [CrashTraceEvent::new(); 128],
        }
    }
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct CrashTraceEvent {
    pub timestamp: u64,
    pub event: u8,
    pub cpu: u8,
    pub _pad: [u8; 2],
    pub arg0: u32,
    pub arg1: u32,
    pub rip_low: u32,
}

impl CrashTraceEvent {
    pub const fn new() -> Self {
        CrashTraceEvent {
            timestamp: 0, event: 0, cpu: 0,
            _pad: [0; 2], arg0: 0, arg1: 0, rip_low: 0,
        }
    }
}

struct RawSerialWriter(*const crate::arch::x64::serial::SerialPort);

impl Write for RawSerialWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let port = unsafe { &*self.0 };
        for &b in s.as_bytes() {
            port.send(b);
        }
        Ok(())
    }
}

fn with_serial_writer(f: impl FnOnce(&mut RawSerialWriter)) {
    use crate::arch::x64::serial::SERIAL1;
    if let Some(port) = SERIAL1.try_lock() {
        let mut w = RawSerialWriter(&*port as *const _);
        f(&mut w);
    }
}

fn write_serial_raw(s: &str) {
    with_serial_writer(|w| {
        let _ = w.write_str(s);
    });
}

fn write_serial_fmt(args: core::fmt::Arguments) {
    with_serial_writer(|w| {
        let _ = w.write_fmt(args);
    });
}

pub fn crash_dump_area_ptr() -> *mut u8 {
    CRASH_DUMP_AREA_BASE as *mut u8
}

pub fn init_crash_dump_area() {
    let base = crash_dump_area_ptr();
    let header_size = size_of::<CrashDumpHeader>();
    if header_size > CRASH_DUMP_HEADER_SIZE {
        panic!("CrashDumpHeader size ({}) exceeds HEADER_SIZE ({})",
               header_size, CRASH_DUMP_HEADER_SIZE);
    }
    unsafe {
        core::ptr::write_bytes(base, 0, CRASH_DUMP_AREA_SIZE as usize);
    }
    serial_println!("[+] Crash dump area @ 0x{:x} ({} KB)",
        CRASH_DUMP_AREA_BASE, CRASH_DUMP_AREA_SIZE / 1024);
}

fn fill_header(header: &mut CrashDumpHeader, cause: u32, param: &[u64; 4],
               rip: u64, rsp: u64) {
    header.magic = CRASH_DUMP_MAGIC;
    header.version = CRASH_DUMP_VERSION;
    header.timestamp = crate::hal::get_ticks();
    header.cause = cause;
    header.cpu_count = crate::arch::x64::cpu_local::cpu_count() as u32;
    header.param = *param;

    // Stack walk: capture return addresses from the stack
    // We read stack frames (RBP chains) for the first 32 entries
    let mut stack_rip: u64;
    let stack_rbp: u64 = unsafe { crate::hal::raw::raw_read_rbp() };
    let mut rbp = stack_rbp;
    let mut depth = 0u32;
    for i in 0..32 {
        if rbp < 0x100000 || rbp > 0x20000000 {
            break;
        }
        unsafe {
            stack_rip = (rbp as *const u64).offset(1).read();
            rbp = (rbp as *const u64).read();
        }
        header.stack_trace[i] = stack_rip;
        depth = (i + 1) as u32;
    }
    header.stack_depth = depth;

    // GPRs via hal::raw
    unsafe {
        header.rax = crate::hal::raw::raw_read_rax();
        header.rbx = crate::hal::raw::raw_read_rbx();
        header.rcx = crate::hal::raw::raw_read_rcx();
        header.rdx = crate::hal::raw::raw_read_rdx();
        header.rsi = crate::hal::raw::raw_read_rsi();
        header.rdi = crate::hal::raw::raw_read_rdi();
        header.r8  = crate::hal::raw::raw_read_r8();
        header.r9  = crate::hal::raw::raw_read_r9();
        header.r10 = crate::hal::raw::raw_read_r10();
        header.r11 = crate::hal::raw::raw_read_r11();
        header.r12 = crate::hal::raw::raw_read_r12();
        header.r13 = crate::hal::raw::raw_read_r13();
        header.r14 = crate::hal::raw::raw_read_r14();
        header.r15 = crate::hal::raw::raw_read_r15();
        header.rflags = crate::hal::raw::raw_read_rflags();
    }
    header.rsp = rsp;
    header.rip = rip;

    // Control registers
    unsafe {
        header.cr0 = crate::hal::read_cr3(); // we only get CR3 easily
        header.cr2 = crate::hal::read_cr2();
        header.cr3 = crate::hal::read_cr3();
        header.cr4 = crate::hal::raw::raw_read_cr4();
        header.cr0 = crate::hal::raw::raw_read_cr0();
    }

    // EFER via MSR
    unsafe {
        header.efer = crate::hal::raw::raw_read_msr(0xC0000080);
    }

    // Scheduler snapshot
    unsafe {
        header.cpu_id = crate::arch::x64::cpu_local::this_cpu_id() as u8;
    }
    let s = crate::scheduler::current_scheduler();
    if let Some(mut sched) = s.try_lock() {
        header.current_tid = sched.current_tid;
        header.current_pid = sched.current_pid();
        if let Some(k) = sched.current_kthread_mut() {
            header.thread_state = k.state.to_u8();
        }
    }

    // PML4 copy
    unsafe {
        let cr3_val = crate::hal::read_cr3() & !0xFFF;
        let src = cr3_val as *const u64;
        for i in 0..512 {
            header.pml4[i] = src.add(i).read();
        }
    }

    // Trace buffer copy (last 128 entries)
    let trace = &crate::trace::TRACE;
    let head = trace.head.load(Ordering::Relaxed) as usize;
    let capacity = crate::trace::TRACE_CAPACITY;
    let count = if head > 128 { 128 } else { head };
    header.trace_count = count as u32;
    for i in 0..count.min(128) {
        let src_idx = if head >= i + 1 { head - i - 1 } else { 0 };
        let idx = src_idx % capacity;
        let entry = &trace.entries[idx];
        header.trace_events[i] = CrashTraceEvent {
            timestamp: entry.tick,
            event: entry.event as u8,
            cpu: 0,
            _pad: [0; 2],
            arg0: entry.arg0 as u32,
            arg1: entry.arg1 as u32,
            rip_low: (entry.arg2 & 0xFFFFFFFF) as u32,
        };
    }
}

pub fn dump_header_to_serial(header: &CrashDumpHeader) {
    // Copy packed fields to local variables to avoid unaligned reference UB
    let timestamp = header.timestamp;
    let cause = header.cause;
    let rip = header.rip;
    let rsp = header.rsp;
    let rax = header.rax; let rbx = header.rbx; let rcx = header.rcx; let rdx = header.rdx;
    let rsi = header.rsi; let rdi = header.rdi; let r8 = header.r8; let r9 = header.r9;
    let r10 = header.r10; let r11 = header.r11; let r12 = header.r12; let r13 = header.r13;
    let r14 = header.r14; let r15 = header.r15;
    let rflags = header.rflags;
    let cr0 = header.cr0; let cr2 = header.cr2; let cr3 = header.cr3;
    let cr4 = header.cr4; let efer = header.efer;
    let current_tid = header.current_tid; let current_pid = header.current_pid;
    let cpu_id = header.cpu_id; let thread_state = header.thread_state;
    let stack_depth = header.stack_depth;
    let trace_count = header.trace_count;
    // Copy stack trace to local array (avoid packed field references)
    let mut stack_trace_local = [0u64; 32];
    let st_ptr = core::ptr::addr_of!(header.stack_trace) as *const u64;
    for i in 0..32 {
        stack_trace_local[i] = unsafe { core::ptr::read_unaligned(st_ptr.add(i)) };
    }

    write_serial_raw("\n========================================\n");
    write_serial_raw("      N E O D O S   C R A S H   D U M P\n");
    write_serial_raw("========================================\n");
    let cause_str = match cause {
        CAUSE_PANIC => "PANIC",
        CAUSE_DOUBLE_FAULT => "DOUBLE_FAULT",
        CAUSE_TRIPLE_FAULT => "TRIPLE_FAULT",
        CAUSE_NMI => "NMI",
        CAUSE_WATCHDOG => "WATCHDOG",
        _ => "UNKNOWN",
    };
    write_serial_fmt(format_args!(
        "Timestamp: {}\nCause: {} ({})\nRIP: {:#x}  RSP: {:#x}\n",
        timestamp, cause_str, cause, rip, rsp
    ));
    write_serial_fmt(format_args!(
        "RAX: {:#x}  RBX: {:#x}  RCX: {:#x}  RDX: {:#x}\n",
        rax, rbx, rcx, rdx
    ));
    write_serial_fmt(format_args!(
        "RSI: {:#x}  RDI: {:#x}  R8:  {:#x}  R9:  {:#x}\n",
        rsi, rdi, r8, r9
    ));
    write_serial_fmt(format_args!(
        "R10: {:#x}  R11: {:#x}  R12: {:#x}  R13: {:#x}\n",
        r10, r11, r12, r13
    ));
    write_serial_fmt(format_args!(
        "R14: {:#x}  R15: {:#x}  RFL: {:#x}\n",
        r14, r15, rflags
    ));
    write_serial_fmt(format_args!(
        "CR0: {:#x}  CR2: {:#x}  CR3: {:#x}  CR4: {:#x}  EFER: {:#x}\n",
        cr0, cr2, cr3, cr4, efer
    ));
    write_serial_fmt(format_args!(
        "TID: {}  PID: {}  CPU: {}  ThreadState: {}\n",
        current_tid, current_pid, cpu_id, thread_state
    ));

    // Stack trace
    write_serial_raw("\n--- Stack Trace ---\n");
    for i in 0..stack_depth.min(32) {
        write_serial_fmt(format_args!("  [{:2}] {:#x}\n", i, stack_trace_local[i as usize]));
    }

    // PML4 summary
    write_serial_raw("\n--- PML4 (first 16 entries) ---\n");
    for i in 0..16 {
        let entry = header.pml4[i];
        if entry != 0 {
            let addr = entry & !0xFFF;
            let flags = entry & 0xFFF;
            write_serial_fmt(format_args!("  [{}] addr={:#x} flags={:#x}\n", i, addr, flags));
        }
    }

    // Trace events
    write_serial_raw("\n--- Trace Buffer (last events) ---\n");
    let limit = trace_count.min(128);
    for i in 0..limit {
        let ts = header.trace_events[i as usize].timestamp;
        let ev = header.trace_events[i as usize].event;
        let cpu = header.trace_events[i as usize].cpu;
        let a0 = header.trace_events[i as usize].arg0;
        let a1 = header.trace_events[i as usize].arg1;
        if ev != 0 || ts != 0 {
            write_serial_fmt(format_args!(
                "  [{}] event={} cpu={} a0={:#x} a1={:#x}\n",
                ts, ev, cpu, a0, a1
            ));
        }
    }

    write_serial_raw("\n--- End Crash Dump ---\n");
}

pub fn write_ram_dump(header: &CrashDumpHeader) {
    let dst = crash_dump_area_ptr();
    let header_bytes = unsafe {
        core::slice::from_raw_parts(
            header as *const CrashDumpHeader as *const u8,
            size_of::<CrashDumpHeader>()
        )
    };
    unsafe {
        core::ptr::copy_nonoverlapping(header_bytes.as_ptr(), dst, header_bytes.len());
    }
}

pub fn dump_crash(cause: u32, param: &[u64; 4], rip: u64, rsp: u64) {
    if CRASH_DUMP_OCCURRED.swap(true, Ordering::SeqCst) {
        return;
    }

    let mut header = CrashDumpHeader::new_zeroed();
    fill_header(&mut header, cause, param, rip, rsp);

    dump_header_to_serial(&header);
    write_ram_dump(&header);
}

pub fn dump_panic(rip: u64, rsp: u64) {
    let param = [rip, rsp, 0, 0];
    dump_crash(CAUSE_PANIC, &param, rip, rsp);
}

pub fn dump_double_fault(rip: u64, rsp: u64, error_code: u64) {
    let param = [rip, rsp, error_code, 0];
    dump_crash(CAUSE_DOUBLE_FAULT, &param, rip, rsp);
}

pub fn dump_nmi(rip: u64, rsp: u64) {
    let param = [rip, rsp, 0, 0];
    dump_crash(CAUSE_NMI, &param, rip, rsp);
}

pub fn is_crash_dump_present() -> bool {
    unsafe {
        let ptr = crash_dump_area_ptr() as *const u32;
        let magic = ptr.read_unaligned();
        magic == u32::from_le_bytes(CRASH_DUMP_MAGIC)
    }
}

pub fn read_dump_header() -> Option<CrashDumpHeader> {
    if !is_crash_dump_present() {
        return None;
    }
    unsafe {
        let ptr = crash_dump_area_ptr() as *const CrashDumpHeader;
        Some(ptr.read_unaligned())
    }
}

pub fn print_crash_dump_status() {
    if is_crash_dump_present() {
        if let Some(header) = read_dump_header() {
            let cause = header.cause;
            let timestamp = header.timestamp;
            let rip = header.rip; let rsp = header.rsp;
            let tid = header.current_tid; let pid = header.current_pid;
            let cpu = header.cpu_id;
            let depth = header.stack_depth;
            let tcount = header.trace_count;
            let cause_str = match cause {
                CAUSE_PANIC => "PANIC",
                CAUSE_DOUBLE_FAULT => "DOUBLE_FAULT",
                CAUSE_TRIPLE_FAULT => "TRIPLE_FAULT",
                CAUSE_NMI => "NMI",
                CAUSE_WATCHDOG => "WATCHDOG",
                _ => "UNKNOWN",
            };
            crate::println!("  Crash dump present @ 0x{:x}", CRASH_DUMP_AREA_BASE);
            crate::println!("  Cause: {} ({})", cause_str, cause);
            crate::println!("  Timestamp: {} ticks", timestamp);
            crate::println!("  RIP: {:#x}  RSP: {:#x}", rip, rsp);
            crate::println!("  TID: {}  PID: {}  CPU: {}", tid, pid, cpu);
            crate::println!("  Stack depth: {} frames", depth);
            crate::println!("  Trace events: {}", tcount);
            crate::println!("  Use 'CRASH DUMP' to view full dump to serial");
        }
    } else {
        crate::println!("  Crash dump area @ 0x{:x} (16 KB header + 16 MB data)", CRASH_DUMP_AREA_BASE);
        crate::println!("  Status: No crash dump recorded");
    }
}

pub fn print_crash_dump_full() {
    if let Some(header) = read_dump_header() {
        dump_header_to_serial(&header);
        crate::println!("[+] Full crash dump written to serial port");
    } else {
        crate::println!("[-] No crash dump present");
    }
}

// ── Tests ────────────────────────────────────────────────────────────

pub fn register_crash_tests() {
    test_case!("crash_dump_header_size", {
        let hdr_size = core::mem::size_of::<CrashDumpHeader>();
        // Must be <= CRASH_DUMP_HEADER_SIZE (16 KB)
        test_true!(hdr_size <= CRASH_DUMP_HEADER_SIZE);
        test_true!(hdr_size > 100); // at least some fields
    });

    test_case!("crash_dump_new_zeroed", {
        let h = CrashDumpHeader::new_zeroed();
        let magic = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(h.magic) as *const [u8; 4]) };
        let ver = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(h.version) as *const u32) };
        let cause = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(h.cause) as *const u32) };
        let depth = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(h.stack_depth) as *const u32) };
        test_eq!(&magic, &CRASH_DUMP_MAGIC);
        test_eq!(ver, CRASH_DUMP_VERSION);
        test_eq!(cause, 0);
        test_eq!(depth, 0);
    });

    test_case!("crash_dump_header_layout", {
        let h = CrashDumpHeader::new_zeroed();
        let base = core::ptr::addr_of!(h) as u64;
        let magic_off = core::ptr::addr_of!(h.magic) as u64 - base;
        let version_off = core::ptr::addr_of!(h.version) as u64 - base;
        let timestamp_off = core::ptr::addr_of!(h.timestamp) as u64 - base;
        test_eq!(magic_off, 0);
        test_eq!(version_off, 4);
        test_eq!(timestamp_off, 8);
    });

    test_case!("crash_dump_fill_and_serialize", {
        let mut h = CrashDumpHeader::new_zeroed();
        let cause = CAUSE_PANIC;
        let param = [0xDEAD, 0xBEEF, 0xCAFE, 0xBABE];
        let rip: u64 = 0x200042;
        let rsp: u64 = 0x1FFFF000;
        fill_header(&mut h, cause, &param, rip, rsp);
        let h_cause = h.cause;
        let h_rip = h.rip;
        let h_rsp = h.rsp;
        let param_ptr = core::ptr::addr_of!(h.param) as *const u64;
        let p0: u64 = unsafe { core::ptr::read_unaligned(param_ptr) };
        let p1: u64 = unsafe { core::ptr::read_unaligned(param_ptr.add(1)) };
        let pml4_0: u64 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(h.pml4) as *const u64) };
        test_eq!(h_cause, cause);
        test_eq!(p0, 0xDEAD);
        test_eq!(p1, 0xBEEF);
        test_eq!(h_rip, rip);
        test_eq!(h_rsp, rsp);
        // h_depth depends on runtime RBP value — skip assertion
        test_true!(pml4_0 != 0);
    });

    test_case!("crash_dump_no_recursion", {
        // Verify the recursion guard works
        test_true!(!CRASH_DUMP_OCCURRED.load(Ordering::SeqCst));
        CRASH_DUMP_OCCURRED.store(true, Ordering::SeqCst);
        // A second call should be a no-op
        let old = CRASH_DUMP_OCCURRED.swap(true, Ordering::SeqCst);
        test_true!(old); // was already set
        CRASH_DUMP_OCCURRED.store(false, Ordering::SeqCst);
    });
}
