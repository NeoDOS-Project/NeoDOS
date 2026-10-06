# NLT (Neo Language Table) — Reference

> Binary internationalization format of NeoDOS. Shared format library:
> `libnlt`. Compiler: `tools/nltc`. Runtime: `libneodos/src/i18n.rs`.
>
> **Supersedes:** the former `docs/design/i18n-design.md` (NLTv1/string-key),
> removed when the numeric-ID NLTv2 format landed. This document is the single
> source of truth for the format.

---

## 1. TOML Source Format

Developers edit `.toml` sources. `nltc` compiles them to the `.nlt` binary.

```toml
[meta]
app = "neoshell"
language = "es-ES"

[ids]
IDS_OK = 1001
IDS_FILES = 1002

[strings]
IDS_OK = "Aceptar"
IDS_DATE = "Fecha: {0}"

# Optional: plural groups (CLDR categories: zero/one/two/few/many/other)
[plural.IDS_FILES]
one = "{0} archivo"
other = "{0} archivos"

# Optional: regional formats (dates, numbers, currency)
[region]
decimal_separator = ","
thousands_separator = "."
currency_symbol = "€"
date_short = "dd/MM/yyyy"
date_long = "d 'de' MMMM 'de' yyyy"
time_pattern = "HH:mm:ss"
time_24h = true
currency_before = false
group_thousands = true
```

### Rules

- `[meta]` requires `app` and `language` (IETF tag).
- `[ids]` maps symbolic names to numeric IDs. If omitted, IDs are auto-assigned
  from 1001 in sorted order. IDs must not collide.
- `[strings]` maps names to single strings.
- `[plural.<NAME>]` defines a plural group; an `other` form is mandatory.
- `[region]` is optional and applies per locale.
- Source files may be UTF-8 or UTF-16LE; a BOM is detected automatically.
- Placeholders: `{0}` (string), `{0:d}` (signed decimal), `{0:u}` (unsigned),
  `{0:x}` / `{0:X}` (hex). Literal braces: `{{` and `}}`.

---

## 2. Binary Format

### 2.1 NLTv3 (current)

```text
Offset  Size  Field
──────  ────  ──────────────────────────────────────
0       4     Magic: "NLT3"
4       2     Version: u16 = 3
6       2     HeaderSize: u16 = 64
8       4     LanguageID: u32 LE
12      4     ApplicationID: u32 LE
16      4     EntryCount: u32 LE (= N)
20      4     Flags: u32 LE
24      4     PayloadOffset: u32 LE (= 64)
28      4     PayloadSize: u32 LE (uncompressed)
32      4     PayloadStored: u32 LE (in-file size)
36      4     PayloadCRC32: u32 LE (over uncompressed payload)
40      4     RegionOffset: u32 LE (0 if absent)
44      4     RegionSize: u32 LE
48      4     SignatureOffset: u32 LE (0 if absent)
52      4     SignatureSize: u32 LE (0 or 64)
56      8     Reserved
64      12*N  IndexTable: { id: u32, offset: u32, flags: u32 }[N]
64+12*N ~     Payload: UTF-8 (or UTF-16LE) string blob
              [Region block]
              [Ed25519 signature (64 bytes)]
```

- The index is sorted by ID and searched with binary search (O(log n)).
- `offset` is relative to the start of the **payload** (the index).
- Flags: `0x01` compressed, `0x02` UTF-16LE, `0x04` RTL, `0x08` signed,
  `0x10` region block present, `0x20` plural entries present.
- Entry flags: `0x01` plural group.
- Compression (LZSS) covers **only** the payload; the region block and
  signature are stored verbatim.
- The signature covers every byte before `SignatureOffset` (header + stored
  payload + region block).

### 2.2 Plural group entry

```text
u8 count                 (categories stored, usually 6)
u8 rel[count]            (offset of each category string, from group start)
UTF-8 strings…           (category order: zero, one, two, few, many, other)
```

Empty categories are stored as an empty NUL-terminated string.

### 2.3 NLTv2 (legacy, read-only)

The previous format (`"NLT2"`, 32-byte header, `{id, offset}` index, UTF-8
only, no compression) is still accepted by the runtime and tools. New files
are always written as NLTv3.

---

## 3. Compiler (`nltc`)

```text
nltc <input.toml> [output.nlt] [options]   Compile TOML → NLTv3
nltc --check <input.toml>                  Validate syntax and semantics
nltc --generate-ids <input.toml>           Assign IDs automatically
nltc --generate-rust <input.toml> [out.rs] Generate Rust constants
nltc --scaffold <app> <language>           Print a starter TOML
nltc --list-langs                          List known languages
nltc --lang-id <tag> / --app-id <name>     Show numeric IDs
nltc --info <file.nlt>                     Inspect a binary NLT
nltc --verify <file.nlt> [hex-pubkey]      Verify an Ed25519 signature
nltc --generate-all <locale-dir> [options] Compile a whole locale directory

Options:
  --no-compress         Disable LZSS compression
  --utf16               Store the string blob as UTF-16LE
  --no-sign             Do not sign the output
  --sign-key <hex32>    Sign with a 32-byte hex seed
  --verbose             Print build statistics
```

### NeoDev integration

`neodev build` / `neodev image` compile every `.toml` under `data/locale/` to
`.nlt` before generating the disk image.

---

## 4. Runtime API (`libneodos/src/i18n.rs`)

