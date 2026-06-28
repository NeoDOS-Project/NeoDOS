#![allow(dead_code)]

use crate::serial_println;
use core::sync::atomic::{fence, Ordering};

struct NvmeInfo {
    bus: u8,
    device: u8,
    func: u8,
    bar0_phys: u64,
}

fn find_nvme_controller() -> Option<NvmeInfo> {
    for bus in 0..=0 {
        for dev in 0..32 {
            for func in 0..8 {
                let vendor = crate::drivers::pci::pci_config_read_word(bus, dev, func, 0);
                if vendor == 0xFFFF || vendor == 0 {
                    if func == 0 { break; }
                    continue;
                }

                let class_rev = crate::drivers::pci::pci_config_read_dword(bus, dev, func, 0x08);
                let class = ((class_rev >> 24) & 0xFF) as u8;
                let subclass = ((class_rev >> 16) & 0xFF) as u8;
                let prog_if = ((class_rev >> 8) & 0xFF) as u8;

                if class == 0x01 && subclass == 0x08 && prog_if == 0x02 {
                    let bar0 = crate::drivers::pci::pci_config_read_dword(bus, dev, func, 0x10);
                    let bar_is_64bit = (bar0 & 0x06) == 0x04;
                    let bar0_phys = if bar_is_64bit {
                        let bar1 = crate::drivers::pci::pci_config_read_dword(bus, dev, func, 0x14);
                        ((bar1 as u64) << 32) | (bar0 as u64 & 0xFFFF_FFF0)
                    } else {
                        (bar0 & 0xFFFF_FFF0) as u64
                    };
                    return Some(NvmeInfo { bus, device: dev, func, bar0_phys });
                }
            }
        }
    }
    None
}

fn nvme_enable(nvme: &NvmeInfo) {
    let cmd = crate::drivers::pci::pci_config_read_word(nvme.bus, nvme.device, nvme.func, 0x04);
    crate::drivers::pci::pci_config_write_word(nvme.bus, nvme.device, nvme.func, 0x04, cmd | 0x06);
}

const NVME_CC: u64 = 0x0014;
const NVME_CSTS: u64 = 0x001C;

const CC_EN: u32 = 1 << 0;

const CSTS_RDY: u32 = 1 << 0;
const CSTS_CFS: u32 = 1 << 1;

const ADMIN_IDENTIFY: u8 = 0x06;
const ADMIN_CREATE_IO_CQ: u8 = 0x05;
const ADMIN_CREATE_IO_SQ: u8 = 0x01;
const ADMIN_DELETE_IO_CQ: u8 = 0x04;
const ADMIN_DELETE_IO_SQ: u8 = 0x00;

const IO_WRITE: u8 = 0x01;
const IO_READ: u8 = 0x02;

const ASQ_ENTRIES: u16 = 64;
const ACQ_ENTRIES: u16 = 64;
const IOSQ_ENTRIES: u16 = 32;
const IOCQ_ENTRIES: u16 = 32;

const SQE_SIZE: u16 = 64;
const CQE_SIZE: u16 = 16;
const PAGE_SIZE_4K: u64 = 4096;

const NVME_MMIO_VIRT: u64 = 0xFFE0_0000;

#[repr(C, align(4096))]
struct IdentifyCtrl {
    _data: [u8; 4096],
}

impl IdentifyCtrl {
    fn nn(&self) -> u32 {
        u32::from_ne_bytes([self._data[384], self._data[385], self._data[386], self._data[387]])
    }
    fn serial(&self) -> &[u8] {
        &self._data[4..24]
    }
    fn model(&self) -> &[u8] {
        &self._data[24..64]
    }
}

#[repr(C, align(4096))]
struct IdentifyNs {
    data: [u8; 4096],
}

impl IdentifyNs {
    fn nsze(&self) -> u64 {
        u64::from_le_bytes(self.data[0..8].try_into().unwrap())
    }
    fn flbas(&self) -> u8 {
        self.data[27] & 0x0F
    }
    fn lbads(&self) -> u8 {
        let flbas = self.flbas();
        let lbaf_off = 128 + (flbas as usize) * 4;
        self.data[lbaf_off + 2] // LBADS is 3rd byte of LBAF entry
    }
}

pub struct NvmeDriver {
    regs_virt: u64,
    dstrd: u8,
    timeout_ms: u32,
    asq_phys: u64,
    acq_phys: u64,
    asq_tail: u16,
    acq_head: u16,
    acq_phase: bool,
    iosq_phys: u64,
    iocq_phys: u64,
    iosq_tail: u16,
    iocq_head: u16,
    iocq_phase: bool,
    io_sq_id: u16,
    io_cq_id: u16,
    dma_buf_phys: u64,
    nsid: u32,
    ns_sectors: u64,
    lbads: u8,
    base_lba: u64,
    bus: u8,
    dev: u8,
    func: u8,
    msi_vector: Option<u8>,
}

unsafe impl Send for NvmeDriver {}

impl NvmeDriver {
    fn mmio_read32(&self, off: u64) -> u32 {
        unsafe { (self.regs_virt as *mut u32).add((off / 4) as usize).read_volatile() }
    }
    fn mmio_write32(&self, off: u64, val: u32) {
        unsafe { (self.regs_virt as *mut u32).add((off / 4) as usize).write_volatile(val) }
    }
    fn mmio_read64(&self, off: u64) -> u64 {
        unsafe { (self.regs_virt as *mut u64).add((off / 8) as usize).read_volatile() }
    }
    fn mmio_write64(&self, off: u64, val: u64) {
        unsafe { (self.regs_virt as *mut u64).add((off / 8) as usize).write_volatile(val) }
    }

    fn sq_db(&self, qid: u16) -> *mut u32 {
        let stride = 4u64 << self.dstrd;
        (self.regs_virt + 0x1000 + (2 * qid as u64) * stride) as *mut u32
    }
    fn cq_db(&self, qid: u16) -> *mut u32 {
        let stride = 4u64 << self.dstrd;
        (self.regs_virt + 0x1000 + (2 * qid as u64 + 1) * stride) as *mut u32
    }

