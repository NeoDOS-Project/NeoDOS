pub fn register_hal_tests() {
    // x86_64-specific: GS base is programmed via IA32_GS_BASE.
    crate::testing::register("x64_hal_v04_abi_msr_safe", || {
        let gs = crate::hal::safe::GsBase::read();
        crate::test_true!(gs != 0);
        Ok(())
    });

    // x86_64-specific: IA32_APIC_BASE low 12 bits are reserved, so the typed
    // read must return a page-aligned address.
    crate::testing::register("x64_msr_read_write_consistency", || {
        let apic = crate::hal::safe::ApicBase::read();
        crate::test_true!((apic & 0xFFFF_FFFF_FFFF_F000) == apic);
        Ok(())
    });

    // x86_64-specific: the raw MSR read and the typed `Msr` wrapper must agree.
    // NOTE: this is not an asm-placement check; that is enforced by tooling.
    crate::testing::register("x64_msr_raw_safe_match", || {
        let raw = unsafe { crate::hal::raw::raw_read_msr(0xC0000101) };
        let safe = crate::hal::safe::read_msr(&crate::hal::safe::GS_BASE);
        crate::test_eq!(raw, safe);
        Ok(())
    });

    // x86_64-specific: CR2 is readable via the HAL.
    crate::testing::register("x64_cr2_page_fault_addr", || {
        let cr2 = crate::hal::safe::read_cr2();
        crate::test_eq!(cr2, 0u64);
        Ok(())
    });

    crate::hal::mmio::register_tests();
}
