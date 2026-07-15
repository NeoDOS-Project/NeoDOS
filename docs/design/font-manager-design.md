# Font Manager — Design Document

> **Version:** v0.1-draft
> **Status:** Design
> **Target Release:** v0.51+
> **ABI Impact:** New ObType(23), new ObInfoClass(38-39), new ObSetInfoClass(48-49)

---

## 1. Problem Analysis

### Current Limitation

NeoDOS embeds a single 8x16 monochrome bitmap font directly in the kernel binary at `neodos-kernel/src/font.rs`. The font is a `const [[u8; 16]; 256]` array compiled statically, with a `draw_char()` free function that iterates pixel rows and calls `RENDERER.put_pixel()`.

### Why Existing Abstractions Cannot Solve It

1. **No font abstraction.** There is no `Font` struct, no `Glyph` type, no `FontFormat` trait. The font is a raw const array with a free function (`font.rs:238`). The console (`console.rs`) imports `font::FONT_WIDTH`, `font::FONT_HEIGHT`, and calls `font::draw_char()` directly. Zero indirection.

2. **No file loading.** The font is compiled into the kernel binary. There is no mechanism to load a font from disk, detect its format, or parse it. The manual Python script `build_font.py` generates `font.rs` from a hardcoded OTF path in the user's Downloads directory -- not part of the build pipeline.

3. **No format extensibility.** Changing the font (size, typeface) requires modifying a Rust source file and recompiling the kernel. Supporting multiple fonts or fallback chains is impossible.

4. **No configuration.** There are no registry keys, environment variables, or filesystem paths for font configuration. The font is immutable at compile time.

5. **Single size, single typeface.** The console cannot display different font sizes or styles. The 256-glyph limit with manual Unicode mapping (`write_codepoint()` in `console.rs:136-150`) restricts character coverage.

6. **No caching.** Every `draw_char` call iterates all 128 pixels (16 rows x 8 columns). For a full-screen redraw (160x50 = 8000 chars), this means 1,024,000 pixel write operations with no glyph cache.

### Audit Summary

| File | Role | Contains font data? |
|------|------|---------------------|
| `neodos-kernel/src/font.rs` | Hardcoded 8x16 bitmap (256 glyphs) | **YES** -- 4096 bytes |
| `neodos-kernel/src/console.rs` | ANSI console, calls `font::draw_char()` | No |
| `neodos-kernel/src/graphics.rs` | Framebuffer renderer, `put_pixel()` | No |
| `neodos-kernel/src/input/vt.rs` | VT shadow buffer (char grid) | No |
| `neodos-kernel/build_font.py` | Manual OTF to font.rs converter | Generator script |
| `tools/neodev/` | Build tool | **No font handling at all** |
| Registry hives | Font configuration | **None exists** |

---

## 2. Solution Design

### 2.1 Architecture Overview

```
+--------------------------------------------------------------+
|                    Font Manager (Ring 0)                      |
|  +-------------+  +--------------+  +--------------------+    |
|  | Format       |  | Font Cache    |  | Ob Integration    |   |
|  | Detection    |  | (LRU glyphs)  |  | \Font\ namespace  |   |
|  | + Dispatch   |  |              |  | ObType::Font 23   |   |
|  +------+-------+  +--------------+  +--------------------+    |
|         |                                                      |
|  +------+--------------------------------------------------+  |
|  |              FontProvider trait                          |  |
|  |  detect(buf) -> bool  |  parse(buf) -> Result<Font>     |  |
|  |  glyph(font, cp) -> Option<Glyph>  |  metrics() -> ...  |  |
|  +------+--------------------------------------------------+  |
|         |                                                      |
|  +------+-------+  +------+--------+  +------+------------+   |
|  | PSF Provider  |  | BDF Provider  |  | Future Providers |   |
|  | (phase 1)     |  | (phase 2+)    |  | (PCF, OTF, TTF) |   |
|  +--------------+  +---------------+  +-------------------+   |
+--------------------------------------------------------------+
         |
         v
+--------------------------------------------------------------+
|                    Consumers                                  |
|  +--------------+  +--------------+  +--------------------+    |
|  | Console      |  | Future GUI   |  | User-mode via      |   |
|  | (console.rs) |  | (win32k etc) |  | ob_query_info      |   |
|  +--------------+  +--------------+  +--------------------+    |
+--------------------------------------------------------------+
```

### 2.2 Format Selection: PSF v2

PC Screen Font v2 is chosen as the initial format because:

