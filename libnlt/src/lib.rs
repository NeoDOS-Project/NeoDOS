//! # libnlt — NeoDOS NLT (Neo Language Table) format library
//!
//! Pure, `no_std`, zero-dependency (except optional Ed25519) implementation of
//! the NLT binary format used by NeoDOS for internationalization.
//!
//! The same code is shared by:
//!   * `tools/nltc`   — the TOML → NLT compiler (host, std).
//!   * `libneodos`    — the Ring 3 runtime loader.
//!   * `userbin/neolocale` / `userbin/nxlocale` — inspection tools.
//!
//! Supported formats:
//!   * **NLTv2** — numeric-ID index, UTF-8 strings, binary search. Read-only
//!     backwards compatibility.
//!   * **NLTv3** — adds optional LZSS compression, UTF-16LE string storage,
//!     plural forms, regional-format metadata, RTL flag and an Ed25519
//!     signature block.
//!
//! All functions are allocation-free: every operation writes into a caller
//! supplied slice.

#![cfg_attr(not(test), no_std)]
#![allow(clippy::needless_range_loop)]

pub mod bidi;
pub mod format;
pub mod lang;
pub mod lzss;
pub mod plural;
pub mod region;
pub mod utf16;

#[cfg(feature = "signatures")]
pub mod signature;

// ── Format constants ───────────────────────────────────────────────────

/// NLTv2 magic (`"NLT2"`).
pub const MAGIC_V2: [u8; 4] = *b"NLT2";
/// NLTv3 magic (`"NLT3"`).
pub const MAGIC_V3: [u8; 4] = *b"NLT3";

/// Fixed size of the NLTv2 header.
pub const HEADER_V2_SIZE: usize = 32;
/// Size of the fixed part of the NLTv3 header.
pub const HEADER_V3_SIZE: usize = 64;

/// Size of one NLTv2 index entry: `id: u32, offset: u32`.
pub const ENTRY_V2_SIZE: usize = 8;
/// Size of one NLTv3 index entry: `id: u32, offset: u32, flags: u32`.
pub const ENTRY_V3_SIZE: usize = 12;

/// NLTv2 version number.
pub const VERSION_V2: u16 = 2;
/// NLTv3 version number.
pub const VERSION_V3: u16 = 3;

// ── NLTv3 flags ────────────────────────────────────────────────────────

/// Payload (index + string blob) is LZSS compressed.
pub const FLAG_COMPRESSED: u32 = 1 << 0;
/// String blob is stored as UTF-16LE instead of UTF-8.
pub const FLAG_UTF16: u32 = 1 << 1;
/// Locale is right-to-left.
pub const FLAG_RTL: u32 = 1 << 2;
/// A signature block is present.
pub const FLAG_SIGNED: u32 = 1 << 3;
/// A regional-format metadata block is present.
pub const FLAG_REGION: u32 = 1 << 4;
/// At least one entry is a plural group.
pub const FLAG_PLURAL: u32 = 1 << 5;

// ── Per-entry flags (NLTv3) ────────────────────────────────────────────

/// The entry's payload is a plural group:
/// `u8 category_count, u8 offsets[category_count] (relative to entry offset)`,
/// followed by the UTF-8 strings in category order.
pub const ENTRY_FLAG_PLURAL: u32 = 1 << 0;

/// Number of CLDR plural categories stored per plural group.
pub const PLURAL_CATEGORIES: usize = 6;

/// Maximum byte length of a single NLT table accepted by the runtime.
pub const MAX_TABLE_SIZE: usize = 16384;

// ── Header ─────────────────────────────────────────────────────────────

