//! Kernel-side integration tests for the shared NLT format library.
//!
//! `libnlt` is the single source of truth for the NLT binary format used by
//! the runtime (`libneodos::i18n`), the compiler (`nltc`) and the tools. These
//! tests run under the in-kernel harness and exercise the pure format logic
//! (no filesystem, no syscalls).

use libnlt::{crc32, lzss, plural, region, utf16, ENTRY_FLAG_PLURAL, VERSION_V3};

use crate::{test_case, test_eq, test_true};

// Build a minimal NLTv3 payload: `[index][strings]`.
fn small_payload() -> ([u8; 64], u32) {
    let mut p = [0u8; 64];
    libnlt::write_index_v3(&mut p, 0, 10, 36, 0);
    libnlt::write_index_v3(&mut p, 1, 20, 40, 0);
    libnlt::write_index_v3(&mut p, 2, 30, 45, 0);
    p[36..40].copy_from_slice(b"ten\0");
    p[40..45].copy_from_slice(b"twen\0");
    p[45..50].copy_from_slice(b"thir\0");
    (p, 3)
}

pub fn register_i18n_tests() {
    test_case!("i18n_lookup_binary_search", {
        let (p, count) = small_payload();
        test_eq!(libnlt::lookup(&p[..50], VERSION_V3, count, 10), Some("ten"));
        test_eq!(libnlt::lookup(&p[..50], VERSION_V3, count, 30), Some("thir"));
    });

    test_case!("i18n_lookup_missing_returns_none", {
        let (p, count) = small_payload();
        test_eq!(libnlt::lookup(&p[..50], VERSION_V3, count, 99), None);
    });

    test_case!("i18n_crc32_known_vector", {
        test_eq!(crc32(b"123456789"), 0xCBF4_3926);
    });

    test_case!("i18n_lzss_roundtrip", {
        let data = b"abcabcabcabcabcabcabcabcabcabc";
        let mut comp = [0u8; 512];
        let mut out = [0u8; 512];
        let n = lzss::compress(data, &mut comp).ok_or("compress failed")?;
        test_true!(n < data.len());
        let m = lzss::decompress(&comp[..n], &mut out).ok_or("decompress failed")?;
        test_eq!(&out[..m], &data[..]);
    });

    test_case!("i18n_plural_english_rules", {
        test_eq!(plural::category(1, 1), plural::ONE);
        test_eq!(plural::category(1, 2), plural::OTHER);
    });

    test_case!("i18n_plural_select_falls_back", {
        // group with 6 slots, only "other" populated
        let mut p = [0u8; 64];
        p[0] = 6;
        let mut cursor = 7usize;
        for c in 0..6usize {
            p[1 + c] = cursor as u8;
            let text: &[u8] = if c == plural::OTHER { b"files\0" } else { b"\0" };
            p[cursor..cursor + text.len()].copy_from_slice(text);
            cursor += text.len();
        }
        test_eq!(plural::select(&p, 0, 1, 3), Some("files"));
    });

    test_case!("i18n_format_placeholders", {
        let mut buf = [0u8; 64];
        let n = libnlt::format::format_template("a{0}b{1:c}", &["X", "Y"], &mut buf);
        test_eq!(core::str::from_utf8(&buf[..n]), Ok("aXb{1:c}"));
    });

    test_case!("i18n_format_specifiers", {
        let mut buf = [0u8; 32];
        let n = libnlt::format::format_template("{0:x}", &["255"], &mut buf);
        test_eq!(core::str::from_utf8(&buf[..n]), Ok("ff"));
    });

    test_case!("i18n_region_number_grouping", {
        let mut raw = [0u8; 64];
        let n = region::encode(
            ",", ".", "€", "dd/MM/yyyy", "d MMMM yyyy", "HH:mm:ss", true, false, true, &mut raw,
        )
        .ok_or("encode failed")?;
        let r = region::parse(&raw[..n]).ok_or("parse failed")?;
        let mut out = [0u8; 32];
        let m = region::format_number(1234567, &r, &mut out);
        test_eq!(core::str::from_utf8(&out[..m]), Ok("1.234.567"));
    });

    test_case!("i18n_bidi_reorders_rtl", {
        let input = "\u{05D0}\u{05D1}\u{05D2}";
        let mut out = [0u8; 32];
        let n = libnlt::bidi::reorder_visual(input, libnlt::bidi::Direction::Rtl, &mut out);
        test_eq!(
            core::str::from_utf8(&out[..n]),
            Ok("\u{05D2}\u{05D1}\u{05D0}")
        );
    });

    test_case!("i18n_utf16_surrogate_transcode", {
        let src = [0x3D, 0xD8, 0x00, 0xDE, 0x00, 0x00];
        let mut dst = [0u8; 8];
        let n = utf16::transcode_one(&src, 0, &mut dst).ok_or("transcode failed")?;
        test_eq!(core::str::from_utf8(&dst[..n]), Ok("\u{1F600}"));
    });

    test_case!("i18n_header_rejects_garbage", {
        test_true!(libnlt::parse_header(b"XXXX").is_none());
        test_true!(libnlt::parse_header(b"NLT3").is_none());
    });

    test_case!("i18n_entry_flag_plural_constant", {
        test_eq!(ENTRY_FLAG_PLURAL, 1);
    });
}
