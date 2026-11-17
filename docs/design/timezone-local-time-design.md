# Time Zone / Local Time — Design Document

> **Version:** v0.1-draft
> **Status:** Design (no implementation)
> **Target Release:** v0.57 — Executive Manager (configuration consolidation); the
> kernel parts are additive and could land earlier
> **Base issue:** #357 (closed) — "Timezone/DST support for the system clock"
> **Related:** #30/#98/#323/#326/#327 (neocfg + libneodos helpers),
> #26/#356/#361 (NTP), #467 (monotonic clock), #579 (regional formats),
> `docs/design/neocfg-design.md`, `docs/design/monotonic-clock-design.md`
> **ABI Impact:** new `ObInfoClass::TimeZoneInfo = 43`, new
> `ObSetInfoClass::TimeZone = 52`; additive Registry values; **no new syscall
> number** (all access via existing `sys_ob_query_info` / `sys_ob_set_info`)

---

## 1. Research

### 1.1 Sources read

| Source | What it establishes |
| --- | --- |
| `docs/architecture/vision.md` | DOS-like UX + NT-like architecture; no timezone content today — this feature is greenfield |
| `docs/kernel/objects.md` | `ObType` enum (max 22), `ObInfoClass` table (0–42), `ObSetInfoClass` table (0–51), namespace, `ob_open`/`ob_create`/`ob_query_info`/`ob_set_info` semantics, user-creatable type list |
| `docs/kernel/syscalls.md` | calling convention, `SyscallError`, `SyscallNum` (RAX 0–99), rule "every new syscall MUST be `sys_ob_*`" |
| `docs/registry/registry.md` | `CurrentControlSet\Control\TimeZoneInformation` key and value names; `\Locale\Language` |
| `docs/design/monotonic-clock-design.md` | the house style for Ob info/set class design docs |
| `neodos-kernel/src/cm/timezone.rs` | the existing `TimeZone` model (`utc_offset_minutes`, DST window in month/day, `to_local`, `load`) + 8 tests |
| `neodos-kernel/src/syscall/ob/query/time.rs` | `DateTime=9`, `TimeZone=40`, `LocalDateTime=41` handlers |
| `neodos-kernel/src/syscall/ob/set/time.rs`, `set/mod.rs` | `DateTime=50` set path + `validate_datetime` |
| `neodos-kernel/src/object/types.rs` | authoritative `ObInfoClass`/`ObSetInfoClass` numbering |
| `neodos-kernel/src/cm/init.rs` | boot defaults for `TimeZoneInformation` (UTC, DST off) |
| `libneodos/src/syscall/{time.rs,ob.rs,types.rs}` | the only userland time API: `ob_set_datetime`, classes 9/40/41/50, `DateTime`, `SysTimeZone` |
| `libntp/src/lib.rs` | `UtcDateTime`, civil conversion, hardcoded `DD/MM/YY` formatting |
| `libneodos/src/i18n.rs`, `libnlt/src/region.rs` | `i18n_format_date`/`i18n_format_time` exist but no `[region]` data is shipped |
| `userbin/datetime/src/main.rs` | reads `LocalDateTime` by default, `/U` for UTC, `/S` sets UTC |

### 1.2 Current state (verified)

- **UTC is authoritative.** The RTC is stored/read as UTC (`drivers/hw/rtc.rs`,
  `rtc_bridge`); `ntpd` steps it with UTC (`apply_time`), and `datetime /S`
  writes UTC.
- **Local time is derived on demand** in the kernel query handler using a fixed
  offset plus a DST window stored in the Registry
  (`cm/timezone.rs::TimeZone::to_local`).
- **The DST model is coarse**: `[start_month/day, end_month/day)`, evaluated on
  the **UTC date**; no transition hour, no per-year rules, no zone name, no
  IANA identifier.
- **There is no local→UTC resolution** (ambiguity/nonexistent times), and no
  way for a tool to learn the IANA id or the *current* effective offset.
