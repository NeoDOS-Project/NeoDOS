use std::{env, fs, process};

const HEADER_SIZE: usize = 0x4000;
const MAGIC: &[u8; 4] = b"NDMP";

const CAUSE_NAMES: &[&str] = &["PANIC", "DOUBLE_FAULT", "TRIPLE_FAULT", "NMI", "WATCHDOG"];

const TRACE_EVENT_NAMES: &[(u8, &str)] = &[
    (0x01, "ContextSwitch"),
    (0x02, "SyscallEnter"),
    (0x03, "SyscallExit"),
    (0x04, "IrqEnter"),
    (0x05, "IrqExit"),
    (0x06, "SchedDecision"),
    (0x07, "IrqTimerTick"),
    (0xFF, "Panic"),
];

fn trace_event_name(code: u8) -> &'static str {
    for &(c, name) in TRACE_EVENT_NAMES {
        if c == code {
            return name;
        }
    }
    "UNKNOWN"
}

fn cause_name(cause: u32) -> String {
    if (cause as usize) < CAUSE_NAMES.len() {
        format!("{} ({})", CAUSE_NAMES[cause as usize], cause)
    } else {
        format!("UNKNOWN({})", cause)
    }
}

fn read_u32_le(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn read_u64_le(data: &[u8], off: usize) -> u64 {
    u64::from_le_bytes([
        data[off], data[off + 1], data[off + 2], data[off + 3],
        data[off + 4], data[off + 5], data[off + 6], data[off + 7],
    ])
}

struct CrashDump {
    version: u32,
    timestamp: u64,
    cause: u32,
    cpu_count: u32,
    param: [u64; 4],
    stack_depth: u32,
    stack_trace: [u64; 32],
    gprs: [u64; 17],
    cr: [u64; 5],
    current_tid: u32,
    current_pid: u32,
    thread_state: u8,
    cpu_id: u8,
    pml4: [u64; 512],
    _trace_count: u32,
    trace_events: Vec<TraceEvent>,
}

#[derive(Debug)]
struct TraceEvent {
    timestamp: u64,
    event: u8,
    cpu: u8,
    arg0: u32,
    arg1: u32,
    rip_low: u32,
}

impl CrashDump {
    fn new(data: &[u8]) -> Result<Self, String> {
        if data.len() < HEADER_SIZE {
            return Err(format!("Data too small: {} < {}", data.len(), HEADER_SIZE));
        }
        if &data[0..4] != MAGIC {
            return Err(format!("Bad magic: {:?} != NDMP", &data[0..4]));
        }

        let mut off = 0;
        let _magic = &data[off..off + 4];
        off += 4;
        let version = read_u32_le(data, off);
        off += 4;
        let timestamp = read_u64_le(data, off);
        off += 8;
        let cause = read_u32_le(data, off);
        off += 4;
        let cpu_count = read_u32_le(data, off);
        off += 4;
        let mut param = [0u64; 4];
        for p in &mut param {
            *p = read_u64_le(data, off);
            off += 8;
        }
        let stack_depth = read_u32_le(data, off);
        off += 4; // skip pad
        off += 4;
        let mut stack_trace = [0u64; 32];
        for frame in &mut stack_trace {
            *frame = read_u64_le(data, off);
            off += 8;
        }

        let reg_names = ["rax", "rbx", "rcx", "rdx", "rsi", "rdi",
                         "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15",
                         "rsp", "rip", "rflags"];
        let mut gprs = [0u64; 17];
        for g in &mut gprs {
            *g = read_u64_le(data, off);
            off += 8;
        }

        let mut cr = [0u64; 5];
        for c in &mut cr {
            *c = read_u64_le(data, off);
            off += 8;
        }

        let current_tid = read_u32_le(data, off);
        off += 4;
        let current_pid = read_u32_le(data, off);
        off += 4;
        let thread_state = data[off];
        off += 1;
        let cpu_id = data[off];
        off += 1;
        off += 6; // pad

        let mut pml4 = [0u64; 512];
        for p in &mut pml4 {
            *p = read_u64_le(data, off);
            off += 8;
        }

        let _trace_count = read_u32_le(data, off);
        off += 32; // +28 pad

        let mut trace_events = Vec::new();
        for _ in 0..128 {
            if off + 24 > data.len() {
                break;
            }
            let ts = read_u64_le(data, off);
            let event = data[off + 8];
            let cpu = data[off + 9];
            let arg0 = read_u32_le(data, off + 12);
            let arg1 = read_u32_le(data, off + 16);
            let rip_low = read_u32_le(data, off + 20);
            trace_events.push(TraceEvent { timestamp: ts, event, cpu, arg0, arg1, rip_low });
            off += 24;
        }

        Ok(Self {
            version, timestamp, cause, cpu_count, param, stack_depth, stack_trace,
            gprs, cr, current_tid, current_pid, thread_state, cpu_id,
            pml4, trace_count, trace_events,
        })
    }

    fn print_summary(&self) {
        println!("{}", "=".repeat(60));
        println!("    N e o D O S   C r a s h   D u m p   A n a l y z e r");
        println!("{}", "=".repeat(60));
        println!("  Magic:          NDMP");
        println!("  Version:        {}", self.version);
        println!("  Timestamp:      {} ticks", self.timestamp);
        println!("  Cause:          {}", cause_name(self.cause));
        println!("  CPU count:      {}", self.cpu_count);
        print!("  Param:          ");
        for (i, p) in self.param.iter().enumerate() {
            if i > 0 { print!(", "); }
            print!("{:#x}", p);
        }
        println!();
        println!();

        println!("  --- Registers ---");
        let reg_names = ["rax", "rbx", "rcx", "rdx", "rsi", "rdi",
                         "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15",
                         "rsp", "rip", "rflags"];
        for (i, name) in reg_names.iter().enumerate() {
            println!("    {:4} = {:#018x}", name.to_uppercase(), self.gprs[i]);
        }

        println!();
        println!("  --- Control Registers ---");
        let cr_names = ["cr0", "cr2", "cr3", "cr4", "efer"];
        for (i, name) in cr_names.iter().enumerate() {
            println!("    {:4} = {:#018x}", name.to_uppercase(), self.cr[i]);
        }

        println!();
        println!("  --- Scheduler ---");
        println!("    TID   = {}", self.current_tid);
        println!("    PID   = {}", self.current_pid);
        println!("    CPU   = {}", self.cpu_id);
        println!("    State = {}", self.thread_state);

        println!();
        println!("  --- Stack Trace ({:2} frames) ---", self.stack_depth);
        for i in 0..self.stack_depth.min(32) as usize {
            println!("    [{:2}] {:#018x}", i, self.stack_trace[i]);
        }

        println!();
        println!("  --- PML4 (first 16 entries) ---");
        for i in 0..16 {
            let entry = self.pml4[i];
            if entry != 0 {
                let addr = entry & !0xFFF;
                let flags = entry & 0xFFF;
                let mut flag_str = String::new();
                if flags & 0x001 != 0 { flag_str.push('P'); }
                if flags & 0x002 != 0 { flag_str.push('W'); }
                if flags & 0x004 != 0 { flag_str.push('U'); }
                if flags & 0x080 != 0 { flag_str.push('G'); }
                println!("    [{:2}] addr={:#010x} flags={:#06x} ({})", i, addr, flags, flag_str);
            }
        }

        println!();
        println!("  --- Trace Buffer ({:3} events) ---", self.trace_events.len());
        let mut count = 0;
        for ev in &self.trace_events {
            if ev.timestamp != 0 {
                println!("    [{:6}] {:15} cpu={} a0={:#010x} a1={:#010x} rip_low={:#010x}",
                    ev.timestamp, trace_event_name(ev.event), ev.cpu,
                    ev.arg0, ev.arg1, ev.rip_low);
                count += 1;
                if count >= 32 {
                    println!("    ... (truncated to 32 events)");
                    break;
                }
            }
        }

        println!("{}", "=".repeat(60));
    }
}

fn find_nearest(sym_map: &[(u64, String)], addr: u64) -> Option<String> {
    let mut best: Option<(u64, &str)> = None;
    for &(sa, ref name) in sym_map {
        if sa <= addr {
            best = Some((sa, name));
        } else {
            break;
        }
    }
    best.map(|(sa, name)| {
        let offset = addr - sa;
        if offset == 0 {
            name.to_string()
        } else {
            format!("{}+{:#x}", name, offset)
        }
    })
}

fn print_usage() {
    eprintln!("Usage: crashdump [--file dump.bin] [--symbols kernel.elf]");
    eprintln!("  Reads from stdin if --file not provided.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut file_path: Option<String> = None;
    let mut symbols_path: Option<String> = None;
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => { print_usage(); return; }
            "--file" | "-f" => { i += 1; file_path = Some(args[i].clone()); }
            "--symbols" | "-s" => { i += 1; symbols_path = Some(args[i].clone()); }
            _ => { eprintln!("Unknown option: {}", args[i]); print_usage(); process::exit(1); }
        }
        i += 1;
    }

    let data: Vec<u8> = if let Some(path) = file_path {
        fs::read(&path).unwrap_or_else(|e| {
            eprintln!("Error reading {}: {}", path, e);
            process::exit(1);
        })
    } else {
        use std::io::Read;
        let mut buf = Vec::new();
        std::io::stdin().read_to_end(&mut buf).unwrap_or_else(|e| {
            eprintln!("Error reading stdin: {}", e);
            process::exit(1);
        });
        buf
    };

    if data.len() < HEADER_SIZE {
        eprintln!("Error: need at least {} bytes, got {}", HEADER_SIZE, data.len());
        process::exit(1);
    }

    let dump = match CrashDump::new(&data) {
        Ok(d) => d,
        Err(e) => { eprintln!("Error parsing header: {}", e); process::exit(1); }
    };

    dump.print_summary();

    // Symbol resolution
    if let Some(ref sym_path) = symbols_path {
        let mut all_addrs = Vec::new();
        let rip = dump.gprs[15];
        if rip != 0 { all_addrs.push(rip); }
        for i in 0..dump.stack_depth.min(32) as usize {
            all_addrs.push(dump.stack_trace[i]);
        }

        let path = std::path::Path::new(sym_path);
        if path.exists() && sym_path.ends_with(".elf") {
            let output = std::process::Command::new("nm")
                .args(["-n", sym_path])
                .output()
                .ok();

            if let Some(output) = output {
                let mut sym_map: Vec<(u64, String)> = Vec::new();
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    let parts: Vec<&str> = line.splitn(3, ' ').collect();
                    if parts.len() >= 3 {
                        if let Ok(addr) = u64::from_str_radix(parts[0], 16) {
                            let sym_type = parts[1].as_bytes()[0];
                            if sym_type == b'T' || sym_type == b't' || sym_type == b'W' || sym_type == b'w' {
                                sym_map.push((addr, parts[2].to_string()));
                            }
                        }
                    }
                }
                sym_map.sort_by_key(|&(addr, _)| addr);

                if !sym_map.is_empty() {
                    println!();
                    println!("  --- Symbol Resolution ---");
                    if rip != 0 {
                        if let Some(sym) = find_nearest(&sym_map, rip) {
                            println!("    RIP: {}", sym);
                        }
                    }
                    println!("    Stack:");
                    for i in 0..dump.stack_depth.min(32) as usize {
                        let addr = dump.stack_trace[i];
                        if addr != 0 {
                            if let Some(sym) = find_nearest(&sym_map, addr) {
                                println!("      [{:2}] {}", i, sym);
                            }
                        }
                    }
                }
            }
        }
    }
}