    fn ring_sq_db(&self, qid: u16, tail: u16) {
        fence(Ordering::SeqCst);
        unsafe { self.sq_db(qid).write_volatile(tail as u32) };
    }
    fn ring_cq_db(&self, qid: u16, head: u16) {
        fence(Ordering::SeqCst);
        unsafe { self.cq_db(qid).write_volatile(head as u32) };
    }

    fn wait_rdy(&self, target: u32) -> bool {
        for _ in 0..self.timeout_ms * 10 {
            if (self.mmio_read32(NVME_CSTS) & CSTS_RDY) == target {
                return true;
            }
            crate::hal::hlt_once();
        }
        false
    }

    fn alloc_contig(count: u16, entry_size: u16) -> (u64, u64) {
        let total = (count as u64) * (entry_size as u64);
        let npages = total.div_ceil(PAGE_SIZE_4K);
        let first = crate::memory::allocate_frame().unwrap_or(0);
        if first == 0 { return (0, 0); }
        for i in 1..npages {
            if crate::memory::allocate_frame().is_none() {
                for j in 0..i { crate::memory::free_frame(first + j * PAGE_SIZE_4K); }
                return (0, 0);
            }
        }
        (first, npages * PAGE_SIZE_4K)
    }

    fn free_contig(phys: u64, size: u64) {
        let npages = size.div_ceil(PAGE_SIZE_4K);
        for i in 0..npages { crate::memory::free_frame(phys + i * PAGE_SIZE_4K); }
    }

    fn zero_phys(phys: u64, size: u64) {
        let ptr = phys as *mut u32;
        for i in 0..(size as usize / 4) {
            unsafe { ptr.add(i).write_volatile(0u32) };
        }
        fence(Ordering::SeqCst);
    }

    /// Write 64-byte admin SQE to ASQ entry 0 using volatile u32 stores.
    /// NVMe SQE layout: DW0=CDW0, DW1=NSID, DW2-5=RSVD, DW6-7=PRP1, DW8-9=PRP2, DW10-15=CDW10-15
    unsafe fn poke_admin_sqe(asq: u64, cdw0: u32, nsid: u32, prp1: u64, cdw10: u32, cdw11: u32) {
        let p = asq as *mut u32;
        p.add(0).write_volatile(cdw0);
        p.add(1).write_volatile(nsid);
        p.add(2).write_volatile(0);
        p.add(3).write_volatile(0);
        p.add(4).write_volatile(0);
        p.add(5).write_volatile(0);
        p.add(6).write_volatile(prp1 as u32);
        p.add(7).write_volatile((prp1 >> 32) as u32);
        p.add(8).write_volatile(0);
        p.add(9).write_volatile(0);
        p.add(10).write_volatile(cdw10);
        p.add(11).write_volatile(cdw11);
        p.add(12).write_volatile(0);
        p.add(13).write_volatile(0);
        p.add(14).write_volatile(0);
        p.add(15).write_volatile(0);
        fence(Ordering::SeqCst);
    }