- **Configuration is by editing the Registry directly**; there is no validated
  API and no userland wrapper (`libneodos/src/syscall/time.rs` only has
  `ob_set_datetime`).
- **Formatting is locale-blind**: `datetime` uses `libntp::format_date`
  (`DD/MM/YY`); `i18n_format_date`/`i18n_format_time` exist but are unused and
  no `[region]` block ships in `data/locale/*`.

### 1.3 Reusable abstractions

- **Registry (Cm)** for persistent configuration (do not invent a second store).
- **Ob info/set classes** on `\Global\Info\DateTime` (native_id 5) as the ABI.
- **`libntp`** civil/Unix conversion for the kernel-free fallback path.
- **`libnlt::region`** for locale date/time patterns.
- **NTP/NTP issue family** for clock accuracy (a separate concern).

---

## 2. Problem analysis

### 2.1 Current limitation

1. **No zone identity.** The system stores a numeric offset, not
   `Europe/Madrid`. Users and tools cannot select a zone by name, and DST rules
   cannot be matched to a real zone.
2. **Inaccurate DST.** Transitions happen at a **day** granularity, not an hour,
   and are evaluated on the UTC date; the real Europe/Madrid switch is at
   `02:00`/`03:00` local. DST is therefore wrong for several hours around each
   transition.
3. **No reverse conversion.** Given a local civil time, the system cannot
   produce the UTC instant, and cannot report the two standard edge cases
   (ambiguous times at fall-back; nonexistent times at spring-forward).
4. **No observable state.** A tool cannot ask "what is the current effective
   offset / is DST active / which zone is configured?"; `ObInfoClass::TimeZone`
   returns only the raw window.
5. **Unvalidated, non-atomic configuration.** Writing the Registry directly can
   leave inconsistent offsets/windows; there is no admin gate, no range
   validation, and no flush guarantee.
6. **Formatting does not follow the locale**, despite the formatter existing.

### 2.2 Why existing abstractions cannot solve it

- The **Registry** is a generic key/value store: it has no timezone semantics,
  validation, or derived state.
- **`ObInfoClass::LocalDateTime`** returns a formatted `SysDateTime` only; it is
  lossy (no offset, no DST flag, no IANA id) and cannot express an arbitrary
  instant or an ambiguous local time.
- **`ObInfoClass::TimeZone`** returns the raw fixed window; it has no identity,
  no transition hour, and no "now" state.
- **`libntp`** converts UTC↔civil but has no zone rules and no DST logic.
- The **i18n runtime** formats numbers/dates but has no timezone conversion and
  no shipped region data.

So the feature needs (a) a richer, validated kernel configuration surface with
an IANA identity and a "current state" query, and (b) a userland zone engine
that performs precise, DST-aware conversions.

---

## 3. Solution design

Two cooperating layers, with a strict boundary:

- **Kernel = authoritative UTC + validated configuration + coarse local
  derivation.** No tzdata blob in the kernel.
- **Userland `libtimezone` = precise TZif conversion** (including DST ambiguity),
  consuming the kernel's UTC and the configured IANA id.

### 3.1 New types / structs / enums

**No new `ObType`.** The configuration and queries ride on the existing
`\Global\Info\DateTime` **Key** object (native_id 5), exactly like
`DateTime=9`/`TimeZone=40`/`LocalDateTime=41`. (`ObType` max is 22 and the rule
is to avoid unnecessary object types.)

Kernel ABI payloads (`neodos-kernel/src/syscall/ob/types.rs`, `#[repr(C)]`):

