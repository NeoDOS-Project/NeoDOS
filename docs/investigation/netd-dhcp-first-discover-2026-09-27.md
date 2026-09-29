# Investigation — e1000 RX init order, DD+len=0 descriptors, and DHCP first-DISCOVER loss (#340)

**Date:** 2026-09-29
**Branch:** `fix/339-e1000-dhcp-rx-race`
**Base commit:** `e0725dd` (#339 link-state fix)
**Environment:** VirtualBox 7.2.20, bridged `enp0s31f6`, NIC type 82540EM,
SMP1, `TICK_INTERVAL_US = 1000`.

This is the continuation of #340 focused on one question:

> Is the e1000 RX ring initialization order really the cause of the
> `DD=1, length=0` descriptors, and does that mechanism make a DHCP OFFER that
> is present on the wire fail to reach the guest?

---

## 1. Baseline

- `git status`: clean before instrumentation (only this document was added).
- Kernel build `RUSTUP_TOOLCHAIN=nightly cargo build --release`: OK.
- Kernel tests: 745/745 PASS.
- #339: driver already reports real `STATUS.LU` link state (unchanged).
- #338: independent; its invariant is not touched here.
- Previously refuted: `netd`/`sys_yield` RX starvation. `handler_yield` runs
  `network_poll_all()` synchronously, so RX is polled even when `netd` is not
  scheduled.

---

## 2. RX initialization order (`drivers/e1000/src/lib.rs`, base `e0725dd`)

Exact current sequence inside `init_e1000_hw()` (Variant A):

```text
t0: CTRL            = 0                         (reset)
t1: CTRL            = ctrl | CTRL_SLU
t2: RCTL            = RCTL_EN | RCTL_UPE | RCTL_MPE | RCTL_BAM | RCTL_SZ_2048 | RCTL_SECRC
t3: RDBAL           = rx_phys & 0xFFFFFFFF
t4: RDBAH           = rx_phys >> 32
t5: RDLEN           = 32 * 16 = 512
t6: RDH             = 0
t7: RDT             = 31
t8: descriptors     : desc[i].addr = buf_phys(i); desc[i].status = 0   (loop i..32)
```

**Defect (documented):** `RCTL.EN` is written at **t2**, *before* the ring base
(t3/t4), length (t5), head (t6), tail (t7) and descriptor buffers (t8). The
controller is therefore enabled while the RX ring is still un-programmed. This
is exactly the condition the Intel 8254x SDM and the Linux `e1000` driver avoid:
Linux has a dedicated `e1000_enable_rx()` called **after** the ring is
configured ("RX initialization sequence fixed — enable RX after corresponding
ring initialization only").

Values are real writes read back from the driver, not reset assumptions.

---

## 3. Descriptor semantics

`struct RxDesc` (`#[repr(C, packed)]`): `addr: u64, length: u16, checksum: u16,
status: u8, errors: u8, special: u16`.

From the 8254x SDM / OSDev and Linux/QEMU sources:

- `status.DD` (bit 0) is set by hardware when it is done with the descriptor.
- `status` is non-zero once the descriptor holds a received packet.
- A completed, in-range packet with no error always carries a **non-zero**
  `length`.
- A descriptor with `DD=1` and `length=0` and no other status bits is an
  **anomaly**: it is the "partial packet of size 0" state produced when the RX
  engine runs against a ring whose descriptors are not (yet) valid.
- Ownership: descriptors `(RDH .. RDT]` belong to hardware; the driver may only
  consume a descriptor after `DD` is set, then clears `status` and advances
  `RDT`.

The QEMU `e1000` bug trackers document this exact anomaly twice:
`[Bug 1402755] e1000 RX ring is filled with partial packets of size 0
(DD bit set, no other status, length 0)` when RX is started against an
un-filled ring, and the fix `e1000: set RX descriptor status in a separate
operation`. The recommended workaround matches Variant B: *"enable the RX
machine only after the descriptor ring is filled for the first time."*

So `DD=1,len=0` is **anomalous**, not a legitimate empty result.

---

## 4. DD+len=0 evidence (Variant A)

Bounded probe at the discard site records `status`, `errors` and the descriptor
index. In every Variant A boot the probe fires:

```text
Variant A (n=25 boots): rx_disc per boot = 320 .. 1248
  (status, errors) = (0x01, 0x00) for 100% of discards
```

The counters are stable across runs and absent when the ring is initialized
before RX is enabled (see §6). Frames are still received normally in the same
runs, so the device is not simply dead.

---

## 5. OFFER ↔ descriptor correlation

Two independent observations were collected per boot: a vNIC pcap (VirtualBox
`--nictrace1`) and the guest driver probe.

**What is proven (wire vs guest):**

- In Variant A, several boots show wire `Discover Offer Discover Offer …`: the
  DHCP server **did** answer DISCOVER #1, yet the guest retried. The guest-side
  `[rx_ok]`/`[rx_disc]` stream shows the OFFER-sized (331-byte) frame is **not**
  returned to the stack during the first-DISCOVER window.
- A buffer-signature scan at the discard site (searching the discarded
  descriptor's bound buffer for the BOOTP magic `63 82 53 63`) found **0**
  DHCP signatures in 5040 discards.

**Honest limitation:** the signature scan cannot close the identity loop,
because a `length=0` discard by definition was not written with payload, so the
discarded descriptor is not itself the OFFER's descriptor. The `DD=1,len=0`
descriptors are a **symptom of the ring/ownership desynchronization**, not
necessarily the OFFER's own slot. Therefore §5 provides:

```text
DEMONSTRATED: DD+len=0 descriptors exist (Variant A).
DEMONSTRATED: OFFER is on the pcap but not delivered to dhcpd (GUEST_MISS).
CORRELATION:  both happen in the same runs (Variant A).
CAUSALITY:    established by the controlled A/B in §6, not by per-frame identity.
```

The per-frame identity (which exact descriptor index carried the OFFER) is
**not** proven, and is not claimed.

---

## 6. A/B experiment

Purely an investigation toggle (`RX_INIT_RING_BEFORE_ENABLE`), never committed:

- **Variant A** = current tree: `RCTL.EN` first, ring programming after.
- **Variant B** = safe order: program `RDBAL/RDBAH/RDLEN/RDH/RDT` and all
  descriptor `addr`/`status`, **then** set `RCTL.EN` last.

No scheduler, `dhcpd`, `netd`, `sys_yield`, or timeout changes were made.

Results (SMP1, bridged, pcap + driver probe per boot):

| Variant | Boots | clean | GUEST_MISS | EXTERNAL_MISS | rx_disc/boot |
| ------- | ----: | ----: | ---------: | ------------: | -----------: |
| A (`RCTL.EN` first) | 25 | 9 | **6** | 6 | 320–1248 |
| B (ring first)       | 25 | 18 | **0** | 7 | **0** |

- Every Variant B boot has `rx_disc = 0` (25/25) while still receiving
  856–6965 valid frames (`[rx_ok]`), so Variant B does not "work by breaking RX".
- Every Variant A boot has 320–1248 `DD=1,len=0` discards.

**The intervention that changes the RX init order changes both the
`DD=1,len=0` condition and the GUEST_MISS class together** — this is the causal
link the task asked for. The independent EXTERNAL_MISS class is unchanged.

---

## 7. DHCP outcome comparison

| Variant | DHCP lease | retries | retry class |
| ------- | ---------- | ------- | ----------- |
| A | 25/25 | 12/25 | 6 GUEST_MISS + 6 EXTERNAL_MISS |
| B | 25/25 | 7/25 | 0 GUEST_MISS + 7 EXTERNAL_MISS |

Both variants always obtain a lease (the retry recovers). Variant B removes the
guest-side loss class entirely; the remaining retries are the independent
external miss.

---

## 8. External miss vs guest miss vs timeout

Kept strictly separate:

- **A — External miss**: wire `Discover Discover Offer …`; server never answered
  DISCOVER #1. Present in **both** variants (6/25 A, 7/25 B). Unchanged by the
  RX fix; external.
- **C — Guest RX loss**: wire `Discover Offer Discover Offer …`; server answered
  DISCOVER #1 but the guest retried. **6/25 in Variant A, 0/25 in Variant B.**
  This is the class the RX init order causes.
- **B — Client timeout**: `dhcpd`'s retry timeout is
  `TIMEOUT_ITERATIONS (200) × YIELD_BATCH (100)` yields ≈ 0.3–0.5 s wall-clock,
  while the server OFFER latency is ~0.3–1.2 s. A late-but-answered OFFER can
  therefore lose the race with the retry. This is a contributing factor, not the
  primary cause, and is not changed here.
- **D — e1000 descriptor anomaly**: `DD=1, length=0`; caused by the init order
  (§6), removed by Variant B.

C is only combined with D on the strength of the controlled A/B intervention,
which is stronger than the (inconclusive) per-frame signature scan.

---

## 9. First divergence

For the guest-loss class, the first divergence is **inside the guest RX path**:
the server OFFER is on the vNIC wire, but the driver's RX poll does not return
it to the stack while `DD=1,len=0` descriptors are being consumed. With the
ring-before-enable order the divergence disappears.

---

## 10. Root cause

```text
ROOT CAUSE: PROVEN
```

for the `DD=1,len=0` condition and the GUEST_MISS class:

```text
RCTL.EN set before the RX ring is programmed
        ↓
hardware runs against an un-programmed ring
        ↓
DD=1, length=0 descriptors (320–1248/boot)
        ↓
RX ownership desynchronization; an OFFER present on the wire is not delivered
        ↓
GUEST_MISS → DISCOVER retry
```

Causality is demonstrated by the controlled A/B: changing only the RX init
order removes the `DD=1,len=0` condition (25/25 → 0) and the GUEST_MISS class
(6/25 → 0) while leaving EXTERNAL_MISS unchanged.

**Not proven / out of scope:** the exact descriptor index that carried a given
OFFER (per-frame identity); and the independent EXTERNAL_MISS class (server-side),
which is unrelated to the init order.

---

## 11. Fix

```text
FIX: NONE
```

Only investigation was performed in the #340 campaign. No permanent code change
was made; the Variant B toggle and all probes were reverted.

**Fix implemented separately (#341):** Variant B was applied to
`drivers/e1000/src/lib.rs` — the RX ring is fully programmed before `RCTL.EN` is
set (commit implementing #341; see `CHANGELOG.md`). The #340 investigation above
is unchanged.

---

## 12. Recommended follow-up

Proposed issue: **`[NET][E1000] Initialize RX ring before enabling RCTL`**

- **Current sequence (Variant A):** `RCTL.EN` → `RDBAL/RDBAH` → `RDLEN` →
  `RDH` → `RDT` → descriptor `addr`/`status`.
- **Correct sequence (Variant B):** `RDBAL/RDBAH` → `RDLEN` → `RDH` → `RDT` →
  descriptor `addr`/`status` → `RCTL.EN` (last).
- **Evidence:** `DD=1,len=0` in 25/25 Variant A boots (320–1248); 0/25 in
  Variant B; RX still healthy in B.
- **Impact:** removes the GUEST_MISS class (server OFFER present on the wire,
  not delivered to `dhcpd`), eliminating the associated DHCP retries.
- **Relation:** #339 (link state) is orthogonal and already fixed; #340
  investigation is this document; the external miss and the `dhcpd` timeout are
  separate and remain.
- **Caveat:** implement carefully and re-validate with repeated DHCP/ping runs
  (SMP1/2/4) plus the kernel suite, since the `RDT`/`RDH` ordering interacts with
  a VirtualBox e1000 emulation quirk (`RDT == RDH` rewrite, VBox bugref 7346).

Recommended cleanup (separate, low risk): also express `dhcpd`'s retry timeout
in wall-clock/tick terms rather than `sys_yield` iterations.

---

## 13. Validation

```text
neodev test:   745/745 (kernel), after reverting all instrumentation
SMP1:          PASS
SMP2:          PASS (checkpoint)
SMP4:          PASS when userland boot completes (checkpoint)
DHCP:          PASS in all A/B boots (lease obtained; retry recovers)
RX:            healthy in both variants (frames received)
DD+len0:       Variant A 320–1248/boot; Variant B 0/boot
WORKTREE:      clean except this document (no probes, no A/B toggle)
```

---

## 14. Final conclusion

**Demonstrated:** starting the e1000 RX engine before the RX ring is programmed
produces `DD=1, length=0` descriptors (320–1248 per boot, 25/25 boots) and is
the cause of the GUEST_MISS class (server OFFER on the wire, never delivered to
`dhcpd`, 6/25 boots). Programming the ring before setting `RCTL.EN` removes both
the anomaly (0/25) and the class (0/25) while RX stays healthy.

**Still hypothesis / not proven:** the exact per-frame descriptor identity of a
lost OFFER (the `length=0` discard carries no payload, so a buffer-signature
match is impossible by construction), and the independent EXTERNAL_MISS class
(server never answers DISCOVER #1) plus the `dhcpd` iteration-counted timeout,
which are unaffected by the RX init order.
