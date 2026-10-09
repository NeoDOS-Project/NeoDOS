# Audio Device / User-Mode API — Design Document

> **Version:** v0.1-draft
> **Status:** Design
> **Target Release:** v0.53+ (optional)
> **Issue:** [#469](https://github.com/NeoDOS-Project/NeoDOS/issues/469)
> **Related:** `VIO-ARCH` (#32), [#36](https://github.com/NeoDOS-Project/NeoDOS/issues/36)
> **ABI Impact:** New `ObType::AudioDevice` (25), new `ObInfoClass` (49–50),
> new `ObSetInfoClass` (58–61)

---

## 1. Problem Analysis

### 1.1 Current Limitation

NeoDOS has no sound. The PCI layer classifies device class `0x04` as
`DeviceClass::Audio` (`drivers/device/mod.rs`), but no driver implements it and
there is no userland API. Doom runs silently today.

### 1.2 Scope and Priority

Audio is **not** a Doom blocker — Doom boots and plays without it. This design
defines the object model and API so a driver can be added later without ABI
churn. It is intentionally backend-agnostic.

---

## 2. Design

### 2.1 Object Model

| Property | Value |
|----------|-------|
| Path | `\Device\Audio` |
| `ObType` | `AudioDevice = 25` |
| Created | by the audio NEM driver on registration (or a stub at boot) |
| User-creatable | No |

A minimal **stub object** can be created at boot even with no driver so that
`ob_open` works and returns `-NoDev` on playback, matching the display
approach.

### 2.2 PCM Ring

Playback uses a kernel-allocated ring mapped into the client (same mechanism as
the display back buffer):

```rust
pub struct PcmBuffer {
    pub base: u64,        // user VA (mmap region)
    pub frames: u64,      // bytes
    pub write_index: u64, // consumed by client
    pub read_index: u64,  // consumed by DMA/driver
    pub format: u32,
    pub channels: u32,
    pub rate: u32,
}
```

The client writes PCM into the ring; the driver consumes it via DMA. The
kernel never copies sample-by-sample in the syscall path.

### 2.3 API

#### ObInfoClass

| Value | Name | Payload |
|-------|------|---------|
| 49 | `AudioInfo` | `{ formats: u32, min_rate: u32, max_rate: u32, max_channels: u32, sample_rates: [u32; 8], _pad }` |
| 50 | `AudioPosition` | `{ frames_played: u64, _pad: u64 }` |

#### ObSetInfoClass

| Value | Name | Payload |
|-------|------|---------|
| 58 | `AudioSetFormat` | `{ format: u32, channels: u32, rate: u32, buffer_frames: u32 }` |
| 59 | `AudioMapBuffer` | none; returns the ring's user VA |
| 60 | `AudioStart` | none |
| 61 | `AudioStop` | none |

Formats: `PCM_S16LE = 1` (required), `PCM_U8 = 2`, `PCM_S32LE = 3` (optional).

### 2.4 Backends

1. **VirtIO Sound** — fits QEMU and the `VIO-ARCH` (§#32) virtqueue
   abstraction; preferred for development.
2. **AC'97 / HDA** — real hardware and VirtualBox.
3. **PC speaker / PIT** — a legacy beep fallback only; not PCM.

The design does not mandate a backend; `AudioInfo.formats` advertises what the
bound driver supports.

### 2.5 libneodos

New module `libneodos/src/audio.rs`:

```rust
pub struct Audio { fd: u8 }
pub struct Pcm { pub base: *mut u8, pub bytes: usize, pub format: u32, pub rate: u32, pub channels: u32 }

impl Audio {
    pub fn open() -> Result<Audio, i64>;
    pub fn info(&self) -> Result<AudioInfo, i64>;
    pub fn configure(&self, format: u32, channels: u32, rate: u32, frames: u32) -> Result<(), i64>;
    pub fn map(&self) -> Result<Pcm, i64>;
    pub fn start(&self) -> Result<(), i64>;
    pub fn stop(&self) -> Result<(), i64>;
    pub fn position(&self) -> Result<u64, i64>;
}
```

A helper `mix_s16()` and an 8-bit sample expander make Doom's SFX path
straightforward.

---

## 3. Alternatives Considered

- **Per-sample `write` syscall**: simple but syscall-heavy and copies through
  the kernel; rejected in favor of the mmap ring.
- **Kernel mixer service**: flexible (multiple clients, software mixing) but
  large; deferred. v1 assumes one client owns the device.
- **Userspace-only via PCI BAR MMIO**: unsafe and bypasses the driver
  isolation model (INV-5, NEM caps); rejected.

---

## 4. Affected Components

| Subsystem | Change | Impact |
|-----------|--------|--------|
| Audio (`drivers/audio/` — NEW) | NEM driver(s) | Large |
| `object/types.rs` | `AudioDevice=25`; classes | Low |
| `object/audio.rs` | Lifecycle ops | Low |
| `syscall/ob/*` | Info/set classes | Moderate |
| `libneodos/audio.rs` | Client API | Moderate |
| `arch/x64/paging.rs` | Map ring frames (shared with display) | Low |
| `nem` caps | `CAP_DMA`, `CAP_IRQ`, `CAP_MMIO` for audio | Low |
| `docs/drivers/overview.md` | Document audio class | Low |

---

## 5. API Contract

| Operation | Returns / Errors |
|-----------|------------------|
| `AudioInfo` | capabilities; `-InvalidType` on wrong fd |
| `AudioSetFormat` | 0; `-InvalidParam` if unsupported; `-Busy` if already started |
| `AudioMapBuffer` | user VA; `-NoDev` without driver |
| `AudioStart`/`Stop` | 0; idempotent |
| `AudioPosition` | frames played |

Handle close/exit MUST stop playback and free the ring.

---

## 6. Test Plan

| Test | Expected |
|------|----------|
| `audio_stub_open` | `ob_open("\\Device\\Audio")` succeeds without a driver |
| `audio_no_driver_nodev` | SetFormat on stub → `-NoDev` |
| `audio_info_caps` | Advertised formats match the driver |
| `audio_map_ring` | Ring VA in mmap region, writable |
| `audio_start_stop` | State transitions correct; double start idempotent |
| `audio_unsupported_format` | Bad format → `-InvalidParam` |
| `audio_ring_position` | `frames_played` advances while started |
| `audio_cleanup_on_exit` | Stop + unmap on process exit |

---

## 7. Files and Modules

### New

| Path | Description |
|------|-------------|
| `neodos-kernel/src/object/audio.rs` | Lifecycle ops |
| `neodos-kernel/src/drivers/audio/mod.rs` | Backend dispatch |
| `neodos-kernel/src/drivers/audio/virtio_snd.rs` | VirtIO Sound backend |
| `libneodos/src/audio.rs` | Client API |
| `userbin/` | `audioinfo.nxe` diagnostic (optional) |

### Modified

| Path | Change |
|------|--------|
| `neodos-kernel/src/object/types.rs` | Type + classes |
| `neodos-kernel/src/syscall/ob/query/mod.rs` | `AudioInfo`, `AudioPosition` |
| `neodos-kernel/src/syscall/ob/set/mod.rs` | Configure/Map/Start/Stop |
| `neodos-kernel/src/boot/mod.rs` | Audio stub init |
| `docs/drivers/overview.md` | Audio category |

---

## 8. Implementation Plan

1. Define `ObType::AudioDevice=25`, classes, and the boot **stub** object.
2. Implement `AudioInfo`/`AudioPosition` on the stub + tests.
3. Add the PCM ring + `AudioMapBuffer`/`Start`/`Stop` on the stub.
4. Implement the VirtIO Sound NEM driver and bind it.
5. libneodos API + a smoke binary.
6. Docs + tests + markdownlint.

---

## 9. Open Questions

1. Single client vs kernel mixer in v1? (Proposed: single client.)
2. Ring size default (proposed: 64 KiB) and underrun policy (silence vs stop)?
3. Should `AudioInfo` include built-in MIDI/FM, or is PCM enough for Doom?
   (Proposed: PCM enough; Doom's music can be rendered to PCM.)
4. Reuse the display's frame-mapping helper or a generic `MmioBuffer` type?

---

## 10. Dependencies

- NEM driver framework + isolation/caps.
- HAL PCI (ECAM) and DMA/IRQ primitives.
- Virtual memory (ring mapping).
- Object Manager.
- Optional `VIO-ARCH` (#32) for VirtIO Sound.
