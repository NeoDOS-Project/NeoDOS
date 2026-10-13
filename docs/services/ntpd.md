# NTP Daemon (`ntpd`) — Design Document

> **Status:** Implemented
> **NeoDOS version:** v0.51+
> **Related:** Issue #26 (NTP client), `docs/networking/userland.md`, `docs/registry/registry.md`

---

## 1. Purpose

`ntpd` is the persistent NTP/SNTP synchronization daemon for NeoDOS. It keeps
the system clock aligned with external time servers. It is a Ring 3 `.NXE`
service, managed by the kernel Service Manager exactly like `dhcpd` and
`netapplier`.

```text
                  ┌──────────────┐
                  │    neocfg    │
                  │ Control Panel│
                  └──────┬───────┘
                         │ writes config
                         ▼
                  ┌──────────────┐
                  │   Registry   │
                  │ Ntpd\Params  │
                  └──────┬───────┘
                         │ reads config / writes status
                         ▼
                    ┌─────────┐
                    │  ntpd   │  (Ring 3 service)
                    └────┬────┘
                         │ ob_set_datetime()
                         ▼
                   System Clock (RTC)
```

Separation of responsibilities is strict:

| Component | Responsibility |
| ----------- | ---------------- |
| `neocfg` / `netcfg` | Write configuration. |
| Registry | Store configuration (`Ntpd\Parameters`) and published status (`Ntpd\Status`). |
| `ntpd` | Read configuration, query NTP, apply time, publish status. Never configures. |
| Kernel clock API | `ObSetInfoClass::DateTime` → `rtc_bridge` → `rtc.nem` → CMOS. |

## 2. Layout

| Component | Path |
| ----------- | ------ |
| Daemon binary | `userbin/ntpd/` → `C:\System\Tools\ntpd.nxe` |
| Protocol library | `libntp/` (no_std, host-testable) |
| Service key | `\Registry\Machine\System\CurrentControlSet\Services\Ntpd` |
| Configuration | `...\Services\Ntpd\Parameters` |
| Status | `...\Services\Ntpd\Status` |
| Clock object | `\Global\Info\DateTime` |

The NTP/SNTP wire format, offset/delay math and civil-time conversion live in
`libntp` so they can be unit tested on the host, mirroring `libdns`. The daemon
supplies transport (UDP sockets via `net.nxl`), Registry access and the
clock-setting call.

## 3. Configuration

`ntpd` reads `\Registry\Machine\System\CurrentControlSet\Services\Ntpd\Parameters`:

| Value | Type | Default | Meaning |
| ------- | ------ | --------- | --------- |
| `Enabled` | REG_DWORD | `1` | Master switch. `0` makes `ntpd` publish `Disabled` and idle. |
| `Servers` | REG_SZ | `pool.ntp.org` | `;`-separated list of hostnames or dotted IPv4 addresses, tried in order. |
| `Interval` | REG_DWORD | `3600` | Seconds between successful synchronizations (minimum 16). |
| `Timeout` | REG_DWORD | `3000` | Per-server response timeout in milliseconds. |
| `KeyId` | REG_DWORD | `0` | RFC 5905 symmetric-key identifier (used with `Key`). |
| `Key` | REG_SZ | *(empty)* | Pre-shared key in hex (e.g. `0x0102…` or `01:02:…`). Non-empty enables authentication. |
| `Mac` | REG_SZ | *(unset = md5)* | MAC algorithm. `off` disables authentication even if `Key` is set. |

### Authentication (RFC 5905 symmetric-key MAC, #361)

When `Key` is configured (and `Mac` is not `off`), `ntpd`:

- appends a **key id + MD5 MAC** to each request
  (`libntp::build_request_authed` and `sign_request_md5`), and
- **rejects any reply without a matching authenticator** before parsing the
  header (`libntp::parse_reply_authed`): a plain 48-byte reply is rejected as
  unauthenticated, a wrong key id as a mismatch, and a bad MAC as a failure.

The MAC is `MD5( key || packet-through-key-id )`, matching the ntpd convention.
MD5 is the RFC 5905 default and is **not** collision-resistant; **NTS (RFC 8915)
is not implemented** (there is no TLS stack in the userland network stack).
Authentication is off by default (no `Key`), so an unconfigured system behaves
exactly as before.

