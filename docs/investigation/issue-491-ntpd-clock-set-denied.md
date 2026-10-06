# #491 — `ntpd` cannot set the clock: stale RTC NEM driver in the image

**Branch:** `fix/491-ntpd-clock-set-denied` (from `develop` @ `9a0b0bc`).
**Issue:** #491. **Scope:** root cause + build-pipeline fix (NeoDev) + a kernel
regression test. No kernel/security/RTC **source** change.

---

## 1. Observed failure

On every boot the NTP service reaches the servers but cannot apply the time:

```text
[ntpd] querying pool.ntp.org (195.95.153.43)
[V3BRIDGE] et=32 found=true driver=7
[RTCSET] packed=0x1a0a06083004 acked=false
[CLKDENY] reason=rtc_write_unacked pid=3
[ntpd] clock set failed: clock set denied
[ntpd] all servers failed; backoff 20s
```

`ntpd::apply_time` maps **every** error from `ob_set_datetime()` to the generic
string `clock set denied`, so the real layer was unknown.

## 2. Reproduction

```bash
git checkout investigation/clock-ntpd-set-denied   # [CLKDENY]/[CLKSET]/[V3BRIDGE]/[RTCSET]
neodev build --quick --image
neodev run --headless --net user --serial /tmp/neodos-serial.txt
```

The instrumented branch (`0b8bb9c`) added `[CLKDENY] reason=...` / `[CLKSET] ok`
in `syscall/ob/set/time.rs`, `[V3BRIDGE]` in `drivers/nem/v3loader.rs` and
`[RTCSET]` in `drivers/rtc_bridge.rs`. The serial above is the actual output.

## 3. Root cause (confirmed)

