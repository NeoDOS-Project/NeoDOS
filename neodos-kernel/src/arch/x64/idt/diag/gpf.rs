//! GDT dump and GPF error decoder.

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
