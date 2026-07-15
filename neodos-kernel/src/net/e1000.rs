use super::types::{MacAddr, Ipv4Addr};
use super::nic::NetworkInterface;
use alloc::boxed::Box;
use alloc::vec::Vec;
use crate::drivers::pci;
use crate::serial_println;

pub const E1000_VENDOR: u16 = 0x8086;
pub const E1000_DEVICE_82540EM: u16 = 0x100E;
pub const E1000_DEVICE_82543GC: u16 = 0x1004;
pub const E1000_DEVICE_82545EM: u16 = 0x100F;
pub const E1000_DEVICE_82574L: u16 = 0x10D3;

const REG_CTRL: u16 = 0x0000;
const REG_STATUS: u16 = 0x0008;
const REG_EECD: u16 = 0x0010;
const REG_EEPROM: u16 = 0x0014;
const REG_CTRL_EXT: u16 = 0x0018;
const REG_ICR: u16 = 0x00C0;
const REG_IMS: u16 = 0x00D0;
const REG_RCTRL: u16 = 0x0100;
const REG_TCTRL: u16 = 0x0400;
const REG_RDBAL: u16 = 0x2800;
const REG_RDBAH: u16 = 0x2804;
const REG_RDLEN: u16 = 0x2808;
const REG_RDH: u16 = 0x2810;
const REG_RDT: u16 = 0x2818;
const REG_TDBAL: u16 = 0x3800;
const REG_TDBAH: u16 = 0x3804;
const REG_TDLEN: u16 = 0x3808;
const REG_TDH: u16 = 0x3810;
const REG_TDT: u16 = 0x3818;
const REG_MTA: u16 = 0x5200;
const REG_RA: u16 = 0x5400;

const RCTL_EN: u32 = 0x00000002;
const RCTL_UPE: u32 = 0x00000008;
const RCTL_MPE: u32 = 0x00000010;
const RCTL_LPE: u32 = 0x00000020;
const RCTL_BSIZE_2048: u32 = 0x00000000;
const RCTL_BSIZE_4096: u32 = 0x00030000;
const RCTL_SECRC: u32 = 0x04000000;
const RCTL_BAM: u32 = 0x00008000;
const RCTL_SZ_2048: u32 = 0x00000000;
const RCTL_SZ_4096: u32 = 0x00030000;

const TCTL_EN: u32 = 0x00000002;
const TCTL_PSP: u32 = 0x00000008;
const TCTL_CT: u32 = 0x00000F00;
const TCTL_COLD: u32 = 0x003F0000;

const CMD_EOP: u8 = 0x01;
const CMD_IFCS: u8 = 0x02;
const CMD_RS: u8 = 0x08;

const STATUS_LINK_UP: u32 = 0x00000002;

const E1000_NUM_RX_DESC: usize = 32;
const E1000_NUM_TX_DESC: usize = 8;
const E1000_RX_BUF_SIZE: usize = 2048;

#[repr(C, packed)]
#[derive(Clone)]
struct E1000RxDesc {
    addr: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

#[repr(C, packed)]
#[derive(Clone)]
struct E1000TxDesc {
    addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

pub struct E1000Nic {
    mmio_base: u32,
    mac: MacAddr,
    ip: Ipv4Addr,
    name: [u8; 16],
    rx_descs: Vec<E1000RxDesc>,
    tx_descs: Vec<E1000TxDesc>,
    rx_bufs: Vec<Vec<u8>>,
    tx_bufs: Vec<Vec<u8>>,
    rx_cur: usize,
    tx_cur: usize,
    bus: u8,
    dev: u8,
    func: u8,
}

impl E1000Nic {
    pub fn try_probe(bus: u8, dev: u8, func: u8) -> Option<Box<dyn NetworkInterface>> {
        let vendor = pci::pci_config_read_word(bus, dev, func, 0);
        let device = pci::pci_config_read_word(bus, dev, func, 2);
        if vendor != E1000_VENDOR { return None; }
        match device {
            E1000_DEVICE_82540EM | E1000_DEVICE_82543GC | E1000_DEVICE_82545EM => {},
            _ => return None,
        }

        let bar0 = pci::read_bar(bus, dev, func, 0);
        let mmio_base = bar0 & 0xFFFFFFF0;
        if mmio_base == 0 { return None; }

        serial_println!("[E1000] Found at {:02x}:{:02x}.{:01x} MMIO=0x{:x}",
            bus, dev, func, mmio_base);

        let mac = Self::read_mac(mmio_base);
        serial_println!("[E1000] MAC address: {}", mac);

        let mut name_buf = [0u8; 16];
        let name_str = alloc::format!("e1000_{:02x}{:02x}", bus, dev);
        let nb = name_str.as_bytes();
        let len = nb.len().min(15);
        name_buf[..len].copy_from_slice(&nb[..len]);

        let rx_descs = {
            let mut v = Vec::with_capacity(E1000_NUM_RX_DESC);
            for _ in 0..E1000_NUM_RX_DESC {
                v.push(E1000RxDesc {
                    addr: 0, length: 0, checksum: 0, status: 0, errors: 0, special: 0,
                });
            }
            v
        };
        let tx_descs = {
            let mut v = Vec::with_capacity(E1000_NUM_TX_DESC);
            for _ in 0..E1000_NUM_TX_DESC {
                v.push(E1000TxDesc {
                    addr: 0, length: 0, cso: 0, cmd: 0, status: 0, css: 0, special: 0,
                });
            }
            v
        };
        let rx_bufs = {
            let mut v = Vec::with_capacity(E1000_NUM_RX_DESC);
            for _ in 0..E1000_NUM_RX_DESC {
                v.push(alloc::vec![0u8; E1000_RX_BUF_SIZE]);
            }
            v
        };
        let tx_bufs = {
            let mut v = Vec::with_capacity(E1000_NUM_TX_DESC);
            for _ in 0..E1000_NUM_TX_DESC {
                v.push(alloc::vec![0u8; E1000_RX_BUF_SIZE]);
            }
            v
        };

        let mut nic = E1000Nic {
            mmio_base,
            mac,
            ip: Ipv4Addr::unspecified(),
            name: name_buf,
            rx_descs,
            tx_descs,
            rx_bufs,
            tx_bufs,
            rx_cur: 0,
            tx_cur: 0,
            bus,
            dev,
            func,
        };
        nic.init_internal();
        Some(Box::new(nic))
    }