The disk image packaged a **stale `rtc.nem`**. It was read from the gitignored
fallback `data/nem_bin/BOOT/rtc.nem` (mtime `2026-09-26 14:51`), which predates
commit `551a509` (#366, `2026-09-30 02:36`) that added the
`EVENT_RTC_WRITE`/`write_datetime` handling to `drivers/rtc/src/lib.rs`.

`neodev build --quick --image` does **not** run `build_nem_drivers()`
(`neodev/src/main.rs`, quick branch) and `image.rs::collect_files()` then falls
back to `data/nem_bin/<cat>/<name>.nem` (`neodev/src/image.rs`). The stale
driver:

1. is still bound to `EVENT_RTC_WRITE` (binding is by event type, done by the
   kernel), so `[V3BRIDGE] et=32 found=true` is printed;
2. does not handle event 32, so it never publishes the `EVENT_RTC_DATA`
   read-back;
3. therefore `rtc_bridge::set_datetime()` returns `false`;
4. the kernel maps that to `SyscallError::Io`;
5. `ntpd` masks it as `clock set denied`.

> `ntpd` fails because the packaged `rtc.nem` predates #366 and lacks
> `EVENT_RTC_WRITE` handling, so the RTC driver never publishes the read-back,
> causing `rtc_bridge::set_datetime()` to return `false` at the
> `ObSetInfoClass::DateTime` handler.

### Evidence that it is *not* a kernel/security/object bug

| Suspect | Result |
| --- | --- |
| `sys_ob_open(..., READ\|WRITE)` denied | **No.** Handler runs (`[CLKDENY]` is emitted from inside it). `\Global\Info\DateTime` is created with `security=None` (`main.rs:259`); `se_access_check(_, None, _) == true` (`security/access.rs:36-39`). |
| Ntpd token not admin | **No.** `is_current_admin()` passed; the denial is after it. |
| Wrong object / path | **No.** `obj.obj_type == Key && obj.native_id == 5` passed. |
| Invalid datetime | **No.** `[RTCSET] packed=0x...` shows a valid payload reached the bridge. |
| RTC bridge bug | **No.** `rtc_bridge` behaves correctly: no ACK ⇒ `false`. |

### Decisive differential

Only the NEM drivers were rebuilt; the kernel was untouched:

```bash
neodev build --nem --image
neodev run --headless --net user --serial /tmp/neodos-serial.txt
```

```text
[RTCSET] packed=0x1a0a06083816 acked=true
[CLKSET] ok 26-10-6 8:56:22 pid=3
[ntpd] synced: offset=753ms delay=0ms stratum=2
```

The same kernel with a current driver sets the clock. This isolates the defect
to the packaged driver, not to any NeoDOS source path.

## 4. Fix

The functional fix lives in the image pipeline (NeoDev #21): image generation
must build the NEM drivers before packaging them, so it never silently uses the
stale `data/nem_bin/**` fallback.

- `neodev build --quick --image` now runs `build_nem_drivers()` before
  `cmd_image` (quick branch).
- Partial builds that request an image (`--kernel --image`, …) also build NEM
  drivers unless `--nem` already did.
- Standalone `neodev image` (without `--no-build`) builds NEM drivers too.
- `--no-build` still means "use existing artifacts".

The NeoDOS side adds no kernel/security/RTC source change (the code is correct)
and adds one regression test (below).

## 5. Tests

- Kernel regression guard `ob_set_datetime_rtc_write_acks`
  (`syscall/ob/set/mod.rs`): when an RTC driver is bound to `EVENT_RTC_WRITE`,
  it reads the current time and writes it back, asserting the driver ACKs. A
  stale driver that binds but does not handle event 32 fails this test — the
  exact #491 condition. It is skipped when no RTC driver is bound.
- Existing `ob_set_datetime_accepts_valid` / `_rejects_invalid` keep covering
  range validation (valid/invalid datetime).

## 6. Validation

### QEMU

- `neodev build --quick --image` (fixed NeoDev): builds `rtc.nem`, image ready.
- QEMU SMP2, user-mode networking: `[RTCSET] acked=true`, `[CLKSET] ok`,
  `[ntpd] synced: offset=1135ms delay=999ms stratum=2`, no repeated
  `clock set denied`.
- Kernel suite (includes `ob_set_datetime_rtc_write_acks`) with the fixed image:

| Config | Kernel tests | Panic |
| --- | --- | --- |
| QEMU SMP1 | 826/826 | one post-test `Option::unwrap()` on `None` (known #490 residual, after NeoInit enters Ring 3) |
| QEMU SMP2 | 826/826 | none |
| QEMU SMP4 | 826/826 | none |

- `[READY_GUARD_STATS] rejected=0 ready_while_running=0 stale_rsp_dispatch=0
  stack_ownership_conflict=0` on every run.

### VirtualBox

- `neodev test --backend virtualbox` (SMP2): **826/826** kernel tests,
  `ob_set_datetime_rtc_write_acks` PASS, no panic.
- `neodev run --backend virtualbox --headless --net user` (SMP2, NAT):
  `[ntpd] synced: offset=1911ms delay=0ms stratum=1`, no `clock set denied`.

### Regression test is a real guard

With the pre-fix NeoDev and the gitignored stale `data/nem_bin/BOOT/rtc.nem`
(`write_cmos` absent), the generated image contains no `EVENT_RTC_WRITE`
handler in the driver and the suite reports:

```text
[FAIL] Kernel tests: 825 passed, 1 failed
  FAIL: ob_set_datetime_rtc_write_acks
[✗] OVERALL: FAILED
```

With the fixed NeoDev (fresh driver), the same test passes. This is the exact
`#491` condition, so the guard is validated in both directions.

## 7. Non-goals

- **#340 (`[SCHED_WARN] tid=5 pid=3 name=Ntpd state=RUNNING`)** — related
  observation, **not part of #491**. It does not affect the clock-set path
  (the syscall completes; the denial is in the RTC write ACK).

### Related observation — NeoInit is left suspended and the shell never starts

On `develop` (and identically with this branch's kernel), `NeoInit` is
activated but left `SUSP`, so it never executes (`NeoInit v…` and
`[neoinit] entering spawn loop…` are absent) and NeoShell never launches. The
boot emits the scheduler diagnostic:

```text
[USERMODE] activated TID=4
[SCHED_WARN] tag=timer TWO+ Running on cpu=0 tids=[0, 3, 5, …]/[0, 1, 0, …] sched.current=3 kprcb_tid=Some(5)
[SCHED_WARN]   tid=4 pid=2 name=neoinit state=SUSP cpu=0 wait=None
```

Evidence that this is **independent of #491** and pre-existing:

- It is present on the unmodified `develop` kernel (no #491 test) and on the
  original reproduction (`investigation/clock-ntpd-set-denied`) where the clock
  set **failed**.
- `ntpd` runs and syncs on the same boots, so it does not block the clock path.
- `ntpd`'s success/failure is uncorrelated with the shell outcome.

It is therefore **not** fixed here (scope: #340 / scheduler boot handoff). The
same class of observation is documented for #476/#482. See the follow-up issue
referenced in §8.

- `ntpd` masking all errors as `clock set denied` — a diagnostic limitation;
  the layer is now identified without changing the message contract.
- Clock discipline / slew (#356), timezone (#357), NTP auth (#361), NTP client
  feature work (#26).
- NE2 multi-leaf / other image-builder work.

## 8. Related issues

- #366 — introduced `EVENT_RTC_WRITE` (the feature the stale driver lacked).
- #26 — NTP client feature.
- #340 — ntpd/netd scheduling warning (explicitly excluded).
- #501 — NeoInit left `SUSP` at the boot handoff; NeoShell never starts
  (related observation above; pre-existing, not #491).
- NeoDOS-Project/NeoDev#21 — image pipeline: never package stale NEM drivers.
- NeoDOS-Project/NeoDev#5 — image manifest hardening (precedent).
- `docs/development/net-recovery-2026-09-26.md` §9.2 already documented the
  stale-gitignored-artifacts risk.
