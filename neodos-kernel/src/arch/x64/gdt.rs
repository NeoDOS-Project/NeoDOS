use lazy_static::lazy_static;
use x86_64::structures::gdt::{GlobalDescriptorTable, Descriptor, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;
use core::sync::atomic::{AtomicBool, Ordering};
use crate::arch::x64::cpu_local::MAX_CPUS;
pub const DOUBLE_FAULT_IST_INDEX: u16 = 1;

#[repr(align(16))]
#[derive(Copy, Clone)]
struct AlignedStack([u8; 4096 * 8]);

static mut DOUBLE_FAULT_STACK: AlignedStack = AlignedStack([0; 4096 * 8]);
/// TSS MUST be in a writable section.  LTO can place statics with
/// internal linkage into the read-only r-x segment, which would
/// silently discard set_kernel_stack() writes and leave RSP0=0
/// (triple fault on the next Ring-3→Ring-0 interrupt).
#[link_section = ".data"]
static mut TSS: TaskStateSegment = TaskStateSegment::new();
static TSS_READY: AtomicBool = AtomicBool::new(false);

// ── F-01 P0.2: TSS per-CPU for SMP (global TSS is RSP0 race) ──────────
#[link_section = ".data"]
static mut PER_CPU_TSS: [TaskStateSegment; MAX_CPUS] = {
    // const fn to init array (TaskStateSegment::new is const)
    let mut arr: [TaskStateSegment; MAX_CPUS] = [TaskStateSegment::new(); MAX_CPUS];
    arr
};
static PER_CPU_TSS_READY: [AtomicBool; MAX_CPUS] = [const { AtomicBool::new(false) }; MAX_CPUS];
static mut PER_CPU_DF_STACK: [AlignedStack; MAX_CPUS] = [AlignedStack([0; 4096 * 8]); MAX_CPUS];

lazy_static! {
    static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        let kernel_code = gdt.add_entry(Descriptor::kernel_code_segment());
        let kernel_data = gdt.add_entry(Descriptor::kernel_data_segment());
        let user_code = gdt.add_entry(Descriptor::user_code_segment());
        let user_data = gdt.add_entry(Descriptor::user_data_segment());
        // P0.2: BSP uses PER_CPU_TSS[0] (global TSS kept for fallback)
        let tss = unsafe { gdt.add_entry(Descriptor::tss_segment(&PER_CPU_TSS[0])) };

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
    // P0.2: TSS per-CPU — use current CPU's TSS if available, else global.
    let cpu = if crate::hal::safe::GsBase::read() != 0 {
        unsafe { crate::arch::x64::cpu_local::this_cpu_id() as usize }
    } else { 0 };
    let per_cpu_ready = cpu < MAX_CPUS && PER_CPU_TSS_READY[cpu].load(Ordering::Relaxed);
    let global_ready = TSS_READY.load(Ordering::Relaxed);
    if !per_cpu_ready && !global_ready {
        return;
    }
    if cfg!(feature = "validation") && stack_top == 0 {
        panic!("set_kernel_stack: attempt to set RSP0=0 (would triple-fault on next Ring3 entry)");
    }
    unsafe {
        if stack_top == 0 {
            kerror!(crate::log::LogSubsys::Sched,
                "[TSS] BLOCKING set_kernel_stack(0) — would cause triple fault!");
            return;
        }
        if per_cpu_ready {
            PER_CPU_TSS[cpu].privilege_stack_table[0] = VirtAddr::new(stack_top);
        } else {
            TSS.privilege_stack_table[0] = VirtAddr::new(stack_top);
        }
        kdebug!(crate::log::LogSubsys::Sched,
            "[TSS_UPDATE] cpu={} RSP0=0x{:x}", cpu, stack_top);
    }
}

pub fn init() {
    use x86_64::instructions::segmentation::{CS, Segment};
    use x86_64::instructions::tables::load_tss;

    unsafe {
        // BSP: setup both global TSS (fallback) and PER_CPU_TSS[0]
        let df_stack_top = DOUBLE_FAULT_STACK.0.as_ptr() as u64 + 4096 * 8;
        TSS.interrupt_stack_table[(DOUBLE_FAULT_IST_INDEX - 1) as usize] = VirtAddr::new(df_stack_top);
        let df_stack_top0 = PER_CPU_DF_STACK[0].0.as_ptr() as u64 + 4096 * 8;
        PER_CPU_TSS[0].interrupt_stack_table[(DOUBLE_FAULT_IST_INDEX - 1) as usize] = VirtAddr::new(df_stack_top0);
        // Copy TSS template to per-CPU entry (privilege stacks will be set later)
        PER_CPU_TSS[0] = TSS;
        // But keep the DF stack we just set
        PER_CPU_TSS[0].interrupt_stack_table[(DOUBLE_FAULT_IST_INDEX - 1) as usize] = VirtAddr::new(df_stack_top0);
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
    PER_CPU_TSS_READY[0].store(true, Ordering::Relaxed);
}

/// Initialize per-CPU GDT/TSS for an AP (called from ap_entry with cpu_id).
pub fn init_ap(cpu_id: usize) {
    use x86_64::instructions::segmentation::{CS, Segment};
    use x86_64::instructions::tables::load_tss;
    if cpu_id >= MAX_CPUS || cpu_id == 0 { return; }
    unsafe {
        let df_stack_top = PER_CPU_DF_STACK[cpu_id].0.as_ptr() as u64 + 4096 * 8;
        PER_CPU_TSS[cpu_id].interrupt_stack_table[(DOUBLE_FAULT_IST_INDEX - 1) as usize] = VirtAddr::new(df_stack_top);
        // Build a per-CPU GDT with its own TSS descriptor
        let mut gdt = GlobalDescriptorTable::new();
        let kernel_code = gdt.add_entry(Descriptor::kernel_code_segment());
        let kernel_data = gdt.add_entry(Descriptor::kernel_data_segment());
        let user_code = gdt.add_entry(Descriptor::user_code_segment());
        let user_data = gdt.add_entry(Descriptor::user_data_segment());
        let tss = gdt.add_entry(Descriptor::tss_segment(&PER_CPU_TSS[cpu_id]));
        // Load the new GDT and TSS — this is per-CPU, so no lock needed
        // We leak the GDT to make it 'static (APs never free it)
        let gdt_box = alloc::boxed::Box::new(gdt);
        let gdt_ptr = alloc::boxed::Box::into_raw(gdt_box);
        (*gdt_ptr).load();
        CS::set_reg(kernel_code);
        load_tss(tss);
        crate::hal::raw::raw_set_segment_regs(kernel_data.0, kernel_data.0, kernel_data.0);
        // GS already set to KPRCB in ap_entry, keep it
    }
    PER_CPU_TSS_READY[cpu_id].store(true, Ordering::Relaxed);
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
        crate::serial_println!(
            "\n!!! BUGCHECK: TSS.RSP0 would be 0 for TID={} PID={} !!!", tid, pid);
        panic!("BUGCHECK: TSS.RSP0=0 for TID={}", tid);
    }
    let cpu = if crate::hal::safe::GsBase::read() != 0 {
        crate::arch::x64::cpu_local::this_cpu_id() as usize
    } else { 0 };
    if cpu < MAX_CPUS && PER_CPU_TSS_READY[cpu].load(Ordering::Relaxed) {
        PER_CPU_TSS[cpu].privilege_stack_table[0] = VirtAddr::new(ks_top);
        kdebug!(crate::log::LogSubsys::Sched,
            "[TSS_UPDATE] cpu={} pid={} tid={} rsp0=0x{:x}", cpu, pid, tid, ks_top);
    } else {
        TSS.privilege_stack_table[0] = VirtAddr::new(ks_top);
        kdebug!(crate::log::LogSubsys::Sched,
            "[TSS_UPDATE] pid={} tid={} rsp0=0x{:x} (global fallback)", pid, tid, ks_top);
    }
}