- **Designed for console framebuffers** -- created by the Linux console subsystem exactly for this use case.
- **Extremely simple** -- fixed-size header, glyph bitmap array, optional Unicode mapping table. Parsable in ~50 lines of `no_std` Rust with no allocation.
- **Self-describing** -- magic number `0x8641ab72` for version 2, header fields for height, width, and glyph count.
- **Unicode table** -- optional hash table maps codepoints to glyph indices, enabling sparse coverage without wasting slots.
- **Compact** -- no rasterization needed. Glyphs are raw bitmaps ready to blit.
- **Existing ecosystem** -- `psftools` (the standard PSF utility suite) can convert BDF, PCF, and other formats to PSF. Many PSF fonts are publicly available.
- **No proprietary format** -- PSF is an open, documented standard used by the Linux kernel.

PSF v2 header (32 bytes):

```rust
#[repr(C, packed)]
struct Psf2Header {
    magic:          [u8; 4],   // 0x72, 0xb5, 0x4a, 0x86 (LE: 0x864a_b572)
    version:        u32,       // 0
    header_size:    u32,       // 32
    flags:          u32,       // bit 0: has unicode table
    num_glyphs:     u32,       // glyph count
    bytes_per_glyph: u32,      // ((width+7)/8) * height
    height:         u32,       // pixels
    width:          u32,       // pixels
}
```

Glyph data follows immediately after the header, then optionally a Unicode mapping table.

### 2.3 New Types and Structures

#### `neodos-kernel/src/font/mod.rs` -- Font Manager Core

```rust
pub struct FontMetrics {
    pub height: u32,
    pub width: u32,
    pub ascent: u32,
    pub descent: u32,
    pub num_glyphs: u32,
    pub is_fixed_width: bool,
}

pub struct Glyph {
    pub width: u32,
    pub height: u32,
    pub data: &'static [u8],
}

pub struct FontHandle {
    pub id: FontId,
    pub format: FontFormatId,
    pub metrics: FontMetrics,
    pub glyphs_data: &'static [u8],
    pub unicode_map: Option<&'static [u8]>,
}
```

#### `neodos-kernel/src/font/provider.rs` -- Provider Trait

```rust
pub struct Detection {
    pub format_id: FontFormatId,
    pub confidence: u8,
}

pub trait FontProvider: Send + Sync {
    fn detect(&self, data: &[u8]) -> Option<Detection>;
    fn parse(&self, data: &[u8]) -> Result<FontHandle, FontError>;
    fn get_glyph(&self, font: &FontHandle, codepoint: char) -> Option<Glyph>;
    fn get_glyph_by_index(&self, font: &FontHandle, index: u32) -> Option<Glyph>;
    fn metrics(&self, font: &FontHandle) -> FontMetrics;
}
```

#### `neodos-kernel/src/font/psf.rs` -- PSF Provider

```rust
pub struct PsfProvider;

impl FontProvider for PsfProvider {
    // Detect via magic bytes 0x72, 0xb5, 0x4a, 0x86 (PSF v2)
    // Parse validates header, stores glyph bitmap offset + unicode map offset
    // get_glyph: unicode_map is searched linearly for codepoint match
    // get_glyph_by_index: index * bytes_per_glyph into glyph_data array
}
```

#### `neodos-kernel/src/font/cache.rs` -- Glyph Cache (placeholder)

```rust
/// Phase 1: no caching (direct lookup every time).
/// Phase 2: LRU cache keyed by (FontId, codepoint).
pub struct GlyphCache;
```

#### `neodos-kernel/src/font/mod.rs` -- Error Types

```rust
pub enum FontError {
    InvalidFormat,
    UnsupportedVersion,
    CorruptData,
    OutOfMemory,
    NotFound,
    AlreadyLoaded,
    ProviderNotFound,
    IoError,
}
```

### 2.4 New ObType

Added to `ObType` in `object/types.rs`:

| Value | Name | Description |
|-------|------|-------------|
| 23 | Font | Typeface resource object (kernel-created only) |

`ObType::Font = 23` represents a loaded font. Font objects are **kernel-created only** via `ObSetInfoClass::FontLoad`. User-mode cannot create font objects via `ob_create`.

### 2.5 Namespace Layout

```
\Font\
|-- Default          -- symlink to default console font
|-- Cascade\         -- directory of fallback fonts (future)
|-- <font_name>      -- individually loaded fonts (e.g., "Terminus", "UniVGA")
```

