//! Language and application ID tables shared by the compiler (`nltc`) and the
//! runtime. IDs are stable: changing one breaks existing `.nlt` files.
//!
//! Known language tags map to small numeric IDs; unknown tags use a CRC32-based
//! hash with the top bit set (`0x8000 | (crc & 0x7FFF)`).

use crate::crc32;

/// Numeric language ID for an IETF tag such as `"es-ES"`.
pub fn lang_to_id(lang: &str) -> u32 {
    // Case-insensitive match without allocating.
    let mut lower = [0u8; 16];
    let n = lang.len().min(16);
    for (i, b) in lang.as_bytes()[..n].iter().enumerate() {
        lower[i] = b.to_ascii_lowercase();
    }
    let l = &lower[..n];
    match l {
        b"en-us" => 1,
        b"es-es" => 2,
        b"fr-fr" => 3,
        b"de-de" => 4,
        b"it-it" => 5,
        b"pt-pt" => 6,
        b"pt-br" => 7,
        b"ca-es" => 8,
        b"eu-es" => 9,
        b"gl-es" => 10,
        b"en-gb" => 11,
        b"ja-jp" => 12,
        b"zh-cn" => 13,
        b"ru-ru" => 14,
        b"ar-sa" => 15,
        b"nl-nl" => 16,
        b"pl-pl" => 17,
        b"sv-se" => 18,
        b"da-dk" => 19,
        b"fi-fi" => 20,
        b"nb-no" => 21,
        b"ko-kr" => 22,
        b"tr-tr" => 23,
        b"cs-cz" => 24,
        b"hu-hu" => 25,
        _ => (crc32(l) & 0x7FFF) | 0x8000,
    }
}

/// Canonical tag for a numeric language ID.
pub fn id_to_lang(id: u32) -> &'static str {
    match id {
        1 => "en-US",
        2 => "es-ES",
        3 => "fr-FR",
        4 => "de-DE",
        5 => "it-IT",
        6 => "pt-PT",
        7 => "pt-BR",
        8 => "ca-ES",
        9 => "eu-ES",
        10 => "gl-ES",
        11 => "en-GB",
        12 => "ja-JP",
        13 => "zh-CN",
        14 => "ru-RU",
        15 => "ar-SA",
        16 => "nl-NL",
        17 => "pl-PL",
        18 => "sv-SE",
        19 => "da-DK",
        20 => "fi-FI",
        21 => "nb-NO",
        22 => "ko-KR",
        23 => "tr-TR",
        24 => "cs-CZ",
        25 => "hu-HU",
        _ => "unknown",
    }
}

/// Numeric application ID for an app name.
pub fn app_to_id(app: &str) -> u32 {
    let mut lower = [0u8; 32];
    let n = app.len().min(32);
    for (i, b) in app.as_bytes()[..n].iter().enumerate() {
        lower[i] = b.to_ascii_lowercase();
    }
    let l = &lower[..n];
    match l {
        b"neoshell" => 1,
        b"neoinit" => 2,
        b"corehelp" => 3,
        b"coredir" => 4,
        b"corecopy" => 5,
        b"coretype" => 6,
        b"neolocale" => 7,
        b"neokey" => 8,
        b"neomem" => 9,
        b"neotop" => 10,
        b"kill" => 11,
        b"ps" => 12,
        b"label" => 13,
        b"fsck" => 14,
        b"poweroff" => 16,
        b"reboot" => 17,
        b"datetime" => 19,
        b"ver" => 20,
        b"echo" => 21,
        b"drives" => 22,
        b"pri" => 23,
        b"cd" => 24,
        b"colors" => 25,
        b"progress" => 26,
        b"vol" => 27,
        b"corerd" => 28,
        b"coremd" => 29,
        b"coreren" => 30,
        b"coredel" => 31,
        b"corecls" => 32,
        b"tree" => 33,
        b"dhcpd" => 34,
        b"netcfg" => 35,
        b"ipconfig" => 36,
        b"cpuinfo" => 37,
        b"stresscmd" => 38,
        b"cmdtest" => 39,
        b"shtest" => 40,
        b"nxlocale" => 41,
        b"nxres" => 42,
        b"nxverify" => 43,
        b"hostname" => 44,
        b"keyb" => 45,
        b"nslookup" => 46,
        b"ping" => 47,
        b"dhcptest" => 48,
        b"ntpd" => 49,
        b"netd" => 50,
        b"netapplier" => 51,
        _ => (crc32(l) & 0x7FFF) | 0x8000,
    }
}

/// Canonical locale tag for a bare language code, e.g. `"es"` → `"es-ES"`.
///
/// Used by [`fallback_chain`] because the language table is keyed by full tags.
fn canonical_for_language(lang: &str) -> Option<&'static str> {
    let mut lower = [0u8; 8];
    let n = lang.len().min(8);
    for (i, b) in lang.as_bytes()[..n].iter().enumerate() {
        lower[i] = b.to_ascii_lowercase();
    }
    match &lower[..n] {
        b"en" => Some("en-US"),
        b"es" => Some("es-ES"),
        b"ca" => Some("ca-ES"),
        b"eu" => Some("eu-ES"),
        b"gl" => Some("gl-ES"),
        b"fr" => Some("fr-FR"),
        b"de" => Some("de-DE"),
        b"it" => Some("it-IT"),
        b"pt" => Some("pt-PT"),
        b"ja" => Some("ja-JP"),
        b"zh" => Some("zh-CN"),
        b"ru" => Some("ru-RU"),
        b"ar" => Some("ar-SA"),
        b"nl" => Some("nl-NL"),
        b"pl" => Some("pl-PL"),
        b"sv" => Some("sv-SE"),
        b"da" => Some("da-DK"),
        b"fi" => Some("fi-FI"),
        b"nb" => Some("nb-NO"),
        b"ko" => Some("ko-KR"),
        b"tr" => Some("tr-TR"),
        b"cs" => Some("cs-CZ"),
        b"hu" => Some("hu-HU"),
        _ => None,
    }
}

