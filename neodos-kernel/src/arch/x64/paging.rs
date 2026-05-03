use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{PageTable, PageTableFlags};
use x86_64::PhysAddr;
use crate::serial_println;

pub unsafe fn make_user_accessible(start_addr: u64, size: u64) {
    let (pml4_frame, _) = Cr3::read();
    let pml4_ptr = pml4_frame.start_address().as_u64() as *mut PageTable;
    let pml4 = &mut *pml4_ptr;
    
    let end_addr = start_addr + size;
    serial_println!("Making memory accessible to user: {:#x} - {:#x}", start_addr, end_addr);

    // In a 1:1 identity mapped system (UEFI), physical == virtual.
    // We will iterate through the pages and add the USER_ACCESSIBLE flag.
    
    for addr in (start_addr..end_addr).step_by(0x1000) { // 4KB steps
        let pml4_index = ((addr >> 39) & 0x1FF) as usize;
        let pml4_entry = &mut pml4[pml4_index];
        
        if !pml4_entry.flags().contains(PageTableFlags::PRESENT) {
            continue;
        }
        
        // Add USER_ACCESSIBLE to PML4 entry
        let mut flags = pml4_entry.flags();
        flags.insert(PageTableFlags::USER_ACCESSIBLE);
        pml4_entry.set_flags(flags);
        
        let pdpt_ptr = (pml4_entry.addr().as_u64()) as *mut PageTable;
        let pdpt = &mut *pdpt_ptr;
        let pdpt_index = ((addr >> 30) & 0x1FF) as usize;
        let pdpt_entry = &mut pdpt[pdpt_index];
        
        if !pdpt_entry.flags().contains(PageTableFlags::PRESENT) {
            continue;
        }
        
        let mut pdpt_flags = pdpt_entry.flags();
        pdpt_flags.insert(PageTableFlags::USER_ACCESSIBLE);
        pdpt_entry.set_flags(pdpt_flags);
        
        if pdpt_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
            continue; // 1GB page, we are done
        }
        
        let pd_ptr = (pdpt_entry.addr().as_u64()) as *mut PageTable;
        let pd = &mut *pd_ptr;
        let pd_index = ((addr >> 21) & 0x1FF) as usize;
        let pd_entry = &mut pd[pd_index];
        
        if !pd_entry.flags().contains(PageTableFlags::PRESENT) {
            continue;
        }
        
        let mut pd_flags = pd_entry.flags();
        pd_flags.insert(PageTableFlags::USER_ACCESSIBLE);
        pd_entry.set_flags(pd_flags);
        
        if pd_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
            continue; // 2MB page, we are done
        }
        
        let pt_ptr = (pd_entry.addr().as_u64()) as *mut PageTable;
        let pt = &mut *pt_ptr;
        let pt_index = ((addr >> 12) & 0x1FF) as usize;
        let pt_entry = &mut pt[pt_index];
        
        if !pt_entry.flags().contains(PageTableFlags::PRESENT) {
            continue;
        }
        
        let mut pt_flags = pt_entry.flags();
        pt_flags.insert(PageTableFlags::USER_ACCESSIBLE);
        pt_entry.set_flags(pt_flags);
    }
    
    // Flush the TLB
    x86_64::instructions::tlb::flush_all();
}