```rust
/// Returned by ObInfoClass::TimeZoneInfo (43). Size = 84, align = 4.
#[repr(C)]
pub struct SysTimeZoneInfo {
    pub utc_offset_minutes: i32,       // standard offset: local = UTC + this
    pub dst_offset_minutes: i32,       // extra minutes while DST active
    pub effective_offset_minutes: i32, // base (+ dst) for "now"
    pub dst_active: u8,                // 1 if DST is active at query time
    pub valid: u8,                     // 1 if the RTC read succeeded
    pub dst_start_month: u8,
    pub dst_start_day: u8,
    pub dst_start_hour: u8,            // NEW: transition hour (local std time)
    pub dst_end_month: u8,
    pub dst_end_day: u8,
    pub dst_end_hour: u8,              // NEW
    pub key_name: [u8; 64],            // IANA id, NUL-terminated (may be empty)
}

/// Payload of ObSetInfoClass::TimeZone (52). Size = 84, align = 4.
#[repr(C)]
pub struct SysTimeZoneConfig {
    pub utc_offset_minutes: i32,
    pub dst_offset_minutes: i32,
    pub dst_enabled: u32,
    pub dst_start_month: u8,
    pub dst_start_day: u8,
    pub dst_start_hour: u8,
    pub dst_end_month: u8,
    pub dst_end_day: u8,
    pub dst_end_hour: u8,
    pub _pad: [u8; 2],
    pub key_name: [u8; 64],
}
```

Kernel-internal model (`neodos-kernel/src/cm/timezone.rs`) — extend the existing
`TimeZone` struct additively:

```rust
pub struct TimeZone {
    pub utc_offset_minutes: i32,
    pub dst_offset_minutes: i32,
    pub dst_enabled: bool,
    pub dst_start_month: u8, pub dst_start_day: u8, pub dst_start_hour: u8, // NEW
    pub dst_end_month: u8,   pub dst_end_day: u8,   pub dst_end_hour: u8,   // NEW
    pub key_name: [u8; 64],                                                 // NEW
}
```

Userland (`libtimezone`, new plain crate — not an NXL, see §4):

```rust
pub struct Instant(i64);                       // Unix seconds (UTC)
pub struct Civil { pub year: i32, pub month: u8, pub day: u8,
                   pub hour: u8, pub minute: u8, pub second: u8 }
pub enum Fold { Earlier, Later }
pub enum LocalResult { Unique(Instant), Ambiguous(Instant, Instant), None }
pub struct Tzif { /* parsed RFC 8536 */ }
impl Tzif {
    pub fn parse(bytes: &[u8]) -> Result<Tzif, TzError>;
    pub fn identifier(&self) -> &str;
    pub fn to_local(&self, utc: Instant) -> Civil;
    pub fn to_instant(&self, local: Civil, fold: Fold) -> LocalResult;
}
pub trait ZoneSource {
    fn by_name(&self, iana: &str) -> Result<Tzif, TzError>;
    fn available(&self) -> &[&str];
    fn version(&self) -> Option<&str>;         // tzdata release, e.g. "2026a"
}
```

### 3.2 New syscalls

**None.** All access uses the existing Object Manager syscalls:

- read config/state: `sys_ob_query_info` (RAX 42) with a new class;
- write config: `sys_ob_set_info` (RAX 43) with a new class.

This satisfies the rule "every new syscall MUST be `sys_ob_*`" trivially (no new
syscall is added) and avoids growing the SSDT.

### 3.3 New `ObInfoClass` / `ObSetInfoClass` variants

| Enum | Value | Name | Direction | Notes |
| --- | --- | --- | --- | --- |
| `ObInfoClass` | 43 | `TimeZoneInfo` | read | Full config + current state; superset of `TimeZone=40` (kept) |
| `ObSetInfoClass` | 52 | `TimeZone` | write | Validated, admin-only configuration |

Numbering check (from `neodos-kernel/src/object/types.rs`): `ObInfoClass` 42 is
`ProcessShutdownState`, so 43 is the next free value. `ObSetInfoClass` 51 is
`SetForegroundProcess`, so 52 is free.

The existing `ObInfoClass::TimeZone=40` and `LocalDateTime=41`, and
`ObSetInfoClass::DateTime=50`, are **unchanged** (ABI v8 compatibility).

### 3.4 New files / modules

Kernel (`neodos-kernel/src/`): **no new module is strictly required.** The change
extends existing files. If `cm/timezone.rs` grows beyond a comfortable size, split
it into a directory module (optional, mechanical):

