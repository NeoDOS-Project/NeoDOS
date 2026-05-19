use core::arch::asm;

#[no_mangle]
#[inline(never)]
pub extern "C" fn enable_interrupts() {
    unsafe { asm!("sti", options(nostack, nomem)); }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn disable_interrupts() {
    unsafe { asm!("cli", options(nostack, nomem)); }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn halt() -> ! {
    loop {
        unsafe { asm!("hlt", options(nostack, nomem)); }
    }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn poweroff() -> ! {
    disable_interrupts();
    unsafe {
        for &(port, val) in &[(0x404u16, 0x2000u16), (0x604u16, 0x2000u16),
                              (0xB004u16, 0x2000u16), (0x4004u16, 0x3400u16)] {
            asm!("out dx, ax", in("dx") port, in("ax") val,
                options(nomem, nostack, preserves_flags));
        }
        asm!("out dx, al", in("dx") 0x64u16, in("al") 0xFEu8,
            options(nomem, nostack, preserves_flags));
    }
    halt()
}

#[inline(never)]
pub fn cpu_info() -> crate::cpu::CpuInfo {
    crate::cpu::get_cpu_info()
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn read_cr2() -> u64 {
    let val: u64;
    unsafe { asm!("mov {}, cr2", out(reg) val, options(nomem, nostack)); }
    val
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn read_cr3() -> u64 {
    let val: u64;
    unsafe { asm!("mov {}, cr3", out(reg) val, options(nomem, nostack)); }
    val
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn write_cr3(val: u64) {
    unsafe { asm!("mov cr3, {}", in(reg) val, options(nomem, nostack)); }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn flush_tlb(virt: u64) {
    unsafe { asm!("invlpg [{}]", in(reg) virt, options(nostack, nomem, preserves_flags)); }
}

/// Returns true if interrupts are currently enabled (IF flag).
#[no_mangle]
#[inline(never)]
pub extern "C" fn interrupts_enabled() -> bool {
    let flags: u64;
    unsafe { asm!("pushfq; pop {}", out(reg) flags, options(nomem, nostack)); }
    (flags & 0x200) != 0
}

/// Execute a single HLT instruction (wait for next interrupt, then return).
/// Unlike `halt()` which never returns, this issues one HLT and returns.
#[no_mangle]
#[inline(never)]
pub extern "C" fn hlt_once() {
    unsafe { asm!("hlt", options(nomem, nostack)); }
}

// ── Force ABI symbol retention ──
#[used]
static KEEP_CPU_ENABLE_INTERRUPTS: unsafe extern "C" fn() = enable_interrupts;
#[used]
static KEEP_CPU_DISABLE_INTERRUPTS: unsafe extern "C" fn() = disable_interrupts;
#[used]
static KEEP_CPU_HALT: unsafe extern "C" fn() -> ! = halt;
#[used]
static KEEP_CPU_POWEROFF: unsafe extern "C" fn() -> ! = poweroff;
#[used]
static KEEP_CPU_READ_CR2: unsafe extern "C" fn() -> u64 = read_cr2;
#[used]
static KEEP_CPU_READ_CR3: unsafe extern "C" fn() -> u64 = read_cr3;
#[used]
static KEEP_CPU_WRITE_CR3: unsafe extern "C" fn(u64) = write_cr3;
#[used]
static KEEP_CPU_FLUSH_TLB: unsafe extern "C" fn(u64) = flush_tlb;
#[used]
static KEEP_CPU_INTERRUPTS_ENABLED: unsafe extern "C" fn() -> bool = interrupts_enabled;
#[used]
static KEEP_CPU_HLT_ONCE: unsafe extern "C" fn() = hlt_once;
#[used]
static KEEP_CPU_INFO: fn() -> crate::cpu::CpuInfo = cpu_info;
