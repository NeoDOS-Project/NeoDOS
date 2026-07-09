//! GDB Remote Protocol Stub — kernel-resident debugger.
//!
//! Communicates over COM1 serial (IRQ4) using GDB's remote protocol.
//! Activated on INT3 (#BP) or #DB (single-step / watchpoint).
//!
//! Wire format:
//!   `$<packet>#<checksum>`  — host → target
//!   `$<packet>#<checksum>`  — target → host (acks handled by GDB)
//!   `+` / `-`               — ack / nak
//!   `0x03` (Ctrl-C)         — interrupt target

// ── COM1 serial port I/O ──

const COM1: u16 = 0x3F8;
const COM1_LSR: u16 = COM1 + 5; // Line Status Register
const LSR_THR_EMPTY: u8 = 0x20; // Transmitter Holding Register Empty
const LSR_DATA_READY: u8 = 0x01; // Data Ready

fn serial_read() -> Option<u8> {
    if crate::hal::x64::inb(COM1_LSR) & LSR_DATA_READY != 0 {
        Some(crate::hal::x64::inb(COM1))
    } else {
        None
    }
}

fn serial_write(byte: u8) {
    while crate::hal::x64::inb(COM1_LSR) & LSR_THR_EMPTY == 0 {}
    crate::hal::x64::outb(COM1, byte);
}

fn serial_write_str(s: &[u8]) {
    for &b in s {
        serial_write(b);
    }
}

// ── Hex helpers ──