```text
neodos-kernel/src/cm/timezone/        (optional split)
    mod.rs        — public API: load(), to_local(), validate()
    model.rs      — TimeZone struct + offset math
    registry.rs   — Registry read/write (load/save/flush)
    convert.rs    — add_minutes / in_dst / transition-hour logic
```

Userland (new, at repo root, following the `libntp`/`libdns` plain-crate
convention):

```text
libtimezone/Cargo.toml
libtimezone/src/lib.rs          — public API (Instant/Civil/Tzif/LocalResult)
libtimezone/src/tzif.rs         — RFC 8536 parser
libtimezone/src/convert.rs      — UTC↔civil + DST resolution
libtimezone/src/source.rs       — ZoneSource over C:\System\Zoneinfo
libtimezone/src/tests.rs        — host tests (fixtures)
```

Zone data (image content, not code):

```text
C:\System\Zoneinfo\Europe\Madrid
C:\System\Zoneinfo\tzdata.version
```

### 3.5 Changes to existing files

| Path | Change |
| --- | --- |
| `neodos-kernel/src/cm/timezone.rs` | Add `dst_start_hour`/`dst_end_hour`/`key_name`; transition-hour-aware `in_dst`/`offset_for`; `validate()`; `save()` + flush |
| `neodos-kernel/src/cm/init.rs` | Seed new defaults `DaylightStartHour=2`, `DaylightEndHour=3`, `TimeZoneKeyName=""` |
| `neodos-kernel/src/syscall/ob/query/time.rs` | Add `TimeZoneInfo=43` handler; `handles()` accepts it |
| `neodos-kernel/src/syscall/ob/set/time.rs` | Add `TimeZone=52` handler (admin-only, validated, persist) |
| `neodos-kernel/src/syscall/ob/types.rs` | Add `SysTimeZoneInfo`, `SysTimeZoneConfig` |
| `neodos-kernel/src/object/types.rs` | Add `ObInfoClass::TimeZoneInfo=43`, `ObSetInfoClass::TimeZone=52` |
| `neodos-kernel/src/testing.rs` | Register new `cm::timezone` tests |
| `libneodos/src/syscall/types.rs` | Mirror `SysTimeZoneInfo`/`SysTimeZoneConfig` |
| `libneodos/src/syscall/time.rs` | Add read wrappers + `ob_set_timezone` |
| `userbin/datetime/src/main.rs` | Use `libtimezone` + `i18n_format_date`/`i18n_format_time` |
| `libneocfg/src/modules/` + `userbin/neocfg/src/neodos_platform.rs` | Add Date/Time & Time Zone module (see `docs/design/neocfg-design.md`) |
| `data/locale/{en-US,es-ES,ca-ES}/*.toml` | Ship `[region]` date/time patterns |
| `docs/registry/registry.md`, `docs/kernel/objects.md`, `docs/kernel/syscalls.md` | Document the new values/classes |

### 3.6 Semantics

- **UTC stays authoritative**; local time is always derived, never stored.
- **Transition fields are interpreted in local standard time** (IANA
  convention); `in_dst` compares against the UTC instant converted to standard
  local time. This makes Europe/Madrid (`+1`, DST `+1`, last Sun Mar/Oct,
  `02:00`/`03:00`) behave correctly.
- **The kernel is the coarse fallback.** Precise behavior, including ambiguity,
  comes from `libtimezone`/TZif. If TZif is absent, `libtimezone` falls back to
  the kernel's fixed offset, then to UTC, and reports the degradation.
- **Set is atomic**: validate the whole payload, write all Registry values, then
  `cm_flush_key`; report failure. A UI must not claim "applied" unless the flush
  succeeded (see the persistence caveat in §6.4).

---

## 4. Alternatives

- **A. Full IANA tzdata engine in the kernel.**
  Rejected: a multi-hundred-KB blob in the kernel, a large new parser surface,
  harder updates, and it contradicts the minimal-kernel / `INV-11` direction.
  The kernel only needs the authoritative instant and a coarse fallback.
