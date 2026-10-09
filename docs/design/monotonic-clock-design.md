# Monotonic Clock / Uptime — Design Document

> **Version:** v0.1-draft
> **Status:** Design
> **Target Release:** v0.52+
> **Issue:** [#467](https://github.com/NeoDOS-Project/NeoDOS/issues/467)
> **Related:** [#283](https://github.com/NeoDOS-Project/NeoDOS/issues/283),
> [#356](https://github.com/NeoDOS-Project/NeoDOS/issues/356)
> **ABI Impact:** New `ObInfoClass` (46–47), new `ObSetInfoClass` (56)

---

## 1. Problem Analysis

### 1.1 Current Limitation

Ring 3 has only wall-clock time with **one-second resolution** and no way to
measure elapsed time:

- `ObInfoClass::DateTime=9` and `LocalDateTime=41` return RTC fields
  (`syscall/ob/query/time.rs`, `syscall/time.rs` exposes only
  `ob_set_datetime`).
- The kernel maintains a 1 kHz tick counter (`tick_rate_hz`, and
  `timer_tick_count` per-CPU in the KPRCB, exposed only inside the
  `CpuStats` snapshot class 24).
- There is no `Sleep(ms)`/wait-with-timeout on the syscall path other than the
  alertable `sys_sleep_ex` (RAX 3), which is not a duration sleep.

### 1.2 Why It Matters

A game loop needs `Sys_Milliseconds` to schedule tics and frames (Doom runs a
35 Hz game tic and a variable render rate). Any app needs timeouts, animation,
and latency measurement. This is one of the three functional Doom
prerequisites (`#467`).

### 1.3 Scope

Design a **monotonic uptime source** plus a **duration sleep**. Wall-clock
discipline (slew/drift, `#356`) is out of scope.

---

## 2. Design

### 2.1 Monotonic Time Source

A single global monotonic counter is advanced from the APIC timer handler at
the tick rate (1 kHz). To keep the read cheap and SMP-safe:

```rust
// kernel/src/time.rs (new)
static UPTIME_TICKS: AtomicU64 = AtomicU64::new(0); // incremented once per tick
pub fn tick() { UPTIME_TICKS.fetch_add(1, Ordering::Relaxed); }
pub fn uptime_ticks() -> u64 { UPTIME_TICKS.load(Ordering::Acquire) }
pub fn tick_hz() -> u32 { crate::timers::tick_hz() } // configured at boot
```

- The timer handler is the **only writer**, so a single `AtomicU64` is enough;
  no per-CPU accumulation is required for millisecond resolution.
- Millisecond value = `ticks * 1000 / tick_hz` computed in u64 to avoid
  truncation; cache `1000 / tick_hz` as a fixed-point multiplier when
  `tick_hz == 1000` (identity) for the hot path.
- The counter starts at 0 at boot (after timer init) and never goes
  backwards.

### 2.2 Optional High-Resolution Counter

For sub-millisecond timing, expose TSC:

```rust
#[repr(C)]
pub struct HighResTime {
    pub tsc: u64,
    pub tsc_khz: u32,
    pub _pad: u32,
}
```

The kernel already reads CPUID `tsc_khz` (`CpuInfoFull.tsc_khz`). `HighResTime`
is optional and documented as non-monotonic-safe across CPUs unless the caller
pins; v1 uses `Uptime` for all policy.

### 2.3 Duration Sleep

Add `ObSetInfoClass::SleepMs = 56` on `\System\Info` (a lightweight operation,
no object payload). Implementation:

1. If `ms == 0`, return immediately.
2. Compute a deadline `uptime_ticks() + ms * tick_hz / 1000`.
3. Block via KWait `WaitReason::Timer` (already present) and have the timer
   handler wake threads whose deadline passed.
4. Return 0.

This complements `#283` (timed waits for `ob_wait`/`poll`) and reuses the same
deadline mechanism.

### 2.4 API

#### ObInfoClass

| Value | Name | Payload |
|-------|------|---------|
| 46 | `Uptime` | out: `{ uptime_ms: u64, tick_hz: u32, _pad: u32 }` (16 bytes) |
| 47 | `HighResTime` | out: `HighResTime` (16 bytes) |

Both are read-only and safe for any process.

#### ObSetInfoClass

| Value | Name | Payload |
|-------|------|---------|
| 56 | `SleepMs` | `u32` milliseconds (little-endian) |

### 2.5 libneodos

Extend `libneodos/src/syscall/time.rs` and re-export:

```rust
pub fn ob_uptime_ms() -> Result<u64, i64>;       // ObInfoClass::Uptime
pub fn ob_high_res_time() -> Result<(u64, u32), i64>;
pub fn ob_sleep_ms(ms: u32) -> Result<(), i64>;  // ObSetInfoClass::SleepMs
```

`libneodos` may additionally maintain a cached `tick_hz` at first use so
clients can convert ticks without a second query.

---

## 3. Alternatives Considered

- **Use the RTC**: one-second resolution and non-monotonic (NTP steps, `#356`).
  Rejected.
- **Per-CPU tick accumulation read via `CpuStats`**: expensive (walks the
  KPRCB of every CPU) and not a stable ABI for a simple uptime read.
  Rejected.
- **TSC as the primary source**: fast but per-CPU and needs cross-CPU
  synchronization; kept as optional `HighResTime` only.
- **Busy-wait sleep**: burns a core and violates AP-8 spirit. Rejected;
  sleep blocks.

---

## 4. Affected Components

| Subsystem | Change | Impact |
|-----------|--------|--------|
| `time` (NEW `kernel/src/time.rs`) | `UPTIME_TICKS`, `tick()`, accessors | New, low |
| `timers` / `idt` timer handler | Call `time::tick()` | Low |
| `syscall/ob/query/time.rs` | `Uptime`, `HighResTime` | Low |
| `syscall/ob/set` | `SleepMs` | Low |
| `object/types.rs` | Classes 46–47, 56 | Low |
| KWait | Deadline wake on `Timer` | Low (reuses existing) |
| `libneodos/syscall/time.rs` | Wrappers | Low |

---

## 5. API Contract

| Operation | Returns / Errors |
|-----------|------------------|
| `Uptime` | 16 bytes; monotonic non-decreasing |
| `HighResTime` | 16 bytes; TSC + kHz |
| `SleepMs` | 0 after ≥ ms (or immediately for 0); `-NoSys` if unsupported |

`Uptime` MUST be non-decreasing across all CPUs and MUST NOT wrap in practice
(u64 ms).

---

## 6. Test Plan

| Test | Expected |
|------|----------|
| `time_uptime_monotonic` | Two reads separated by a tick satisfy `t2 >= t1` |
| `time_uptime_advances` | After `SleepMs(20)`, uptime increased ~20 ms |
| `time_sleep_zero_fast` | `SleepMs(0)` returns without blocking |
| `time_sleep_blocks` | Reader state is Blocked during `SleepMs(100)` |
| `time_highres_tsc_nonzero` | `HighResTime.tsc` advances; `tsc_khz` > 0 |
| `time_wrap_free` | Counter is u64; no 32-bit truncation |
| `time_smp_visible` | Uptime increments observed on all CPUs |

---

## 7. Files and Modules

### New

| Path | Description |
|------|-------------|
| `neodos-kernel/src/time.rs` | Monotonic counter + accessors |

### Modified

| Path | Change |
|------|--------|
| `neodos-kernel/src/boot/mod.rs` | `mod time;` |
| `neodos-kernel/src/arch/x64/idt/mod.rs` | Call `time::tick()` in the timer handler |
| `neodos-kernel/src/syscall/ob/query/time.rs` | `Uptime`, `HighResTime` |
| `neodos-kernel/src/syscall/ob/set/mod.rs` | `SleepMs` |
| `neodos-kernel/src/object/types.rs` | New classes |
| `libneodos/src/syscall/time.rs` | Wrappers |
| `docs/services/ntpd.md`, `docs/kernel/*` | Document uptime vs wall clock |

---

## 8. Implementation Plan

1. Add `time.rs` with `UPTIME_TICKS` and call `time::tick()` from the timer
   handler.
2. Add `Uptime` (46) and `HighResTime` (47) queries + tests.
3. Add `SleepMs` (56) using KWait deadlines; reuse the `#283` timeout path if
   present.
4. libneodos wrappers.
5. Docs + tests + markdownlint.

---

## 9. Open Questions

1. Should `SleepMs` live on `\System\Info` or be a per-thread operation? (It is
   thread-scoped in effect; the object is just the entry point.)
2. Should `Uptime` include a `suspend` discipline (stop counting during S3)?
   (v1: count continuously; revisit with Power Manager.)
3. Do we expose nanoseconds, or ms + TSC is enough? (Proposed: ms + TSC.)

---

## 10. Dependencies

- Timer/APIC handler (`arch/x64/idt`, `timers`).
- KWait wait engine (`WaitReason::Timer`).
- Object Manager info/set classes.
- CPUID (`tsc_khz`) for `HighResTime`.
- No VFS/scheduler/block dependencies (INV-1).