    unsafe fn poke_io_sqe(sq: u64, cdw0: u32, nsid: u32, prp1: u64, slba: u64, nlb: u16) {
        let p = sq as *mut u32;
        p.add(0).write_volatile(cdw0);
        p.add(1).write_volatile(nsid);
        p.add(2).write_volatile(0);
        p.add(3).write_volatile(0);
        p.add(4).write_volatile(0);
        p.add(5).write_volatile(0);
        p.add(6).write_volatile(prp1 as u32);
        p.add(7).write_volatile((prp1 >> 32) as u32);
        p.add(8).write_volatile(0);
        p.add(9).write_volatile(0);
        p.add(10).write_volatile(slba as u32);
        p.add(11).write_volatile((slba >> 32) as u32);
        p.add(12).write_volatile((nlb as u32).wrapping_sub(1));
        p.add(13).write_volatile(0);
        p.add(14).write_volatile(0);
        p.add(15).write_volatile(0);
        fence(Ordering::SeqCst);
    }

    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    fn admin_cmd_raw(asq: u64, acq: u64, sq_doorbell: *mut u32, acq_head: u16, _acq_phase: bool,
                     opcode: u8, nsid: u32, cdw10: u32, cdw11: u32, prp1: u64, _timeout_ms: u32,
                     asq_tail: &mut u16)
        -> Result<(u32, u16, u16, bool), u16>
    {
        let cid = *asq_tail;
        let cdw0 = (opcode as u32) | ((cid as u32) << 16);

        unsafe {
            // Write SQE to ASQ entry[asq_tail]
            let sq_entry = asq + (*asq_tail as u64) * SQE_SIZE as u64;
            Self::poke_admin_sqe(sq_entry, cdw0, nsid, prp1, cdw10, cdw11);

            // Ring doorbell: new tail = asq_tail + 1
            fence(Ordering::SeqCst);
            *asq_tail = (*asq_tail + 1) % ASQ_ENTRIES;
            sq_doorbell.write_volatile(*asq_tail as u32);

            // Poll CQE
            let cqe = acq + (acq_head as u64) * CQE_SIZE as u64;
            let cqe32 = cqe as *mut u32;
            for i in 0..100000000 {
                let w3 = cqe32.add(3).read_volatile();
                let cq_cid = (w3 & 0xFFFF) as u16; // CID in DW3 bits 15:0 (QEMU NVMe convention)
                if cq_cid == cid {
                    let w0 = cqe32.add(0).read_volatile();
                    let w1 = cqe32.add(1).read_volatile();
                    let w2 = cqe32.add(2).read_volatile();
                    let status_field = (w3 >> 16) as u16;
                    let phase = (status_field & 0x1) != 0;
                    let status = status_field >> 1;
                    serial_println!("[NVMe] CQE[{}]: {:08x} {:08x} {:08x} {:08x} cid={} phase={} status={}",
                        acq_head, w0, w1, w2, w3, cq_cid, phase, status);
                    let new_head = (acq_head + 1) % ACQ_ENTRIES;
                    return Ok((w0, status, new_head, phase));
                }
                // Yield to QEMU via port 0x80 to let NVMe emulation run (TCG)
                if i & 0x7FFF == 0 {
                    crate::hal::raw::raw_debug_port_write(0u8);
                }
            }
            let cqe = acq + (acq_head as u64) * CQE_SIZE as u64;
            let cqe32 = cqe as *mut u32;
            // Dump ACQ
            let dump_w0 = cqe32.add(0).read_volatile();
            let dump_w1 = cqe32.add(1).read_volatile();
            let dump_w2 = cqe32.add(2).read_volatile();
            let dump_w3 = cqe32.add(3).read_volatile();
            serial_println!("[NVMe] ACQ[0]: {:08x} {:08x} {:08x} {:08x}",
                dump_w0, dump_w1, dump_w2, dump_w3);
            for acq_i in 0..4 {
                let acq_pos = acq as *mut u32;
                let off = acq_i * 4;
                serial_println!("[NVMe] ACQ[{}]: {:08x} {:08x} {:08x} {:08x}",
                    acq_i,
                    acq_pos.add(off).read_volatile(),
                    acq_pos.add(off + 1).read_volatile(),
                    acq_pos.add(off + 2).read_volatile() ,
                    acq_pos.add(off + 3).read_volatile());
            }
            // Dump ASQ entry 0 and 1
            let sqe = asq as *mut u32;
            serial_println!("[NVMe] ASQ[0]: {:08x} {:08x} {:08x} {:08x} | {:08x} {:08x} {:08x} {:08x} | {:08x} {:08x} {:08x} {:08x} | {:08x} {:08x} {:08x} {:08x}",
                sqe.add(0).read_volatile(), sqe.add(1).read_volatile(),
                sqe.add(2).read_volatile(), sqe.add(3).read_volatile(),
                sqe.add(4).read_volatile(), sqe.add(5).read_volatile(),
                sqe.add(6).read_volatile(), sqe.add(7).read_volatile(),
                sqe.add(8).read_volatile(), sqe.add(9).read_volatile(),
                sqe.add(10).read_volatile(), sqe.add(11).read_volatile(),
                sqe.add(12).read_volatile(), sqe.add(13).read_volatile(),
                sqe.add(14).read_volatile(), sqe.add(15).read_volatile());
            let sqe1 = (asq + 64) as *mut u32;
            serial_println!("[NVMe] ASQ[1]: {:08x} {:08x} {:08x} {:08x} | {:08x} {:08x} {:08x} {:08x} | {:08x} {:08x} {:08x} {:08x} | {:08x} {:08x} {:08x} {:08x}",
                sqe1.add(0).read_volatile(), sqe1.add(1).read_volatile(),
                sqe1.add(2).read_volatile(), sqe1.add(3).read_volatile(),
                sqe1.add(4).read_volatile(), sqe1.add(5).read_volatile(),
                sqe1.add(6).read_volatile(), sqe1.add(7).read_volatile(),
                sqe1.add(8).read_volatile(), sqe1.add(9).read_volatile(),
                sqe1.add(10).read_volatile(), sqe1.add(11).read_volatile(),
                sqe1.add(12).read_volatile(), sqe1.add(13).read_volatile(),
                sqe1.add(14).read_volatile(), sqe1.add(15).read_volatile());
            let sqe2 = (asq + 128) as *mut u32;
            serial_println!("[NVMe] ASQ[2]: {:08x} {:08x} {:08x} {:08x} | {:08x} {:08x} {:08x} {:08x} | {:08x} {:08x} {:08x} {:08x} | {:08x} {:08x} {:08x} {:08x}",
                sqe2.add(0).read_volatile(), sqe2.add(1).read_volatile(),
                sqe2.add(2).read_volatile(), sqe2.add(3).read_volatile(),
                sqe2.add(4).read_volatile(), sqe2.add(5).read_volatile(),
                sqe2.add(6).read_volatile(), sqe2.add(7).read_volatile(),
                sqe2.add(8).read_volatile(), sqe2.add(9).read_volatile() ,
                sqe2.add(10).read_volatile(), sqe2.add(11).read_volatile(),
                sqe2.add(12).read_volatile(), sqe2.add(13).read_volatile(),
                sqe2.add(14).read_volatile(), sqe2.add(15).read_volatile());
            Err(0xFFFF)
        }
    }

    fn admin_cmd(&mut self, opcode: u8, nsid: u32, cdw10: u32, cdw11: u32, prp1: u64) -> Result<u32, u16> {
        let doorbell = self.sq_db(0);
        match Self::admin_cmd_raw(self.asq_phys, self.acq_phys, doorbell,
                                  self.acq_head, self.acq_phase,
                                  opcode, nsid, cdw10, cdw11, prp1, self.timeout_ms,
                                  &mut self.asq_tail)
        {
            Ok((cdw0, status, new_head, new_phase)) => {
                self.acq_head = new_head;
                self.acq_phase = new_phase;
                self.ring_cq_db(0, self.acq_head);
                if status != 0 {
                    serial_println!("[NVMe] CMD err: op=0x{:x} st=0x{:04x}", opcode, status);
                    return Err(status);
                }
                Ok(cdw0)
            }
            Err(e) => {
                serial_println!("[NVMe] CMD timeout: op=0x{:x} nsid={} cdw10={} prp1=0x{:x}",
                    opcode, nsid, cdw10, prp1);
                Err(e)
            }
        }
    }

