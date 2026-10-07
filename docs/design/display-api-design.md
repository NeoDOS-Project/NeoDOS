# Display / Framebuffer User-Mode API — Design Document

> **Version:** v0.1-draft
> **Status:** Design
> **Target Release:** v0.52+
> **Issue:** [#465](https://github.com/NeoDOS-Project/NeoDOS/issues/465)
> **ABI Impact:** New `ObType::Display` (23), new `ObInfoClass` (43), new
> `ObSetInfoClass` (52–54)

---

## 1. Problem Analysis

### 1.1 Current Limitation

NeoDOS can only draw to the screen from Ring 0, and only as a text console.
There is no path for a Ring 3 process to produce pixels.

`neodos-kernel/src/graphics/mod.rs` is a kernel-only singleton:

```rust
pub struct FramebufferInfo {
    pub base_address: u64,
    pub size: usize,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
}

pub static RENDERER: Mutex<Option<Renderer>> = Mutex::new(None);
```

The `Renderer` walks the GOP framebuffer obtained by the UEFI bootloader
(`neodos-bootloader/src/main.rs`) and exposes only `put_pixel()` (a
`write_volatile`) and `clear()`. It is consumed exclusively by
`console/` and `font.rs` to render the 4 virtual terminals.

### 1.2 Why Existing Abstractions Cannot Solve It

1. **No user-facing display object.** `ObType` (`object/types.rs`) has no
   Display/GraphicsDevice/Framebuffer variant. `ObInfoClass` /
   `ObSetInfoClass` have no video classes.
2. **No framebuffer mapping to Ring 3.** The GOP framebuffer is mapped by the
   kernel as UC- and, when above 4 GiB, through `map_phys_range_above_4g()`
   (`arch/x64/paging.rs`). None of that is exposed to Ring 3.
3. **`Section` is not suitable.** `object/section.rs` already provides
   `alloc_section`/`map_view`, but:
   - sections are capped at `size <= 0x100000` (1 MB) and `MAX_SECTIONS = 32`;
   - a view is backed by freshly allocated anonymous 4 KB pages
     (`mmap_alloc_page`), not by the physical scanout buffer;
   - there is no present/flip/scaling — a section is just shared memory.
4. **Per-pixel MMIO from Ring 3 would be unusable.** Even if the raw FB were
   mapped, uncacheable `write_volatile` per pixel scales terribly (the console
   already avoids it with `rep stosd`), and it would expose the physical
   format and address to userland.
5. **No format/resolution abstraction.** Doom renders at 320×200 (or 640×400)
   with an 8-bit palette; the GOP mode is typically 800×600 XRGB. Something
   must own conversion + scaling + present.
6. **No VT/hardware-mode coordination.** Section 11 of the source of truth
   gives each VT a text shadow buffer. A graphical client must be able to take
   over a VT, suppress text output on it, and restore the console afterwards.

### 1.3 Audit Summary

| File | Role | Relevant to display? |
|------|------|----------------------|
| `neodos-kernel/src/graphics/mod.rs` | `FramebufferInfo`, `Renderer`, `RENDERER` | **YES** — kernel-only pixel path |
| `neodos-kernel/src/console/mod.rs` | Text rendering, scroll, cursor | **YES** — must yield FB in graphics mode |
| `neodos-kernel/src/graphics/font.rs` | Bitmap glyph blitting | No |
| `neodos-kernel/src/input/vt.rs` | Per-VT shadow buffers | **YES** — VT ownership/mode |
| `neodos-kernel/src/input/manager.rs` | `active_vt`, per-VT state | **YES** — present gating |
| `neodos-kernel/src/object/section.rs` | Anonymous shared memory | Partial — not FB-backed |
| `neodos-kernel/src/arch/x64/paging.rs` | FB UC- mapping, mmap 4 KB demand paging | **YES** — map back buffer |
| `neodos-kernel/src/object/types.rs` | Ob types / info classes | **YES** — new variants |
| `neodos-bootloader/src/main.rs` | GOP `FramebufferInfo` | No (already consumed) |
| `libneodos/` | User-mode library | **YES** — new `display` module |

### 1.4 Scope of This Document

This document designs the **display / framebuffer API for Ring 3**. It does not
design a compositor, window manager, or GPU acceleration; those are follow-ups
(`ROADMAP.md` v3.x). The first delivery target is "a Ring 3 process can blit a
software surface to the screen at a stable rate" — sufficient for Doom.

---

## 2. Solution Design

### 2.1 Architecture Overview

```text
+--------------------------------------------------------------+
|                    Ring 3 client (e.g. Doom)                  |
|   display.open() -> info() -> map() -> [render] -> present()  |
+------------------------------+-------------------------------+
                               | int 0x80 (ob_open/query/set_info)
                               v
+--------------------------------------------------------------+
|                    Display subsystem (Ring 0)                 |
|  +----------------+  +-----------------+  +----------------+  |
|  | \Device\Display |  | Surface (back   |  | Present engine |  |
|  | ObType::Display |  | buffer, frames) |  | convert+scale  |  |
|  +----------------+  +-----------------+  +----------------+  |
|         |                     |                    |          |
|  +------+---------------------+--------------------+-------+  |
|  |              VT binding (vt_num, graphics mode)          |  |
|  +----------------------------------------------------------+  |
+------------------------------+-------------------------------+
                               | kernel memcpy/scale
                               v
+--------------------------------------------------------------+
|            GOP framebuffer (UC-, maybe >4 GiB)                |
+--------------------------------------------------------------+
```

Key idea: **the kernel owns the physical framebuffer; the client owns a
cacheable back buffer mapped into its mmap region; `present` copies/ converts /
scales back buffer → framebuffer.**

### 2.2 Object Model

A single kernel-created singleton object:

| Property | Value |
|----------|-------|
| Path | `\Device\Display` |
| `ObType` | `Display = 23` |
| Created | `display::init()` at boot, after GOP info is available (Phase 3.86) |
| User-creatable | No (`ob_create` rejects it) |
| Open access | READ (info) / WRITE (map + present) |

> **Type-number allocation (resolved).** `ObType::Display = 23`. The Font
> Manager draft (`docs/design/font-manager-design.md`) has been renumbered to
> `ObType::Font = 24`. The authoritative registry remains `object/types.rs`.

### 2.3 Back Buffer (Surface)

The Display owns one or more **surfaces**. A surface is:

- a set of physical frames allocated from the frame allocator
  (`hal::alloc_page`), owned by the Display per INV-5;
- mapped into the opening process's **mmap region**
  (`0x2000_0000..0x2200_0000`) as writable cacheable user pages, satisfying
  Rule 3.2.3 (user-accessible PTEs only in allowed regions);
- in a canonical format independent of the physical FB.

Canonical surface format for v1: `XRGB8888` (32 bpp, little-endian
`0x00RRGGBB`). Rationale: matches the overwhelmingly common GOP format, avoids
palette handling in the kernel, and gives Doom a trivial 8→32 expansion.

`Indexed8` (a single shared palette + 8-bit indices) is a possible later
surface format for low-memory clients; not in v1.

Surfaces are sized `width * height * 4`, contiguous virtual, page-granular
physical. MAX surface = 4 MB (covers 1024×768×4) for v1.

### 2.4 Present Engine

`present` moves pixels from the surface to the GOP framebuffer:

1. **Format conversion** — `XRGB8888` → FB format. GOP exposes only
   `base_address/size/width/height/stride` today; the pixel format is assumed
   `XRGB8888` for v1 and validated/params extended later.
2. **Scale** — if `surface.width/height != fb.width/height`, nearest-neighbor
   scale. This is the path Doom needs (320×200 → 800×600).
3. **Copy** — for the 1:1 case, one `memcpy` per row (`rep movsd`-style, same
   primitive the console already uses) instead of per-pixel volatile writes.
4. **Damage/dirty rect** — `present` accepts an optional rect. Full-screen
   present when the rect is null. Dirty rects are a v1.1 optimization.

Present is **synchronous** within the syscall for v1 (the caller blocks until
the copy is issued). A queued/IRP path is future work (see §9).

### 2.5 VT Binding & Graphics Mode

The source-of-truth VT model (§11) is preserved:

- A client is bound to the VT it is running on (`eprocess.vt_num`).
- The first successful `DisplayMapBuffer` claims the VT for graphics unless
  another owner holds it.
- Each VT gains a mode:

```rust
pub enum VtMode {
    Text,
    Graphics { owner_pid: u32 },
}
```

- **Present gating**: `present` renders only when `active_vt` matches the
  client's VT. When inactive, the present is a no-op (the surface is still
  writable) so background clients do not corrupt the visible VT.