/// Parsed NLT header, normalised across v2 and v3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub version: u16,
    pub language_id: u32,
    pub application_id: u32,
    /// Number of index entries.
    pub entry_count: u32,
    /// v3 feature flags (0 for v2).
    pub flags: u32,
    /// Offset of the payload (index table) from the start of the file.
    pub payload_offset: u32,
    /// Uncompressed payload size in bytes.
    pub payload_size: u32,
    /// Stored payload size in bytes (equals `payload_size` when uncompressed).
    pub payload_stored: u32,
    /// CRC32 of the uncompressed payload.
    pub payload_crc32: u32,
    /// Offset of the regional-format block (0 when absent).
    pub region_offset: u32,
    pub region_size: u32,
    /// Offset of the signature block (0 when absent).
    pub signature_offset: u32,
    pub signature_size: u32,
}

impl Header {
    pub fn is_v3(&self) -> bool {
        self.version == VERSION_V3
    }
    pub fn is_compressed(&self) -> bool {
        self.flags & FLAG_COMPRESSED != 0
    }
    pub fn is_utf16(&self) -> bool {
        self.flags & FLAG_UTF16 != 0
    }
    pub fn is_signed(&self) -> bool {
        self.flags & FLAG_SIGNED != 0
    }
    pub fn has_region(&self) -> bool {
        self.flags & FLAG_REGION != 0
    }
    pub fn is_rtl(&self) -> bool {
        self.flags & FLAG_RTL != 0
    }
    /// Size of a single index entry for this header's version.
    pub fn entry_size(&self) -> usize {
        if self.is_v3() {
            ENTRY_V3_SIZE
        } else {
            ENTRY_V2_SIZE
        }
    }
    /// Total index size in bytes.
    pub fn index_size(&self) -> usize {
        self.entry_count as usize * self.entry_size()
    }
}