```rust
pub fn i18n_init();
pub fn i18n_language() -> &'static str;          // "es-ES"
pub fn i18n_active_locale() -> &'static str;
pub fn i18n_set_language(tag: &str);             // change + reload
pub fn i18n_is_rtl() -> bool;
pub fn i18n_load(app: &str) -> Result<(), ()>;   // NLTv2/v3
pub fn i18n_get_id(id: u32) -> &'static str;     // "?" on miss
pub fn i18n_try_get_id(id: u32) -> Option<&'static str>;
pub fn i18n_plural(id: u32, n: u64) -> &'static str;
pub fn i18n_format(id: u32, args: &[&str]) -> &'static str;
pub fn i18n_format_str(tmpl: &str, args: &[&str]) -> &'static str;
pub fn i18n_region() -> Option<Region<'static>>;
pub fn i18n_reorder_visual(input: &str, out: &mut [u8]) -> usize;
pub fn i18n_available_locales() -> &'static str;
pub fn i18n_unload(app: &str);
pub fn i18n_reload_all();
pub fn i18n_load_from_package() -> Result<(), ()>;
pub fn i18n_verify_signature(data: &[u8], pk: &[u8;32]) -> bool; // feature
```

### Macros

```rust
tr_id!(IDS_OK)              // → i18n_get_id(IDS_OK)
tr_fmt!(IDS_DATE, &[&day])  // → i18n_format(IDS_DATE, &[&day])
plural_id!(IDS_FILES, n)    // → i18n_plural(IDS_FILES, n)
```

---

## 5. Constants in Code

Generate once from the TOML and commit, or keep them in sync manually:

```bash
nltc --generate-rust neoshell.toml src/ids.rs
```

```rust
mod ids;
use ids::*;
write_str(tr_id!(IDS_OK).as_bytes());
```

---

## 6. File Locations

```text
C:\System\Locale\
  en-US\
    neoshell.nlt
    neoinit.nlt
    ...
  es-ES\
  ca-ES\
```

### Fallback chain

1. `C:\System\Locale\{lang}\{app}.nlt`
2. `C:\System\Locale\{lang-only}\{app}.nlt` (e.g. `es`)
3. `C:\System\Locale\en-US\{app}.nlt`
4. On miss, `i18n_get_id()` returns `"?"`.

Apps may also ship `resources/locale/{lang}/{app}.nlt` and load them via
`i18n_load_from_package()`.

---

## 7. Registry

```text
HKLM\System\CurrentControlSet\Control\Locale
  Language   REG_SZ   "en-US"
```

`i18n_init()` reads this key; the kernel seeds `"en-US"` at boot
(`ensure_language_default()`). Change it at runtime with `nxlocale set <tag>`
or the shell built-in `LOCALE SET <tag>` (both trigger `i18n_reload_all()`).

Future: `HKCU\Control\Locale\Language` for per-user locale (issue #92).

---

## 8. Standard Language IDs

| ID | Tag   | Language            |
|----|-------|---------------------|
| 1  | en-US | English             |
| 2  | es-ES | Español             |
| 3  | fr-FR | Français            |
| 4  | de-DE | Deutsch             |
| 5  | it-IT | Italiano            |
| 8  | ca-ES | Català              |
| 12 | ja-JP | 日本語              |
| 13 | zh-CN | 简体中文            |
| 14 | ru-RU | Русский             |
| 15 | ar-SA | العربية (RTL)       |

See `nltc --list-langs`. IDs `0x8000+` are CRC32-derived for unknown tags.

---

## 9. Standard Application IDs

| ID  | App      | ID   | App        |
|-----|----------|------|------------|
| 1   | neoshell | 15   | (unused)   |
| 2   | neoinit  | 16   | poweroff   |
| 3   | corehelp | 17   | reboot     |
| 7   | neolocale| 34   | dhcpd      |
| 8   | neokey   | 35   | netcfg     |
| 9   | neomem   | 36   | ipconfig   |

See `libnlt/src/lang.rs` for the full table. Unknown apps get `0x8000+`.

---

## 10. Tools

| Tool        | Purpose                                      |
|-------------|----------------------------------------------|
| `nltc`      | Compiler TOML → NLTv3 (host)                 |
| `neolocale` | validate, stats, diff, check, create (host)  |
| `nxlocale`  | list, current, region, set (Ring 3 .NXE)     |

```bash
nltc --scaffold miapp es-ES > miapp.toml
nltc --generate-rust miapp.toml src/ids.rs
nltc miapp.toml data/locale/es-ES/miapp.nlt
neolocale check data/locale en-US
```

---

## 11. Development Rule

**No hardcoded user-visible strings in User-Bin.** Every visible message must
live in the NLT system and be translated to:

- en-US
- es-ES
- ca-ES

---

## 12. Adding a Language

1. Create `data/locale/{locale}/` with a `.toml` per app.
2. Run `nltc --generate-all data/locale/{locale}`.
3. The runtime loads `C:\System\Locale\{locale}\` automatically.
4. Unknown tags fall back to a CRC32-hashed ID.

## 13. Adding a Key

1. Add the entry to `[ids]` and `[strings]` in all locale sources.
2. Recompile: `nltc --generate-all data/locale/{locale}`.
3. Declare `const IDS_NEW: u32 = N;` and use `tr_id!(IDS_NEW)`.

---

## 14. Feature Status

| Feature                        | Status | Notes                                  |
|--------------------------------|--------|----------------------------------------|
| NLTv2 read compatibility       | Done   | `libnlt` parses v2 + v3                |
| NLTv3 numeric IDs, binary search | Done | runtime + compiler                    |
| LZSS compression               | Done   | kept only when it shrinks the payload  |
| UTF-16LE string storage        | Done   | transcoded to UTF-8 on load            |
| Plural forms                   | Done   | CLDR categories, `plural_id!`          |
| Regional formats               | Done   | numbers, currency, dates, times        |
| RTL / bidi                     | Done   | run reordering for console display     |
| Ed25519 signatures             | Done   | `libnlt/signatures`, optional at runtime |
| Per-user locale (#92)          | Pending| Registry `HKCU` + login integration    |