- **Console suppression**: while a VT is in `Graphics`, `console` MUST NOT
  draw text to the physical framebuffer for that VT (cursor hidden, writes go
  to the shadow buffer only). On return to `Text`, the console clears and
  redraws from the VT shadow buffer — reusing the existing switch redraw path.
- **VT switch away/back**: switching away saves nothing extra (the surface is
  in RAM); switching back re-presents the last surface. No redraw of console
  over an active graphics VT.
- **Release**: on process exit or handle close, the surface is freed, views
  unmapped, and the VT returns to `Text`.

### 2.6 New Types and Structures

#### Kernel: `neodos-kernel/src/display/mod.rs`

```rust
pub struct DisplayInfo {
    pub fb_width: u32,
    pub fb_height: u32,
    pub fb_stride: u32,     // in pixels
    pub fb_format: u32,     // DisplayFormat::Xrgb8888 = 1
    pub surface_width: u32, // current back buffer size
    pub surface_height: u32,
    pub flags: u32,         // bit0: double_buffered, bit1: front_owner
    pub _pad: u32,
}

pub struct DisplayMap {
    pub width: u32,   // requested (0 = native fb)
    pub height: u32,
    pub format: u32,  // must be Xrgb8888 in v1
    pub _pad: u32,
}

pub struct DisplayPresent {
    pub x: i32,       // damage rect; 0,0,0,0 => full frame
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub flags: u32,   // bit0: use damage rect
    pub _pad: u32,
}
```