- **B. Keep the current fixed model and just document Registry editing.**
  Rejected: no validation, no identity, no transition hour, no reverse
  conversion, and every tool would re-implement DST — the current defect.
- **C. A new `sys_ob_timezone` syscall.**
  Rejected: `ob_query_info`/`ob_set_info` with a class already covers it, keeps
  the SSDT stable, and follows the established pattern (`DateTime=9/50`).
- **D. Package `libtimezone` as an NXL (`libtimezone.nxl`).**
  Rejected for the first milestone: NXLs are global, load-once, never unloaded,
  and the loader only checks `version != 0`; the existing reuse libraries
  (`libntp`, `libdns`, `libnet-config`) are plain crates. A plain, host-testable
  crate is the better fit. An NXL wrapper can be added later if runtime sharing
  or independent versioning is genuinely needed.
- **E. Per-user / per-process time zones.**
  Deferred: the system has a single timezone today; per-session zones belong to
  the Session Manager work and are out of scope.
- **F. Reuse the monotonic clock for conversion.**
  Rejected: conversion is wall-clock, not duration; `#467` is orthogonal.

---

## 5. Affected components

| Subsystem | Change | Impact |
| --- | --- | --- |
| `cm::timezone` | Model + transition hours + identity + validate/save | Medium (additive) |
| `cm::init` | New default values | Low |
| `syscall/ob/query` | New `TimeZoneInfo` handler | Low |
| `syscall/ob/set` | New `TimeZone` handler | Low |
| `object/types.rs` | Two new class constants | Low |
| `syscall/ob/types.rs` | Two new `repr(C)` payloads | Low |
| Object Manager | **No new `ObType`**, no new namespace object | None |
| Registry | Additive values under the existing key | Low |
| `libneodos` | New read wrappers + `ob_set_timezone` + mirrored structs | Low |
| `libtimezone` (new) | TZif engine | New crate |
| `datetime`, `neocfg` | Consume the new API; locale formatting | Low |
| i18n | Ship `[region]` data; use existing formatters | Low |
| **Scheduler / VFS / memory / drivers** | **None** (DAG preserved, `INV-1`) | None |
| Docs | objects.md, syscalls.md, registry.md, this doc | Low |

---

## 6. API contract

### 6.1 `ObInfoClass::TimeZoneInfo` (43) — query

| Aspect | Value |
| --- | --- |
| Object | `\Global\Info\DateTime` (`ObType::Key`, `native_id == 5`) |
| Args | `fd`, class 43, `buf_ptr`, `buf_size` |
| Buffer | `SysTimeZoneInfo`, 84 bytes |
| Returns | 84 on success |
| Errors | `Inval` (wrong object/native_id or `buf_size < 84`), `BadF` (bad fd), `Fault` (bad user pointer) |
| Preconditions | fd open with READ; valid user buffer |
| Postconditions | No state change; `valid=0` if the RTC read failed |

### 6.2 `ObSetInfoClass::TimeZone` (52) — set

| Aspect | Value |
| --- | --- |
| Object | `\Global\Info\DateTime` (`ObType::Key`, `native_id == 5`) |
| Args | `fd`, class 52, `buf_ptr`, `buf_size` |
| Buffer | `SysTimeZoneConfig`, 84 bytes |
| Returns | 0 on success (persisted + flushed) |
| Errors | `Acces` (not admin), `Inval` (wrong object, size, or any out-of-range field), `Io` (Registry flush failed) |
| Preconditions | fd open READ and WRITE; caller is admin; all fields valid: offsets in `[-1440, 1440]`, `dst_offset_minutes` in `[0, 1440]`, months `1..=12`, days valid for the month, hours `0..=23`, `key_name` ≤ 63 printable bytes |
| Postconditions | Registry `TimeZoneInformation` updated and flushed; subsequent `TimeZoneInfo`/`LocalDateTime` reflect it |

### 6.3 `libneodos` wrappers (proposed)