    pub fn probe_all() -> [Option<NvmeDriver>; 1] {
        let info = match find_nvme_controller() {
            Some(i) => i,
            None => return [None,],
        };

        serial_println!("[NVMe] Controller found: bar0=0x{:016X}", info.bar0_phys);
        nvme_enable(&info);
        let bar0_phys = info.bar0_phys;

        crate::arch::x64::paging::split_2mb_page(NVME_MMIO_VIRT).ok();
        let bar_size = 0x2000u64 + 131072u64;
        use x86_64::structures::paging::PageTableFlags;
        let mmio_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_CACHE;
        if !crate::arch::x64::paging::map_mmio_4k(NVME_MMIO_VIRT, bar0_phys, bar_size, mmio_flags) {
            serial_println!("[NVMe] Failed to map BAR");
            return [None,];
        }
        let bar = NVME_MMIO_VIRT;

        let cap = unsafe { (bar as *const u64).read_volatile() };
        let dstrd = ((cap >> 32) & 0xF) as u8;
        let to = ((cap >> 24) & 0xFF) as u32;
        let timeout_ms = core::cmp::max(to * 500, 2000);
        serial_println!("[NVMe] CAP=0x{:016X} DSTRD={} TO={}", cap, dstrd, to);

        if (cap & (1 << 43)) == 0 {
            serial_println!("[NVMe] NVM CSS not supported");
            return [None,];
        }

        // Reset
        serial_println!("[NVMe] Resetting...");
        unsafe { (bar as *mut u32).add(0x14 / 4).write_volatile(0u32) };
        fence(Ordering::SeqCst);
        crate::hal::hlt_once();
        if !Self::wait_rdy_raw(bar, timeout_ms, 0) {
            serial_println!("[NVMe] Reset timeout");
            return [None,];
        }

        // Allocate admin queues + DMA buffer
        let (asq_phys, asq_sz) = Self::alloc_contig(ASQ_ENTRIES, SQE_SIZE);
        if asq_phys == 0 { return [None,]; }
        let (acq_phys, acq_sz) = Self::alloc_contig(ACQ_ENTRIES, CQE_SIZE);
        if acq_phys == 0 { Self::free_contig(asq_phys, asq_sz); return [None,]; }
        Self::zero_phys(asq_phys, asq_sz);
        Self::zero_phys(acq_phys, acq_sz);
        // Use DMA buffer at a fixed address outside the kernel heap range (0x1000000-0x2000000)
        // to avoid potential conflicts with the linked_list allocator. 3MB (0x300000) is safe.
        let dma_phys = 0x300000u64;
        let dma_sz = 4096;
        Self::zero_phys(dma_phys, dma_sz);

        // Verify ACQ zeroed
        let test_ptr = acq_phys as *mut u32;
        let test_dw0 = unsafe { test_ptr.add(0).read_volatile() };
        let test_dw2 = unsafe { test_ptr.add(2).read_volatile() };
        let test_dw3 = unsafe { test_ptr.add(3).read_volatile() };
        serial_println!("[NVMe] ACQ[0] after zero: {:08x} {:08x} {:08x} {:08x}",
            test_dw0, unsafe { test_ptr.add(1).read_volatile() }, test_dw2, test_dw3);

        // Program registers
        unsafe {
            (bar as *mut u32).add(0x24 / 4).write_volatile(
                ((ACQ_ENTRIES as u32 - 1) << 16) | (ASQ_ENTRIES as u32 - 1));
            (bar as *mut u64).add(0x28 / 8).write_volatile(asq_phys);
            (bar as *mut u64).add(0x30 / 8).write_volatile(acq_phys);
        }
        fence(Ordering::SeqCst);
        crate::hal::hlt_once();

        // Enable controller
        // CC bits: [0]=EN, [3:2]=CSS=NVM, [5:4]=IOCQES, [7:6]=IOSQES, [19:16]=MPS
        // Admin SQ entry=64B, Admin CQ entry=16B (spec-mandated fixed sizes)
        // IOSQES=6 (log2(64)), IOCQES=4 (log2(16))
        let cc_val = 1u32 | (6 << 16) | (4 << 20);
        unsafe { (bar as *mut u32).add(0x14 / 4).write_volatile(cc_val) };
        fence(Ordering::SeqCst);
        crate::hal::hlt_once();
        if !Self::wait_rdy_raw(bar, timeout_ms, CSTS_RDY) {
            serial_println!("[NVMe] Enable timeout");
            Self::free_contig(asq_phys, asq_sz); Self::free_contig(acq_phys, acq_sz); Self::free_contig(dma_phys, dma_sz);
            return [None,];
        }
        let cc_after = unsafe { (bar as *const u32).add(0x14 / 4).read_volatile() };
        serial_println!("[NVMe] CC after enable: 0x{:08x} (expected 0x{:08x})", cc_after, cc_val);
        serial_println!("[NVMe] Enabled ASQ=0x{:x} ACQ=0x{:x} DMA=0x{:x}", asq_phys, acq_phys, dma_phys);

        let mut drv = NvmeDriver {
            regs_virt: bar, dstrd, timeout_ms,
            asq_phys, acq_phys, asq_tail: 0, acq_head: 0, acq_phase: false,
            iosq_phys: 0, iocq_phys: 0,
            iosq_tail: 0, iocq_head: 0, iocq_phase: true,
            io_sq_id: 1, io_cq_id: 1,
            dma_buf_phys: dma_phys,
            nsid: 0, ns_sectors: 0, lbads: 9, base_lba: 0,
            bus: info.bus, dev: info.device, func: info.func,
            msi_vector: None,
        };

        // Identify Controller
        serial_println!("[NVMe] Identify controller...");
        Self::zero_phys(dma_phys, 4096);
        if drv.admin_cmd(ADMIN_IDENTIFY, 0, 1, 0, dma_phys).is_err() { // CNS=1: Identify Controller
            serial_println!("[NVMe] Identify controller failed");
            return [None,];
        }
        // Dump ASQ after SQE write
        let asq8 = drv.asq_phys as *const u8;
        serial_println!("[NVMe] ASQ[0..64]: {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X}",
            unsafe { asq8.add(0).read_volatile() }, unsafe { asq8.add(1).read_volatile() },
            unsafe { asq8.add(2).read_volatile() }, unsafe { asq8.add(3).read_volatile() },
            unsafe { asq8.add(4).read_volatile() }, unsafe { asq8.add(5).read_volatile() },
            unsafe { asq8.add(6).read_volatile() }, unsafe { asq8.add(7).read_volatile() },
            unsafe { asq8.add(8).read_volatile() }, unsafe { asq8.add(9).read_volatile() },
            unsafe { asq8.add(10).read_volatile() }, unsafe { asq8.add(11).read_volatile() },
            unsafe { asq8.add(12).read_volatile() }, unsafe { asq8.add(13).read_volatile() },
            unsafe { asq8.add(14).read_volatile() }, unsafe { asq8.add(15).read_volatile() },
            unsafe { asq8.add(16).read_volatile() }, unsafe { asq8.add(17).read_volatile() },
            unsafe { asq8.add(18).read_volatile() }, unsafe { asq8.add(19).read_volatile() },
            unsafe { asq8.add(20).read_volatile() }, unsafe { asq8.add(21).read_volatile() },
            unsafe { asq8.add(22).read_volatile() }, unsafe { asq8.add(23).read_volatile() },
            unsafe { asq8.add(24).read_volatile() }, unsafe { asq8.add(25).read_volatile() },
            unsafe { asq8.add(26).read_volatile() }, unsafe { asq8.add(27).read_volatile() },
            unsafe { asq8.add(28).read_volatile() }, unsafe { asq8.add(29).read_volatile() },
            unsafe { asq8.add(30).read_volatile() }, unsafe { asq8.add(31).read_volatile() },
            unsafe { asq8.add(32).read_volatile() }, unsafe { asq8.add(33).read_volatile() },
            unsafe { asq8.add(34).read_volatile() }, unsafe { asq8.add(35).read_volatile() },
            unsafe { asq8.add(36).read_volatile() }, unsafe { asq8.add(37).read_volatile() },
            unsafe { asq8.add(38).read_volatile() }, unsafe { asq8.add(39).read_volatile() },
            unsafe { asq8.add(40).read_volatile() }, unsafe { asq8.add(41).read_volatile() },
            unsafe { asq8.add(42).read_volatile() }, unsafe { asq8.add(43).read_volatile() },
            unsafe { asq8.add(44).read_volatile() }, unsafe { asq8.add(45).read_volatile() },
            unsafe { asq8.add(46).read_volatile() }, unsafe { asq8.add(47).read_volatile() },
            unsafe { asq8.add(48).read_volatile() }, unsafe { asq8.add(49).read_volatile() },
            unsafe { asq8.add(50).read_volatile() }, unsafe { asq8.add(51).read_volatile() },
            unsafe { asq8.add(52).read_volatile() }, unsafe { asq8.add(53).read_volatile() },
            unsafe { asq8.add(54).read_volatile() }, unsafe { asq8.add(55).read_volatile() },
            unsafe { asq8.add(56).read_volatile() }, unsafe { asq8.add(57).read_volatile() },
            unsafe { asq8.add(58).read_volatile() }, unsafe { asq8.add(59).read_volatile() },
            unsafe { asq8.add(60).read_volatile() }, unsafe { asq8.add(61).read_volatile() },
            unsafe { asq8.add(62).read_volatile() }, unsafe { asq8.add(63).read_volatile() });

        let ctrl = unsafe { &*(dma_phys as *const IdentifyCtrl) };
        let nn = ctrl.nn();
        let sn = core::str::from_utf8(&ctrl.serial()[..20]).unwrap_or("?");
        let mn = core::str::from_utf8(&ctrl.model()[..40]).unwrap_or("?");
        // Dump DMA[0-64] header + DMA[384-400] around NN field
        let dma8 = dma_phys as *const u8;
        serial_println!("[NVMe] {} {} (ns={})", sn.trim_end(), mn.trim_end(), nn);
        serial_println!("[NVMe] DMA[0..64]: {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X}",
            unsafe { dma8.add(0).read_volatile() }, unsafe { dma8.add(1).read_volatile() },
            unsafe { dma8.add(2).read_volatile() }, unsafe { dma8.add(3).read_volatile() },
            unsafe { dma8.add(4).read_volatile() }, unsafe { dma8.add(5).read_volatile() },
            unsafe { dma8.add(6).read_volatile() }, unsafe { dma8.add(7).read_volatile() },
            unsafe { dma8.add(8).read_volatile() }, unsafe { dma8.add(9).read_volatile() },
            unsafe { dma8.add(10).read_volatile() }, unsafe { dma8.add(11).read_volatile() },
            unsafe { dma8.add(12).read_volatile() }, unsafe { dma8.add(13).read_volatile() },
            unsafe { dma8.add(14).read_volatile() }, unsafe { dma8.add(15).read_volatile() },
            unsafe { dma8.add(16).read_volatile() }, unsafe { dma8.add(17).read_volatile() },
            unsafe { dma8.add(18).read_volatile() }, unsafe { dma8.add(19).read_volatile() },
            unsafe { dma8.add(20).read_volatile() }, unsafe { dma8.add(21).read_volatile() },
            unsafe { dma8.add(22).read_volatile() }, unsafe { dma8.add(23).read_volatile() },
            unsafe { dma8.add(24).read_volatile() }, unsafe { dma8.add(25).read_volatile() },
            unsafe { dma8.add(26).read_volatile() }, unsafe { dma8.add(27).read_volatile() },
            unsafe { dma8.add(28).read_volatile() }, unsafe { dma8.add(29).read_volatile() },
            unsafe { dma8.add(30).read_volatile() }, unsafe { dma8.add(31).read_volatile() },
            unsafe { dma8.add(32).read_volatile() }, unsafe { dma8.add(33).read_volatile() },
            unsafe { dma8.add(34).read_volatile() }, unsafe { dma8.add(35).read_volatile() },
            unsafe { dma8.add(36).read_volatile() }, unsafe { dma8.add(37).read_volatile() },
            unsafe { dma8.add(38).read_volatile() }, unsafe { dma8.add(39).read_volatile() },
            unsafe { dma8.add(40).read_volatile() }, unsafe { dma8.add(41).read_volatile() },
            unsafe { dma8.add(42).read_volatile() }, unsafe { dma8.add(43).read_volatile() },
            unsafe { dma8.add(44).read_volatile() }, unsafe { dma8.add(45).read_volatile() },
            unsafe { dma8.add(46).read_volatile() }, unsafe { dma8.add(47).read_volatile() },
            unsafe { dma8.add(48).read_volatile() }, unsafe { dma8.add(49).read_volatile() },
            unsafe { dma8.add(50).read_volatile() }, unsafe { dma8.add(51).read_volatile() },
            unsafe { dma8.add(52).read_volatile() }, unsafe { dma8.add(53).read_volatile() },
            unsafe { dma8.add(54).read_volatile() }, unsafe { dma8.add(55).read_volatile() },
            unsafe { dma8.add(56).read_volatile() }, unsafe { dma8.add(57).read_volatile() },
            unsafe { dma8.add(58).read_volatile() }, unsafe { dma8.add(59).read_volatile() },
            unsafe { dma8.add(60).read_volatile() }, unsafe { dma8.add(61).read_volatile() },
            unsafe { dma8.add(62).read_volatile() }, unsafe { dma8.add(63).read_volatile() });
        // Dump DMA[380-400] around NN field
        serial_println!("[NVMe] DMA[380-399]: {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X}",
            unsafe { dma8.add(380).read_volatile() }, unsafe { dma8.add(381).read_volatile() },
            unsafe { dma8.add(382).read_volatile() }, unsafe { dma8.add(383).read_volatile() },
            unsafe { dma8.add(384).read_volatile() }, unsafe { dma8.add(385).read_volatile() },
            unsafe { dma8.add(386).read_volatile() }, unsafe { dma8.add(387).read_volatile() },
            unsafe { dma8.add(388).read_volatile() }, unsafe { dma8.add(389).read_volatile() },
            unsafe { dma8.add(390).read_volatile() }, unsafe { dma8.add(391).read_volatile() },
            unsafe { dma8.add(392).read_volatile() }, unsafe { dma8.add(393).read_volatile() },
            unsafe { dma8.add(394).read_volatile() }, unsafe { dma8.add(395).read_volatile() },
            unsafe { dma8.add(396).read_volatile() }, unsafe { dma8.add(397).read_volatile() },
            unsafe { dma8.add(398).read_volatile() }, unsafe { dma8.add(399).read_volatile() });
        // Try CNS=2: Active Namespace ID List
        Self::zero_phys(dma_phys, 4096);
        serial_println!("[NVMe] Identify active NSID list (CNS=2)...");
        if drv.admin_cmd(ADMIN_IDENTIFY, 0, 2, 0, dma_phys).is_ok() {
            let nsid0 = unsafe { (dma_phys as *const u32).read_volatile() };
            serial_println!("[NVMe] Active NSID[0]={}", nsid0);
        } else {
            serial_println!("[NVMe] CNS=2 failed");
        }
        // Dump DMA[64-128] around FR and RAB
        serial_println!("[NVMe] DMA[64-95]: {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X}",
            unsafe { dma8.add(64).read_volatile() }, unsafe { dma8.add(65).read_volatile() },
            unsafe { dma8.add(66).read_volatile() }, unsafe { dma8.add(67).read_volatile() },
            unsafe { dma8.add(68).read_volatile() }, unsafe { dma8.add(69).read_volatile() },
            unsafe { dma8.add(70).read_volatile() }, unsafe { dma8.add(71).read_volatile() },
            unsafe { dma8.add(72).read_volatile() }, unsafe { dma8.add(73).read_volatile() },
            unsafe { dma8.add(74).read_volatile() }, unsafe { dma8.add(75).read_volatile() },
            unsafe { dma8.add(76).read_volatile() }, unsafe { dma8.add(77).read_volatile() },
            unsafe { dma8.add(78).read_volatile() }, unsafe { dma8.add(79).read_volatile() },
            unsafe { dma8.add(80).read_volatile() }, unsafe { dma8.add(81).read_volatile() },
            unsafe { dma8.add(82).read_volatile() }, unsafe { dma8.add(83).read_volatile() },
            unsafe { dma8.add(84).read_volatile() }, unsafe { dma8.add(85).read_volatile() },
            unsafe { dma8.add(86).read_volatile() }, unsafe { dma8.add(87).read_volatile() },
            unsafe { dma8.add(88).read_volatile() }, unsafe { dma8.add(89).read_volatile() },
            unsafe { dma8.add(90).read_volatile() }, unsafe { dma8.add(91).read_volatile() },
            unsafe { dma8.add(92).read_volatile() }, unsafe { dma8.add(93).read_volatile() },
            unsafe { dma8.add(94).read_volatile() }, unsafe { dma8.add(95).read_volatile() });
        // Dump DMA[200-264] around OACS and other fields
        serial_println!("[NVMe] DMA[200-231]: {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X} {:02X}{:02X}{:02X}{:02X}",
            unsafe { dma8.add(200).read_volatile() }, unsafe { dma8.add(201).read_volatile() },
            unsafe { dma8.add(202).read_volatile() }, unsafe { dma8.add(203).read_volatile() },
            unsafe { dma8.add(204).read_volatile() }, unsafe { dma8.add(205).read_volatile() },
            unsafe { dma8.add(206).read_volatile() }, unsafe { dma8.add(207).read_volatile() },
            unsafe { dma8.add(208).read_volatile() }, unsafe { dma8.add(209).read_volatile() },
            unsafe { dma8.add(210).read_volatile() }, unsafe { dma8.add(211).read_volatile() },
            unsafe { dma8.add(212).read_volatile() }, unsafe { dma8.add(213).read_volatile() },
            unsafe { dma8.add(214).read_volatile() }, unsafe { dma8.add(215).read_volatile() },
            unsafe { dma8.add(216).read_volatile() }, unsafe { dma8.add(217).read_volatile() },
            unsafe { dma8.add(218).read_volatile() }, unsafe { dma8.add(219).read_volatile() },
            unsafe { dma8.add(220).read_volatile() }, unsafe { dma8.add(221).read_volatile() },
            unsafe { dma8.add(222).read_volatile() }, unsafe { dma8.add(223).read_volatile() },
            unsafe { dma8.add(224).read_volatile() }, unsafe { dma8.add(225).read_volatile() },
            unsafe { dma8.add(226).read_volatile() }, unsafe { dma8.add(227).read_volatile() },
            unsafe { dma8.add(228).read_volatile() }, unsafe { dma8.add(229).read_volatile() },
            unsafe { dma8.add(230).read_volatile() }, unsafe { dma8.add(231).read_volatile() });

        let ns_to_try = if nn > 0 { nn } else { 1 };

        // Identify Namespace
        Self::zero_phys(dma_phys, 4096);
        serial_println!("[NVMe] Identify ns {}...", ns_to_try);
        if drv.admin_cmd(ADMIN_IDENTIFY, ns_to_try, 0, 0, dma_phys).is_err() {
            // Dump ASQ[2] after failure
            let sq_dbg = drv.asq_phys as *mut u32;
            serial_println!("[NVMe] ASQ[2]: {:08x} {:08x} {:08x} {:08x} | {:08x} {:08x} {:08x} {:08x} | {:08x} {:08x} {:08x} {:08x} | {:08x} {:08x} {:08x} {:08x}",
                unsafe { sq_dbg.add(32).read_volatile() }, unsafe { sq_dbg.add(33).read_volatile() },
                unsafe { sq_dbg.add(32+2).read_volatile() }, unsafe { sq_dbg.add(32+3).read_volatile() },
                unsafe { sq_dbg.add(32+4).read_volatile() }, unsafe { sq_dbg.add(32+5).read_volatile() },
                unsafe { sq_dbg.add(32+6).read_volatile() }, unsafe { sq_dbg.add(32+7).read_volatile() },
                unsafe { sq_dbg.add(32+8).read_volatile() }, unsafe { sq_dbg.add(32+9).read_volatile() },
                unsafe { sq_dbg.add(32+10).read_volatile() }, unsafe { sq_dbg.add(32+11).read_volatile() },
                unsafe { sq_dbg.add(32+12).read_volatile() }, unsafe { sq_dbg.add(32+13).read_volatile() },
                unsafe { sq_dbg.add(32+14).read_volatile() }, unsafe { sq_dbg.add(32+15).read_volatile() });
            serial_println!("[NVMe] Identify ns {} failed", ns_to_try);
            return [None,];
        }
        let ns = unsafe { &*(dma_phys as *const IdentifyNs) };
        let nsze = ns.nsze();
        let flbas = ns.flbas();
        let lbads = ns.lbads();
        // Dump LBAF entries
        serial_println!("[NVMe] NS data: nsze={} flbas={} lbads={} ncap={} nuse={}",
            nsze, flbas, lbads, u64::from_le_bytes(ns.data[8..16].try_into().unwrap()),
            u64::from_le_bytes(ns.data[16..24].try_into().unwrap()));
        serial_println!("[NVMe] NS data: nsze={} flbas={} lbads={} ncap={} nuse={}",
            nsze, flbas, lbads, u64::from_le_bytes(ns.data[8..16].try_into().unwrap()),
            u64::from_le_bytes(ns.data[16..24].try_into().unwrap()));
        for i in 0..4 {
            let off = 128 + i * 4;
            serial_println!("[NVMe] LBAF[{}]: {:02X} {:02X} {:02X} {:02X}", i,
                ns.data[off], ns.data[off+1], ns.data[off+2], ns.data[off+3]);
        }
        serial_println!("[NVMe] RAW[27]={:02X} RAW[26]={:02X} RAW[28]={:02X} RAW[29]={:02X} RAW[30]={:02X}",
            ns.data[27], ns.data[26], ns.data[28], ns.data[29], ns.data[30]);
        serial_println!("[NVMe] NS {}: {} sectors, LBA data size={} bytes", ns_to_try, nsze, 1u64 << lbads);

        // Create I/O Completion Queue
        let io_cq_id = 1;
        let (iocq_phys, iocq_sz) = Self::alloc_contig(IOCQ_ENTRIES, CQE_SIZE);
        if iocq_phys == 0 { return [None,]; }
        Self::zero_phys(iocq_phys, iocq_sz);

        // QEMU expects cqid in CDW10[15:0], qsize in CDW10[31:16] (swapped vs spec)
        let cq_cdw10 = (io_cq_id as u32) | ((IOCQ_ENTRIES as u32 - 1) << 16);
        let cq_cdw11 = 1u32; // PC=1
        serial_println!("[NVMe] Create IOCQ {}...", io_cq_id);
        if drv.admin_cmd(ADMIN_CREATE_IO_CQ, 0, cq_cdw10, cq_cdw11, iocq_phys).is_err() {
            serial_println!("[NVMe] Create IOCQ failed");
            Self::free_contig(iocq_phys, iocq_sz);
            return [None,];
        }
        drv.iocq_phys = iocq_phys;
        drv.io_cq_id = io_cq_id;

        // Create I/O Submission Queue
        let io_sq_id = 1;
        let (iosq_phys, iosq_sz) = Self::alloc_contig(IOSQ_ENTRIES, SQE_SIZE);
        if iosq_phys == 0 { return [None,]; }
        Self::zero_phys(iosq_phys, iosq_sz);

        // QEMU expects sqid in CDW10[15:0], qsize in CDW10[31:16] (swapped vs spec)
        let sq_cdw10 = (io_sq_id as u32) | ((IOSQ_ENTRIES as u32 - 1) << 16);
        let sq_cdw11 = (io_cq_id as u32) << 16 | 1; // PC=1, CQID=1
        serial_println!("[NVMe] Create IOSQ {}...", io_sq_id);
        if drv.admin_cmd(ADMIN_CREATE_IO_SQ, 0, sq_cdw10, sq_cdw11, iosq_phys).is_err() {
            serial_println!("[NVMe] Create IOSQ failed");
            drv.admin_cmd(ADMIN_DELETE_IO_CQ, 0, io_cq_id as u32, 0, 0).ok();
            Self::free_contig(iocq_phys, iocq_sz);
            Self::free_contig(iosq_phys, iosq_sz);
            return [None,];
        }
        drv.iosq_phys = iosq_phys;
        drv.io_sq_id = io_sq_id;
        drv.nsid = ns_to_try;
        drv.ns_sectors = nsze;
        drv.lbads = lbads;

        serial_println!("[NVMe] Ready: {} sectors x {}B, QID {}→{}",
            nsze, 1u64 << lbads, io_sq_id, io_cq_id);

        if let Ok(vec) = crate::interrupts::msi::msi_request(drv.bus, drv.dev, drv.func, nvme_irq_handler) {
            drv.msi_vector = Some(vec);
            serial_println!("[NVMe] MSI vector {} configured", vec);
        } else {
            serial_println!("[NVMe] Warning: Failed to configure MSI");
        }

        [Some(drv)]
    }