The `\Font\` directory is created during `font_init()` at boot time (Phase 5-6, after VFS is available).

### 2.6 New ObInfoClass Variants

Added to `ObInfoClass` in `object/types.rs`:

| Class | Name | Value | Description |
|-------|------|-------|-------------|
| 38 | FontMetrics | 38 | Query font metrics (returns FontMetricsInfo) |
| 39 | FontGlyph | 39 | Query a single glyph by Unicode codepoint |

#### FontMetrics (38) -- Output struct

```rust
#[repr(C)]
pub struct FontMetricsInfo {
    pub height: u32,
    pub width: u32,
    pub ascent: u32,
    pub descent: u32,
    pub num_glyphs: u32,
    pub is_fixed_width: u8,
    pub _pad: [u8; 3],
}
```

Total: 24 bytes.

#### FontGlyph (39) -- Query + Result

User provides:
```rust
#[repr(C)]
pub struct FontGlyphQuery {
    pub codepoint: u32,
    pub buf_size: u32,
    pub _pad: [u8; 8],
}
```

Kernel writes at same buffer pointer:
```rust
#[repr(C)]
pub struct FontGlyphResult {
    pub width: u32,
    pub height: u32,
    pub row_stride: u32,
    pub data_size: u32,
    // Followed by row_stride * height bytes of bitmap data
}
```

If `buf_size < sizeof(FontGlyphResult)`, returns `-InvalidParam`. The caller must know glyph dimensions (via FontMetrics) to allocate the correct buffer.

### 2.7 New ObSetInfoClass Variants

Added to `ObSetInfoClass` in `object/types.rs`:

| Class | Name | Value | Admin? | Description |
|-------|------|-------|--------|-------------|
| 48 | FontLoad | 48 | Yes | Load a font file from VFS into `\Font\<name>` |
| 49 | FontSetDefault | 49 | Yes | Set the `\Font\Default` symlink target |

#### FontLoad (48)

Input buffer:
```rust
#[repr(C)]
pub struct FontLoadInfo {
    pub path: [u8; 256],       // VFS path to font file (null-terminated)
    pub name: [u8; 64],        // name for \Font\<name> object
    pub flags: u32,            // 0=load, 1=load+setdefault
}
```

Flow:
1. Validate caller is admin.
2. Resolve VFS path, read file into heap buffer.
3. Run format detection (iterate registered providers).
4. If no provider matches, return `-NotSupported`.
5. Call `provider.parse()` to validate and create FontHandle.
6. Register ObObject of type `Font` with `native_id = font_id`.
7. Insert into namespace at `\Font\<name>`.
8. If `flags & 1`, update `\Font\Default` symlink.

#### FontSetDefault (49)

Input buffer:
```rust
#[repr(C)]
pub struct FontSetDefaultInfo {
    pub name: [u8; 64];
}
```

Flow:
1. Validate caller is admin.
2. Verify `\Font\<name>` exists and is `ObType::Font`.
3. Update or create symlink at `\Font\Default -> \Font\<name>`.

### 2.8 Internal Font Manager API

Kernel-internal functions (not syscalls, consumed by console and future GUI):

```rust
pub fn font_init() -> Result<(), FontError>;
pub fn font_register_provider(provider: &'static dyn FontProvider) -> Result<(), FontError>;
pub fn font_load_from_buffer(name: &str, data: &[u8]) -> Result<ObId, FontError>;
pub fn font_get_glyph(font_id: ObId, codepoint: char) -> Option<Glyph>;
pub fn font_get_metrics(font_id: ObId) -> Option<FontMetrics>;
pub fn font_get_default() -> Option<ObId>;
pub fn font_set_default(font_id: ObId) -> Result<(), FontError>;
pub fn font_render_glyph(glyph: &Glyph, x: usize, y: usize, fg: u32, bg: u32);
```

### 2.9 PSF Provider Detail

#### Detection

- PSF v1: first 2 bytes = `[0x36, 0x04]`
- PSF v2: first 4 bytes = `[0x72, 0xb5, 0x4a, 0x86]` (LE: 0x864a_b572)

#### Parsing (PSF v2)

1. Read 32-byte header.
2. Validate `version == 0`, `header_size >= 32`.
3. Calculate `bytes_per_glyph = ((width + 7) / 8) * height`; must match header field.
4. Glyph data starts at `header_size`; length is `num_glyphs * bytes_per_glyph`.
5. If `flags & 1`, Unicode table starts after glyph data.
   - PSF v2 unicode table: sequence of entries
   - `0xFF` + `glyph_index` (1+2 bytes LE) = glyph has no mapping
   - Otherwise: Unicode codepoint in UTF-8-like encoding + `0xFF` + `glyph_index`
6. Return FontHandle.

#### Glyph Lookup

- By index (for ASCII/Latin-1): `data[header_size + index * bytes_per_glyph .. ]` -- O(1)
- By codepoint: linear scan of unicode table -- O(n), typically < 512 entries

### 2.10 Console Integration

The console (`console.rs`) is modified to:

1. On boot, `font_get_default()` obtains the default font's ObId.
2. Replace all direct `font::FONT_WIDTH`, `font::FONT_HEIGHT`, `font::draw_char()` with:
   - `font_get_metrics(default_font_id)` for dimensions
   - `font_get_glyph(default_font_id, codepoint)` for glyph data
   - `font_render_glyph(&glyph, x, y, fg, bg)` for drawing
3. `scroll()` and `clear_screen()` remain unchanged (operate on raw framebuffer).
4. The shadow buffer (`VtShadowBuffer`) continues to store u8 char indices.

#### Boot-Time Embedded Font Fallback

Before VFS is ready, the console needs a fallback font. An embedded PSF v2 font (~4KB) is compiled into the kernel (replacing the current raw array). The Font Manager loads this via `font_load_from_buffer("embedded", embedded_psf_data)` during Phase 3 (before VFS is available), so the console can render during early boot.

After VFS is ready (Phase 5-6), `\Font\Default` is resolved to the on-disk font, and the console transparently switches.

### 2.11 Registry Integration

The following keys are added to the SYSTEM hive:

```
\Registry\Machine\System\CurrentControlSet\Services\FontManager
    DefaultFont  REG_SZ   "Terminus"     -- name of default font
    FontPath     REG_SZ   "\System\Fonts" -- directory for font files
```

Future:
```
\Registry\Machine\Software\Microsoft\Windows NT\CurrentVersion\Fonts
    <name>  REG_SZ  path\to\font.psf
```

### 2.12 NeoDev Integration

The following changes to `tools/neodev/`:

1. **`config.rs`**: Add `fonts: Vec<String>` field to `Config` struct for font file paths.
2. **`build.rs`**: Add a `fonts` stage invoked between library collection and image creation. For each font in `Config.fonts`:
   - Validate format via PSF magic detection (verify header, reject malformed fonts).
   - Report format, glyph count, and dimensions.
3. **`image.rs`**: Add font collection in `collect_files()`:
   - Copy font files to `C:\System\Fonts\` directory.
   - Generate `C:\System\Fonts\fonts.list` manifest with name, format, size.
   - Create default font symlink via registry.
4. Add a default PSF font (e.g., Terminus 8x16 or Unifont from `tools/fonts/`) to the repository under `tools/fonts/default.psf`.

### 2.13 Framebuffer Independence

The Font Manager does NOT depend on the framebuffer implementation. `font_render_glyph()` takes a generic pixel-writing callback:

```rust
pub fn font_render_glyph<F>(glyph: &Glyph, x: usize, y: usize, fg: u32, bg: u32, put_pixel: F)
where
    F: Fn(usize, usize, u32),
```

The console passes `RENDERER.put_pixel()` as the callback. A future GUI subsystem passes its own pixel-writing function. The Font Manager itself has no direct dependency on `graphics.rs`.

---

## 3. Alternatives Considered

### Alternative A: Keep embedded font, add PSF loading only for user-mode

**Description:** Keep the current `font.rs` as the only kernel font. Add a `sys_load_font` syscall that lets user-mode programs load PSF files and query glyphs via syscall.

**Rejected because:**
- The console runs in Ring 0 and needs a kernel font; we would need two parallel font systems (kernel embedded + user-mode loaded).
- The hardcoded font would still be in the kernel binary, violating the goal of removing embedded fonts.
- User-mode programs would need to duplicate text rendering logic.
- No namespace integration, no configuration, no format extensibility for the console.

### Alternative B: TTF/OTF rasterization instead of PSF

**Description:** Use FreeType or a minimalist TrueType rasterizer to load TTF/OTF fonts directly.

**Rejected because:**
- TTF rasterization is extremely complex (hinting, curve rendering, bytecode interpreter). Minimal TTF renderers still require >2000 lines of code.
- Memory and CPU overhead is unacceptable for a console: TTF rasterization of a single glyph requires memory allocation, Bezier decomposition, and scanline filling.
- The PSF format is designed for precisely this use case -- bitmap console fonts. Adding TTF support is deferred to the future GUI subsystem.

### Alternative C: BDF (Bitmap Distribution Format) only

**Description:** Use BDF (X11 bitmap font format) instead of PSF.

**Rejected because:**
- BDF is a text-based format (ASCII). Parsing text in `no_std` is more complex and memory-intensive than parsing a binary format.
- BDF files are significantly larger than equivalent PSF files (text encoding vs raw bitmaps).
- PSF is the standard for Linux console fonts, with better tooling and availability.
- BDF support can be added later as a provider without changing the Font Manager.

---

## 4. Affected Components

| Subsystem | Change Description | Impact |
|-----------|-------------------|--------|
| **Object Manager** (`object/`) | New `ObType::Font=23`, new `ObInfoClass` variants (38-39), new `ObSetInfoClass` variants (48-49), new `ObOperations` for Font | Moderate |
| **Font** (`font/` -- NEW) | Complete new module: `mod.rs`, `provider.rs`, `psf.rs`, `cache.rs` | New subsystem |
| **Console** (`console.rs`) | Remove direct `font::` imports; use Font Manager API | High |
| **Framebuffer/Graphics** (`graphics.rs`) | No direct change; `font_render_glyph` uses callback | None |
| **VT** (`input/vt.rs`) | No change to shadow buffer format | None |
| **Syscall dispatch** (`syscall/`) | No new syscalls; existing `ob_query_info`/`ob_set_info` handle new classes | Low |
| **Registry** (`cm/`, `scripts/gen_system_hiv.py`) | Add `Services\FontManager` and `Software\Fonts` keys | Low |
| **NeoDev** (`tools/neodev/`) | Add font validation, font copy to image, `fonts.list` generation | Moderate |
| **Boot sequence** (`main.rs`) | Add `font::init()` call in appropriate phase | Low |
| **Locking/Concurrency** | Font Manager uses `Mutex<FontRegistry>` for provider/object registry | Low |

---

## 5. API Contract

### 5.1 Syscall: ob_query_info (RAX=42) -- existing

No new syscall. New classes dispatched via existing handler.

#### FontMetrics (class=38)

| Aspect | Detail |
|--------|--------|
| Args | `fd` (font fd), `class=38`, `buf`, `size` |
| Returns | Bytes written (≥24) |
| Errors | `-InvalidType` if fd is not `ObType::Font`, `-Fault` if buf invalid, `-InvalidParam` if size < 24 |
| Preconditions | fd obtained from `ob_open` on a `ObType::Font` object |
| Postconditions | `buf` contains `FontMetricsInfo` |

#### FontGlyph (class=39)

| Aspect | Detail |
|--------|--------|
| Args | `fd` (font fd), `class=39`, `buf` (contains `FontGlyphQuery` on input), `size` |
| Returns | Bytes written (≥16 + bitmap data) |
| Errors | `-InvalidType` if fd not Font, `-Fault` if buf invalid, `-InvalidParam` if codepoint not found or buf too small |
| Preconditions | fd obtained from `ob_open` on `ObType::Font` |
| Postconditions | `buf` contains `FontGlyphResult` followed by bitmap data |

### 5.2 Syscall: ob_set_info (RAX=43) -- existing

#### FontLoad (class=48)

| Aspect | Detail |
|--------|--------|
| Args | `fd` (handle to `\Font\` directory), `class=48`, `buf` (FontLoadInfo), `size` |
| Returns | `0` on success |
| Errors | `-Perm` if not admin, `-NotFound` if VFS path invalid, `-NotSupported` if format unknown, `-NoMem` if allocation fails, `-AlreadyExists` if font name exists, `-Io` if VFS read fails |
| Preconditions | Caller must have admin token. VFS must be available. |
| Postconditions | Font loaded at `\Font\<name>`, data cached in kernel heap |

#### FontSetDefault (class=49)

| Aspect | Detail |
|--------|--------|
| Args | `fd` (handle to `\Font\` directory), `class=49`, `buf` (FontSetDefaultInfo), `size` |
| Returns | `0` on success |
| Errors | `-Perm` if not admin, `-NotFound` if font name doesn't exist, `-InvalidParam` if name empty |
| Preconditions | Caller must have admin token. The named font must exist. |
| Postconditions | `\Font\Default` symlink updated. Console will use new font on next redraw. |

### 5.3 Internal API

#### `font_get_glyph(font_id: ObId, codepoint: char) -> Option<Glyph>`

| Aspect | Detail |
|--------|--------|
| Args | `font_id`: ObId of a Font object, `codepoint`: Unicode scalar value |
| Returns | `Some(Glyph)` if glyph exists, `None` if not found |
| Errors | None (returns None on any error) |
| Preconditions | `font_id` must be valid Font ObId |
| Thread safety | Yes (FontHandle is immutable after loading) |

#### `font_render_glyph(glyph, x, y, fg, bg, put_pixel)`

| Aspect | Detail |
|--------|--------|
| Args | glyph reference, pixel coords, colors, callback |
| Returns | Nothing |
| Preconditions | Framebuffer must be initialized, `put_pixel` must be safe |
| Thread safety | Caller must ensure framebuffer access is coordinated |

---

## 6. Test Plan

### 6.1 Format Detection Tests

| Test | Description | Expected |
|------|-------------|----------|
| `font_detect_psf2_magic` | Feed PSF v2 header bytes to detection | Returns `Detection` with `FontFormatId::Psf2` |
| `font_detect_psf1_magic` | Feed PSF v1 header bytes to detection | Returns `Detection` with `FontFormatId::Psf1` |
| `font_detect_random_data` | Feed non-font data | Returns `None` |
| `font_detect_empty_data` | Feed empty slice | Returns `None` |
| `font_detect_short_header` | Feed only 3 bytes | Returns `None` (both v1 and v2 require more) |
| `font_detect_register_unregister` | Register then unregister a provider | Detection stops working after unregister |

### 6.2 PSF Parsing Tests

| Test | Description | Expected |
|------|-------------|----------|
| `font_parse_psf2_valid` | Parse a valid PSF v2 font | Returns `Ok(FontHandle)` with correct metrics |
| `font_parse_psf2_corrupt_version` | Version field != 0 | Returns `Err(UnsupportedVersion)` |
| `font_parse_psf2_corrupt_header_size` | header_size < 32 | Returns `Err(CorruptData)` |
| `font_parse_psf2_wrong_glyph_size` | bytes_per_glyph mismatches width/height | Returns `Err(CorruptData)` |
| `font_parse_psf1_valid` | Parse a valid PSF v1 font | Returns `Ok(FontHandle)` |
| `font_parse_psf2_with_unicode` | Parse PSF v2 with Unicode table | Unicode mappings are correctly loaded |
| `font_parse_psf2_without_unicode` | Parse PSF v2 without Unicode table | Glyph lookup by codepoint == glyph lookup by index for first 256 |

### 6.3 Glyph Lookup Tests

| Test | Description | Expected |
|------|-------------|----------|
| `font_glyph_by_index_ascii` | Look up 'A' (index 65) in ASCII range | Returns correct 'A' bitmap |
| `font_glyph_by_codepoint_unicode` | Look up U+00E9 (e-acute) via Unicode table | Returns correct glyph |
| `font_glyph_missing_codepoint` | Look up nonexistent codepoint | Returns `None` |
| `font_glyph_index_out_of_range` | Index >= num_glyphs | Returns `None` |
| `font_glyph_zero_width` | Look up space (0x20) | Returns glyph with all zero rows |

### 6.4 Metrics Tests

| Test | Description | Expected |
|------|-------------|----------|
| `font_metrics_matches_header` | Metrics match PSF header fields | All fields match |
| `font_metrics_ob_query` | Query via `ob_query_info(class=38)` | Returns `FontMetricsInfo` matching FontHandle |
| `font_metrics_on_invalid_type` | Query FontMetrics on a Pipe fd | Returns `-InvalidType` |

### 6.5 Font Loading Tests

| Test | Description | Expected |
|------|-------------|----------|
| `font_load_from_path` | Load a font from VFS path | Font appears at `\Font\<name>` |
| `font_load_nonexistent_path` | Load from non-existent VFS path | Returns `-NotFound` |
| `font_load_unsupported_format` | Load a .txt file as font | Returns `-NotSupported` |
| `font_load_duplicate_name` | Load two fonts with the same name | Second returns `-AlreadyExists` |
| `font_load_not_admin` | Non-admin tries to load font | Returns `-Perm` |
| `font_set_default` | Set `\Font\Default` symlink | Symlink points to correct font |
| `font_get_default` | Query default font | Returns correct ObId |
| `font_set_default_not_admin` | Non-admin tries to set default | Returns `-Perm` |

### 6.6 Console Integration Tests

| Test | Description | Expected |
|------|-------------|----------|
| `console_uses_font_manager` | After boot, console uses Font Manager | `console_max_row()` returns value based on font metrics |
| `console_fallback_font_embedded` | Embedded font is available before VFS | `font_get_default()` returns embedded font ObId |
| `console_switch_font_dynamic` | Load new font, set as default, call redraw | Console uses new font metrics |
| `console_embedded_font_matches_old` | Embedded PSF produces same pixels as old font# | Pixel-for-pixel identical rendering |

### 6.7 Provider Registration Tests

| Test | Description | Expected |
|------|-------------|----------|
| `font_register_provider` | Register a test provider | Provider is called during detection |
| `font_register_duplicate` | Register the same provider twice | Second registration is ignored |
| `font_unregister_provider` | Unregister a provider | Provider no longer called during detection |
| `font_no_providers_registered` | Detection with empty provider list | Returns `None` for any input |

### 6.8 FontDestroy/Cleanup Tests

| Test | Description | Expected |
|------|-------------|----------|
| `font_destroy_releases_memory` | Destroy a font object via `ob_destroy` | Memory buffer is freed |
| `font_destroy_removes_namespace` | Destroy removes from `\Font\` | `ob_open("\\Font\\<name>")` returns `-NotFound` |
| `font_destroy_with_references` | Destroy while console still references | Returns `-RefCountHeld` |

### 6.9 NeoDev Validation Tests

| Test | Description | Expected |
|------|-------------|----------|
| `neodev_validate_valid_psf` | Validate a valid PSF file | Returns success |
| `neodev_validate_invalid_file` | Validate a corrupted file | Returns format error |
| `neodev_validate_empty_file` | Validate empty file | Returns error |
| `neodev_font_copied_to_image` | Build image with font configured | Font file exists at `C:\System\Fonts\` |

---

## 7. Files and Modules

### New files

| Path | Description |
|------|-------------|
| `neodos-kernel/src/font/mod.rs` | Font Manager core: types, init, public API, FontRegistry |
| `neodos-kernel/src/font/provider.rs` | FontProvider trait, Detection struct, FontFormatId enum |
| `neodos-kernel/src/font/psf.rs` | PSF v1 and v2 format provider |
| `neodos-kernel/src/font/cache.rs` | Glyph cache placeholder (struct only, no logic) |
| `neodos-kernel/src/font/embedded.rs` | Embedded PSF v2 font data for boot-time fallback |
| `neodos-kernel/src/syscall/font_handlers.rs` | Handlers for FontLoad and FontSetDefault ob_set_info classes (optional; can inline in handlers.rs) |

### Generated/modified files

| Path | Change |
|------|--------|
| `neodos-kernel/src/main.rs` | Add `mod font;` + call `font::init()` in boot sequence |
| `neodos-kernel/src/console.rs` | Replace `use crate::font;` -> `use crate::font_manager;`, use Font Manager API for all glyph operations |
| `neodos-kernel/src/font.rs` | **Deleted** -- replaced by `font/mod.rs` + `font/embedded.rs` |
| `neodos-kernel/src/graphics.rs` | No change (Font Manager takes put_pixel callback) |
| `neodos-kernel/src/object/types.rs` | Add `ObType::Font = 23`; `ObInfoClass::FontMetrics = 38`, `FontGlyph = 39`; `ObSetInfoClass::FontLoad = 48`, `FontSetDefault = 49` |
| `neodos-kernel/src/object/mod.rs` | Register `ObOperations` for Font type |
| `neodos-kernel/src/syscall/handlers.rs` | Add dispatch for ObSetInfoClass::FontLoad and FontSetDefault |
| `neodos-kernel/src/input/vt.rs` | No change (shadow buffer format unchanged) |
| `neodos-kernel/src/input/manager.rs` | No change |
| `neodos-kernel/build_font.py` | **Deleted** -- replaced by NeoDev-managed PSF font pipeline |
| `tools/neodev/src/config.rs` | Add `fonts: Vec<String>` field |
| `tools/neodev/src/build.rs` | Add font validation stage |
| `tools/neodev/src/image.rs` | Add font collection to `collect_files()` |
| `scripts/gen_system_hiv.py` | Add `Services\FontManager` and `Software\Fonts` keys |
| `docs/objects.md` | Add Font type to ObType table, namespace, info classes |
| `docs/syscalls.md` | No new syscalls, but document new info classes |
| `docs/IMPROVEMENTS.md` | Add future Font Manager improvements |
| `tools/fonts/default.psf` | New default PSF font file (e.g., Terminus 8x16) |

---

## 8. Implementation Plan

### Step 1: Create font module skeleton

- Create `neodos-kernel/src/font/mod.rs` with `FontMetrics`, `Glyph`, `FontHandle`, `FontError`, `FontFormatId`.
- Create `neodos-kernel/src/font/provider.rs` with `Detection` and `FontProvider` trait.
- Create `neodos-kernel/src/font/psf.rs` with `PsfProvider` struct.
- Create `neodos-kernel/src/font/cache.rs` with empty `GlyphCache` struct.
- Create `neodos-kernel/src/font/embedded.rs` with embedded PSF v2 font data.
- Add `mod font;` to `main.rs`.
- Verify `cargo build` succeeds.

### Step 2: Implement PSF provider

- Implement `PsfProvider::detect()` -- check magic bytes.
- Implement `PsfProvider::parse()` -- validate header, compute offsets.
- Implement `PsfProvider::get_glyph()` -- Unicode table scan.
- Implement `PsfProvider::get_glyph_by_index()` -- direct index lookup.
- Implement `PsfProvider::metrics()`.
- Write tests for each function in `psf.rs`.

### Step 3: Implement Font Manager core

- Implement `font_init()` -- create `\Font\` directory object, register PSF provider.
- Implement `font_register_provider()` -- add to static provider list.
- Implement `font_load_from_buffer()` -- detect, parse, create ObObject, namespace insert.
- Implement `font_get_glyph()` / `font_get_metrics()` / `font_get_default()` / `font_set_default()`.
- Implement `font_render_glyph()` with put_pixel callback.
- Write FontRegistry as `Mutex<...>` protected structure.

### Step 4: Create embedded PSF fallback font

- Convert the existing 8x16 bitmap data from `font.rs` into a valid PSF v2 binary blob.
- Embed as a `const` byte array in `embedded.rs`.
- Load via `font_load_from_buffer("embedded", embedded_data)` during boot Phase 3.

### Step 5: Update Ob types and syscall handlers

- Add `ObType::Font = 23` to `object/types.rs`.
- Add `ObInfoClass::FontMetrics = 38`, `FontGlyph = 39`.
- Add `ObSetInfoClass::FontLoad = 48`, `FontSetDefault = 49`.
- Implement `ObOperations` for Font objects (`FontOps`).
- Add `on_destroy` handler to free FontHandle and buffer.
- Add handler paths in `syscall/handlers.rs` for the new info classes.
- Add permission check (admin only) for FontLoad and FontSetDefault.

### Step 6: Refactor console to use Font Manager

- Replace `use crate::font` with `use crate::font` (now the Font Manager module).
- Replace `font::FONT_WIDTH` / `font::FONT_HEIGHT` with `font_get_metrics(default_id)` calls.
- Replace `font::draw_char(c, x, y, fg, bg)` with:
  - `let glyph = font_get_glyph(default_id, codepoint)`
  - `font_render_glyph(&glyph, x, y, fg, bg, |x,y,c| RENDERER.put_pixel(x,y,c))`
- Remove the shadow buffer's dependency on fixed font dimensions (make dynamic).
- Update `scroll()` to use font height from Font Manager.
- Verify console renders correctly with embedded fallback.

### Step 7: Integrate with NeoDev

- Add `fonts` field to `Config` struct in `config.rs`.
- Add font validation stage in `build.rs`:
  - Read PSF header, validate magic, report metrics.
- Add `C:\System\Fonts\` directory in image builder.
- Add font collection in `collect_files()`:
  - Copy `.psf` files to `C:\System\Fonts\`.
  - Generate `fonts.list` manifest.
- Add `tools/fonts/default.psf` (Terminus 8x16 or equivalent).

### Step 8: Registry configuration

- Add `Services\FontManager\DefaultFont = "Terminus"` to `gen_system_hiv.py`.
- Add `Services\FontManager\FontPath = "\System\Fonts"`.
- Add `Software\Fonts` key structure for user-mode font queries.

### Step 9: Boot sequence integration

- In `main.rs`, after VFS initialization:
  - Call `font::font_init()`.
  - Load embedded fallback font.
  - Attempt to load default font from `C:\System\Fonts\default.psf`.
  - If disk font succeeds, set it as default; otherwise keep embedded.

### Step 10: Tests

- Write all tests from Section 6.
- Run existing ANSI console tests to verify no regression.
- Run full test suite: `cargo run --bin neodev -- test`.

### Step 11: Documentation

- Update `docs/objects.md` with ObType::Font, namespace, info classes.
- Update `docs/font-manager-design.md` if implementation diverges from design.
- Add architecture section to `docs/font-manager-design.md`.
- Update `docs/IMPROVEMENTS.md` with future improvements.

---

## 9. Future Work (IMPROVEMENTS.md entries)

| Item | Priority | Complexity | Description |
|------|----------|------------|-------------|
| Glyph LRU cache | Medium | Low | Cache recently used glyphs to avoid repeated Unicode table scans |
| BDF provider | Low | Low | Add support for X11 Bitmap Distribution Format |
| PCF provider | Low | Medium | Add support for X11 Portable Compiled Format |
| Font fallback chain | Medium | Medium | If a glyph is missing in primary font, search Cascade\ directory |
| Unicode full coverage | Low | High | Load a large font (e.g., Unifont) for full BMP coverage |
| Font size scaling | Low | High | Scale bitmap fonts to multiple sizes via nearest-neighbor |
| Registry font enumeration | Low | Low | Add `CmEnumKey` for `\Registry\Machine\Software\Fonts` |
| User-mode font loading | Low | Low | Allow non-admin processes to load fonts into their namespace |
| Anti-aliased glyph rendering | Low | High | Future GUI feature: render glyphs with alpha blending |
| TrueType/OpenType provider | Low | Very High | Full vector font rasterization (requires FreeType or equivalent) |

---

## 10. Migration Path

### Phase 1 (v0.51): Font Manager + PSF loading

- Font Manager exists alongside the old `font.rs`.
- Console uses Font Manager by default.
- Old `font.rs` is kept as compilation fallback until Phase 2.
- Embedded PSF font replaces the old const array for boot-time rendering.

### Phase 2 (v0.52): Old font.rs removal

- Remove `neodos-kernel/src/font.rs`.
- Remove `neodos-kernel/build_font.py`.
- Remove all references to `font::FONT_WIDTH`, `font::FONT_HEIGHT`, `font::draw_char()`.
- Console exclusively uses Font Manager.

### Phase 3 (v0.53+): Provider expansion

- Add BDF provider.
- Add glyph cache.
- Add font fallback chain.

---

## 11. Dependencies

The Font Manager has minimal dependencies:

- `alloc` (for heap buffer during font loading)
- `spin::Mutex` (for FontRegistry)
- `object/` types (for ObId, ObType, ObOperations)
- `vfs/` (for reading font files from disk, only in FontLoad path)
- No dependencies on `graphics`, `console`, `input`, `scheduler`, `memory` (beyond heap alloc)
