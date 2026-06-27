#![allow(dead_code)]

use core::sync::atomic::{fence, Ordering};
use core::alloc::Layout;
use alloc::alloc::alloc_zeroed;

use crate::drivers::block::BlockDevice;
use crate::drivers::pci::{pci_config_read_dword, pci_config_read_word, pci_config_write_word};
use crate::irp::{self, IrpId, IrpOp, IrpTagMap, NCQ_MAX_TAGS};
use crate::serial_println;

/// Saved AHCI port info so we can reclaim the port after NEM AHCI driver overrides it.
/// Stores (abar, port, clb, fb) — the DMA buffer addresses needed to restore the port.
static BOOT_AHCI_INFO: spin::Mutex<Option<(u64, usize, u32, u32)>> = spin::Mutex::new(None);

const MAX_PORTS: usize = 2;
const MAX_CMD_SLOTS: usize = 32;
const MAX_PRD_ENTRIES: usize = 8;
const DMA_BUF_SIZE: usize = 4096;

const HBA_GHC: u64 = 0x04;
const HBA_PI: u64 = 0x0C;

const HBA_GHC_AE: u32 = 0x8000_0000;
const HBA_GHC_HR: u32 = 0x0000_0001;

const PORT_STRIDE: u64 = 0x80;
const PORT_REG_BASE: u64 = 0x100;

const PORT_CLB: u64 = 0x00;
const PORT_CLBU: u64 = 0x04;
const PORT_FB: u64 = 0x08;
const PORT_FBU: u64 = 0x0C;
const PORT_CMD: u64 = 0x18;
const PORT_IS: u64 = 0x10;
const PORT_TFD: u64 = 0x20;
const PORT_SIG: u64 = 0x24;
const PORT_SSTS: u64 = 0x28;
const PORT_SCTL: u64 = 0x2C;
const PORT_SERR: u64 = 0x30;
const PORT_CI: u64 = 0x38;
const PORT_IE: u64 = 0x14;

const CMD_ST: u32 = 0x0001;
const CMD_FRE: u32 = 0x0010;
const CMD_POD: u32 = 0x0002;
const CMD_SUD: u32 = 0x0004;
const CMD_CR: u32 = 0x8000;
const CMD_FR: u32 = 0x4000;

const SATA_SIG_ATA: u32 = 0x0000_0101;
const SATA_SIG_ATAPI: u32 = 0xEB14_0101;

const TFD_BSY: u32 = 0x80;
const TFD_DRQ: u32 = 0x08;

const ATA_CMD_READ_DMA_EXT: u8 = 0x25;
const ATA_CMD_WRITE_DMA_EXT: u8 = 0x35;
const ATA_CMD_READ_FPDMA_QUEUED: u8 = 0x60;
const ATA_CMD_WRITE_FPDMA_QUEUED: u8 = 0x61;
const ATA_CMD_IDENTIFY_DEVICE: u8 = 0xEC;
/// Per NCQ slot DMA buffer size (4KB = 8 sectors max per PRDT)
const NCQ_SLOT_BUF_SIZE: usize = DMA_BUF_SIZE;

const PORT_SACT: u64 = 0x34;

/// NCQ capability bit in IDENTIFY DEVICE data word 76.
const NCQ_SUPPORT_BIT: u16 = 1 << 8;

#[repr(C, packed)]
struct PrdtEntry {
    data_base: u32,
    data_base_hi: u32,
    reserved: u32,
    count: u32,
}

#[repr(C)]
struct CmdTableInner {
    cfis: [u8; 64],
    acmd: [u8; 16],
    reserved: [u8; 48],
    prdt: [PrdtEntry; MAX_PRD_ENTRIES],
}

#[repr(C)]
struct CmdHeader {
    opts: u16,
    prdtl: u16,
    prdbc: u32,
    ctba: u32,
    ctba_hi: u32,
    reserved: [u32; 4],
}

#[repr(C, align(1024))]
struct CmdList([CmdHeader; MAX_CMD_SLOTS]);

#[repr(C, align(256))]
struct RecvFis([u8; 256]);

#[repr(C, align(128))]
struct CmdTable(CmdTableInner);

const EMPTY_CMD_HEADER: CmdHeader = CmdHeader {
    opts: 0, prdtl: 0, prdbc: 0, ctba: 0, ctba_hi: 0, reserved: [0; 4],
};
const EMPTY_PRD: PrdtEntry = PrdtEntry {
    data_base: 0, data_base_hi: 0, reserved: 0, count: 0,
};

// ── Heap-allocated AHCI buffers (replaces static buffers, v0.40) ──
// Allocated from kernel heap during probe(). Pointers stored in the
// struct so they're alive for as long as BootAhci is registered.