    fn wait_rdy_raw(bar: u64, timeout_ms: u32, target: u32) -> bool {
        for _ in 0..timeout_ms * 10 {
            if (unsafe { (bar as *mut u32).add(0x1C / 4).read_volatile() } & CSTS_RDY) == target {
                return true;
            }
            crate::hal::hlt_once();
        }
        false
    }

    fn sector_size(&self) -> u64 { 1u64 << self.lbads }

    pub fn read_sectors(&mut self, lba: u64, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        let lba = lba + self.base_lba;
        let nlb = count as u16;
        let sz = self.sector_size();
        let total = (count as u64) * sz;
        if count > 8 || (buf.len() as u64) < total || total > PAGE_SIZE_4K { return Err(()); }
        let prp1 = self.dma_buf_phys;
        self.io_cmd(IO_READ, lba, nlb, prp1).map_err(|_| ())?;
        unsafe { core::ptr::copy_nonoverlapping(prp1 as *const u8, buf.as_mut_ptr(), total as usize); }
        Ok(())
    }

    pub fn write_sectors(&mut self, lba: u64, count: u8, buf: &[u8]) -> Result<(), ()> {
        let lba = lba + self.base_lba;
        let nlb = count as u16;
        let sz = self.sector_size();
        let total = (count as u64) * sz;
        if count > 8 || (buf.len() as u64) < total || total > PAGE_SIZE_4K { return Err(()); }
        let prp1 = self.dma_buf_phys;
        unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), prp1 as *mut u8, total as usize); }
        self.io_cmd(IO_WRITE, lba, nlb, prp1).map_err(|_| ())
    }

    pub fn set_base_lba(&mut self, lba: u32) { self.base_lba = lba as u64; }
    pub fn base_lba(&self) -> u32 { self.base_lba as u32 }

    pub fn read_sector(&mut self, lba: u32) -> Result<[u8; 512], ()> {
        let mut buf = [0u8; 512];
        let sz = self.sector_size();
        if !(512..=4096).contains(&sz) { return Err(()); }
        let total = (sz as usize).max(512);
        let block_size = (1u64 << self.lbads) as u16;
        let nlb = (total as u16).div_ceil(block_size);
        let prp1 = self.dma_buf_phys;
        self.io_cmd(IO_READ, lba as u64 + self.base_lba, nlb, prp1).map_err(|_| ())?;
        unsafe { core::ptr::copy_nonoverlapping(prp1 as *const u8, buf.as_mut_ptr(), 512usize.min(total)); }
        Ok(buf)
    }

    pub fn write_sector(&mut self, lba: u32, data: &[u8; 512]) -> Result<(), ()> {
        let sz = self.sector_size();
        if !(512..=4096).contains(&sz) { return Err(()); }
        let total = (sz as usize).max(512);
        let block_size = (1u64 << self.lbads) as u16;
        let nlb = (total as u16).div_ceil(block_size);
        let prp1 = self.dma_buf_phys;
        unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), prp1 as *mut u8, 512); }
        self.io_cmd(IO_WRITE, lba as u64 + self.base_lba, nlb, prp1).map_err(|_| ())
    }

    pub fn num_sectors(&self) -> Option<u64> { Some(self.ns_sectors) }

    fn io_cmd(&mut self, opcode: u8, slba: u64, nlb: u16, prp1: u64) -> Result<(), u16> {
        let idx = self.iosq_tail;
        let cdw0 = (opcode as u32) | ((idx as u32) << 16);
        let sqe_addr = self.iosq_phys + (idx as u64) * SQE_SIZE as u64;
        unsafe {
            Self::poke_io_sqe(sqe_addr, cdw0, self.nsid, prp1, slba, nlb);
        }
        self.iosq_tail = (self.iosq_tail + 1) % IOSQ_ENTRIES;
        self.ring_sq_db(self.io_sq_id, self.iosq_tail);

        let cqe = self.iocq_phys + (self.iocq_head as u64) * CQE_SIZE as u64;
        let cqe32 = cqe as *mut u32;
        for _iter in 0..self.timeout_ms * 10 {
            let w3 = unsafe { cqe32.add(3).read_volatile() };
            let status_field = (w3 >> 16) as u16;
            if ((status_field & 0x1) != 0) == self.iocq_phase && (w3 & 0xFFFF) as u16 == idx {
                let status = status_field >> 1;
                self.iocq_head = (self.iocq_head + 1) % IOCQ_ENTRIES;
                if self.iocq_head == 0 { self.iocq_phase = !self.iocq_phase; }
                self.ring_cq_db(self.io_cq_id, self.iocq_head);
                if status != 0 { return Err(status); }
                return Ok(());
            }
            crate::hal::hlt_once();
        }
        serial_println!("[NVMe] IOCQ timeout op={} head={} idx={}", opcode, self.iocq_head, idx);
        Err(0xFFFF)
    }
}

