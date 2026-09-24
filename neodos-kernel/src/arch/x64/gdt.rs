use lazy_static::lazy_static;
use x86_64::structures::gdt::{GlobalDescriptorTable, Descriptor, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;
use core::sync::atomic::{AtomicBool, Ordering};

pub const DOUBLE_FAULT_IST_INDEX: u16 = 1;

#[repr(align(16))]
struct AlignedStack([u8; 4096 * 8]);

static mut DOUBLE_FAULT_STACK: AlignedStack = AlignedStack([0; 4096 * 8]);
/// TSS MUST be in a writable section.  LTO can place statics with
/// internal linkage into the read-only r-x segment, which would
/// silently discard set_kernel_stack() writes and leave RSP0=0
/// (triple fault on the next Ring-3→Ring-0 interrupt).
#[link_section = ".data"]
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
        // INVARIANT: RSP0 must never be 0 — a Ring 3 → Ring 0 transition with
        // RSP0=0 causes the CPU to push the exception frame at address 0,
        // producing a #PF → #DF → triple fault → machine reset.
        if cfg!(feature = "validation") && stack_top == 0 {
            panic!("set_kernel_stack: attempt to set RSP0=0 (would triple-fault on next Ring3 entry)");
        }
        unsafe {
            if stack_top == 0 {
                kerror!(crate::log::LogSubsys::Sched,
                    "[TSS] BLOCKING set_kernel_stack(0) — would cause triple fault!");
                return;
            }
            TSS.privilege_stack_table[0] = VirtAddr::new(stack_top);
            kdebug!(crate::log::LogSubsys::Sched,
                "[TSS_UPDATE] RSP0=0x{:x}", stack_top);
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

/// Prepare to return to a Ring 3 thread by updating TSS.RSP0.
/// Must be called BEFORE every iretq that transitions to Ring 3.
/// This is the single source of truth for TSS.RSP0 updates — all callers
/// (timer handler, syscall_try_resched, usermode entry) MUST use this.
///
/// # Safety
/// `ks_top` must be the `kernel_stack_top` of the thread that will execute
/// after the iretq.  Must be called with interrupts disabled.
pub unsafe fn prepare_ring3_return(ks_top: u64, tid: u32, pid: u32) {
    if ks_top == 0 {
        // BUGCHECK: kernel_stack_top=0 would cause the CPU to push the
        // interrupt frame at address 0 on the next Ring 3 → Ring 0
        // transition, producing a #PF → #DF → triple fault → machine reset.
        crate::serial_println!(
            "\n!!! BUGCHECK: TSS.RSP0 would be 0 for TID={} PID={} !!!", tid, pid);
        panic!("BUGCHECK: TSS.RSP0=0 for TID={}", tid);
    }
    TSS.privilege_stack_table[0] = VirtAddr::new(ks_top);
    kdebug!(crate::log::LogSubsys::Sched,
        "[TSS_UPDATE] pid={} tid={} rsp0=0x{:x}", pid, tid, ks_top);
}