fn ahci_alloc_buffers() -> (*mut CmdList, *mut RecvFis, *mut CmdTable, *mut u8) {
    let cl_layout = Layout::from_size_align(
        core::mem::size_of::<CmdList>() * MAX_PORTS, 1024).unwrap();
    let rf_layout = Layout::from_size_align(
        core::mem::size_of::<RecvFis>() * MAX_PORTS, 256).unwrap();
    let ct_layout = Layout::from_size_align(
        core::mem::size_of::<CmdTable>() * MAX_PORTS, 128).unwrap();

    let cl_ptr = unsafe { alloc_zeroed(cl_layout) } as *mut CmdList;
    let rf_ptr = unsafe { alloc_zeroed(rf_layout) } as *mut RecvFis;
    let ct_ptr = unsafe { alloc_zeroed(ct_layout) } as *mut CmdTable;
    let db_ptr = unsafe { alloc_zeroed(Layout::new::<[u8; DMA_BUF_SIZE * MAX_PORTS]>()) };

    if cl_ptr.is_null() || rf_ptr.is_null() || ct_ptr.is_null() || db_ptr.is_null() {
        // Free partial allocations on OOM
        if !cl_ptr.is_null() { unsafe { alloc::alloc::dealloc(cl_ptr as *mut u8, cl_layout); } }
        if !rf_ptr.is_null() { unsafe { alloc::alloc::dealloc(rf_ptr as *mut u8, rf_layout); } }
        if !ct_ptr.is_null() { unsafe { alloc::alloc::dealloc(ct_ptr as *mut u8, ct_layout); } }
        if !db_ptr.is_null() { unsafe { alloc::alloc::dealloc(db_ptr, Layout::new::<[u8; DMA_BUF_SIZE * MAX_PORTS]>()); } }
        return (core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut());
    }

    (cl_ptr, rf_ptr, ct_ptr, db_ptr)
}

/// Send IDENTIFY DEVICE command to detect NCQ capability.
/// Returns true if device supports NCQ (word 76 bit 8).
fn self_detect_ncq(abar: u64, port: usize, is_atapi: bool,
    cmd_list: *mut CmdList, cmd_table: *mut CmdTable, dma_buf: *mut u8) -> bool
{
    if is_atapi {
        return false;
    }

    unsafe {
        let ct = &mut *cmd_table.add(port);
        let ct_inner = &mut ct.0;
        ct_inner.cfis = [0; 64];
        ct_inner.cfis[0] = 0x27;
        ct_inner.cfis[1] = 0x80;
        ct_inner.cfis[2] = ATA_CMD_IDENTIFY_DEVICE;
        ct_inner.cfis[7] = 0x40;

        for i in 0..MAX_PRD_ENTRIES {
            ct_inner.prdt[i] = EMPTY_PRD;
        }
        let dbuf = dma_buf.add(DMA_BUF_SIZE * port);
        ct_inner.prdt[0].data_base = dbuf as u32;
        ct_inner.prdt[0].data_base_hi = 0;
        ct_inner.prdt[0].count = (512 - 1) | (1 << 31);

        let cl = &mut *cmd_list.add(port);
        cl.0[0] = CmdHeader {
            opts: 5 | (1 << 6),
            prdtl: 1,
            prdbc: 0,
            ctba: ct as *mut CmdTable as u32,
            ctba_hi: 0,
            reserved: [0; 4],
        };

        fence(Ordering::SeqCst);
        port_write32(abar, port, PORT_CI, 1);

        for _ in 0..10_000_000 {
            let ci = port_read32(abar, port, PORT_CI);
            if (ci & 1) == 0 { break; }
            core::hint::spin_loop();
        }

        let tfd = port_read32(abar, port, PORT_TFD);
        if (tfd & (TFD_BSY | TFD_DRQ | 1)) != 0 {
            return false;
        }

        // Read word 76 from IDENTIFY data (512 bytes at dma_buf)
        let identify = dbuf as *const u16;
        let word76 = identify.add(76).read_volatile();
        (word76 & NCQ_SUPPORT_BIT) != 0
    }
}

fn mmio_read32(base: u64, offset: u64) -> u32 {
    unsafe { (base as *const u32).add(offset as usize / 4).read_volatile() }
}

fn mmio_write32(base: u64, offset: u64, val: u32) {
    unsafe { (base as *mut u32).add(offset as usize / 4).write_volatile(val); }
}

fn mmio_read64(base: u64, offset: u64) -> u64 {
    unsafe { (base as *const u64).add(offset as usize / 8).read_volatile() }
}