fn rd_u16(data: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([data[off], data[off + 1]])
}
fn rd_u32(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

/// Parse and validate an NLT header (v2 or v3).
///
/// Returns `None` on truncated data, bad magic, unsupported version or an
/// inconsistent index/payload size.
pub fn parse_header(data: &[u8]) -> Option<Header> {
    if data.len() < 8 {
        return None;
    }
    let magic = &data[..4];
    if magic == MAGIC_V2 {
        if data.len() < HEADER_V2_SIZE {
            return None;
        }
        let entry_count = rd_u32(data, 16);
        let index_size = entry_count as usize * ENTRY_V2_SIZE;
        let payload_offset = HEADER_V2_SIZE as u32;
        let payload_size = index_size as u32;
        if HEADER_V2_SIZE + index_size > data.len() {
            return None;
        }
        Some(Header {
            version: rd_u16(data, 4),
            language_id: rd_u32(data, 8),
            application_id: rd_u32(data, 12),
            entry_count,
            flags: 0,
            payload_offset,
            payload_size,
            payload_stored: payload_size,
            payload_crc32: 0,
            region_offset: 0,
            region_size: 0,
            signature_offset: 0,
            signature_size: 0,
        })
    } else if magic == MAGIC_V3 {
        if data.len() < HEADER_V3_SIZE {
            return None;
        }
        let version = rd_u16(data, 4);
        if version != VERSION_V3 {
            return None;
        }
        let entry_count = rd_u32(data, 16);
        let flags = rd_u32(data, 20);
        let payload_offset = rd_u32(data, 24);
        let payload_size = rd_u32(data, 28);
        let payload_stored = rd_u32(data, 32);
        let payload_crc32 = rd_u32(data, 36);
        let region_offset = rd_u32(data, 40);
        let region_size = rd_u32(data, 44);
        let signature_offset = rd_u32(data, 48);
        let signature_size = rd_u32(data, 52);

        // Index must fit in the (uncompressed) payload.
        let index_size = entry_count as usize * ENTRY_V3_SIZE;
        if index_size > payload_size as usize {
            return None;
        }
        // Stored payload must fit in the file.
        let po = payload_offset as usize;
        if po.checked_add(payload_stored as usize)? > data.len() {
            return None;
        }
        // Optional blocks must fit in the file.
        if region_size > 0 && (region_offset as usize + region_size as usize) > data.len() {
            return None;
        }
        if signature_size > 0 && (signature_offset as usize + signature_size as usize) > data.len() {
            return None;
        }
        Some(Header {
            version,
            language_id: rd_u32(data, 8),
            application_id: rd_u32(data, 12),
            entry_count,
            flags,
            payload_offset,
            payload_size,
            payload_stored,
            payload_crc32,
            region_offset,
            region_size,
            signature_offset,
            signature_size,
        })
    } else {
        None
    }
}

// ── Payload (index + strings) helpers ──────────────────────────────────
//
// A decoded payload is laid out as:
//   [ index entries ][ string blob ]
// For NLTv2 each index entry is `id: u32, offset: u32` (12 -> 8 bytes).
// For NLTv3 each index entry is `id: u32, offset: u32, flags: u32`.
// `offset` is relative to the start of the payload.

/// Read index entry `i` from a decoded payload.
///
/// Returns `(id, string_offset, entry_flags)`.
pub fn read_index(payload: &[u8], version: u16, i: usize) -> Option<(u32, u32, u32)> {
    let entry_size = if version == VERSION_V3 {
        ENTRY_V3_SIZE
    } else {
        ENTRY_V2_SIZE
    };
    let off = i.checked_mul(entry_size)?;
    if off + entry_size > payload.len() {
        return None;
    }
    let id = rd_u32(payload, off);
    let str_off = rd_u32(payload, off + 4);
    let flags = if version == VERSION_V3 {
        rd_u32(payload, off + 8)
    } else {
        0
    };
    Some((id, str_off, flags))
}

/// Write an NLTv3 index entry into a mutable payload.
pub fn write_index_v3(payload: &mut [u8], i: usize, id: u32, str_off: u32, flags: u32) {
    let off = i * ENTRY_V3_SIZE;
    payload[off..off + 4].copy_from_slice(&id.to_le_bytes());
    payload[off + 4..off + 8].copy_from_slice(&str_off.to_le_bytes());
    payload[off + 8..off + 12].copy_from_slice(&flags.to_le_bytes());
}

/// Binary-search a decoded payload for `id`.
///
/// The index is required to be sorted by ID (the compiler guarantees this).
/// Returns `(string_offset, entry_flags)`.
pub fn find_entry(payload: &[u8], version: u16, entry_count: u32, id: u32) -> Option<(u32, u32)> {
    if entry_count == 0 {
        return None;
    }
    let mut lo: i64 = 0;
    let mut hi: i64 = entry_count as i64 - 1;
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        let (mid_id, off, flags) = read_index(payload, version, mid as usize)?;
        if mid_id == id {
            return Some((off, flags));
        } else if mid_id < id {
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }
    None
}

/// Return a NUL-terminated UTF-8 string at `off` within `payload`.
pub fn str_at(payload: &[u8], off: u32) -> Option<&str> {
    let start = off as usize;
    if start >= payload.len() {
        return None;
    }
    let end = payload[start..].iter().position(|&b| b == 0)?;
    core::str::from_utf8(&payload[start..start + end]).ok()
}

/// Look up a single (non-plural) string by ID.
pub fn lookup(payload: &[u8], version: u16, entry_count: u32, id: u32) -> Option<&str> {
    let (off, _flags) = find_entry(payload, version, entry_count, id)?;
    str_at(payload, off)
}

// ── CRC32 (IEEE 802.3, same polynomial as ZIP/PNG) ─────────────────────

/// Compute CRC32 over `data`.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

// ── High-level decode ──────────────────────────────────────────────────

/// A decoded NLT payload.
#[derive(Debug, Clone, Copy)]
pub struct Decoded<'a> {
    pub header: Header,
    /// Decoded payload: `[index][UTF-8 string blob]`.
    pub payload: &'a [u8],
    /// Raw regional-format block (copied out of the file), if present.
    pub region_raw: Option<&'a [u8]>,
}

