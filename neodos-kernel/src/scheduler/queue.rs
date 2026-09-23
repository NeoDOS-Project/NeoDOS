//! Scheduler run queue — extracted from mod.rs
use crate::scheduler::types::{Kthread, BOOT_TID, IDLE_TID, PRIORITY_COUNT, TIME_SLICES, ThreadState};
use crate::scheduler::Scheduler;

impl Scheduler {
    pub fn enqueue_to_cpu_run_queue(k: &Kthread) {
        if k.tid == BOOT_TID || k.tid == IDLE_TID {
            return;
        }
        let cpu = k.cpu as usize;
        if cpu >= crate::arch::x64::cpu_local::MAX_CPUS { return; }
        let my_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() } as usize;
        unsafe {
            let run_queue = crate::arch::x64::cpu_local::cpu_run_queue_mut(cpu);
            if run_queue.contains(k.tid) {
                return; // already in runqueue — avoid duplicate
            }
            run_queue.push(k.tid);
            #[cfg(feature = "forensic")]
            {
                if k.pid >= 4 {
                    let rq_len = run_queue.len();
                    crate::serial_println!("[SMPSCHED] ENQUEUE_OK pid={} tid={} cpu_target={} rq_len={}", k.pid, k.tid, cpu, rq_len);
                }
            }
        }
        // Send IPI_RESCHEDULE to the target CPU if it's a different CPU
        #[cfg(feature = "forensic")]
        if cpu != my_cpu && k.pid >= 4 {
            crate::serial_println!("[SMPSCHED] WAKE_NOTIFY from_cpu={} target_cpu={} pid={} tid={} action=IPI_RESCHEDULE", my_cpu, cpu, k.pid, k.tid);
        } else if k.pid >= 4 {
            #[cfg(feature = "forensic")]
            crate::serial_println!("[SMPSCHED] WAKE_NOTIFY from_cpu={} target_cpu={} pid={} tid={} action=same_cpu_no_ipi", my_cpu, cpu, k.pid, k.tid);
        }
        if cpu != my_cpu {
            unsafe {
                let kprcb = crate::arch::x64::cpu_local::kprcb_page(cpu);
                if let Some(kprcb_addr) = kprcb {
                    let apic_id = core::ptr::read_volatile(
                        (kprcb_addr + 4) as *const u32 // apic_id at offset 0x004
                    );
                    crate::arch::x64::ipi::send_ipi(
                        apic_id,
                        crate::arch::x64::ipi::IPI_RESCHEDULE,
                    );
                }
            }
        }
    }

    /// Transition a thread to Ready state and enqueue it exactly once.
    /// Safe to call if thread is already Ready (no-op, avoids duplicate enqueue).
    /// Must be called under scheduler lock + interrupts disabled.
    pub fn make_thread_ready(k: &mut Kthread) {
        if k.state == ThreadState::Ready {
            return;
        }
        k.state = ThreadState::Ready;
        let idx = (k.priority as usize).min(PRIORITY_COUNT as usize - 1);
        k.time_slice_remaining = TIME_SLICES[idx];
        k.ticks_since_scheduled = 0;
        Self::enqueue_to_cpu_run_queue(k);
    }

    /// Remove a thread from its assigned CPU's per-CPU run queue.
    /// Called when a thread transitions away from Ready state.
    /// Safe to call even if the thread is not currently in the run queue.
    /// Must be called under scheduler lock + interrupts disabled.
    pub fn remove_from_run_queue(k: &Kthread) {
        let cpu = k.cpu as usize;
        if cpu >= crate::arch::x64::cpu_local::MAX_CPUS {
            return;
        }
        unsafe {
            crate::arch::x64::cpu_local::remove_from_cpu_run_queue(cpu, k.tid);
        }
    }

    /// Try to dequeue the next thread from the current CPU's local run queue.
    /// Returns the TID if found, or None if the queue is empty.
    pub(crate) fn try_dequeue_local() -> Option<u32> {
        unsafe {
            let run_queue = crate::arch::x64::cpu_local::this_cpu_run_queue_mut();
            run_queue.pop()
        }
    }

}