fn mmio_write64(base: u64, offset: u64, val: u64) {
    unsafe { (base as *mut u64).add(offset as usize / 8).write_volatile(val); }
}

fn port_reg_addr(abar: u64, port: usize, reg: u64) -> u64 {
    abar + PORT_REG_BASE + (port as u64) * PORT_STRIDE + reg
}

fn port_read32(abar: u64, port: usize, reg: u64) -> u32 {
    let addr = port_reg_addr(abar, port, reg);
    mmio_read32(addr, 0)
}

fn port_write32(abar: u64, port: usize, reg: u64, val: u32) {
    let addr = port_reg_addr(abar, port, reg);
    mmio_write32(addr, 0, val);
}

fn port_is_idle(abar: u64, port: usize) -> bool {
    let cmd = port_read32(abar, port, PORT_CMD);
    (cmd & CMD_CR) == 0 && (cmd & CMD_FR) == 0
}

fn port_wait_idle(abar: u64, port: usize, timeout_ms: u32) -> bool {
    for _ in 0..timeout_ms * 1000 {
        if port_is_idle(abar, port) {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn port_reset_and_start(abar: u64, port: usize) -> bool {
    let cmd = port_read32(abar, port, PORT_CMD);
    if (cmd & CMD_ST) != 0 || (cmd & CMD_FRE) != 0 {
        port_write32(abar, port, PORT_CMD, cmd & !(CMD_ST | CMD_FRE));
        for _ in 0..10000 {
            let c = port_read32(abar, port, PORT_CMD);
            if (c & (CMD_CR | CMD_FR)) == 0 { break; }
        }
    }

    port_write32(abar, port, PORT_IS, 0xFFFF_FFFF);
    port_write32(abar, port, PORT_SERR, 0xFFFF_FFFF);
    port_write32(abar, port, PORT_IE, 0);

    port_write32(abar, port, PORT_CMD, CMD_ST | CMD_FRE | CMD_POD | CMD_SUD);
    for _ in 0..10000 {
        let c = port_read32(abar, port, PORT_CMD);
        if (c & CMD_CR) == 0 { break; }
    }
    true
}

pub struct BootAhci {
    abar: u64,
    port: usize,
    base_lba: u64,
    num_sectors: u64,
    is_atapi: bool,
    ncq_supported: bool,
    // Heap-allocated DMA buffers (v0.40)
    cmd_list: *mut CmdList,
    recv_fis: *mut RecvFis,
    cmd_table: *mut CmdTable,
    dma_buf: *mut u8,
    // NCQ per-slot command tables (32 slots)
    ncq_cmd_tables: *mut CmdTable,
    // NCQ per-slot DMA buffers (32 * 4KB)
    ncq_dma_bufs: *mut u8,
    // NCQ tag → IRP mapping
    tag_map: IrpTagMap,
}

impl Drop for BootAhci {
    fn drop(&mut self) {
        unsafe {
            if !self.cmd_list.is_null() {
                alloc::alloc::dealloc(
                    self.cmd_list as *mut u8,
                    Layout::from_size_align_unchecked(
                        core::mem::size_of::<CmdList>() * MAX_PORTS, 1024),
                );
            }
            if !self.recv_fis.is_null() {
                alloc::alloc::dealloc(
                    self.recv_fis as *mut u8,
                    Layout::from_size_align_unchecked(
                        core::mem::size_of::<RecvFis>() * MAX_PORTS, 256),
                );
            }
            if !self.cmd_table.is_null() {
                alloc::alloc::dealloc(
                    self.cmd_table as *mut u8,
                    Layout::from_size_align_unchecked(
                        core::mem::size_of::<CmdTable>() * MAX_PORTS, 128),
                );
            }
            if !self.dma_buf.is_null() {
                alloc::alloc::dealloc(
                    self.dma_buf,
                    Layout::new::<[u8; DMA_BUF_SIZE * MAX_PORTS]>());
            }
            if !self.ncq_cmd_tables.is_null() {
                let ncq_ct_layout = Layout::from_size_align_unchecked(
                    core::mem::size_of::<CmdTable>() * NCQ_MAX_TAGS, 128);
                alloc::alloc::dealloc(self.ncq_cmd_tables as *mut u8, ncq_ct_layout);
            }
            if !self.ncq_dma_bufs.is_null() {
                let ncq_db_layout = Layout::from_size_align_unchecked(
                    NCQ_SLOT_BUF_SIZE * NCQ_MAX_TAGS, 64);
                alloc::alloc::dealloc(self.ncq_dma_bufs, ncq_db_layout);
            }
        }
    }
}

unsafe impl Send for BootAhci {}

impl BootAhci {
    pub fn probe() -> Option<Self> {
        let mut found = None;
        for bus in 0..=0 {
            for dev in 0..32 {
                for func in 0..8 {
                    let vendor = pci_config_read_word(bus, dev, func, 0);
                    if vendor == 0xFFFF || vendor == 0 {
                        if func == 0 { break; }
                        continue;
                    }
                    let class_rev = pci_config_read_dword(bus, dev, func, 0x08);
                    let class = ((class_rev >> 24) & 0xFF) as u8;
                    let subclass = ((class_rev >> 16) & 0xFF) as u8;
                    if class == 0x01 && subclass == 0x06 {
                        let bar5 = pci_config_read_dword(bus, dev, func, 0x24);
                        let abar = (bar5 & 0xFFFF_FFF0) as u64;
                        if abar == 0 {
                            continue;
                        }
                        let cmd = pci_config_read_word(bus, dev, func, 0x04);
                        pci_config_write_word(bus, dev, func, 0x04, cmd | 0x06);
                        found = Some((abar, bus, dev, func));
                        break;
                    }
                }
                if found.is_some() { break; }
            }
            if found.is_some() { break; }
        }

        let (abar, bus, dev, func) = found?;
        serial_println!("[AHCI] Found AHCI controller at PCI {:02x}:{:02x}.{:01x} ABAR=0x{:x}", bus, dev, func, abar);

        let pi = mmio_read32(abar, HBA_PI);
        serial_println!("[AHCI] Ports implemented: 0x{:08x}", pi);

        let ghc = mmio_read32(abar, HBA_GHC);
        if (ghc & HBA_GHC_AE) == 0 {
            mmio_write32(abar, HBA_GHC, ghc | HBA_GHC_AE);
            fence(Ordering::SeqCst);
        }

        let mut active_port = None;
        for p in 0..MAX_PORTS {
            if (pi & (1 << p)) == 0 {
                continue;
            }
            let ssts = port_read32(abar, p, PORT_SSTS);
            let det = ssts & 0x0F;
            if det != 0x03 {
                continue;
            }
            let sig = port_read32(abar, p, PORT_SIG);
            if sig != SATA_SIG_ATA && sig != SATA_SIG_ATAPI {
                continue;
            }
            active_port = Some(p);
            break;
        }

        let port = active_port?;
        let sig = port_read32(abar, port, PORT_SIG);
        let is_atapi = sig == SATA_SIG_ATAPI;
        serial_println!("[AHCI] Using port {} sig=0x{:08x} {}", port, sig, if is_atapi { "ATAPI" } else { "ATA" });

        // ── Allocate heap buffers for DMA (v0.40, replaces static buffers) ──
        let (cmd_list, recv_fis, cmd_table, dma_buf) = ahci_alloc_buffers();
        if cmd_list.is_null() || recv_fis.is_null() || cmd_table.is_null() || dma_buf.is_null() {
            serial_println!("[AHCI] Failed to allocate DMA buffers from heap");
            return None;
        }
        serial_println!(
            "[AHCI] DMA buffers allocated: cmd_list=0x{:p} recv_fis=0x{:p} cmd_table=0x{:p} dma_buf=0x{:p}",
            cmd_list, recv_fis, cmd_table, dma_buf
        );

        let (clb_val, fb_val) = unsafe {
            let clb = &*cmd_list.add(port) as *const CmdList as u32;
            let clbu = 0u32;
            port_write32(abar, port, PORT_CLB, clb);
            port_write32(abar, port, PORT_CLBU, clbu);

            let fb = &*recv_fis.add(port) as *const RecvFis as u32;
            let fbu = 0u32;
            port_write32(abar, port, PORT_FB, fb);
            port_write32(abar, port, PORT_FBU, fbu);

            port_write32(abar, port, PORT_SERR, port_read32(abar, port, PORT_SERR));
            (clb, fb)
        };

        if !port_reset_and_start(abar, port) {
            serial_println!("[AHCI] Port {} failed to start", port);
            return None;
        }

        // ── Allocate NCQ per-slot buffers (32 slots) ──
        let ncq_ct_layout = Layout::from_size_align(
            core::mem::size_of::<CmdTable>() * NCQ_MAX_TAGS, 128).unwrap();
        let ncq_db_layout = Layout::from_size_align(
            NCQ_SLOT_BUF_SIZE * NCQ_MAX_TAGS, 64).unwrap();
        let ncq_ct_ptr = unsafe { alloc_zeroed(ncq_ct_layout) } as *mut CmdTable;
        let ncq_db_ptr = unsafe { alloc_zeroed(ncq_db_layout) };
        if ncq_ct_ptr.is_null() || ncq_db_ptr.is_null() {
            serial_println!("[AHCI] Failed to allocate NCQ buffers");
            if !ncq_ct_ptr.is_null() { unsafe { alloc::alloc::dealloc(ncq_ct_ptr as *mut u8, ncq_ct_layout); } }
            if !ncq_db_ptr.is_null() { unsafe { alloc::alloc::dealloc(ncq_db_ptr, ncq_db_layout); } }
            return None;
        }

        // ── Detect NCQ support via IDENTIFY DEVICE ──
        let ncq_supported = self_detect_ncq(abar, port, is_atapi, cmd_list, cmd_table, dma_buf);
        if ncq_supported {
            serial_println!("[AHCI] Device supports NCQ (32 tags)");
        } else {
            serial_println!("[AHCI] Device does NOT support NCQ, using legacy path");
        }

        let num_sectors = 0x0012_4F00u64;
        serial_println!("[AHCI] Boot AHCI ready on port {}", port);
        *BOOT_AHCI_INFO.lock() = Some((abar, port, clb_val, fb_val));

        Some(BootAhci {
            abar, port, base_lba: 0, num_sectors, is_atapi,
            ncq_supported,
            cmd_list, recv_fis, cmd_table, dma_buf,
            ncq_cmd_tables: ncq_ct_ptr,
            ncq_dma_bufs: ncq_db_ptr,
            tag_map: IrpTagMap::new(),
        })
    }

    /// Reclaim the AHCI port after a NEM AHCI driver overrides PORT_CLB/PORT_FB.
    /// Must be called after boot_load_all() but before any further DMA via BootAhci.
    pub fn reclaim_ahci_port() {
        let info = BOOT_AHCI_INFO.lock();
        let Some((abar, port, saved_clb, saved_fb)) = *info else { return };

        serial_println!("[AHCI] Reclaiming port {} after NEM driver init", port);

        // Stop the port so we can safely change CLB/FB
        let cmd = port_read32(abar, port, PORT_CMD);
        if (cmd & CMD_ST) != 0 || (cmd & CMD_FRE) != 0 {
            port_write32(abar, port, PORT_CMD, cmd & !(CMD_ST | CMD_FRE));
            for _ in 0..10000 {
                let c = port_read32(abar, port, PORT_CMD);
                if (c & (CMD_CR | CMD_FR)) == 0 { break; }
            }
        }

        // Restore BootAhci's own DMA buffer addresses (overwritten by NEM driver)
        port_write32(abar, port, PORT_CLB, saved_clb);
        port_write32(abar, port, PORT_CLBU, 0);
        port_write32(abar, port, PORT_FB, saved_fb);
        port_write32(abar, port, PORT_FBU, 0);

        // Clear error status
        port_write32(abar, port, PORT_IS, 0xFFFF_FFFF);
        port_write32(abar, port, PORT_SERR, 0xFFFF_FFFF);

        // Restart the port with BootAhci's buffers
        port_write32(abar, port, PORT_CMD, CMD_ST | CMD_FRE | CMD_POD | CMD_SUD);
        for _ in 0..10000 {
            let c = port_read32(abar, port, PORT_CMD);
            if (c & CMD_CR) == 0 { break; }
        }

        serial_println!("[AHCI] Port reclaimed successfully (BootAhci buffers restored)");
    }

    fn dma_xfer(&mut self, lba: u64, count: u8, buf: *const u8, is_write: bool) -> Result<(), ()> {
        let port = self.port;
        let abar = self.abar;
        let abs_lba = self.base_lba.wrapping_add(lba);

        if self.is_atapi {
            return Err(());
        }

        unsafe {
            let ct = &mut *self.cmd_table.add(port);
            let ct_inner = &mut ct.0;
            ct_inner.cfis = [0; 64];
            ct_inner.cfis[0] = 0x27;
            ct_inner.cfis[1] = 0x80;
            if is_write {
                ct_inner.cfis[2] = ATA_CMD_WRITE_DMA_EXT;
            } else {
                ct_inner.cfis[2] = ATA_CMD_READ_DMA_EXT;
            }
            ct_inner.cfis[4] = (abs_lba & 0xFF) as u8;
            ct_inner.cfis[5] = ((abs_lba >> 8) & 0xFF) as u8;
            ct_inner.cfis[6] = ((abs_lba >> 16) & 0xFF) as u8;
            ct_inner.cfis[7] = 0x40;
            ct_inner.cfis[8] = ((abs_lba >> 24) & 0xFF) as u8;
            ct_inner.cfis[9] = ((abs_lba >> 32) & 0xFF) as u8;
            ct_inner.cfis[10] = ((abs_lba >> 40) & 0xFF) as u8;
            ct_inner.cfis[12] = count;
            ct_inner.cfis[13] = 0;

            let dbuf = self.dma_buf.add(DMA_BUF_SIZE * port);
            let dbuf_phys = dbuf as u32;
            if is_write {
                core::ptr::copy_nonoverlapping(buf, dbuf, (count as usize) * 512);
            }

            let nprd = 1;
            for i in 0..MAX_PRD_ENTRIES {
                ct_inner.prdt[i] = EMPTY_PRD;
            }
            ct_inner.prdt[0].data_base = dbuf_phys;
            ct_inner.prdt[0].data_base_hi = 0;
            ct_inner.prdt[0].count = ((count as u32) * 512 - 1) | (1 << 31);

            let cl = &mut *self.cmd_list.add(port);
            cl.0[0] = CmdHeader {
                opts: 5 | (1 << 6),
                prdtl: nprd as u16,
                prdbc: 0,
                ctba: ct as *mut CmdTable as u32,
                ctba_hi: 0,
                reserved: [0; 4],
            };
        }

        fence(Ordering::SeqCst);
        crate::boot_benchmark::ahci_cmd_start();
        let cmd_start = crate::boot_benchmark::boot_time_now();
        port_write32(abar, port, PORT_CI, 1);

        let mut poll_count: u64 = 0;
        let mut cmd_timed_out = false;
        for _ in 0..10_000_000 {
            let ci = port_read32(abar, port, PORT_CI);
            poll_count += 1;
            if (ci & 1) == 0 {
                break;
            }
            if poll_count % 10_000 == 0 {
                if crate::boot_benchmark::elapsed_ms(cmd_start, crate::boot_benchmark::boot_time_now()) > 1000 {
                    cmd_timed_out = true;
                    break;
                }
            }
            core::hint::spin_loop();
        }
        crate::boot_benchmark::ahci_cmd_polled(poll_count);
        let wait = crate::boot_benchmark::elapsed_ms(cmd_start, crate::boot_benchmark::boot_time_now());
        crate::boot_benchmark::ahci_cmd_done(wait);

        if cmd_timed_out {
            crate::boot_benchmark::ahci_cmd_timeout();
        }

        let tfd = port_read32(abar, port, PORT_TFD);
        if (tfd & (TFD_BSY | TFD_DRQ | 1)) != 0 {
            let serr = port_read32(abar, port, PORT_SERR);
            crate::boot_benchmark::ahci_dma_failure();
            serial_println!("[AHCI] DMA error op={} lba={} tfd=0x{:02x} serr=0x{:08x}",
                if is_write { "WR" } else { "RD" }, lba, tfd, serr);
            return Err(());
        }

        if !is_write {
            unsafe {
                let dbuf = self.dma_buf.add(DMA_BUF_SIZE * port);
                core::ptr::copy_nonoverlapping(dbuf, buf as *mut u8, (count as usize) * 512);
            }
        }

        Ok(())
    }

    /// NCQ batch transfer: issue up to 32 concurrent FPDMA QUEUED commands.
    /// Each entry in `cmds` is (lba, count, buf, is_write).
    /// Returns number of successfully completed commands.
    pub fn ncq_batch_xfer(&mut self, cmds: &[(u64, u8, *mut u8, bool)]) -> usize {
        if !self.ncq_supported || cmds.is_empty() {
            return 0;
        }

        let batch_size = cmds.len().min(NCQ_MAX_TAGS);
        let port = self.port;
        let abar = self.abar;
        let mut ci_mask: u32 = 0;
        let mut tag_of_slot = [0u8; NCQ_MAX_TAGS];

        unsafe {
            let cl = &mut *self.cmd_list.add(port);

            for slot in 0..batch_size {
                let (lba, count, buf, is_write) = cmds[slot];
                let abs_lba = self.base_lba.wrapping_add(lba);
                let tag = slot as u8;

                let ncq_ct = &mut *self.ncq_cmd_tables.add(tag as usize);
                let ncq_ct_inner = &mut ncq_ct.0;

                ncq_ct_inner.cfis = [0; 64];
                ncq_ct_inner.cfis[0] = 0x27;
                ncq_ct_inner.cfis[1] = 0x80; // C=1 (command FIS)
                if is_write {
                    ncq_ct_inner.cfis[2] = ATA_CMD_WRITE_FPDMA_QUEUED;
                } else {
                    ncq_ct_inner.cfis[2] = ATA_CMD_READ_FPDMA_QUEUED;
                }
                ncq_ct_inner.cfis[4] = (abs_lba & 0xFF) as u8;
                ncq_ct_inner.cfis[5] = ((abs_lba >> 8) & 0xFF) as u8;
                ncq_ct_inner.cfis[6] = ((abs_lba >> 16) & 0xFF) as u8;
                ncq_ct_inner.cfis[7] = 0x40; // LBA bit
                ncq_ct_inner.cfis[8] = ((abs_lba >> 24) & 0xFF) as u8;
                ncq_ct_inner.cfis[9] = ((abs_lba >> 32) & 0xFF) as u8;
                ncq_ct_inner.cfis[10] = ((abs_lba >> 40) & 0xFF) as u8;
                // NCQ tag in sector count register bits [4:0], FUA in bit 7
                ncq_ct_inner.cfis[12] = tag << 3;
                ncq_ct_inner.cfis[13] = 0; // sector count exp
                // NCQ auxiliary: features_exp = tag >> 3 (for tags > 7)
                ncq_ct_inner.cfis[3] = tag >> 3;

                // Per-slot DMA buffer
                let slot_dma = self.ncq_dma_bufs.add(NCQ_SLOT_BUF_SIZE * tag as usize);
                if is_write {
                    core::ptr::copy_nonoverlapping(buf, slot_dma, (count as usize) * 512);
                }

                // PRDT: 1 entry per slot (max 8 sectors = 4096 bytes fits in 4KB buffer)
                for i in 0..MAX_PRD_ENTRIES {
                    ncq_ct_inner.prdt[i] = EMPTY_PRD;
                }
                ncq_ct_inner.prdt[0].data_base = slot_dma as u32;
                ncq_ct_inner.prdt[0].data_base_hi = 0;
                ncq_ct_inner.prdt[0].count = ((count as u32) * 512 - 1) | (1 << 31);

                // Write SActive before CI (AHCI spec: SActive must be written before CI)
                port_write32(abar, port, PORT_SACT, 1u32 << tag);

                // Set up command header for this slot
                let ct_phys = self.ncq_cmd_tables.add(tag as usize) as u32;
                cl.0[tag as usize] = CmdHeader {
                    opts: 5 | (1 << 6),
                    prdtl: 1,
                    prdbc: 0,
                    ctba: ct_phys,
                    ctba_hi: 0,
                    reserved: [0; 4],
                };

                ci_mask |= 1u32 << tag;
                tag_of_slot[slot] = tag;
            }

            fence(Ordering::SeqCst);
            crate::boot_benchmark::ahci_cmd_start();
            let cmd_start = crate::boot_benchmark::boot_time_now();

            // Issue all commands at once
            port_write32(abar, port, PORT_CI, ci_mask);

            // Poll PORT_CI until all bits clear (all commands complete)
            let mut poll_count: u64 = 0;
            let mut cmd_timed_out = false;
            for _ in 0..10_000_000 {
                let ci = port_read32(abar, port, PORT_CI);
                poll_count += 1;
                if (ci & ci_mask) == 0 {
                    break;
                }
                if poll_count % 50_000 == 0 {
                    if crate::boot_benchmark::elapsed_ms(cmd_start, crate::boot_benchmark::boot_time_now()) > 2000 {
                        cmd_timed_out = true;
                        break;
                    }
                }
                core::hint::spin_loop();
            }
            crate::boot_benchmark::ahci_cmd_polled(poll_count);
            let wait = crate::boot_benchmark::elapsed_ms(cmd_start, crate::boot_benchmark::boot_time_now());
            crate::boot_benchmark::ahci_cmd_done(wait);

            if cmd_timed_out {
                crate::boot_benchmark::ahci_cmd_timeout();
            }

            // Check for errors and copy back data for reads
            let tfd = port_read32(abar, port, PORT_TFD);
            let mut completed = 0;
            for slot in 0..batch_size {
                let tag = tag_of_slot[slot];
                let ci_still = port_read32(abar, port, PORT_CI);
                if (ci_still & (1u32 << tag)) != 0 {
                    continue;
                }
                let (lba, _count, buf, is_write) = cmds[slot];
                let abs_lba = self.base_lba.wrapping_add(lba);

                if (tfd & (TFD_BSY | TFD_DRQ | 1)) != 0 {
                    let serr = port_read32(abar, port, PORT_SERR);
                    serial_println!("[AHCI] NCQ error slot={} tag={} lba={} tfd=0x{:02x} serr=0x{:08x}",
                        slot, tag, abs_lba, tfd, serr);
                    continue;
                }

                if !is_write {
                    let slot_dma = self.ncq_dma_bufs.add(NCQ_SLOT_BUF_SIZE * tag as usize);
                    core::ptr::copy_nonoverlapping(slot_dma, buf, (_count as usize) * 512);
                }
                completed += 1;
            }

            completed
        }
    }
}

impl BootAhci {
    /// Queue multiple IRPs for NCQ batch execution. Up to 32 IRPs can be
    /// queued; they are dispatched in a single FPDMA QUEUED command batch.
    /// Returns number of IRPs successfully submitted.
    pub fn ncq_submit_irp_batch(&mut self, irp_ids: &[IrpId]) -> usize {
        if !self.ncq_supported || irp_ids.is_empty() {
            return 0;
        }

        let batch_size = irp_ids.len().min(NCQ_MAX_TAGS);
        let mut cmds = [(0u64, 0u8, core::ptr::null_mut(), false); NCQ_MAX_TAGS];
        let mut tag_for_irp: [u8; NCQ_MAX_TAGS] = [0xFF; NCQ_MAX_TAGS];
        let mut valid = 0usize;

        for i in 0..batch_size {
            let irp_id = irp_ids[i];
            let params = match irp::irp_get_params(irp_id) {
                Some(p) => p,
                None => continue,
            };
            let tag = match self.tag_map.alloc_tag() {
                Some(t) => t,
                None => continue,
            };
            if !self.tag_map.assign(tag, irp_id) {
                continue;
            }
            let is_write = matches!(params.op, irp::IrpOp::Write);
            cmds[valid] = (params.lba, params.count, params.buf, is_write);
            tag_for_irp[valid] = tag;
            valid += 1;
        }

        if valid == 0 {
            return 0;
        }

        let completed = self.ncq_batch_xfer(&cmds[..valid]);

        // Complete IRPs based on completion results
        for i in 0..valid {
            let tag = tag_for_irp[i];
            if let Some(irp_id) = self.tag_map.free(tag) {
                if i < completed {
                    irp::irp_complete_result(irp_id, Ok(()));
                } else {
                    irp::irp_complete_result(irp_id, Err(()));
                }
            }
        }

        completed
    }
}

impl BlockDevice for BootAhci {
    fn submit_irp(&mut self, irp_id: IrpId) -> Result<(), ()> {
        let params = irp::irp_get_params(irp_id).ok_or(())?;
        match params.op {
            IrpOp::Read => {
                // Use NCQ if supported (single command via NCQ slot 0)
                if self.ncq_supported {
                    let tag = self.tag_map.alloc_tag().unwrap_or(0);
                    self.tag_map.assign(tag, irp_id);
                    let cmds = [(params.lba, params.count, params.buf, false)];
                    let completed = self.ncq_batch_xfer(&cmds);
                    self.tag_map.free(tag);
                    if completed > 0 {
                        irp::irp_complete_result(irp_id, Ok(()));
                    } else {
                        irp::irp_complete_result(irp_id, Err(()));
                    }
                } else {
                    let buf_slice = unsafe { core::slice::from_raw_parts_mut(params.buf, params.buf_len) };
                    let count = (params.buf_len / 512) as u8;
                    let result = self.read_blocks(params.lba, count, buf_slice);
                    irp::irp_complete_result(irp_id, result);
                }
            }
            IrpOp::Write => {
                let buf_slice = unsafe { core::slice::from_raw_parts(params.buf, params.buf_len) };
                let count = (params.buf_len / 512) as u8;
                let result = self.write_blocks(params.lba, count, buf_slice);
                irp::irp_complete_result(irp_id, result);
            }
            _ => {
                irp::irp_complete_result(irp_id, Ok(()));
                return Ok(());
            }
        }
        Ok(())
    }

    fn read_blocks(&mut self, lba: u64, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        self.dma_xfer(lba, count, buf.as_ptr(), false)
    }

    fn write_blocks(&mut self, lba: u64, count: u8, buf: &[u8]) -> Result<(), ()> {
        self.dma_xfer(lba, count, buf.as_ptr(), true)
    }

    fn num_sectors(&self) -> Option<u64> {
        Some(self.num_sectors)
    }

    fn set_base_lba(&mut self, lba: u64) {
        self.base_lba = lba;
    }

    fn base_lba(&self) -> u64 {
        self.base_lba
    }

    /// Poll for NCQ tag-based completion: check if a specific tag has completed.
    fn poll_irp(&mut self, irp_id: IrpId) -> irp::IrpStatus {
        if !self.ncq_supported {
            return irp::irp_get_status(irp_id);
        }
        // Check if any completed tag matches this IRP
        let ci = port_read32(self.abar, self.port, PORT_CI);
        for t in 0..NCQ_MAX_TAGS as u8 {
            if (ci & (1u32 << t)) == 0 {
                if let Some(mapped) = self.tag_map.lookup(t) {
                    if mapped == irp_id {
                        return irp::IrpStatus::Completed;
                    }
                }
            }
        }
        irp::irp_get_status(irp_id)
    }
}
