# Raw Keyboard Event API — Design Document

> **Version:** v0.1-draft
> **Status:** Design
> **Target Release:** v0.52+
> **Issue:** [#466](https://github.com/NeoDOS-Project/NeoDOS/issues/466)
> **Related:** [#391](https://github.com/NeoDOS-Project/NeoDOS/issues/391),
> [#465](https://github.com/NeoDOS-Project/NeoDOS/issues/465)
> **ABI Impact:** New `ObInfoClass` (44–45), new `ObSetInfoClass` (55), new
> KWait `WaitReason` variant

---

## 1. Problem Analysis

### 1.1 Current Limitation

Ring 3 cannot see raw key events. `sys_read(fd=0)` returns **cooked UTF-8
bytes** from the calling process's VT queue: NeoKBD translates a scancode
through the active layout, composes dead keys, and pushes the resulting code
point. There is no `key down` / `key up`, no physical scancode, and no way to
distinguish the left and right Shift or the numeric keypad from arrows.

`libneodos/src/keyboard.rs` only manages state: layout, caps, repeat, LEDs, and
modifiers via `ObInfoClass::KeyboardInfo=35`, `KeyboardCaps=36`,
`KeyboardLayouts=37` and `ObSetInfoClass::KeyboardSet*=43-47`.

### 1.2 Existing Pipeline

The kernel already produces everything needed — it just does not expose it:

- PS/2 IRQ1 → `kbd/event.rs` (lock-free SPSC scancode queue) →
  `kbd::process_scancode` (`kbd/mod.rs:166`).
- NeoKBD emits Event Bus events per source-of-truth §11.2.3:
  `EVENT_KEYDOWN=27`, `EVENT_KEYUP=28`, `EVENT_KEY_CHAR=29`,
  `EVENT_KBD_MODIFIER=30`.
- The cooked byte is pushed into the active VT's `VtInputQueue`
  (`input/manager.rs`, `VtInputQueue` in `input/vt.rs`).

There is no per-process event ring and no info class that reads it.

### 1.3 Why It Matters

Games (Doom needs arrows, Ctrl, Esc, Enter, Space with press/release), TUI
frameworks, and any app with configurable keybindings need raw events. This is
a Doom prerequisite (`#466`), and it depends on extended keys being correct
(`#391` reports arrows/Home/End/Del are broken today).

### 1.4 Scope

Design the **raw event channel + API**. Do not redesign NeoKBD layouts; do fix
the E0 extended-scancode handling as a prerequisite.

---

## 2. Design

### 2.1 KeyEvent ABI

```rust
#[repr(C)]
pub struct KeyEvent {
    pub scancode: u16,   // raw set-1 scancode (E0-prefixed keys normalized)
    pub vk: u16,         // NeoDOS virtual key (arrows, F-keys, ...)
    pub codepoint: u32,  // layout translation, 0 if none (e.g. key up)
    pub modifiers: u8,   // KBD_SHIFT/CTRL/ALT/ALTGR/CAPS/NUMLOCK/SCROLLLOCK
    pub pressed: u8,     // 1 = make, 0 = break
    pub flags: u8,       // bit0 extended (E0), bit1 repeat
    pub _pad: u8,
}
```

16 bytes. `scancode` is preserved for clients that want physical keys;
`codepoint` allows text entry without the cooked path. `vk` is a stable
enumeration defined once (arrows, edit keys, F1–F12, keypad) so clients do not
hard-code scancodes.

### 2.2 Per-VT Raw Mode

Raw events are a **per-VT mode**, not a global one, so the shell's cooked
behavior is preserved on other VTs.

```text
VT raw=False (default):  NeoKBD -> cooked codepoint -> VtInputQueue -> sys_read
VT raw=True:             NeoKBD -> KeyEvent ring (only); no cooked bytes
```

- Enabling raw on a VT suppresses cooked byte delivery for that VT. Otherwise
  a client could consume the same physical key twice (once raw, once on
  stdin).
- The raw ring is owned by the **active VT's foreground/client process** and
  is drained by that process. This matches the existing foreground-PID model
  (`input/manager.rs::foreground_pid`).
- Switching VTs does not disable raw mode; it merely stops the client from
  being scheduled to drain (VT gating at read time).

### 2.3 Raw Event Ring

Bounded, lock-free, single-consumer:

```rust
pub struct KeyEventRing {
    buf: [KeyEvent; 256],   // pre-allocated, no IRQ alloc (INV-2)
    head: AtomicUsize,
    tail: AtomicUsize,
    dropped: AtomicU64,
}
```

- Producer: NeoKBD `process_scancode`, running in IRQ/DPC context. Never
  allocates; a full ring increments `dropped` and overwrites the oldest entry
  (a game would rather lose an old key than stall).
- Consumer: the foreground process via `KeyboardRead`/`KeyboardPoll`.
- One ring per VT (4 total), embedded in `InputManager`.

### 2.4 Blocking and Polling

- **Blocking**: `KeyboardRead` drains up to N events; if none, it blocks via
  KWait. Add `WaitReason::Keyboard(u32)` (VT id in the low bits), appended
  after the existing variants per source-of-truth Rule 12.1.4.
- **Polling**: `KeyboardPoll` returns the count immediately (0 if empty).
- **`sys_poll` (RAX 24)**: add keyboard fd readiness (POLLIN) so clients can
  multiplex keyboard + other fds. Implementation maps to `ring.has_data()`.

### 2.5 API

#### ObInfoClass

| Value | Name | Payload |
|-------|------|---------|
| 44 | `KeyboardRead` | in: `{ count: u32, timeout_ms: u32 }`; out: `count * KeyEvent` |
| 45 | `KeyboardPoll` | out: `KeyEvent` array; returns number written |

`KeyboardRead` with `timeout_ms == 0` blocks indefinitely; `timeout_ms > 0`
uses the KWait timer (see `#283`/`#467` for timed waits).

#### ObSetInfoClass

| Value | Name | Payload |
|-------|------|---------|
| 55 | `KeyboardSetRawMode` | 1 byte: 0 = cooked, 1 = raw (per caller's VT) |

`KeyboardSetRawMode` is per-VT and idempotent. It is automatically cleared
when the VT's foreground process exits (cleanup hook), so a crashed game does
not leave the shell deaf.

### 2.6 libneodos API

Extend `libneodos/src/keyboard.rs`:

```rust
#[repr(C)]
pub struct KeyEvent { /* as above */ }

pub const VK_UP: u16 = ...;
pub const VK_DOWN: u16 = ...;
// ... full VK table

pub fn kbd_set_raw(enabled: bool) -> Result<(), i64>;
pub fn kbd_read_event(timeout_ms: u32) -> Result<KeyEvent, i64>; // blocks when 0
pub fn kbd_poll_event() -> Result<Option<KeyEvent>, i64>;
```

### 2.7 Extended Keys (#391)

The E0 prefix is currently folded (`kbd/mod.rs:168` sets a pending flag) and
some keys collide with numpad. Before exposing `vk`, the E0 set must be split:
Left/Right Ctrl, Left/Right Alt, arrows, Home/End/Insert/Delete, PageUp/Down,
numpad `/`, keypad Enter. This is a prerequisite, not part of the API surface.

---

## 3. Alternatives Considered

- **Global raw mode**: simplest, but it breaks the shell on all VTs.
  Rejected.
- **Extend the cooked VT byte protocol with escape sequences** (ANSI CSI for
  key up): reuses stdin but conflates text with events and cannot express
  press/release cleanly. Rejected.
- **A new syscall** `sys_read_keys`: rejected per the architecture rule that
  new syscalls MUST be `sys_ob_*`; the KeyboardDevice Ob object already exists.
- **Deliver via Event Bus subscription to userland**: the Event Bus is
  kernel/driver oriented; a per-VT SPSC ring is simpler and faster.

---

## 4. Affected Components

| Subsystem | Change | Impact |
|-----------|--------|--------|
| `kbd/mod.rs`, `kbd/event.rs` | Emit `KeyEvent` into the active VT ring; fix E0 | Moderate |
| `input/vt.rs` | `KeyEventRing` type | Low |
| `input/manager.rs` | Per-VT ring + raw flag; wake readers | Moderate |
| `syscall/ob/query`, `ob/set` | KeyboardRead/Poll/SetRawMode | Moderate |
| `object/types.rs` | Info classes 44–45, set class 55 | Low |
| `syscall/poll` | Keyboard fd readiness | Low |
| `scheduler` / KWait | `WaitReason::Keyboard` | Low |
| `libneodos/keyboard.rs` | Events + raw API + VK table | Moderate |
| `docs/kernel/interrupts.md`, VT docs | Document raw mode | Low |

---

## 5. API Contract

| Operation | Preconditions | Returns / Errors |
|-----------|---------------|------------------|
| `KeyboardSetRawMode(1)` | fd = `\Device\Keyboard`, WRITE | 0; `-Perm` without WRITE |
| `KeyboardRead(count, 0)` | raw mode on for caller's VT | ≥1 events, blocking; `-Again` if raw off |
| `KeyboardRead(count, t)` | same | events or 0 on timeout |
| `KeyboardPoll` | any | 0..count events, non-blocking |
| `sys_poll` on kbd fd | fd open | POLLIN if ring non-empty |

Clearing raw mode, closing the fd, or foreground-process exit MUST leave the
VT in cooked mode.

---

## 6. Test Plan

| Test | Expected |
|------|----------|
| `kbd_ring_push_pop` | FIFO order preserved |
| `kbd_ring_overflow` | 257th push drops oldest, bumps `dropped` |
| `kbd_raw_mode_suppresses_cooked` | Raw on → `sys_read` sees no bytes |
| `kbd_raw_mode_per_vt` | Raw on VT1 does not affect VT0 |
| `kbd_read_blocks_wakes` | Reader blocked, IRQ event wakes it |
| `kbd_read_timeout` | No event → returns 0 after timeout |
| `kbd_poll_empty` | Returns 0, does not block |
| `kbd_raw_cleared_on_exit` | Foreground exit → cooked restored |
| `kbd_ext_e0_arrows` | Up/Down/Home/End produce distinct `vk` (#391) |
| `kbd_modifiers_in_event` | Shift/Ctrl/Alt bits correct on make and break |
| `kbd_poll_syscall` | `sys_poll` reports POLLIN when events pending |

---

## 7. Files and Modules

### Modified

| Path | Change |
|------|--------|
| `neodos-kernel/src/kbd/mod.rs` | Emit `KeyEvent`, E0 handling |
| `neodos-kernel/src/kbd/event.rs` | Route events to the active VT ring |
| `neodos-kernel/src/input/vt.rs` | `KeyEventRing`, `KeyEvent` |
| `neodos-kernel/src/input/manager.rs` | Per-VT ring + raw flag + wake |
| `neodos-kernel/src/object/types.rs` | New info/set classes |
| `neodos-kernel/src/syscall/ob/query/mod.rs` | KeyboardRead/Poll |
| `neodos-kernel/src/syscall/ob/set/mod.rs` | KeyboardSetRawMode |
| `neodos-kernel/src/syscall/handlers.rs` | `sys_poll` keyboard readiness |
| `neodos-kernel/src/scheduler/*` | `WaitReason::Keyboard` |
| `libneodos/src/keyboard.rs` | KeyEvent + raw API + VK table |
| `docs/kernel/interrupts.md`, `docs/userland/shell.md` | Document raw mode |

---

## 8. Implementation Plan

1. Fix E0 extended scancodes and add the `vk` mapping (#391).
2. Define `KeyEvent` + `KeyEventRing`; embed per-VT rings in `InputManager`.
3. Route NeoKBD events to the active VT's ring.
4. Add `KeyboardPoll` (non-blocking) and tests.
5. Add `KeyboardRead` with KWait blocking + timeout.
6. Add `KeyboardSetRawMode` + cooked suppression + exit cleanup.
7. Wire `sys_poll`.
8. libneodos API + VK constants.
9. Docs + `neodev test` + markdownlint.

---

## 9. Open Questions

1. Should raw mode be per-process or per-VT? (Proposed: per-VT.)
2. `vk` table source of truth — kernel-owned constant shared with libneodos via
   ABI? (Proposed: yes, frozen like other ABI enums.)
3. Should a raw-mode client still receive `EVENT_KEY_CHAR` convenience events,
   or is `codepoint` in `KeyEvent` enough? (Proposed: enough.)
4. Interaction with the Ctrl+C handler (`kbd/mod.rs:247`): in raw mode, Ctrl+C
   must be delivered as an event, not converted to a signal.

---

## 10. Dependencies

- `kbd` / NeoKBD (`kbd/mod.rs`, `kbd/event.rs`).
- `input` (VT manager).
- KWait wait engine.
- Object Manager (info/set classes).
- No dependency on VFS/scheduler/block drivers (INV-1).