    fn read_mac(mmio_base: u32) -> MacAddr {
        let mut mac_bytes = [0u8; 6];
        unsafe {
            let lo = core::ptr::read_volatile((mmio_base as u64 + REG_RA as u64) as *const u32);
            let hi = core::ptr::read_volatile((mmio_base as u64 + (REG_RA + 4) as u64) as *const u32);
            mac_bytes[0] = (lo & 0xFF) as u8;
            mac_bytes[1] = ((lo >> 8) & 0xFF) as u8;
            mac_bytes[2] = ((lo >> 16) & 0xFF) as u8;
            mac_bytes[3] = ((lo >> 24) & 0xFF) as u8;
            mac_bytes[4] = (hi & 0xFF) as u8;
            mac_bytes[5] = ((hi >> 8) & 0xFF) as u8;
        }
        MacAddr(mac_bytes)
    }

    fn read_reg(&self, reg: u16) -> u32 {
        unsafe { core::ptr::read_volatile((self.mmio_base as u64 + reg as u64) as *const u32) }
    }

    fn write_reg(&self, reg: u16, val: u32) {
        unsafe { core::ptr::write_volatile((self.mmio_base as u64 + reg as u64) as *mut u32, val); }
    }

    fn init_internal(&mut self) {
        self.write_reg(REG_CTRL, 0);
        let ctrl = self.read_reg(REG_CTRL);
        self.write_reg(REG_CTRL, ctrl | 0x40);

        self.write_reg(REG_RCTRL, RCTL_EN | RCTL_UPE | RCTL_MPE | RCTL_BAM | RCTL_SZ_2048 | RCTL_SECRC);

        let rx_desc_phys = self.rx_descs.as_ptr() as u64;
        self.write_reg(REG_RDBAL, (rx_desc_phys & 0xFFFFFFFF) as u32);
        self.write_reg(REG_RDBAH, (rx_desc_phys >> 32) as u32);
        self.write_reg(REG_RDLEN, (E1000_NUM_RX_DESC * core::mem::size_of::<E1000RxDesc>()) as u32);
        self.write_reg(REG_RDH, 0);
        self.write_reg(REG_RDT, (E1000_NUM_RX_DESC - 1) as u32);

        for i in 0..E1000_NUM_RX_DESC {
            let buf_phys = self.rx_bufs[i].as_ptr() as u64;
            self.rx_descs[i].addr = buf_phys;
            self.rx_descs[i].status = 0;
        }

        self.write_reg(REG_TCTRL, TCTL_EN | TCTL_PSP | TCTL_CT | TCTL_COLD);

        let tx_desc_phys = self.tx_descs.as_ptr() as u64;
        self.write_reg(REG_TDBAL, (tx_desc_phys & 0xFFFFFFFF) as u32);
        self.write_reg(REG_TDBAH, (tx_desc_phys >> 32) as u32);
        self.write_reg(REG_TDLEN, (E1000_NUM_TX_DESC * core::mem::size_of::<E1000TxDesc>()) as u32);
        self.write_reg(REG_TDH, 0);
        self.write_reg(REG_TDT, 0);

        self.write_reg(REG_IMS, 0x1F6DC);
        self.write_reg(REG_ICR, 0xFFFFFFFF);

        serial_println!("[E1000] Hardware initialized at MMIO=0x{:x}", self.mmio_base);
    }
}

impl NetworkInterface for E1000Nic {
    fn mac_address(&self) -> MacAddr { self.mac }

    fn name(&self) -> &str {
        let len = self.name.iter().position(|&b| b == 0).unwrap_or(16);
        core::str::from_utf8(&self.name[..len]).unwrap_or("e1000")
    }

