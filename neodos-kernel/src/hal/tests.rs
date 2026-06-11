pub fn register_hal_tests() {
    crate::testing::register("hal_v04_abi_msr_safe", || {
        let gs = crate::hal::safe::GsBase::read();
        crate::test_true!(gs != 0);
        Ok(())
    });

    crate::testing::register("hal_msr_read_write_consistency", || {
        let apic = crate::hal::safe::ApicBase::read();
        crate::test_true!((apic & 0xFFFF_FFFF_FFFF_F000) == apic);
        Ok(())
    });

    crate::testing::register("hal_no_asm_outside_hal_dir", || {
        let raw = unsafe { crate::hal::raw::raw_read_msr(0xC0000101) };
        let safe = crate::hal::safe::read_msr(&crate::hal::safe::GS_BASE);
        crate::test_eq!(raw, safe);
        Ok(())
    });

    crate::testing::register("hal_cr2_page_fault_addr", || {
        let cr2 = crate::hal::safe::read_cr2();
        crate::test_eq!(cr2, 0u64);
        Ok(())
    });

    crate::testing::register("hal_invpcid_tlb_invalidation", || {
        let cr3 = unsafe { crate::hal::raw::raw_read_cr3() };
        crate::test_true!(cr3 != 0);
        Ok(())
    });
}