impl Drop for NvmeDriver {
    fn drop(&mut self) {
        // Try to delete I/O queues
        if self.iosq_phys != 0 && self.io_sq_id != 0 {
            self.admin_cmd(ADMIN_DELETE_IO_SQ, 0, self.io_sq_id as u32, 0, 0).ok();
        }
        if self.iocq_phys != 0 && self.io_cq_id != 0 {
            self.admin_cmd(ADMIN_DELETE_IO_CQ, 0, self.io_cq_id as u32, 0, 0).ok();
        }

        // Disable controller
        let cc = self.mmio_read32(NVME_CC);
        self.mmio_write32(NVME_CC, cc & !CC_EN);
        self.wait_rdy(0);

        // Free resources
        if self.asq_phys != 0 { Self::free_contig(self.asq_phys, ASQ_ENTRIES as u64 * SQE_SIZE as u64); }
        if self.acq_phys != 0 { Self::free_contig(self.acq_phys, ACQ_ENTRIES as u64 * CQE_SIZE as u64); }
        if self.iosq_phys != 0 { Self::free_contig(self.iosq_phys, IOSQ_ENTRIES as u64 * SQE_SIZE as u64); }
        if self.iocq_phys != 0 { Self::free_contig(self.iocq_phys, IOCQ_ENTRIES as u64 * CQE_SIZE as u64); }
        if self.dma_buf_phys != 0 { Self::free_contig(self.dma_buf_phys, PAGE_SIZE_4K); }

        if let Some(vec) = self.msi_vector {
            crate::interrupts::msi::msi_release(self.bus, self.dev, self.func, vec);
        }
    }
}

fn nvme_irq_handler(vector: u8) {
    // Basic MSI handler for NVMe.
    // Right now the driver polls synchronously, so we just acknowledge the interrupt.
    crate::serial_println!("[NVMe] MSI interrupt fired on vector {}", vector);
}
