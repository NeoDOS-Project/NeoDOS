use lazy_static::lazy_static;
use x86_64::structures::gdt::{GlobalDescriptorTable, Descriptor, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;
use core::sync::atomic::{AtomicBool, Ordering};

pub const DOUBLE_FAULT_IST_INDEX: u16 = 1;

#[repr(align(16))]
struct AlignedStack([u8; 4096 * 8]);

static mut DOUBLE_FAULT_STACK: AlignedStack = AlignedStack([0; 4096 * 8]);
static mut TSS: TaskStateSegment = TaskStateSegment::new();
static TSS_READY: AtomicBool = AtomicBool::new(false);

lazy_static! {
    static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        let kernel_code = gdt.add_entry(Descriptor::kernel_code_segment());
        let kernel_data = gdt.add_entry(Descriptor::kernel_data_segment());
        let user_code = gdt.add_entry(Descriptor::user_code_segment());
        let user_data = gdt.add_entry(Descriptor::user_data_segment());
        let tss = unsafe { gdt.add_entry(Descriptor::tss_segment(&TSS)) };

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

pub fn set_kernel_stack(stack_top: u64) {
    if TSS_READY.load(Ordering::Relaxed) {
        unsafe {
            TSS.privilege_stack_table[0] = VirtAddr::new(stack_top);
        }
    }
}

pub fn init() {
    use x86_64::instructions::segmentation::{CS, Segment};
    use x86_64::instructions::tables::load_tss;

    unsafe {
        let df_stack_top = DOUBLE_FAULT_STACK.0.as_ptr() as u64 + 4096 * 8;
        TSS.interrupt_stack_table[(DOUBLE_FAULT_IST_INDEX - 1) as usize] = VirtAddr::new(df_stack_top);
    }

    GDT.0.load();
    unsafe {
        CS::set_reg(GDT.1.kernel_code);
        load_tss(GDT.1.tss);

        crate::hal::raw::raw_set_segment_regs(GDT.1.kernel_data.0, GDT.1.kernel_data.0, GDT.1.kernel_data.0);
        crate::hal::raw::raw_set_gs(GDT.1.kernel_data.0);
        crate::hal::raw::raw_set_fs(GDT.1.kernel_data.0);
    }

    TSS_READY.store(true, Ordering::Relaxed);
}

pub fn get_selectors() -> &'static Selectors {
    &GDT.1
}
