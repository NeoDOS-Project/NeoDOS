use alloc::vec::Vec;
use crate::net::ipv4::{Ipv4Header, IPV4_HDR_MIN_LEN};
use crate::net::udp::UdpHeader;

pub const ICMP_TYPE_ECHO_REPLY: u8 = 0;
pub const ICMP_TYPE_ECHO_REQUEST: u8 = 8;
pub const ICMP_TYPE_DEST_UNREACH: u8 = 3;
pub const ICMP_TYPE_TIME_EXCEEDED: u8 = 11;

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct IcmpHeader {
    pub icmp_type: u8,
    pub code: u8,
    pub checksum: u16,
    pub rest_of_header: u32,
}

impl IcmpHeader {
    pub fn echo_request(id: u16, seq: u16) -> Self {
        IcmpHeader {
            icmp_type: ICMP_TYPE_ECHO_REQUEST,
            code: 0,
            checksum: 0,
            rest_of_header: u32::from_be((id as u32) << 16 | seq as u32),
        }
    }

    pub fn echo_reply(id: u16, seq: u16) -> Self {
        IcmpHeader {
            icmp_type: ICMP_TYPE_ECHO_REPLY,
            code: 0,
            checksum: 0,
            rest_of_header: u32::from_be((id as u32) << 16 | seq as u32),
        }
    }

    pub fn echo_identifier(&self) -> u16 {
        (u32::from_be(self.rest_of_header) >> 16) as u16
    }

    pub fn echo_sequence(&self) -> u16 {
        (u32::from_be(self.rest_of_header) & 0xFFFF) as u16
    }

    pub fn new(icmp_type: u8, code: u8) -> Self {
        IcmpHeader {
            icmp_type,
            code,
            checksum: 0,
            rest_of_header: 0,
        }
    }

    pub fn is_echo_request(&self) -> bool {
        self.icmp_type == ICMP_TYPE_ECHO_REQUEST && self.code == 0
    }

    pub fn is_echo_reply(&self) -> bool {
        self.icmp_type == ICMP_TYPE_ECHO_REPLY && self.code == 0
    }
}

pub fn compute_icmp_checksum(header: &IcmpHeader, data: &[u8]) -> u16 {
    let hdr_bytes = unsafe {
        core::slice::from_raw_parts(
            header as *const IcmpHeader as *const u8,
            core::mem::size_of::<IcmpHeader>(),
        )
    };
    let mut sum = 0u32;
    let mut i = 0;
    while i + 1 < hdr_bytes.len() {
        let word = u16::from_be_bytes([hdr_bytes[i], hdr_bytes[i + 1]]);
        sum = sum.wrapping_add(word as u32);
        i += 2;
    }
    i = 0;
    while i + 1 < data.len() {
        let word = u16::from_be_bytes([data[i], data[i + 1]]);
        sum = sum.wrapping_add(word as u32);
        i += 2;
    }
    if i < data.len() {
        sum = sum.wrapping_add((data[i] as u32) << 8);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

pub fn compute_icmp_checksum_raw(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut i = 0;
    while i + 1 < data.len() {
        let word = u16::from_be_bytes([data[i], data[i + 1]]);
        sum = sum.wrapping_add(word as u32);
        i += 2;
    }
    if i < data.len() {
        sum = sum.wrapping_add((data[i] as u32) << 8);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// Build an ICMP Destination Unreachable (Type 3, Code 3 — Port Unreachable).
/// Includes the original IP header + first 8 bytes of the offending payload.
pub fn build_port_unreachable(original_ip: &Ipv4Header, original_udp: &[u8]) -> Vec<u8> {
    let mut reply = Vec::with_capacity(8 + IPV4_HDR_MIN_LEN + 8);
    let hdr = IcmpHeader::new(ICMP_TYPE_DEST_UNREACH, 3);
    let hdr_bytes = unsafe {
        core::slice::from_raw_parts(
            &hdr as *const IcmpHeader as *const u8,
            core::mem::size_of::<IcmpHeader>(),
        )
    };
    reply.extend_from_slice(hdr_bytes);
    reply.extend_from_slice(&[0u8; 4]);
    let ip_bytes = unsafe {
        core::slice::from_raw_parts(
            original_ip as *const Ipv4Header as *const u8,
            IPV4_HDR_MIN_LEN,
        )
    };
    reply.extend_from_slice(ip_bytes);
    let udp_len = core::mem::size_of::<UdpHeader>().min(original_udp.len());
    reply.extend_from_slice(&original_udp[..udp_len]);
    let cs = compute_icmp_checksum_raw(&reply);
    reply[2] = (cs >> 8) as u8;
    reply[3] = (cs & 0xFF) as u8;
    reply
}

pub fn build_echo_reply(request: &IcmpHeader, data: &[u8]) -> alloc::vec::Vec<u8> {
    let mut reply = alloc::vec::Vec::with_capacity(core::mem::size_of::<IcmpHeader>() + data.len());
    let header = IcmpHeader::echo_reply(request.echo_identifier(), request.echo_sequence());
    let hdr_bytes = unsafe {
        core::slice::from_raw_parts(
            &header as *const IcmpHeader as *const u8,
            core::mem::size_of::<IcmpHeader>(),
        )
    };
    reply.extend_from_slice(hdr_bytes);
    reply.extend_from_slice(data);
    let checksum = compute_icmp_checksum(&header, data);
    let cs_bytes = checksum.to_be_bytes();
    reply[2] = cs_bytes[0];
    reply[3] = cs_bytes[1];
    reply
}
