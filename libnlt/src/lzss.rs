//! LZSS compression for NLTv3 payloads (`I18N-P7`).
//!
//! Byte-oriented LZSS with a 4 KiB sliding window, minimum match length 3 and
//! maximum match length 18. Tokens are packed 8 to a flag byte (MSB first):
//!
//! ```text
//! flag bit = 1 → literal: 1 byte follows
//! flag bit = 0 → match:   2 bytes follow
//!                          b0 = offset >> 4
//!                          b1 = (offset & 0x0F) << 4 | (length - 3)
//! ```
//!
//! The implementation is deliberately small and allocation-free; it is only
//! used by the host compiler, so raw speed is secondary to determinism.

const WINDOW_SIZE: usize = 4096;
const WINDOW_MASK: usize = WINDOW_SIZE - 1;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 18;
const MAX_INPUT: usize = 16384;

/// Worst-case compressed size for `src_len` bytes.
pub fn compress_bound(src_len: usize) -> usize {
    src_len + src_len / 8 + 16
}

/// Compress `src` into `dst`.
///
/// Returns the number of bytes written, or `None` if `dst` is too small or the
/// input exceeds [`MAX_INPUT`].
pub fn compress(src: &[u8], dst: &mut [u8]) -> Option<usize> {
    if src.len() > MAX_INPUT {
        return None;
    }
    if src.is_empty() {
        return Some(0);
    }
    if dst.len() < compress_bound(src.len()) {
        return None;
    }

    let mut head = [u16::MAX; 256];
    let mut prev = [u16::MAX; MAX_INPUT];

    let mut out = 0usize;
    let mut flag_pos = 0usize;
    let mut flag: u8 = 0;
    let mut flag_bits = 0u8;

    let mut i = 0usize;
    while i < src.len() {
        // Determine the best match at position i.
        let mut best_len = 0usize;
        let mut best_off = 0usize;

        if i + MIN_MATCH <= src.len() {
            let h = hash3(&src[i..]);
            let mut candidate = head[h];
            let mut chain = 0;
            while candidate != u16::MAX && chain < 32 {
                let c = candidate as usize;
                if c >= i {
                    break;
                }
                let max_len = (src.len() - i).min(MAX_MATCH);
                let mut l = 0usize;
                while l < max_len && src[c + l] == src[i + l] {
                    l += 1;
                }
                if l > best_len && l >= MIN_MATCH {
                    best_len = l;
                    best_off = i - c;
                    if l == MAX_MATCH {
                        break;
                    }
                }
                candidate = prev[c];
                chain += 1;
            }
            // Insert current position into the hash chain.
            prev[i] = head[h];
            head[h] = i as u16;
        }

        if flag_bits == 0 {
            flag_pos = out;
            out += 1; // reserve flag byte
            flag = 0;
        }

        if best_len >= MIN_MATCH {
            // match token
            let off = best_off & WINDOW_MASK;
            let len = best_len - MIN_MATCH;
            dst[out] = (off >> 4) as u8;
            dst[out + 1] = (((off & 0x0F) << 4) as u8) | (len as u8);
            out += 2;
            // Insert skipped positions into the hash chain so future matches
            // can reference them.
            for k in 1..best_len {
                let p = i + k;
                if p + MIN_MATCH <= src.len() {
                    let h = hash3(&src[p..]);
                    prev[p] = head[h];
                    head[h] = p as u16;
                }
            }
            i += best_len;
        } else {
            flag |= 1 << (7 - flag_bits);
            dst[out] = src[i];
            out += 1;
            i += 1;
        }

        flag_bits += 1;
        if flag_bits == 8 {
            dst[flag_pos] = flag;
            flag_bits = 0;
        }
    }

    if flag_bits > 0 {
        dst[flag_pos] = flag;
    }

    Some(out)
}

/// Decompress `src` into `dst`, returning the number of bytes produced.
///
/// Returns `None` on truncated or malformed input.
pub fn decompress(src: &[u8], dst: &mut [u8]) -> Option<usize> {
    let mut out = 0usize;
    let mut i = 0usize;

    while i < src.len() {
        let flag = src[i];
        i += 1;
        for bit in 0..8 {
            if i >= src.len() {
                return Some(out);
            }
            if flag & (1 << (7 - bit)) != 0 {
                // literal
                if out >= dst.len() {
                    return None;
                }
                dst[out] = src[i];
                out += 1;
                i += 1;
            } else {
                // match
                if i + 1 >= src.len() {
                    return None;
                }
                let b0 = src[i] as usize;
                let b1 = src[i + 1] as usize;
                i += 2;
                let off = (b0 << 4) | (b1 >> 4);
                let len = (b1 & 0x0F) + MIN_MATCH;
                if off == 0 || off > out {
                    return None;
                }
                let start = out - off;
                if out + len > dst.len() {
                    return None;
                }
                for k in 0..len {
                    dst[out + k] = dst[start + k];
                }
                out += len;
            }
        }
    }
    Some(out)
}

fn hash3(s: &[u8]) -> usize {
    let a = s[0] as usize;
    let b = if s.len() > 1 { s[1] as usize } else { 0 };
    let c = if s.len() > 2 { s[2] as usize } else { 0 };
    ((a << 8) ^ (b << 4) ^ c) & 0xFF
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(data: &[u8]) {
        let mut comp = [0u8; 4096];
        let mut out = [0u8; 16384];
        let n = compress(data, &mut comp).expect("compress");
        let m = decompress(&comp[..n], &mut out).expect("decompress");
        assert_eq!(&out[..m], data);
    }

    #[test]
    fn empty() {
        let mut comp = [0u8; 64];
        assert_eq!(compress(&[], &mut comp), Some(0));
    }

    #[test]
    fn literal_only() {
        roundtrip(b"abcdefgh"); // shorter than min match
    }

    #[test]
    fn repetitive_compresses() {
        let data = b"abcabcabcabcabcabcabcabcabcabc";
        roundtrip(data);
        let mut comp = [0u8; 4096];
        let n = compress(data, &mut comp).unwrap();
        assert!(n < data.len(), "expected compression: {n} >= {}", data.len());
    }

    #[test]
    fn long_mixed() {
        let mut data = [0u8; 3000];
        for (i, b) in data.iter_mut().enumerate() {
            *b = ((i * 7) % 251) as u8;
        }
        roundtrip(&data);
    }
}