```rust
pub fn ob_get_datetime() -> Result<DateTime, i64>;          // class 9
pub fn ob_get_local_datetime() -> Result<DateTime, i64>;    // class 41
pub fn ob_get_timezone() -> Result<SysTimeZone, i64>;       // class 40 (kept)
pub fn ob_get_timezone_info() -> Result<SysTimeZoneInfo, i64>; // class 43
pub fn ob_set_timezone(cfg: &SysTimeZoneConfig) -> Result<(), i64>; // class 52
```

### 6.4 Persistence caveat

`cm_flush_key` serializes the hive and writes it, but `NeoDosFsV2::write` does
not commit the superblock (`neodos-kernel/src/fs/neofs/neodos_v2.rs`), and there
is no `fsync`. Until durable flush lands, a successful `ob_set_timezone` return
means "written to the hive and flush attempted", not "durable across a crash".
The neocfg UI MUST surface this ("persist pending") rather than "applied". This
is a dependency on the Registry/FS durability work, not a defect of this design.

### 6.5 `libtimezone` (proposed)

| Function | Args | Returns | Errors |
| --- | --- | --- | --- |
| `Tzif::parse` | `&[u8]` | `Tzif` | `TzError::BadMagic/BadVersion/Truncated/Unsupported` |
| `Tzif::to_local` | `Instant` | `Civil` | — |
| `Tzif::to_instant` | `Civil`, `Fold` | `LocalResult` | — |
| `ZoneSource::by_name` | `&str` | `Tzif` | `TzError::NotFound` |
| `ZoneSource::version` | — | `Option<&str>` | — |

---

## 7. Test plan

At least three cases per invariant. Kernel tests are registered in
`neodos-kernel/src/testing.rs`; userland tests run on the host with fixtures.

**INV-1 — UTC is authoritative; local time is derived, never stored.**

| Test | Expected |
| --- | --- |
| `tz_query_does_not_mutate_rtc` | `TimeZoneInfo` query leaves the RTC fields unchanged |
| `tz_local_is_derived` | Changing `utc_offset_minutes` changes `LocalDateTime` without touching `DateTime` |
| `tz_query_valid_flag` | With no RTC, `valid=0` and fields are zeroed |

**INV-2 — Configuration is validated and applied atomically.**

| Test | Expected |
| --- | --- |
| `tz_set_rejects_bad_offset` | `utc_offset_minutes=2000` → `Inval`, Registry unchanged |
| `tz_set_rejects_bad_month` | month `13` → `Inval` |
| `tz_set_rejects_bad_hour` | hour `24` → `Inval` |
| `tz_set_rejects_oversize_key` | `key_name` without NUL in 64 bytes → `Inval` |
| `tz_set_roundtrip` | Valid set → `TimeZoneInfo` returns the same values |

**INV-3 — Offset math normalizes day/month/year and honours the DST window with hours.**

| Test | Expected |
| --- | --- |
| `tz_offset_positive` | `+60` at 12:00 → 13:00 (existing, kept) |
| `tz_offset_wraps_next_day` | 23:30 + 60 → 00:30 next day (existing, kept) |
| `tz_dst_boundary_hour` | Madrid: 01:59 local std vs 02:00 local std flips DST at the hour, not the day |
| `tz_dst_southern_wrap` | Wrapping window still works (existing, kept) |

**INV-4 — `libtimezone` matches TZif fixtures (Europe/Madrid as a test case).**

| Test | Expected |
| --- | --- |
| `tzif_parse_madrid` | Parses `Europe/Madrid`; `identifier() == "Europe/Madrid"` |
| `madrid_spring_forward_gap_is_none` | 2026-03-29 02:30 local → `LocalResult::None` |
| `madrid_fall_back_is_ambiguous` | 2026-10-25 02:30 local → `Ambiguous(early, late)` |
| `madrid_fold_selects` | `Fold::Earlier`/`Fold::Later` pick the correct UTC instant |
| `madrid_offset_summer_winter` | Summer `+2`, winter `+1` |

