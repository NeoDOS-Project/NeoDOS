use super::types::Ipv4Addr;

pub const IPV4_HDR_MIN_LEN: usize = 20;
pub const IPV4_DEFAULT_TTL: u8 = 64;
pub const IPV4_PROTO_ICMP: u8 = 1;
pub const IPV4_PROTO_TCP: u8 = 6;
pub const IPV4_PROTO_UDP: u8 = 17;

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Ipv4Header {
    pub version_ihl: u8,
    pub dscp_ecn: u8,
    pub total_length: u16,
    pub identification: u16,
    pub flags_fragment: u16,
    pub ttl: u8,
    pub protocol: u8,
    pub header_checksum: u16,
    pub src: [u8; 4],
    pub dst: [u8; 4],
}

impl Ipv4Header {
    pub fn version(&self) -> u8 { self.version_ihl >> 4 }
    pub fn ihl(&self) -> u8 { self.version_ihl & 0x0F }
    pub fn header_len(&self) -> usize { (self.ihl() as usize) * 4 }
    pub fn total_len(&self) -> usize { u16::from_be(self.total_length) as usize }
    pub fn payload_len(&self) -> usize { self.total_len().saturating_sub(self.header_len()) }
    pub fn ident(&self) -> u16 { u16::from_be(self.identification) }
    pub fn ttl(&self) -> u8 { self.ttl }
    pub fn protocol(&self) -> u8 { self.protocol }
    pub fn src_ip(&self) -> Ipv4Addr { Ipv4Addr(self.src) }
    pub fn dst_ip(&self) -> Ipv4Addr { Ipv4Addr(self.dst) }
    pub fn checksum(&self) -> u16 { u16::from_be(self.header_checksum) }

    pub fn set_src(&mut self, ip: Ipv4Addr) { self.src = ip.0; }
    pub fn set_dst(&mut self, ip: Ipv4Addr) { self.dst = ip.0; }

    pub fn is_valid(&self) -> bool {
        self.version() == 4 && self.ihl() >= 5 && self.total_len() >= IPV4_HDR_MIN_LEN
    }

    pub fn src_ip_octets(&self) -> &[u8] { &self.src }
    pub fn dst_ip_octets(&self) -> &[u8] { &self.dst }
}

pub fn compute_ip_checksum(header: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut i = 0;
    while i + 1 < header.len() {
        let word = u16::from_be_bytes([header[i], header[i + 1]]);
        sum = sum.wrapping_add(word as u32);
        i += 2;
    }
    if i < header.len() {
        sum = sum.wrapping_add((header[i] as u32) << 8);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

pub fn build_ipv4_header(
    src: Ipv4Addr, dst: Ipv4Addr, protocol: u8, payload_len: usize,
    identification: u16,
) -> Ipv4Header {
    let total_len = (IPV4_HDR_MIN_LEN + payload_len) as u16;
    let mut hdr = Ipv4Header {
        version_ihl: 0x45,
        dscp_ecn: 0,
        total_length: total_len.to_be(),
        identification: identification.to_be(),
        flags_fragment: 0x4000u16.to_be(),
        ttl: IPV4_DEFAULT_TTL,
        protocol,
        header_checksum: 0,
        src: src.0,
        dst: dst.0,
    };
    let checksum = compute_ip_checksum(
        unsafe { core::mem::transmute::<&Ipv4Header, &[u8; IPV4_HDR_MIN_LEN]>(&hdr) }
    );
    hdr.header_checksum = checksum.to_be();
    hdr
}

pub fn ip_fragment_needed(total_len: usize, mtu: usize) -> bool {
    total_len > mtu
}