The Registry is the **single source of truth**. `ntpd` never writes
configuration; a future `neocfg` NTP module (see ADM-5 #30 / TOOL-NEOCFG #98)
edits these values.

If the `Parameters` key is absent, `ntpd` falls back to the built-in defaults
above and keeps running, so a missing hive entry never disables time sync
silently.

## 4. Service lifecycle

`ntpd` follows the standard Service Manager model (`docs/design/service-manager-design.md`):

```text
Stopped → Starting → Running → Stopping → Stopped
                         │
                         └──► Failed (restart policy OnCrash, max 3)
```

- The default hive declares `Ntpd` with `StartType=Auto` and
  `RestartPolicy=OnCrash`, so it starts at boot and is respawned on crash.
- Process identity is the service name (`Ntpd`); see
  `spawn_usermode(..., name, ...)` in the Service Manager.
- Shutdown is initiated by the Service Manager (`ServiceStop`). Since #358 this
  is a **graceful** shutdown: the service is notified (user APC + the
  `ProcessShutdownState` flag), and `ntpd` observes it during its alertable
  waits, publishes `State=Stopped`, and exits voluntarily. If it does not exit
  within the bounded timeout, the Service Manager force-terminates it (see
  `docs/design/service-manager-design.md` §3.10).

### Failure handling

`ntpd` must never block boot or crash the system:

| Condition | Behavior |
| ----------- | ---------- |
| No NIC / no IP (link not required) | Publish `WaitingNetwork`, wait, retry with backoff. |
| DNS failure | Try next server; publish `Error` with the DNS reason. |
| Server timeout | Try next server; publish `Error`. |
| Invalid/NTP-KoD reply | Rejected by `libntp`; try next server. |
| Clock set denied | Publish `Error`; retry next cycle. |
| All servers fail | Exponential backoff 5 s → 10 s → … → 300 s. |

On success the backoff resets and the next sync is scheduled `Interval`
seconds later.

## 5. Status

`ntpd` publishes diagnostics under `...\Services\Ntpd\Status` (created on first
use):

| Value | Type | Meaning |
| ------- | ------ | --------- |
| `State` | REG_SZ | `Synced`, `Error`, `WaitingNetwork`, `Disabled`. |
| `Server` | REG_SZ | Configured server name last used. |
| `OffsetMs` | REG_DWORD (i32) | Last measured clock offset, milliseconds. |
| `DelayMs` | REG_DWORD | Last measured round-trip delay, milliseconds. |
| `Stratum` | REG_DWORD | Stratum of the last reply. |
| `LastSync` | REG_DWORD | Unix time of the last successful step. |
| `LastError` | REG_SZ | Last error string (empty on success). |
| `SyncCount` | REG_DWORD | Successful syncs since start. |

Service lifecycle state (Running/Failed/PID) is exposed separately by the
kernel via `ObInfoClass::ServiceStatus` on the `\Service\Ntpd` object.

## 6. Clock integration

`ntpd` applies the corrected time through the existing Object Manager clock
object:

```text
ntpd → libneodos::ob_set_datetime()
     → sys_ob_set_info(fd, ObSetInfoClass::DateTime, SysDateTime)
     → handler_ob_set_info (admin-only, validates fields)
     → rtc_bridge::set_datetime()
     → EVENT_RTC_WRITE → rtc.nem → CMOS
```

This is a **direct step** (equivalent to `ntpdate`). The clock API is
admin-only, matching `SetHostname`.

## 7. Known limitations

| Limitation | Tracking |
| ----------- | ---------- |
| No clock discipline: no slew, no drift compensation, no step threshold. Each sync is a hard step. | New Issue: clock discipline |
| Timezone/DST: NTP time is UTC and is written to the RTC directly; no local-time offset is applied. | New Issue: timezone support |
| RTC resolution is 1 second, bounding offset accuracy; `libntp` still computes sub-second offsets. | This document |
| No timed sleep in userland; periodic waits use an RDTSC budget. | #283, #307 |
| Symmetric-key MD5 MAC supported (RFC 5905, #361); **NTS (RFC 8915) is not** — no TLS in the userland stack. MD5 is not collision-resistant. | #361 (MAC done), NTS out of scope |
| Service shutdown is now graceful (notification + bounded timeout). | #358 (resolved) |

## 7.1 Build/packaging dependency

The NeoDOS image contents are currently defined by a hardcoded list inside
NeoDev (`src/image.rs::collect_files`). Adding `ntpd` required adding it to that
list; otherwise the built `ntpd.nxe` is silently omitted from the image. The
permanent fix (auto-discovery) is tracked in NeoDOS-Project/NeoDev#5.

## 8. Testing

- `libntp` unit tests (host): request encoding, reply validation, offset/delay
  math, NTP↔Unix conversion, civil-time round trips, config parsing, **MD5
  (RFC 1321) vectors and MAC authentication** (accept a valid MAC; reject
  missing/key-id-mismatch/bad-MAC/tampered replies).
- Kernel tests: `ObSetInfoClass::DateTime` field validation.
- E2E (VirtualBox/QEMU): see `docs/development/testing.md`.
