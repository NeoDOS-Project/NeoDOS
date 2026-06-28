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
