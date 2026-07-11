use crate::hal::raw;

#[no_mangle]
#[inline(never)]
pub extern "C" fn enable_interrupts() {
    unsafe { raw::raw_sti(); }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn disable_interrupts() {
    unsafe { raw::raw_cli(); }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn halt() -> ! {
    unsafe { raw::raw_halt() }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn reboot() -> ! {
    disable_interrupts();
    unsafe {
        raw::raw_outb(0xCF9, 0x06);
        raw::raw_outb(0x64, 0xFE);
    }
    halt()
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn poweroff() -> ! {
    disable_interrupts();
    unsafe {
        for &(port, val) in &[(0x404u16, 0x2000u16), (0x604u16, 0x2000u16),
                              (0xB004u16, 0x2000u16), (0x4004u16, 0x3400u16)] {
            raw::raw_outw(port, val);
        }
        raw::raw_outb(0x64u16, 0xFEu8);
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
    unsafe { raw::raw_read_cr2() }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn read_cr3() -> u64 {
    unsafe { raw::raw_read_cr3() }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn write_cr3(val: u64) {
    unsafe { raw::raw_write_cr3(val); }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn flush_tlb(virt: u64) {
    unsafe { raw::raw_invlpg(virt); }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn interrupts_enabled() -> bool {
    let flags = unsafe { raw::raw_read_rflags() };
    (flags & 0x200) != 0
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn hlt_once() {
    unsafe { raw::raw_hlt_once(); }
}

// ── Force ABI symbol retention ──
#[used]
static KEEP_CPU_ENABLE_INTERRUPTS: unsafe extern "C" fn() = enable_interrupts;
#[used]
static KEEP_CPU_DISABLE_INTERRUPTS: unsafe extern "C" fn() = disable_interrupts;
#[used]
static KEEP_CPU_HALT: unsafe extern "C" fn() -> ! = halt;
#[used]
static KEEP_CPU_REBOOT: unsafe extern "C" fn() -> ! = reboot;
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
