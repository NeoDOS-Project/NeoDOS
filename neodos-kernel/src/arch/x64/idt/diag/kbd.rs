//! IRQ33 keyboard CPU-ownership diagnostics.

// ── Phase 6: IRQ33 CPU-ownership diagnostics (counters + ring, no serial per IRQ) ──
// Records which CPU/APIC received each keyboard IRQ. Decoder path unchanged.
pub(crate) const KBD_IRQ_RING_SIZE: usize = 64;
#[derive(Clone, Copy)]
pub(crate) struct KbdIrqEntry {
    pub(crate) seq: u64,
    pub(crate) cpu: u32,
    pub(crate) apic: u32,
    pub(crate) tid: u32,
    pub(crate) scancode: u8,
}
pub(crate) static KBD_IRQ_SEQ: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub(crate) static KBD_IRQ_HEAD: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
pub(crate) static mut KBD_IRQ_RING: [KbdIrqEntry; KBD_IRQ_RING_SIZE] = [KbdIrqEntry { seq: 0, cpu: 0xFFFFFFFF, apic: 0xFFFFFFFF, tid: 0, scancode: 0 }; KBD_IRQ_RING_SIZE];
pub(crate) static KBD_IRQ_CNT: [core::sync::atomic::AtomicU64; 16] = [const { core::sync::atomic::AtomicU64::new(0) }; 16];

/// Total keyboard IRQs observed across all CPUs (Phase 9 auto-dump).
pub fn kbd_irq_total() -> u64 {
    let mut t = 0u64;
    for c in &KBD_IRQ_CNT { t += c.load(core::sync::atomic::Ordering::Relaxed); }
    t
}

/// Reset IRQ33 ownership counters (Phase 6: clean baseline before shell).
pub fn kbd_irq_reset() {
    KBD_IRQ_SEQ.store(0, core::sync::atomic::Ordering::Relaxed);
    KBD_IRQ_HEAD.store(0, core::sync::atomic::Ordering::Relaxed);
    for c in &KBD_IRQ_CNT {
        c.store(0, core::sync::atomic::Ordering::Relaxed);
    }
    unsafe {
        for i in 0..KBD_IRQ_RING_SIZE {
            KBD_IRQ_RING[i].cpu = 0xFFFFFFFF;
        }
    }
}

/// Dump IRQ33 per-CPU ownership: counters + ring (cpu/apic/tid/scancode).
/// Called from Ctrl+Alt+V / syscall 99 alongside vt_diag_dump. No routing change.
pub fn kbd_irq_dump() {
    unsafe { core::arch::asm!("cli"); }
    crate::println!("[KBD_IRQ] per-CPU counts (max 16 CPUs):");
    let mut total = 0u64;
    for cpu in 0..16 {
        let n = KBD_IRQ_CNT[cpu].load(core::sync::atomic::Ordering::Relaxed);
        if n > 0 {
            crate::println!("[KBD_IRQ] cpu={} count={}", cpu, n);
        }
        total += n;
    }
    crate::println!("[KBD_IRQ] total={}", total);
    let head = KBD_IRQ_HEAD.load(core::sync::atomic::Ordering::Relaxed);
    let n = head.min(KBD_IRQ_RING_SIZE);
    let start = if head < KBD_IRQ_RING_SIZE { 0 } else { head % KBD_IRQ_RING_SIZE };
    for i in 0..n {
        let idx = (start + i) % KBD_IRQ_RING_SIZE;
        let e = unsafe { KBD_IRQ_RING[idx] };
        if e.cpu == 0xFFFFFFFF { continue; }
        crate::println!("[KBD_IRQ][{}] seq={} cpu={} apic={} tid={} sc=0x{:02x}",
            i, e.seq, e.cpu, e.apic, e.tid, e.scancode);
    }
    // Distinct producer CPUs (|producer_cpus| must be 1 for SPSC VALID)
    let mut seen = [0xFFFFFFFFu32; 16];
    let mut nseen = 0usize;
    for i in 0..n {
        let idx = (start + i) % KBD_IRQ_RING_SIZE;
        let e = unsafe { KBD_IRQ_RING[idx] };
        if e.cpu == 0xFFFFFFFF { continue; }
        let mut found = false;
        for j in 0..nseen { if seen[j] == e.cpu { found = true; break; } }
        if !found && nseen < 16 { seen[nseen] = e.cpu; nseen += 1; }
    }
    crate::serial_println!("[KBD_IRQ] producer_cpus={} (SPSC VALID iff 1)", nseen);
    for j in 0..nseen {
        crate::serial_println!("[KBD_IRQ] producer_cpu={}", seen[j]);
    }
    unsafe { core::arch::asm!("sti"); }
}