#### Kernel: `neodos-kernel/src/display/surface.rs`

```rust
pub struct Surface {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: u32,
    pub frames: Vec<u64>,   // physical frames (owned)
    pub user_va: u64,       // mapped base in mmap region (0 = unmapped)
    pub size: u64,
}

impl Surface {
    pub fn alloc(width: u32, height: u32, format: u32) -> Option<Surface>;
    pub fn map_into_current(&mut self) -> Result<u64, DisplayError>;
    pub fn unmap(&mut self);
    pub fn free(self);
}
```

#### Kernel: `neodos-kernel/src/display/present.rs`

```rust
pub fn present(surface: &Surface, fb: &FramebufferInfo, rect: Option<DamageRect>);
fn scale_nearest(src: &[u32], dst: *mut u32, sw: u32, sh: u32, dw: u32, dh: u32);
fn convert_xrgb(src: &[u32], dst: *mut u32, n: usize);
```

#### Kernel: `neodos-kernel/src/object/display.rs`

```rust
pub struct DisplayObOps;
impl ObOperations for DisplayObOps {
    fn on_destroy(&self, _id: ObId, _native_id: u64) {
        // release VT, free surfaces, unmap views
    }
}
pub static DISPLAY_OPS: DisplayObOps = DisplayObOps;
```

### 2.7 New ObInfoClass / ObSetInfoClass

Current highest `ObInfoClass` is `42`; highest `ObSetInfoClass` is `51`.

#### ObInfoClass

| Value | Name | Direction | Payload |
|-------|------|-----------|---------|
| 43 | `DisplayInfo` | out | `DisplayInfo` (32 bytes) |

#### ObSetInfoClass

| Value | Name | Admin? | Payload |
|-------|------|--------|---------|
| 52 | `DisplayMapBuffer` | No | `DisplayMap` (in); returns surface VA |
| 53 | `DisplayPresent` | No | `DisplayPresent` (in); returns 0 |
| 54 | `DisplayFreeBuffer` | No | none; returns 0 |

`DisplayMapBuffer`/`DisplayFreeBuffer` are idempotent per handle: mapping twice
returns the existing VA; freeing twice is a no-op. `present` before mapping
returns `-InvalidParam`.

### 2.8 Internal Display API (kernel-internal)

```rust
pub fn display_init(fb: FramebufferInfo);                 // boot, creates \Device\Display
pub fn display_info() -> DisplayInfo;
pub fn display_claim_vt(pid: u32, vt: u8) -> Result<(), DisplayError>;
pub fn display_release_vt(pid: u32);
pub fn display_present(pid: u32, rect: Option<DamageRect>) -> Result<(), DisplayError>;
pub fn display_vt_mode(vt: u8) -> VtMode;
```

