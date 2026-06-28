pub const UDP_HDR_LEN: usize = 8;

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct UdpHeader {
    pub src_port: u16,
    pub dst_port: u16,
    pub length: u16,
    pub checksum: u16,
}

impl UdpHeader {
    pub fn new(src_port: u16, dst_port: u16, payload_len: usize) -> Self {
        UdpHeader {
            src_port: src_port.to_be(),
            dst_port: dst_port.to_be(),
            length: ((UDP_HDR_LEN + payload_len) as u16).to_be(),
            checksum: 0,
        }
    }

    pub fn src_port(&self) -> u16 { u16::from_be(self.src_port) }
    pub fn dst_port(&self) -> u16 { u16::from_be(self.dst_port) }
    pub fn len(&self) -> usize { u16::from_be(self.length) as usize }
    pub fn payload_len(&self) -> usize { self.len().saturating_sub(UDP_HDR_LEN) }
}

pub fn compute_udp_checksum(header: &UdpHeader, src_ip: [u8; 4], dst_ip: [u8; 4], payload: &[u8]) -> u16 {
    let mut sum = 0u32;

    // Pseudo-header: src_ip(4) + dst_ip(4) + zero(1) + protocol(1) + UDP length(2)
    sum = sum.wrapping_add((src_ip[0] as u32) << 8 | src_ip[1] as u32);
    sum = sum.wrapping_add((src_ip[2] as u32) << 8 | src_ip[3] as u32);
    sum = sum.wrapping_add((dst_ip[0] as u32) << 8 | dst_ip[1] as u32);
    sum = sum.wrapping_add((dst_ip[2] as u32) << 8 | dst_ip[3] as u32);
    sum = sum.wrapping_add(17u32); // UDP protocol
    let udp_len = (UDP_HDR_LEN + payload.len()) as u16;
    sum = sum.wrapping_add(udp_len as u32);

    // UDP header + payload
    let hdr_bytes = unsafe {
        core::slice::from_raw_parts(
            header as *const UdpHeader as *const u8,
            core::mem::size_of::<UdpHeader>(),
        )
    };
    let mut i = 0;
    while i + 1 < hdr_bytes.len() {
        let word = u16::from_be_bytes([hdr_bytes[i], hdr_bytes[i + 1]]);
        sum = sum.wrapping_add(word as u32);
        i += 2;
    }
    i = 0;
    while i + 1 < payload.len() {
        let word = u16::from_be_bytes([payload[i], payload[i + 1]]);
        sum = sum.wrapping_add(word as u32);
        i += 2;
    }
    if i < payload.len() {
        sum = sum.wrapping_add((payload[i] as u32) << 8);
    }

    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    let cs = !(sum as u16);
    if cs == 0 { 0xFFFF } else { cs }
}
