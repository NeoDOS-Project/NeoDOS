# Dead-code triage (2026-11)

> **Snapshot:** `develop` after PRs #802, #803 and the APIC/console batch.
> **Tool:** `cargo build --message-format=json`, filtering `dead_code` diagnostics.

## Result

The compiler `dead_code` inventory went from **329 to 226** warnings.

| Batch | Contents |
| ------- | -------- |
| Namespace/URN | `handler_waitpid`, `se_access_check_sid`, `urn_read/write/seek`, dead namespace wrappers, auto-registration helpers |
| Reserved API | MSR catalog and event-bus ABI constants documented as intentional |
| APIC | Unused MSR/register constants and internal helpers left over after the IPI consolidation (#508) |
| Console/drivers | ANSI test getters and the unused `DeviceEvent` registry |
| Privileges/exceptions | Token privilege constants (#56) and exception vectors marked as reserved ABI |

## Kept intentionally

| Surface | Why it stays |
| --------- | ------------ |
| `hal/safe/msr.rs` | Typed safe MSR catalog; entries are added before their first caller |
| `eventbus` event/source/flag constants | ABI values consumed by NEM drivers |
| `SE_*_PRIVILEGE` | Reserved for privilege enforcement (#56) |
| `EXCEPTION_*` | Architectural x86 exception vectors |
| Isolation validation helpers | Security hooks that need wiring, not deletion |

## Remaining clusters

| File | Warnings | Proposed action |
| ------ | -------- | --------------- |
| `drivers/nem/management/isolation.rs` | 11 | Wire the page-fault/permission hooks during the isolation review, or delete the unused validators |
| `arch/x64/cpu_local.rs` | 9 | Audit per-CPU accessors against the scheduler macros; keep the ones used by diagnostics |
| `net/tcp.rs` | 8 | Decide whether the unused handshake helpers belong to the receive path or should go |
| `net/nic.rs` | 8 | Wire the mask/gateway/MAC getters into `ipconfig`/query paths |
| `drivers/nem/management/hotreload.rs` | 7 | Consumed by the driver admin CLI (#416, #417) |
| `interrupts/ioapic.rs` | 7 | Check the routing helpers against the MSI work (#676) |
| `net/types.rs` | 7 | Protocol constants and the unused `IpProtocol` enum |
| `drivers/nem/runtime/mod.rs` | 6 | Public driver-management API; wire through the admin CLI |
| `memory/mod.rs` | 6 | Diagnostic accessors; keep or remove per caller audit |
| `arch/x64/smp.rs` | 5 | Diagnostic helpers |

## Policy

1. New code must not increase the `dead_code` count.
2. An API added ahead of its caller needs a doc comment, an owning issue and a
   site-local `#[allow(dead_code)]` with the reason.
3. Security/diagnostic hooks are wired, not deleted, unless the owning feature is
   explicitly dropped.