`display_present` checks `vt_mode(eprocess.vt_num) == Graphics{owner: pid}` and
`active_vt == vt` before touching the framebuffer.

### 2.9 libneodos API

New module `libneodos/src/display.rs`:

```rust
pub struct Display { fd: u8 }
pub struct Surface { pub base: *mut u32, pub width: u32, pub height: u32, pub stride: u32 }

impl Display {
    pub fn open() -> Result<Display, i64>;              // ob_open("\\Device\\Display", RW)
    pub fn info(&self) -> Result<DisplayInfo, i64>;      // ObInfoClass::DisplayInfo
    pub fn map(&self, w: u32, h: u32) -> Result<Surface, i64>; // DisplayMapBuffer
    pub fn present(&self, rect: Option<(i32,i32,u32,u32)>) -> Result<(), i64>;
    pub fn free_buffer(&self) -> Result<(), i64>;
}
```

Plus a convenience helper for software renderers:

```rust
pub fn blit_indexed8(palette: &[u32; 256], src: &[u8], dst: &mut [u32], count: usize);
```

This is the exact primitive an 8-bit Doom renderer needs and keeps the palette
conversion out of the kernel.

### 2.10 Boot Integration

Phase 3.86 (after memory/paging and before the userland loader; near the
Keyboard Manager at 3.875):

1. `display_init(boot_info.fb_info)` — store `FramebufferInfo`, create
   `\Device\Display` (ObType::Display), install `DISPLAY_OPS`.
2. Initialize all VTs to `VtMode::Text`.

Failure is non-fatal and consistent with `graphics::init`: if
`fb.base_address == 0`, the Display object is still created but
`DisplayMapBuffer`/`present` return `-NoDev`.

### 2.11 Concurrency & IRQ Rules

- `present` runs in syscall context, never in IRQ context (INV-2/INV-3).
- The Display state (`Mutex<DisplayState>`) is held only for bookkeeping, not
  across the pixel copy beyond what is required; the copy is not an allocation.
- Frame ownership: surface frames are owned by the Display until `free`
  (INV-5). `present` only reads them.
- No dependency on scheduler, VFS, or block drivers (INV-1).

---

## 3. Alternatives Considered

### Alternative A: Expose the raw physical framebuffer to Ring 3

Map the GOP framebuffer UC- into the user mmap region and let apps write
pixels directly.

**Rejected because:**

- Uncacheable per-pixel writes are slow; a 320×200×35 fps loop would crawl.
- Exposes the physical address and exact GOP pixel format to userland.
- No scaling, so every app must know the native mode.
- No VT gating: a background process could corrupt the visible VT.
- A malicious/buggy app can scribble over the whole scanout.

### Alternative B: Reuse `Section`

Allocate the surface as an anonymous `Section` and `SectionMapView` it.

**Rejected because:**

- 1 MB section cap (`size > 0x100000` rejected) and only 32 slots; a 640×400
  surface is exactly 1 MB and leaves no room.
- Sections are RAM-backed, not tied to the FB, so a present/copy path is
  still required.
- No VT integration, no format/scale policy, no singleton display semantics.

Sections remain the right tool for generic shared memory; Display is a
different abstraction.

### Alternative C: Pixel-push syscall

`sys_draw_rect(x, y, w, h, ptr)` copying from a user buffer.

**Rejected because:**

- One syscall per blit adds overhead and forces the kernel to own the
  scale/convert of arbitrary user data.
- Wins only if userland cannot map memory — which it can (mmap).
- Can be layered later as a convenience, not as the base primitive.

### Alternative D: VirtIO GPU / 3D engine

Drive a VirtIO-GPU device instead of GOP.

**Deferred:** the platform currently targets GOP/OVMF. VirtIO GPU is tracked
with the VirtIO block (`VIO-*`) and is post-1.0 (`ROADMAP.md` v1.3).

---

## 4. Affected Components

