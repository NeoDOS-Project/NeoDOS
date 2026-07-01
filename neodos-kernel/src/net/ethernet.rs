use super::types::MacAddr;
use alloc::vec::Vec;

pub const ETH_HDR_LEN: usize = 14;
pub const ETH_TYPE_IPV4: u16 = 0x0800;
pub const ETH_TYPE_ARP: u16 = 0x0806;
pub const ETH_MIN_PAYLOAD: usize = 46;
pub const ETH_MAX_PAYLOAD: usize = 1500;

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct EthernetHeader {
    pub dst: [u8; 6],
    pub src: [u8; 6],
    pub ether_type: u16,
}

impl EthernetHeader {
    pub fn new(dst: MacAddr, src: MacAddr, ether_type: u16) -> Self {
        EthernetHeader {
            dst: dst.0,
            src: src.0,
            ether_type: ether_type.to_be(),
        }
    }

    pub fn dst_mac(&self) -> MacAddr { MacAddr(self.dst) }
    pub fn src_mac(&self) -> MacAddr { MacAddr(self.src) }
    pub fn ethertype(&self) -> u16 { u16::from_be(self.ether_type) }

    pub fn is_ipv4(&self) -> bool { self.ethertype() == ETH_TYPE_IPV4 }
    pub fn is_arp(&self) -> bool { self.ethertype() == ETH_TYPE_ARP }
}

/// Build a complete Ethernet frame: header + payload.
pub fn build_ethernet_frame(dst: MacAddr, src: MacAddr, ether_type: u16, payload: &[u8]) -> Vec<u8> {
    let eth = EthernetHeader::new(dst, src, ether_type);
    let eth_bytes = unsafe {
        core::slice::from_raw_parts(
            &eth as *const EthernetHeader as *const u8,
            ETH_HDR_LEN,
        )
    };
    let mut frame = Vec::with_capacity(ETH_HDR_LEN + payload.len());
    frame.extend_from_slice(eth_bytes);
    frame.extend_from_slice(payload);
    frame
}

pub fn compute_eth_fcs(packet: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in packet {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}