    fn set_ip_address(&mut self, ip: Ipv4Addr) { self.ip = ip; }
    fn ip_address(&self) -> Ipv4Addr { self.ip }

    fn send_packet(&mut self, packet: &[u8]) -> Result<(), ()> {
        let len = packet.len().min(E1000_RX_BUF_SIZE - 4);
        if len == 0 { return Err(()); }

        let tx_buf = &mut self.tx_bufs[self.tx_cur];
        tx_buf[..len].copy_from_slice(&packet[..len]);

        let desc = &mut self.tx_descs[self.tx_cur];
        desc.addr = tx_buf.as_ptr() as u64;
        desc.length = len as u16;
        desc.cmd = CMD_EOP | CMD_IFCS | CMD_RS;
        desc.status = 0;

        if len >= 14 {
            let dst_mac = MacAddr::from_slice(&packet[0..6]);
            let src_mac = MacAddr::from_slice(&packet[6..12]);
            let ethertype = u16::from_be_bytes([packet[12], packet[13]]);
            serial_println!("[E1000] TX desc={} len={} Dst={} Src={} EtherType=0x{:04X}",
                self.tx_cur, len, dst_mac, src_mac, ethertype);
        } else {
            serial_println!("[E1000] TX desc={} len={}", self.tx_cur, len);
        }
        crate::net::counters::COUNTERS.tx_packets.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        crate::net::counters::COUNTERS.tx_bytes.fetch_add(len as u64, core::sync::atomic::Ordering::Relaxed);

        // Ensure descriptor writes are visible to hardware before ringing TX doorbell
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);

        let old_tdt = self.read_reg(REG_TDT);
        self.write_reg(REG_TDT, (old_tdt + 1) % E1000_NUM_TX_DESC as u32);

        self.tx_cur = (self.tx_cur + 1) % E1000_NUM_TX_DESC;
        Ok(())
    }

    fn poll_packet(&mut self, buf: &mut [u8]) -> Option<usize> {
        let desc = &self.rx_descs[self.rx_cur];
        if desc.status & 0x01 == 0 {
            return None;
        }
        let len = desc.length as usize;
        if len > buf.len() || len == 0 {
            self.rx_descs[self.rx_cur].status = 0;
            core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
            let old_rdt = self.read_reg(REG_RDT);
            self.write_reg(REG_RDT, (old_rdt + 1) % E1000_NUM_RX_DESC as u32);
            self.rx_cur = (self.rx_cur + 1) % E1000_NUM_RX_DESC;
            return None;
        }
        buf[..len].copy_from_slice(&self.rx_bufs[self.rx_cur][..len]);

        let dma_addr = self.rx_descs[self.rx_cur].addr;
        let status = self.rx_descs[self.rx_cur].status;
        serial_println!("[E1000] RX desc={} len={} status=0x{:02X} dma=0x{:016X}",
            self.rx_cur, len, status, dma_addr);
        if len >= 14 {
            let raw = &self.rx_bufs[self.rx_cur];
            let dst_mac = MacAddr::from_slice(&raw[0..6]);
            let src_mac = MacAddr::from_slice(&raw[6..12]);
            let ethertype = u16::from_be_bytes([raw[12], raw[13]]);
            serial_println!("[E1000]   Dst={}", dst_mac);
            serial_println!("[E1000]   Src={}", src_mac);
            serial_println!("[E1000]   EtherType=0x{:04X}", ethertype);
        }
        crate::net::counters::COUNTERS.rx_packets.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        crate::net::counters::COUNTERS.rx_bytes.fetch_add(len as u64, core::sync::atomic::Ordering::Relaxed);

        self.rx_descs[self.rx_cur].status = 0;
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
        let old_rdt = self.read_reg(REG_RDT);
        self.write_reg(REG_RDT, (old_rdt + 1) % E1000_NUM_RX_DESC as u32);
        self.rx_cur = (self.rx_cur + 1) % E1000_NUM_RX_DESC;
        Some(len)
    }

    fn is_link_up(&self) -> bool {
        self.read_reg(REG_STATUS) & STATUS_LINK_UP != 0
    }
}

pub fn probe_e1000() -> Option<u32> {
    for bus in 0..=1 {
        for dev in 0..32 {
            for func in 0..8 {
                let vendor = pci::pci_config_read_word(bus, dev, func, 0);
                if vendor == 0xFFFF || vendor == 0 { continue; }
                let device = pci::pci_config_read_word(bus, dev, func, 2);
                if vendor == E1000_VENDOR && (device == E1000_DEVICE_82540EM
                    || device == E1000_DEVICE_82543GC || device == E1000_DEVICE_82545EM
                    || device == E1000_DEVICE_82574L)
                {
                    if let Some(nic) = E1000Nic::try_probe(bus, dev, func) {
                        let id = super::nic::nic_register(nic)?;
                        serial_println!("[E1000] Registered as NIC {}", id);
                        return Some(id);
                    }
                }
            }
        }
    }
    None
}