| Subsystem | Change | Impact |
|-----------|--------|--------|
| Object Manager (`object/`) | `ObType::Display=23`; register `\Device\Display`; `DisplayObOps` | Moderate |
| Display (`display/` — NEW) | `mod.rs`, `surface.rs`, `present.rs`, `object/display.rs` | New subsystem |
| Graphics (`graphics.rs`) | Expose `FramebufferInfo` accessors; no pixel path change | Low |
| Console (`console/`) | Skip physical text draw when VT mode is Graphics | Moderate |
| VT/Input (`input/vt.rs`, `input/manager.rs`) | `VtMode`, per-VT graphics owner | Moderate |
| Paging (`arch/x64/paging.rs`) | Map owned frames into mmap region as user-writable | Moderate |
| Syscall (`syscall/handlers.rs`, `ob/query`, `ob/set`) | Dispatch `DisplayInfo` (43), `DisplayMapBuffer/Present/FreeBuffer` (52–54) | Moderate |
| `object/types.rs` | New ObType + info classes | Low |
| `libneodos` | New `display.rs`, export table entry | Moderate |
| Boot (`main.rs`) | `display::init()` at Phase 3.86 | Low |
| NeoDev | None (no new on-disk artifacts) | None |
| Docs | `objects.md`, `syscalls.md`, `overview.md` | Low |

---

## 5. API Contract

### 5.1 `ob_open` — `\Device\Display`

| Aspect | Detail |
|--------|--------|
| Path | `\\Device\\Display` |
| Access | READ for info, WRITE for map/present |
| Returns | fd to a `ObType::Display` object |
| Errors | `-NoDev` if the display is not initialized, `-NotFound` if path invalid |

### 5.2 `ob_query_info` — DisplayInfo (class=43)

| Aspect | Detail |
|--------|--------|
| Args | `fd`, `class=43`, `buf`, `size` |
| Returns | Bytes written (32) |
| Errors | `-InvalidType` if fd is not Display, `-Fault` if buf invalid, `-InvalidParam` if size < 32 |
| Preconditions | fd from `ob_open` on `\Device\Display` |
| Postconditions | `buf` contains `DisplayInfo` |

### 5.3 `ob_set_info` — DisplayMapBuffer (class=52)

| Aspect | Detail |
|--------|--------|
| Args | `fd`, `class=52`, `buf` (`DisplayMap`), `size` |
| Returns | Surface base VA (≥ 1) in the caller's mmap region |
| Errors | `-InvalidParam` (bad size/format), `-NoMem` (allocation failed), `-Busy` (VT owned by another PID), `-NoDev` (no FB), `-Perm` (no WRITE access) |
| Preconditions | Caller has WRITE; size in `1..=1024×768`; format `Xrgb8888` |
| Postconditions | Frames allocated and mapped user-writable; VT claimed for caller |

### 5.4 `ob_set_info` — DisplayPresent (class=53)

| Aspect | Detail |
|--------|--------|
| Args | `fd`, `class=53`, `buf` (`DisplayPresent`), `size` |
| Returns | 0 |
| Errors | `-InvalidParam` (no surface mapped / bad rect), `-NoDev`, `-Perm` |
| Preconditions | `DisplayMapBuffer` succeeded; caller owns the VT |
| Postconditions | Surface blitted (converted/scaled) to the active FB, or no-op if VT inactive |
| Notes | Never blocks on IRQ/alloc; runs to completion in syscall context |

### 5.5 `ob_set_info` — DisplayFreeBuffer (class=54)

| Aspect | Detail |
|--------|--------|
| Args | `fd`, `class=54`, `buf=NULL`, `size=0` |
| Returns | 0 |
| Errors | `-Perm` |
| Postconditions | Views unmapped, frames freed, VT released (→ Text) |

### 5.6 Handle close / process exit

Closing the last Display handle for a PID (or `sys_exit`) MUST run the same
cleanup as `DisplayFreeBuffer`: unmap, free frames, release the VT
(`ObOperations::on_destroy`).

---

## 6. Test Plan

### 6.1 Object / Info

| Test | Expected |
|------|----------|
| `display_object_created_at_boot` | `ob_open("\\Device\\Display")` succeeds |
| `display_info_fields` | `DisplayInfo` width/height/stride match BootInfo |
| `display_info_wrong_type` | Query class 43 on a Pipe fd → `-InvalidType` |
| `display_info_small_buffer` | size < 32 → `-InvalidParam` |
| `display_not_creatable` | `ob_create(Display)` → `-InvalidType` |

### 6.2 Surface Mapping