impl<'a> Decoded<'a> {
    pub fn lookup(&self, id: u32) -> Option<&str> {
        lookup(self.payload, self.header.version, self.header.entry_count, id)
    }
    /// Look up a plural group and select the form for `n`.
    pub fn plural(&self, id: u32, n: u64) -> Option<&str> {
        let (off, flags) = find_entry(
            self.payload,
            self.header.version,
            self.header.entry_count,
            id,
        )?;
        if flags & ENTRY_FLAG_PLURAL == 0 {
            return str_at(self.payload, off);
        }
        plural::select(self.payload, off, self.header.language_id, n)
    }
    /// Look up a regional-format block (NLTv3 only).
    pub fn region(&self) -> Option<region::Region<'a>> {
        self.region_raw.and_then(region::parse)
    }
}

/// Decode an NLT file with no dynamic allocation.
///
/// * `data`    — raw file bytes.
/// * `scratch` — scratch buffer used for LZSS decompression (may be empty when
///   the table is not compressed).
/// * `out`     — buffer that receives the decoded `[index][UTF-8 strings]`
///   payload followed by any regional-format block.
///
/// Returns the decoded view borrowing `out`.
pub fn decode<'a>(
    data: &[u8],
    scratch: &mut [u8],
    out: &'a mut [u8],
) -> Option<Decoded<'a>> {
    let header = parse_header(data)?;
    let po = header.payload_offset as usize;
    let stored = &data[po..po + header.payload_stored as usize];

    // 1. Materialise the uncompressed payload into `scratch` (or use as-is).
    let payload: &[u8] = if header.is_compressed() {
        let n = lzss::decompress(stored, scratch)?;
        if n != header.payload_size as usize {
            return None;
        }
        &scratch[..n]
    } else {
        if stored.len() != header.payload_size as usize {
            return None;
        }
        stored
    };

    // 2. Verify integrity when a checksum is present.
    if header.is_v3() && crc32(payload) != header.payload_crc32 {
        return None;
    }

    // 3. Transcode UTF-16 -> UTF-8 and rewrite the index offsets.
    let payload_len = if header.is_utf16() {
        utf16::rewrite_index_utf16(payload, header.version, header.entry_count, out)?
    } else {
        if out.len() < payload.len() {
            return None;
        }
        out[..payload.len()].copy_from_slice(payload);
        payload.len()
    };

    // 4. Copy the optional regional-format block right after the payload.
    let region_raw = if header.has_region() && header.region_size > 0 {
        let start = header.region_offset as usize;
        let end = start + header.region_size as usize;
        if end > data.len() || payload_len + header.region_size as usize > out.len() {
            return None;
        }
        out[payload_len..payload_len + header.region_size as usize]
            .copy_from_slice(&data[start..end]);
        Some(&out[payload_len..payload_len + header.region_size as usize])
    } else {
        None
    };

    Some(Decoded {
        header,
        payload: &out[..payload_len],
        region_raw,
    })
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_rejects_garbage() {
        assert!(parse_header(b"").is_none());
        assert!(parse_header(b"XXXX").is_none());
        assert!(parse_header(b"NLT3").is_none());
    }

    #[test]
    fn crc32_known_vector() {
        // CRC32("123456789") = 0xCBF43926
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn lookup_binary_search() {
        // payload: 3 entries (id, off, flags) + strings
        let mut p = [0u8; 64];
        // index (12 bytes each)
        write_index_v3(&mut p, 0, 10, 36, 0);
        write_index_v3(&mut p, 1, 20, 40, 0);
        write_index_v3(&mut p, 2, 30, 45, 0);
        p[36..40].copy_from_slice(b"ten\0");
        p[40..45].copy_from_slice(b"twen\0");
        p[45..50].copy_from_slice(b"thir\0");
        assert_eq!(lookup(&p[..50], VERSION_V3, 3, 20), Some("twen"));
        assert_eq!(lookup(&p[..50], VERSION_V3, 3, 99), None);
    }
}