**INV-5 — Degradation is deterministic and reported.**

| Test | Expected |
| --- | --- |
| `zone_missing_falls_back_to_offset` | Unknown id → kernel fixed offset, `Degraded` reported |
| `zone_missing_then_utc` | No offset either → UTC, `Degraded` reported |
| `tzif_corrupt_rejected` | Corrupt bytes → `TzError`, no panic |

**INV-6 — Access control.**

| Test | Expected |
| --- | --- |
| `tz_set_requires_admin` | Non-admin set → `Acces` |
| `tz_query_any_process` | Non-admin query → 84 bytes |
| `tz_set_wrong_object` | Set on a non-`\Global\Info\DateTime` Key → `Inval` |

**Integration (QEMU/VirtualBox).**

| Test | Expected |
| --- | --- |
| `datetime_shows_local` | `datetime` shows Madrid local time; `datetime /U` shows UTC |
| `datetime_uses_locale_format` | With an `es-ES` `[region]`, output matches the pattern |
| `neocfg_timezone_module` | Selecting a zone updates `TimeZoneInfo`; UI reports persistence truthfully |

---

## 8. Implementation plan

Sequential, independently testable steps:

1. **Kernel model + math.** Extend `TimeZone` with `dst_start_hour`/`dst_end_hour`/
   `key_name`; make `in_dst`/`offset_for` hour-aware. Files:
   `neodos-kernel/src/cm/timezone.rs`. Add unit tests (INV-3).
2. **Registry defaults + load/save.** Seed new defaults in
   `neodos-kernel/src/cm/init.rs`; add `validate()` and `save()` (write all
   values + `cm_flush_key`) in `cm/timezone.rs`. Tests: INV-2 (validation).
3. **ABI structs + classes.** Add `SysTimeZoneInfo`/`SysTimeZoneConfig`
   (`syscall/ob/types.rs`) and `ObInfoClass::TimeZoneInfo=43` /
   `ObSetInfoClass::TimeZone=52` (`object/types.rs`).
4. **Query handler.** Implement class 43 in
   `neodos-kernel/src/syscall/ob/query/time.rs` (extend `handles()`); tests INV-1.
5. **Set handler.** Implement class 52 in
   `neodos-kernel/src/syscall/ob/set/time.rs` (admin gate + validate + save);
   tests INV-2/INV-6. Register tests in `neodos-kernel/src/testing.rs`.
6. **libneodos.** Mirror structs in `libneodos/src/syscall/types.rs`; add wrappers
   in `libneodos/src/syscall/time.rs` and re-exports.
7. **libtimezone (new crate).** TZif parser + conversions + `ZoneSource`; host
   tests INV-4/INV-5; `libtimezone/Cargo.toml` (no_std target, std tests).
8. **Zone data.** Add a curated `C:\System\Zoneinfo` set (at least
   `Europe/Madrid`) + `tzdata.version` to the image list in NeoDev.
9. **datetime.** Switch display to `libtimezone` + `i18n_format_date`/
   `i18n_format_time`; keep `/U` and `/S` semantics (`/S` still sets UTC).
10. **i18n region data.** Ship `[region]` blocks in
    `data/locale/{en-US,es-ES,ca-ES}/*.toml`; verify with `nltc`.
11. **neocfg Date/Time module.** Implement via `libneocfg` + the new wrappers
    (per `docs/design/neocfg-design.md`); persistence-truthful UI.
12. **Docs + validation.** Update `docs/kernel/objects.md`, `docs/kernel/syscalls.md`,
    `docs/registry/registry.md`; run `neodev test` and
    `npx markdownlint '**/*.md' --config .markdownlint.json`.

**Dependencies / ordering:** steps 1–6 are kernel+ABI and self-contained;
7–8 are userland; 9–11 are consumers; 12 is docs. Step 8 (zone data) can proceed
in parallel with 1–6. Per-user time zones and automatic tzdata download are
explicit non-goals.

---

*End of design. No code, public API, or issue state is changed by this document.*