fn hex_val(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

fn hex_byte(v: u8) -> [u8; 2] {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    [HEX[(v >> 4) as usize], HEX[(v & 0xF) as usize]]
}

fn hex_word64(v: u64) -> [u8; 16] {
    let mut buf = [0u8; 16];
    for i in 0..8 {
        let b = ((v >> (56 - i * 8)) & 0xFF) as u8;
        let h = hex_byte(b);
        buf[i * 2] = h[0];
        buf[i * 2 + 1] = h[1];
    }
    buf
}

fn from_hex(s: &[u8]) -> u64 {
    let mut v: u64 = 0;
    for &c in s {
        v = (v << 4) | hex_val(c) as u64;
    }
    v
}

// ── Register context (x86_64 GDB order) ──

#[repr(C)]
pub struct GdbRegs {
    pub rax: u64, pub rbx: u64, pub rcx: u64, pub rdx: u64,
    pub rsi: u64, pub rdi: u64, pub rbp: u64, pub rsp: u64,
    pub r8:  u64, pub r9:  u64, pub r10: u64, pub r11: u64,
    pub r12: u64, pub r13: u64, pub r14: u64, pub r15: u64,
    pub rip: u64, pub eflags: u64,
    pub cs: u32, pub ss: u32, pub ds: u32, pub es: u32, pub fs: u32, pub gs: u32,
}

#[no_mangle]
pub static mut GDB_REGS: GdbRegs = GdbRegs {
    rax: 0, rbx: 0, rcx: 0, rdx: 0,
    rsi: 0, rdi: 0, rbp: 0, rsp: 0,
    r8: 0,  r9: 0,  r10: 0, r11: 0,
    r12: 0, r13: 0, r14: 0, r15: 0,
    rip: 0, eflags: 0,
    cs: 0, ss: 0, ds: 0, es: 0, fs: 0, gs: 0,
};

/// Serialise registers to GDB hex format (g packet).
fn regs_to_hex() -> [u8; 400] {
    let mut buf = [0u8; 400];
    let mut offset = 0;
    macro_rules! put_reg {
        ($reg:expr) => {
            let h = hex_word64($reg);
            buf[offset..offset + 16].copy_from_slice(&h);
            offset += 16;
        };
    }
    let r = unsafe { &GDB_REGS };
    put_reg!(r.rax); put_reg!(r.rbx); put_reg!(r.rcx); put_reg!(r.rdx);
    put_reg!(r.rsi); put_reg!(r.rdi); put_reg!(r.rbp); put_reg!(r.rsp);
    put_reg!(r.r8);  put_reg!(r.r9);  put_reg!(r.r10); put_reg!(r.r11);
    put_reg!(r.r12); put_reg!(r.r13); put_reg!(r.r14); put_reg!(r.r15);
    put_reg!(r.rip); put_reg!(r.eflags);
    put_reg!(r.cs as u64); put_reg!(r.ss as u64); put_reg!(r.ds as u64);
    put_reg!(r.es as u64); put_reg!(r.fs as u64); put_reg!(r.gs as u64);
    let _ = offset;
    buf
}

/// Parse GDB hex register packet into GDB_REGS (G packet).
fn regs_from_hex(data: &[u8]) {
    let r = unsafe { &mut GDB_REGS };
    let mut pos = 0;
    macro_rules! get_reg {
        ($field:expr) => {
            if pos + 16 <= data.len() {
                $field = from_hex(&data[pos..pos + 16]);
                pos += 16;
            }
        };
    }
    get_reg!(r.rax); get_reg!(r.rbx); get_reg!(r.rcx); get_reg!(r.rdx);
    get_reg!(r.rsi); get_reg!(r.rdi); get_reg!(r.rbp); get_reg!(r.rsp);
    get_reg!(r.r8);  get_reg!(r.r9);  get_reg!(r.r10); get_reg!(r.r11);
    get_reg!(r.r12); get_reg!(r.r13); get_reg!(r.r14); get_reg!(r.r15);
    get_reg!(r.rip); get_reg!(r.eflags);
    // Segment regs: only low 32 bits matter
    if pos + 16 <= data.len() {
        r.cs = from_hex(&data[pos..pos + 16]) as u32; pos += 16;
    }
    if pos + 16 <= data.len() {
        r.ss = from_hex(&data[pos..pos + 16]) as u32;
    }
}

// ── Packet send/recv ──

fn send_packet(data: &[u8]) {
    serial_write(b'$');
    let mut csum: u8 = 0;
    for &b in data {
        serial_write(b);
        csum = csum.wrapping_add(b);
    }
    serial_write(b'#');
    let h = hex_byte(csum);
    serial_write(h[0]);
    serial_write(h[1]);
}

fn recv_packet() -> Result<alloc::vec::Vec<u8>, ()> {
    let mut buf = alloc::vec::Vec::new();
    loop {
        match serial_read() {
            None => {
                // No data — return what we have or empty
                if buf.is_empty() {
                    return Err(());
                }
                break;
            }
            Some(b'+') => {} // ACK, ignore
            Some(b'-') => return Err(()),
            Some(b'\x03') => return Err(()), // Ctrl-C
            Some(b'$') => {
                buf.clear();
                loop {
                    match serial_read() {
                        Some(b'#') => {
                            // Read 2-char checksum (validate, but ignore mismatch)
                            let _c1 = serial_read();
                            let _c2 = serial_read();
                            return Ok(buf);
                        }
                        Some(c) => buf.push(c),
                        None => {} // wait
                    }
                }
            }
            Some(_) => {} // ignore stray bytes
        }
    }
    Ok(buf)
}

// ── GDB main loop ──

/// Called from the breakpoint trampoline. Runs the GDB stub loop
/// until the user types 'c' (continue) or 's' (step).
/// Returns true if single-step was requested.
pub fn gdb_main() -> bool {
    // Signal SIGTRAP (05) to GDB
    send_packet(b"S05");
    loop {
        match recv_packet() {
            Err(()) => continue,
            Ok(pkt) => {
                if pkt.is_empty() {
                    continue;
                }
                match pkt[0] {
                    b'?' => send_packet(b"S05"),
                    b'g' => send_packet(&regs_to_hex()),
                    b'G' => {
                        regs_from_hex(&pkt[1..]);
                        send_packet(b"OK");
                    }
                    b'm' => {
                        // m addr,length
                        let args = core::str::from_utf8(&pkt[1..]).unwrap_or("");
                        let comma = args.find(',').unwrap_or(args.len());
                        let addr = from_hex(args[..comma].as_bytes()) as *const u8;
                        let len = from_hex(args[comma + 1..].as_bytes()) as usize;
                        let mut reply = alloc::vec![0u8; len * 2];
                        for i in 0..len {
                            let byte = unsafe { core::ptr::read_volatile(addr.add(i)) };
                            let h = hex_byte(byte);
                            reply[i * 2] = h[0];
                            reply[i * 2 + 1] = h[1];
                        }
                        send_packet(&reply);
                    }
                    b'M' => {
                        // M addr,length:data
                        let args = core::str::from_utf8(&pkt[1..]).unwrap_or("");
                        let colon = args.find(':').unwrap_or(args.len());
                        let addr_str = &args[..colon];
                        let rest = &args[colon + 1..];
                        let comma = addr_str.find(',').unwrap_or(addr_str.len());
                        let addr = from_hex(addr_str[..comma].as_bytes()) as *mut u8;
                        let len = from_hex(addr_str[comma + 1..].as_bytes()) as usize;
                        let data = rest.as_bytes();
                        for i in 0..len {
                            if i * 2 + 1 < data.len() {
                                let val = (hex_val(data[i * 2]) << 4) | hex_val(data[i * 2 + 1]);
                                unsafe { core::ptr::write_volatile(addr.add(i), val); }
                            }
                        }
                        send_packet(b"OK");
                    }
                    b'c' => return false, // continue
                    b's' => {
                        // Set TF (trap flag) in saved eflags
                        unsafe { GDB_REGS.eflags |= 0x100; }
                        return true; // single-step
                    }
                    b'k' => {
                        serial_write_str(b"\r\n[KD] Kill requested, halting\r\n");
                        loop { unsafe { core::arch::asm!("hlt"); } }
                    }
                    b'Z' | b'z' => {
                        // Z0,addr,kind — insert sw breakpoint
                        // z0,addr,kind — remove sw breakpoint
                        // We support Z0/z0 only (software breakpoints)
                        if pkt.len() > 1 && pkt[1] == b'0' {
                            let args = core::str::from_utf8(&pkt[2..]).unwrap_or("");
                            let rest = if args.starts_with(',') { &args[1..] } else { args };
                            let comma = rest.find(',').unwrap_or(rest.len());
                            let addr = from_hex(rest[..comma].as_bytes()) as *mut u8;
                            if pkt[0] == b'Z' {
                                // Insert INT3 (0xCC) at address
                                unsafe { core::ptr::write_volatile(addr, 0xCC); }
                            } else {
                                // Remove — restore original byte (we don't save it, best effort)
                                // In practice GDB sends z0 after removing the breakpoint in memory
                            }
                            send_packet(b"OK");
                        } else {
                            send_packet(b""); // unsupported
                        }
                    }
                    b'H' => {
                        // Hc/g thread — we have 1 thread, always OK
                        send_packet(b"OK");
                    }
                    b'T' => {
                        // T thread — thread alive? reply OK for any
                        send_packet(b"OK");
                    }
                    b'q' => {
                        let query = core::str::from_utf8(&pkt[1..]).unwrap_or("");
                        if query.starts_with("Supported") {
                            send_packet(b"PacketSize=400;qXfer:memory-map:read+");
                        } else if query.starts_with("Attached") {
                            send_packet(b"1");
                        } else if query.starts_with("C") {
                            send_packet(b"QC1");
                        } else {
                            send_packet(b""); // unsupported
                        }
                    }
                    _ => send_packet(b""), // unsupported
                }
            }
        }
    }
}

// ── Assembly trampoline for breakpoint entry ──

core::arch::global_asm!(
    ".global gdb_breakpoint_entry",
    "gdb_breakpoint_entry:",
    // CPU pushed: SS, RSP, RFLAGS, CS, RIP (if CPL change) or just RIP, CS, RFLAGS (same CPL)
    // For Ring 3→0: pushed SS, RSP, RFLAGS, CS, RIP
    // For Ring 0→0: pushed RIP, CS, RFLAGS
    // We save all GP registers
    "push r15",
    "push r14",
    "push r13",
    "push r12",
    "push r11",
    "push r10",
    "push r9",
    "push r8",
    "push rbp",
    "push rdi",
    "push rsi",
    "push rdx",
    "push rcx",
    "push rbx",
    "push rax",
    // Save segment registers
    "mov rax, ds",
    "push rax",
    "mov rax, es",
    "push rax",
    "mov rax, fs",
    "push rax",
    "mov rax, gs",
    "push rax",
    // rdi = pointer to saved regs (RSP after our 19 pushes = 19*8 = 152 bytes above current RSP)
    "mov rdi, rsp",
    "call gdb_stub_save_and_run",
    // Restore segment regs
    "pop rax",
    "mov gs, rax",
    "pop rax",
    "mov fs, rax",
    "pop rax",
    "mov es, rax",
    "pop rax",
    "mov ds, rax",
    // Restore GP registers
    "pop rax",
    "pop rbx",
    "pop rcx",
    "pop rdx",
    "pop rsi",
    "pop rdi",
    "pop rbp",
    "pop r8",
    "pop r9",
    "pop r10",
    "pop r11",
    "pop r12",
    "pop r13",
    "pop r14",
    "pop r15",
    // Return from interrupt
    "iretq",
);

/// Called from assembly trampoline with rdi = ptr to saved regs.
/// Saves registers into GDB_REGS, calls gdb_main, restores.
#[no_mangle]
pub unsafe extern "C" fn gdb_stub_save_and_run(ctx: *mut u64) {
    // Layout at ctx (16 GP + 4 segment = 20× 8 bytes)
    // [0]: gs, [1]: fs, [2]: es, [3]: ds
    // [4]: rax, [5]: rbx, [6]: rcx, [7]: rdx
    // [8]: rsi, [9]: rdi, [10]: rbp, [11]: r8
    // [12]: r9, [13]: r10, [14]: r11, [15]: r12
    // [16]: r13, [17]: r14, [18]: r15
    // Above: RIP, CS, RFLAGS[, RSP, SS]

    let slice = core::slice::from_raw_parts_mut(ctx, 20);
    GDB_REGS.gs = slice[0] as u32;
    GDB_REGS.fs = slice[1] as u32;
    GDB_REGS.es = slice[2] as u32;
    GDB_REGS.ds = slice[3] as u32;
    GDB_REGS.rax = slice[4];
    GDB_REGS.rbx = slice[5];
    GDB_REGS.rcx = slice[6];
    GDB_REGS.rdx = slice[7];
    GDB_REGS.rsi = slice[8];
    GDB_REGS.rdi = slice[9];
    GDB_REGS.rbp = slice[10];
    GDB_REGS.r8  = slice[11];
    GDB_REGS.r9  = slice[12];
    GDB_REGS.r10 = slice[13];
    GDB_REGS.r11 = slice[14];
    GDB_REGS.r12 = slice[15];
    GDB_REGS.r13 = slice[16];
    GDB_REGS.r14 = slice[17];
    GDB_REGS.r15 = slice[18];

    // Read interrupt frame above our saved regs (mutable for write-back)
    let int_frame = ctx.add(20) as *mut u64;
    GDB_REGS.rip = *int_frame;
    GDB_REGS.cs = *int_frame.add(1) as u32;
    GDB_REGS.eflags = *int_frame.add(2);
    GDB_REGS.rsp = *int_frame.add(3);

    let step = gdb_main();

    // Write back modified registers
    slice[4] = GDB_REGS.rax;
    slice[5] = GDB_REGS.rbx;
    slice[6] = GDB_REGS.rcx;
    slice[7] = GDB_REGS.rdx;
    slice[8] = GDB_REGS.rsi;
    slice[9] = GDB_REGS.rdi;
    slice[10] = GDB_REGS.rbp;
    slice[11] = GDB_REGS.r8;
    slice[12] = GDB_REGS.r9;
    slice[13] = GDB_REGS.r10;
    slice[14] = GDB_REGS.r11;
    slice[15] = GDB_REGS.r12;
    slice[16] = GDB_REGS.r13;
    slice[17] = GDB_REGS.r14;
    slice[18] = GDB_REGS.r15;

    // Update interrupt frame RIP/RFLAGS
    *int_frame = GDB_REGS.rip;
    let flags = int_frame.add(2);
    *flags = GDB_REGS.eflags;
    if step {
        *flags |= 0x100u64; // Set TF for single-step
    }
}