| Test | Expected |
|------|----------|
| `display_map_native` | `DisplayMap{0,0}` returns VA and footprint `fb_w*fb_h*4` |
| `display_map_custom` | 320×200 returns a valid VA |
| `display_map_writable` | Write a pixel through the VA, read it back |
| `display_map_too_large` | 4096×4096 → `-NoMem` |
| `display_map_bad_format` | format ≠ Xrgb8888 → `-InvalidParam` |
| `display_map_twice_idempotent` | Second map returns the same VA |
| `display_map_wrong_vt_owner` | Second PID on same VT → `-Busy` |

### 6.3 Present

| Test | Expected |
|------|----------|
| `display_present_unmapped` | Present before map → `-InvalidParam` |
| `display_present_native` | 1:1 copy: FB pixel equals surface pixel |
| `display_present_scaled` | 320×200 → 640×400: corner pixels map by nearest neighbor |
| `display_present_inactive_vt_nop` | Present while `active_vt != vt`: FB unchanged |
| `display_present_damage` | Only the damage rect changes |
| `display_present_format` | `0x00RRGGBB` lands correctly in FB (XRGB) |

### 6.4 VT / Console Integration

| Test | Expected |
|------|----------|
| `vt_mode_starts_text` | All VTs are `Text` at boot |
| `vt_mode_graphics_on_map` | Claimed VT becomes `Graphics{owner}` |
| `console_suppressed_in_graphics` | `sys_write` to console does not touch FB |
| `vt_switch_back_redraws` | Switching to a Text VT redraws from shadow buffer |
| `vt_release_on_free` | FreeBuffer returns VT to `Text` |
| `vt_release_on_exit` | Process exit releases the VT and frees frames |

### 6.5 Memory / Invariants

| Test | Expected |
|------|----------|
| `display_frames_freed_on_free` | Frame allocator bitmap returns to baseline |
| `display_frames_freed_on_exit` | No leak across process exit (INV-5) |
| `display_no_alloc_in_present` | Present path performs no heap allocation |
| `display_user_va_in_mmap` | Surface VA ∈ `0x2000_0000..0x2200_0000` (Rule 3.2.3) |

### 6.6 End-to-End Smoke

| Test | Expected |
|------|----------|
| `display_fill_and_present` | Kernel/user test fills the surface with a pattern, presents, samples FB |
| `doom_smoke` (userland) | A `.NXE` fills 320×200 with moving pattern at 35 fps without tearing-induced faults |

---

## 7. Files and Modules

### New files

| Path | Description |
|------|-------------|
| `neodos-kernel/src/display/mod.rs` | Display core: types, init, VT binding, public API |
| `neodos-kernel/src/display/surface.rs` | Surface allocation/mapping/free |
| `neodos-kernel/src/display/present.rs` | Format conversion + nearest-neighbor scaling + blit |
| `neodos-kernel/src/object/display.rs` | `DisplayObOps` lifecycle |
| `libneodos/src/display.rs` | User-mode display/surface API + `blit_indexed8` |

### Modified files

| Path | Change |
|------|--------|
| `neodos-kernel/src/object/types.rs` | `ObType::Display=23`; `ObInfoClass::DisplayInfo=43`; `ObSetInfoClass::DisplayMapBuffer=52`, `DisplayPresent=53`, `DisplayFreeBuffer=54` |
| `neodos-kernel/src/syscall/handlers.rs` | Dispatch new info/set classes |
| `neodos-kernel/src/syscall/ob/query/mod.rs` | `DisplayInfo` query |
| `neodos-kernel/src/syscall/ob/set/mod.rs` | Map/Present/Free handlers |
| `neodos-kernel/src/graphics/mod.rs` | Accessors for `FramebufferInfo`; no longer the only writer of the FB |
| `neodos-kernel/src/console/mod.rs` | Respect `VtMode::Graphics` (suppress physical text) |
| `neodos-kernel/src/input/vt.rs` | `VtMode` definition |
| `neodos-kernel/src/input/manager.rs` | Per-VT mode + owner, present gating helpers |
| `neodos-kernel/src/arch/x64/paging.rs` | Map display frames into the mmap region as user pages |
| `neodos-kernel/src/main.rs` | `mod display;` + `display::init(fb_info)` at Phase 3.86 |
| `libneodos/src/lib.rs` / export | Register the display module/export entry |
| `docs/kernel/objects.md` | Display ObType, namespace, info classes |
| `docs/kernel/syscalls.md` | Document classes 43, 52–54 |
| `docs/architecture/overview.md` | Display subsystem in the map |
| `docs/development/testing.md` | New test groups |

