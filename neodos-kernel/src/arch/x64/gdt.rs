use lazy_static::lazy_static;
use x86_64::structures::gdt::{GlobalDescriptorTable, Descriptor, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

// IST number for double fault handler (1-7).  TSS array index = IST - 1.
pub const DOUBLE_FAULT_IST_INDEX: u16 = 1;

    // 16-byte alignment for all TSS stacks is critical: the CPU pushes 5×8 bytes and
    // the asm handlers push 15×8 bytes (= 160 = 0 mod 16) before calling Rust code.
    // Without this alignment, SSE instructions (movaps/movdqa) in the compiled kernel
    // will #GP fault, causing syscall instability.
    #[repr(align(16))]
    struct AlignedStack([u8; 4096 * 8]); // 32 KB

lazy_static! {
    static ref TSS: TaskStateSegment = {
        let mut tss = TaskStateSegment::new();
        tss.interrupt_stack_table[(DOUBLE_FAULT_IST_INDEX - 1) as usize] = {
            static mut STACK: AlignedStack = AlignedStack([0; 4096 * 8]);
            let ptr = unsafe { STACK.0.as_ptr() };
            VirtAddr::from_ptr(ptr) + (4096 * 8) as u64
        };
        // Ring 0 stack for interrupts originating from Ring 3
        tss.privilege_stack_table[0] = {
            static mut RSP0_STACK: AlignedStack = AlignedStack([0; 4096 * 8]);
            let ptr = unsafe { RSP0_STACK.0.as_ptr() };
            VirtAddr::from_ptr(ptr) + (4096 * 8) as u64
        };
        tss
    };
}

lazy_static! {
    static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        let kernel_code = gdt.add_entry(Descriptor::kernel_code_segment());
        let kernel_data = gdt.add_entry(Descriptor::kernel_data_segment());
        let user_code = gdt.add_entry(Descriptor::user_code_segment());
        let user_data = gdt.add_entry(Descriptor::user_data_segment());
        let tss = gdt.add_entry(Descriptor::tss_segment(&TSS));
        
        (gdt, Selectors {
            kernel_code,
            kernel_data,
            user_code,
            user_data,
            tss,
        })
    };
}

pub struct Selectors {
    pub kernel_code: SegmentSelector,
    pub kernel_data: SegmentSelector,
    pub user_code: SegmentSelector,
    pub user_data: SegmentSelector,
    pub tss: SegmentSelector,
}

pub fn init() {
    use x86_64::instructions::segmentation::{CS, Segment};
    use x86_64::instructions::tables::load_tss;

    GDT.0.load();
    unsafe {
        CS::set_reg(GDT.1.kernel_code);
        load_tss(GDT.1.tss);
        
        // Set other segment registers to kernel data segment
        core::arch::asm!(
            "mov ds, {0:x}",
            "mov es, {0:x}",
            "mov ss, {0:x}",
            "mov gs, {0:x}",
            "mov fs, {0:x}",
            in(reg) GDT.1.kernel_data.0
        );
    }
}

pub fn get_selectors() -> &'static Selectors {
    &GDT.1
}
