pub fn unicode_to_utf8(codepoint: u16) -> [u8; 4] {
    let mut buf = [0u8; 4];
    let cp = codepoint as u32;
    if cp < 0x80 {
        buf[0] = cp as u8;
    } else if cp < 0x800 {
        buf[0] = 0xC0 | (cp >> 6) as u8;
        buf[1] = 0x80 | (cp & 0x3F) as u8;
    } else {
        buf[0] = 0xE0 | (cp >> 12) as u8;
        buf[1] = 0x80 | ((cp >> 6) & 0x3F) as u8;
        buf[2] = 0x80 | (cp & 0x3F) as u8;
    }
    buf
}
