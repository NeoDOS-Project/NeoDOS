use x86_64::registers::control::{Cr3, Cr3Flags};
use x86_64::structures::paging::{PageTable, PageTableFlags};
use x86_64::PhysAddr;
use crate::serial_println;

#[repr(align(4096))]
struct AlignedPageTable(PageTable);

static mut PML4: AlignedPageTable = AlignedPageTable(PageTable::new());
static mut PDPT: AlignedPageTable = AlignedPageTable(PageTable::new());
static mut PD: [AlignedPageTable; 4] = [
    AlignedPageTable(PageTable::new()),
    AlignedPageTable(PageTable::new()),
    AlignedPageTable(PageTable::new()),
    AlignedPageTable(PageTable::new()),
];

pub unsafe fn init_custom_page_tables() {
    serial_println!("[+] Initializing custom Page Tables...");
    
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;

    // SAFETY: These are static, properly aligned page tables
    // We assume bootloader left identity mapping for 0-4GB intact
    // Using pointer arithmetic to get physical addresses from kernel virtual pointers
    
    // 1. Link PML4[0] to PDPT
    let pdpt_addr = PhysAddr::new(&PDPT as *const _ as u64);
    if let Err(_) = PhysAddr::try_new(pdpt_addr.as_u64()) {
        panic!("PDPT address invalid - bootloader may not have identity mapped this region");
    }
    PML4.0[0].set_addr(pdpt_addr, flags);

    // 2. Link PDPT[0..4] to PD[0..4]
    for i in 0..4 {
        let pd_addr = PhysAddr::new(&PD[i] as *const _ as u64);
        if let Err(_) = PhysAddr::try_new(pd_addr.as_u64()) {
            panic!("PD[{}] address invalid", i);
        }
        PDPT.0[i].set_addr(pd_addr, flags);
    }

    // 3. Populate PD to identity map 0 to 4GB (using 2MB huge pages)
    // WARNING: Assumes bootloader provided valid RAM in this region
    for i in 0..4 {
        for j in 0..512 {
            let addr = (i * 512 + j) as u64 * 0x200000;
            let mut entry_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::HUGE_PAGE;
            
            // If the address is within our target User Mode range (0x400000 - 0x800000), make it accessible to Ring 3
            if addr >= 0x400000 && addr < 0x800000 {
                entry_flags |= PageTableFlags::USER_ACCESSIBLE;
            }
            
            PD[i].0[j].set_addr(PhysAddr::new(addr), entry_flags);
        }
    }

    // 4. Load the new PML4 into CR3
    let pml4_addr = PhysAddr::new(&PML4 as *const _ as u64);
    match x86_64::structures::paging::PhysFrame::from_start_address(pml4_addr) {
        Ok(frame) => {
            Cr3::write(frame, Cr3Flags::empty());
            serial_println!("[+] Custom Page Tables loaded! 4GB identity mapped.");
        },
        Err(_) => panic!("PML4 address 0x{:x} not aligned to 4KB", pml4_addr.as_u64()),
    }
}