---

## 8. Implementation Plan

### Step 1 — Display object skeleton

- Add `ObType::Display = 23` and the info/set class variants.
- Create `display/mod.rs` with `DisplayInfo`, `DisplayMap`, `DisplayPresent`,
  `DisplayError`.
- `display_init(fb)`: store FB info, create `\Device\Display` via
  `ob_create_object_path`, install `DISPLAY_OPS`.
- `cargo build` + boot smoke (object visible via `kobj`).

### Step 2 — Surface

- Implement `Surface::alloc` (frame allocator, page-granular).
- Implement `map_into_current` (map frames into the mmap region user-writable
  cacheable) and `unmap`/`free`.
- Tests from §6.2 (without VT yet).

### Step 3 — Present engine

- Implement `convert_xrgb` (identity for v1) and `scale_nearest`.
- Implement `present(surface, fb, rect)` with 1:1 `memcpy` fast path.
- Tests from §6.3 (without VT gating).

### Step 4 — Syscall classes

- Wire `DisplayInfo` (43), `DisplayMapBuffer` (52), `DisplayPresent` (53),
  `DisplayFreeBuffer` (54).
- Handle cleanup on `close`/exit via `DisplayObOps::on_destroy`.
- Tests from §6.1/6.5.

### Step 5 — VT integration

- Add `VtMode` to `input/vt.rs`, per-VT mode/owner to `input/manager.rs`.
- Gate present on `active_vt`; suppress console text in Graphics mode.
- Release on free/exit; restore text on VT switch back.
- Tests from §6.4.

### Step 6 — libneodos

- `display.rs` with `Display`/`Surface` and `blit_indexed8`.
- Export table entry + link.
- Build a small `.NXE` smoke test that fills/presents.

### Step 7 — Docs & validation

- Update `objects.md`, `syscalls.md`, `overview.md`, `testing.md`.
- `cargo build` in `neodos-kernel/`, `neodev test`, `neodev check-deps`,
  `npx markdownlint '**/*.md' --config .markdownlint.json`.
- `display_fill_and_present` end-to-end on QEMU.

---

## 9. Future Work

| Item | Priority | Complexity | Description |
|------|----------|------------|-------------|
| Dirty-rect present | High | Low | Present only changed regions |
| Double buffer / flip | High | Medium | Two surfaces, flip on present (tearing control) |
| Vsync / vblank hook | Medium | Medium | Tie present to the 1 kHz timer or a vblank IRQ |
| `Indexed8` surfaces | Medium | Medium | Shared palette, 8-bit surfaces for low-memory clients |
| Cursor plane | Medium | Medium | Hardware/software cursor composited at present |
| Format negotiation | Medium | Low | Read GOP pixel format instead of assuming XRGB8888 |
| Async present via IRP | Low | High | Queue present and continue rendering |
| Damage via KWait/Event | Low | Medium | Signal vblank/complete through the event bus |
| GPU / VirtIO-GPU | Low | Very High | Hardware acceleration path (`ROADMAP.md` v1.3) |
| Multi-head | Experimental | Very High | Multiple `\Device\DisplayN` objects |

---

## 10. Dependencies

- `memory` / frame allocator (`hal::alloc_page`, `map_page`, `unmap_page`).
- `arch/x64/paging` (mmap region, user-accessible mapping).
- `object` (Ob types, `ObOperations`, namespace).
- `graphics` (`FramebufferInfo`, boot-provided FB).
- `input` (VT manager for ownership/gating).
- `alloc` (`Vec` for surface frames).
- **MUST NOT** depend on scheduler, VFS, or block drivers (INV-1).
- **MUST NOT** allocate or block in IRQ context (INV-2/INV-3).

---

## 11. Open Questions

1. **Surface format** — is `XRGB8888` sufficient for the GOP modes OVMF
   exposes, or should we read the format from the bootloader now?
2. **Tearing** — accept single-buffer tearing for v1, or require double
   buffering before shipping Doom?
3. **Ownership model** — one graphics client per VT (proposed) or one global
   client with the others queueing?
4. **Present targets** — should `present` be restricted to the caller's VT, or
   allow an explicit VT/CRTC index for future multi-head?