/// Fallback locale chain for `tag`, most specific first, deduplicated (#580).
///
/// `"es-MX"` → `["es-MX", "es", "es-ES", "en-US"]`; `"en-US"` → `["en-US", "en"]`.
/// The canonical tag comes from [`canonical_for_language`] (or the language
/// table for already-canonical tags), so region-less tags still resolve.
pub fn fallback_chain(tag: &str) -> ([&str; 4], usize) {
    let lang_only = match tag.find('-') {
        Some(i) => &tag[..i],
        None => tag,
    };
    let canonical = match canonical_for_language(lang_only) {
        Some(c) => c,
        None => id_to_lang(lang_to_id(lang_only)),
    };
    let candidates = [tag, lang_only, canonical, "en-US"];

    let mut buf: [&str; 4] = [""; 4];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < candidates.len() {
        let c = candidates[i];
        if !c.is_empty() && c != "unknown" && n < 4 && !buf[..n].contains(&c) {
            buf[n] = c;
            n += 1;
        }
        i += 1;
    }
    (buf, n)
}

/// Human-readable English name for a known language ID.
pub fn lang_name(id: u32) -> &'static str {
    match id {
        1 => "English",
        2 => "Spanish",
        3 => "French",
        4 => "German",
        5 => "Italian",
        6 => "Portuguese",
        7 => "Brazilian Portuguese",
        8 => "Catalan",
        9 => "Basque",
        10 => "Galician",
        11 => "British English",
        12 => "Japanese",
        13 => "Chinese (Simplified)",
        14 => "Russian",
        15 => "Arabic",
        16 => "Dutch",
        17 => "Polish",
        18 => "Swedish",
        19 => "Danish",
        20 => "Finnish",
        21 => "Norwegian Bokmål",
        22 => "Korean",
        23 => "Turkish",
        24 => "Czech",
        25 => "Hungarian",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_tags() {
        assert_eq!(lang_to_id("en-US"), 1);
        assert_eq!(lang_to_id("es-es"), 2); // case-insensitive
        assert_eq!(lang_to_id("ca-ES"), 8);
        assert_eq!(id_to_lang(8), "ca-ES");
    }

    #[test]
    fn unknown_tags_hash() {
        let id = lang_to_id("xx-XX");
        assert!(id & 0x8000 != 0);
        assert_eq!(id, lang_to_id("xx-XX"));
    }

    #[test]
    fn app_ids() {
        assert_eq!(app_to_id("neoshell"), 1);
        assert_eq!(app_to_id("NXLOCALE"), 41);
        assert_eq!(app_to_id("neolocale"), 7);
        assert!(app_to_id("made-up") & 0x8000 != 0);
    }

    #[test]
    fn known_app_ids_are_unique_and_low() {
        // Keep in sync with the `match` in `app_to_id` (#583).
        const KNOWN: &[&str] = &[
            "neoshell", "neoinit", "corehelp", "coredir", "corecopy", "coretype",
            "neolocale", "neokey", "neomem", "neotop", "kill", "ps", "label",
            "fsck", "poweroff", "reboot", "datetime", "ver", "echo", "drives",
            "pri", "cd", "colors", "progress", "vol", "corerd", "coremd",
            "coreren", "coredel", "corecls", "tree", "dhcpd", "netcfg",
            "ipconfig", "cpuinfo", "stresscmd", "cmdtest", "shtest", "nxlocale",
            "nxres", "nxverify", "hostname", "keyb", "nslookup", "ping",
            "dhcptest", "ntpd", "netd", "netapplier",
        ];
        let mut ids: Vec<u32> = KNOWN.iter().map(|a| app_to_id(a)).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate known app id");
        for (app, id) in KNOWN.iter().zip(KNOWN.iter().map(|a| app_to_id(a))) {
            assert!(id < 0x8000, "{app} id {id:#x} collides with the CRC range");
        }
    }

    #[test]
    fn app_id_is_case_insensitive_and_stable() {
        assert_eq!(app_to_id("NeoCfg"), app_to_id("neocfg"));
        assert_eq!(app_to_id("made-up"), app_to_id("MADE-UP"));
    }

    #[test]
    fn known_langs_round_trip() {
        for tag in ["en-US", "es-ES", "ca-ES", "fr-FR", "de-DE", "ja-JP", "ar-SA"] {
            let id = lang_to_id(tag);
            assert_eq!(id_to_lang(id), tag, "round-trip failed for {tag}");
            assert!(!lang_name(id).is_empty());
        }
    }

    #[test]
    fn fallback_chain_negotiates_region_then_language_then_default() {
        let (c, n) = fallback_chain("es-MX");
        assert_eq!(&c[..n], &["es-MX", "es", "es-ES", "en-US"]);

        let (c, n) = fallback_chain("en-US");
        assert_eq!(&c[..n], &["en-US", "en"]);

        let (c, n) = fallback_chain("ca");
        assert_eq!(&c[..n], &["ca", "ca-ES", "en-US"]);

        let (c, n) = fallback_chain("xx-YY");
        // Unknown language with a region: unknown canonical is dropped.
        assert_eq!(&c[..n], &["xx-YY", "xx", "en-US"]);
    }
}
