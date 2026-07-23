# NeoDOS Service Manager — Design Document

> **Status:** Draft v1 — Pre-implementation design review
> **Target Kernel:** v0.51+
> **Filosofía:** NT-like Service Control Manager, Object Manager-centric

---

## 1. Research

### 1.1 What Exists

| Component | File | Status |
| ----------- | ------ | -------- |
| Registry `Services\NeoInit\AutoStartServices` | `userbin/neoinit/src/main.rs` | ✅ Reads semicolon-separated list, calls `spawn_detached()` |
| `spawn_detached()` in NeoInit | `userbin/neoinit/src/main.rs:56` | ✅ Spawns via `ob_create(Process)`, closes handle — no tracking |
| `\Registry\Machine\System\CurrentControlSet\Services\` | Kernel Phase 3.881 | ✅ Registry path exists |
| NEM driver lifecycle | `src/drivers/nem/` | ✅ 7-state machine, dependency resolution, certification pipeline |
| EPROCESS/KTHREAD | `src/scheduler/mod.rs` | ✅ Process/thread model, token, handle table |

### 1.2 Relevant Syscalls (Table from docs/syscalls.md)

| RAX | Name | Purpose |
| ----- | ------ | --------- |
| 60 | `ob_open` | Open Ob object by path |
| 61 | `ob_create` | Create Ob object (Process, Pipe, etc.) |
| 62 | `ob_query_info` | Query object properties |
| 63 | `ob_set_info` | Set object properties |
| 65 | `ob_wait` | Wait on objects (signaled states) |
| 66 | `ob_destroy` | Destroy object |

### 1.3 Existing ObType Values (docs/kernel/objects.md)

| Value | Type | Description |
| ------- | ------ | ------------- |
| 18 | Socket | Network socket |
| 19 | Session | User session (planned USR-P2a) |
| **20** | **(free)** | **Available** |

### 1.4 Existing ObInfoClass Values (last used: 23)

Classes 24, 25, 26, 27, 28 are planned for USR-P2b/P2c/P5b. Next free: **29**.

### 1.5 Existing ObSetInfoClass Values (last used: 27)

Classes 28, 29, 30, 31, 32 are planned for USR-P2b/P1e/P5b. Next free: **33**.

---

## 2. Problem Analysis

### 2.1 Current Limitations

1. **No service lifecycle management.** NeoInit's `spawn_detached()` is fire-and-forget. Once spawned, the service is untracked — no way to query its status, restart it, or stop it gracefully.

2. **No restart policy.** If a service process crashes (page fault, exit), NeoInit does not respawn it. The service remains dead until the next system boot.

3. **No dependency resolution.** Services cannot declare dependencies on other services or drivers. NeoInit starts services sequentially in an undefined order.

4. **No service identity.** Services are regular processes with no Ob object representing the "service" abstraction. There is no `\Registry\Machine\System\CurrentControlSet\Services\<Name>` that the kernel understands as a managed entity.

5. **No security boundary.** Any process can start/stop any service. No ACL checks on service operations.

6. **No status reporting.** A service's running state (Starting/Running/Stopping/Stopped/Failed) is not exposed anywhere. Users/administrators cannot ask "is dhcpd running?".

### 2.2 Why Existing Abstractions Cannot Solve This

- **NeoInit (PID 1)** is a user-mode process with no special authority to manage other processes beyond spawning and waiting. It has no kernel-level tracking of spawned children, no way to enforce restart policy, and no isolation from the shell.

- **NEM driver lifecycle** is kernel-managed with 7 states, but it is specific to kernel-mode drivers. Services are Ring 3 `.NXE` binaries — they cannot use the NEM certification pipeline, isolation slots, or ABI negotiation.

- **EPROCESS** tracks individual processes but has no concept of "this process represents a managed service". A service may have multiple processes (main + worker threads), but there's no grouping mechanism.

- **Registry** stores configuration strings (`AutoStartServices = "dhcpd;ntpd"`), but the kernel does not interpret them as service declarations. The Registry is a passive data store, not an active manager.

A new kernel subsystem — the **Service Manager (Sm)** — is needed to bridge this gap.

---

## 3. Solution Design

### 3.1 Architecture Overview

The Service Manager is a kernel subsystem (like Cm) that manages the lifecycle of Ring 3 service processes. It exposes services as Ob objects of type `ObType::Service` (20) in the namespace `\Service\`.

```text
┌─────────────────────────────────────────────────────────┐
│                  Service Manager (Sm)                    │
│  ┌─────────────┐  ┌──────────────┐  ┌────────────────┐  │
│  │ ServiceList  │  │ State Machine│  │ Restart Policy │  │
│  │ Vec<Service> │  │ 5 states     │  │ never/crash/   │  │
│  │              │  │ + transitions│  │ always         │  │
│  └──────┬───────┘  └──────┬───────┘  └───────┬────────┘  │
│         │                 │                  │           │
│  ┌──────┴───────┐  ┌──────┴───────┐  ┌──────┴────────┐  │
│  │Dependency    │  │Process       │  │Registry       │  │
│  │Resolver      │  │Tracker       │  │Backend        │  │
│  │(topo-sort)   │  │(PID→Service) │  │(Cm read/write)│  │
│  └──────────────┘  └──────────────┘  └───────────────┘  │
└─────────────────────────┬───────────────────────────────┘
                          │
              ┌───────────┴───────────┐
              │  Ob Namespace         │
              │  \Service\<Name>      │
              │  \Registry\Machine\   │
              │   System\Current-     │
              │   ControlSet\Services\│
              └───────────────────────┘
```

**Data flow:**

1. Boot: Sm reads Registry `Services\*\*` to discover configured services
2. Sm creates `ObType::Service` objects in `\Service\<Name>`
3. Sm starts auto-start services (dependency-sorted)
4. User/Admin sends control via `ob_set_info` on service handle
5. Sm monitors service processes via KWait (ChildExit)
6. On crash: Sm applies restart policy (respawn or mark Failed)

### 3.2 New Types/Structs/Enums

#### ServiceState

```rust
#[repr(u8)]
pub enum ServiceState {
    Stopped   = 0,  // Not running, ready to start
    Starting  = 1,  // Process spawned, waiting for init handshake
    Running   = 2,  // Process active (PID tracked)
    Stopping  = 3,  // Stop requested, process being terminated
    Failed    = 4,  // Process crashed and restart policy exhausted
}
```

#### ServiceStartType

```rust
#[repr(u8)]
pub enum ServiceStartType {
    Boot      = 0,  // Started during kernel Phase 3.85 (NEM drivers only)
    System    = 1,  // Started before user login (Phase 4, before shell)
    Auto      = 2,  // Started by Sm on boot after System services
    Demand    = 3,  // Started on request (manual)
    Disabled  = 4,  // Cannot be started
}
```

#### ServiceRestartPolicy

```rust
#[repr(u8)]
pub enum ServiceRestartPolicy {
    Never       = 0,  // Do not restart
    OnCrash     = 1,  // Restart only if exit code != 0
    Always      = 2,  // Restart regardless of exit code
}
```

#### Service (main struct)

```rust
pub struct Service {
    pub name: [u8; 64],            // Null-terminated, e.g. "Dhcpd"
    pub display_name: [u8; 128],   // Human-readable, e.g. "DHCP Client"
    pub binary_path: [u8; 256],    // Ob path, e.g. "\Global\FileSystem\C:\System\Tools\dhcpd.nxe"
    pub state: ServiceState,
    pub start_type: ServiceStartType,
    pub restart_policy: ServiceRestartPolicy,
    pub pid: u32,                  // 0 = not running
    pub obj_id: ObId,              // Ob object ID for this service
    pub exit_count: u32,           // Number of times the process has exited
    pub last_exit_code: i64,       // Exit code from last termination
    pub dependencies: [u8; 256],   // Semicolon-separated service names
    pub failure_count: u32,        // Consecutive crash count (reset on clean stop)
    pub max_failures: u32,         // Max consecutive crashes before state→Failed
    pub security: SecurityDescriptor, // Who can start/stop/query
}
```

#### ServiceManager (global singleton)

```rust
pub struct ServiceManager {
    pub services: Vec<Service>,     // All registered services
    pub dependency_graph: DependencyGraph,  // Topological dependency map
}
```

Global: `SERVICE_MANAGER: Mutex<ServiceManager>`

#### ObType::Service = 20

New variant in `ObType` enum:

```rust
pub enum ObType {
    // ... existing ...
    Session = 19,
    Service = 20,   // NEW
}
```

### 3.3 New ObInfoClass/ObSetInfoClass Variants

#### ObInfoClass (new variants)

| Class | Name | Description | Returns |
| ------- | ------ | ------------- | --------- |
| 29 | ServiceState | Query service state | `ServiceState` (1 byte) + PID (4 bytes) |
| 30 | ServiceConfig | Query service configuration | Binary: start_type(1) + restart_policy(1) + max_failures(4) + display_name(128) + binary_path(256) |
| 31 | ServiceStatus | Comprehensive status | Binary: state(1) + pid(4) + exit_count(4) + last_exit_code(8) + failure_count(4) + uptime_ticks(8) |

#### ObSetInfoClass (new variants)

| Class | Name | Description | Args |
| ------- | ------ | ------------- | ------ |
| 33 | ServiceStart | Start a Demand service | None |
| 34 | ServiceStop | Stop a running service gracefully | `timeout_ms` (u32, 0 = force kill) |
| 35 | ServiceRestart | Stop then restart | `timeout_ms` (u32) |
| 36 | ServiceSetConfig | Modify service configuration | Binary: start_type(1) + restart_policy(1) + max_failures(4) |

### 3.4 New Syscall: `sys_ob_service` (RAX = 77)

Since RAX 77 is the next available slot (currently `MAX_VALID = 76`), and the rule mandates `sys_ob_*` for new syscalls:

```rust
// RAX = 77
pub fn sys_ob_service(
    fd: u64,         // Handle to service Ob object (obtained via ob_open)
    control: u32,    // 0=START, 1=STOP, 2=RESTART, 3=QUERY_STATUS, 4=SET_CONFIG
    buf: u64,        // User-space buffer for input/output
    buf_len: u64,    // Size of buffer
) -> u64             // Bytes written or error code
```

**Why a dedicated syscall instead of only ob_set_info?** Service control operations (START/STOP/RESTART) involve cross-process process management (spawning, killing), which is semantically richer than a simple property set. The control code dispatch keeps the handler clean while still operating on Ob handles. `ob_set_info` with ServiceStart/Stop/Restart classes remains as the primary API; `sys_ob_service` is a convenience multiplexer.

**Implementation:** The handler validates the fd is an `ObType::Service`, checks security, then dispatches to `sm_start()`, `sm_stop()`, `sm_restart()`, `sm_query_status()`, or `sm_set_config()`.

### 3.5 New Files

| File | Purpose |
| ------ | --------- |
| `src/services/mod.rs` | Module root, `ServiceManager` struct, `init_service_manager()` |
| `src/services/manager.rs` | `ServiceManager` implementation: CRUD, dependency resolution, start/stop orchestration |
| `src/services/state.rs` | `ServiceState`, `ServiceStartType`, `ServiceRestartPolicy` enums, state machine transitions |
| `src/services/tracker.rs` | Process monitoring: KWait integration for ChildExit, restart policy enforcement |
| `src/services/registry_backend.rs` | Read/write service configuration from `\Registry\Machine\System\CurrentControlSet\Services\<Name>` |
| `src/services/dependency.rs` | `DependencyGraph` struct, topological sort, cycle detection |

### 3.6 Changes to Existing Files

| File | Change |
| ------ | -------- |
| `src/object/types.rs` | Add `Service = 20` to `ObType` enum. Add `ObInfoClass::ServiceState(29)`, `::ServiceConfig(30)`, `::ServiceStatus(31)`. Add `ObSetInfoClass::ServiceStart(33)`, `::ServiceStop(34)`, `::ServiceRestart(35)`, `::ServiceSetConfig(36)`. |
| `src/syscall/mod.rs` | Add `MAX_VALID = 77`. Add SSDT entry for `sys_ob_service` at RAX 77. Update `validate_abi()`. |
| `src/syscall/ob.rs` | Add `handler_ob_service()`. Add case arms in `handler_ob_query_info` for classes 29-31. Add case arms in `handler_ob_set_info` for classes 33-36. |
| `src/syscall/permission.rs` | Add `service_start/stop/set_config` permission entries. Service operations require admin token. |
| `src/globals.rs` | Add `SERVICE_MANAGER: Mutex<ServiceManager>` global. |
| `src/main.rs` | Call `init_service_manager()` in Phase 3.882 (after Registry init). Call `sm_start_auto_services()` in Phase 4 (after NeoInit spawn). |
| `userbin/neoinit/src/main.rs` | Remove `spawn_service()` / `AutoStartServices` logic. Delegate to kernel Service Manager. Add `sm_register_service()` call for each entry in Registry. |
| `libneodos/src/syscall.rs` | Add `SERVICE_CONTROL_START(0)`..`SET_CONFIG(4)` constants. Add `sys_ob_service()` wrapper. Add `ObInfoClass::ServiceState(29)` etc. Add `ObSetInfoClass::ServiceStart(33)` etc. |
| `docs/syscalls.md` | Add RAX 77 entry. Update MAX_VALID. |
| `docs/objects.md` | Add ObType::Service=20. Add namespace path `\Service\`. Add info classes 29-31 and set classes 33-36. |

### 3.7 Namespace Layout

```text
\Service\                       — new root directory (created at init)
├── \Service\Dhcpd              — ObType::Service object for DHCP client
├── \Service\Ntpd               — ObType::Service object for NTP client
├── \Service\Syslog             — ObType::Service object for system logger
└── \Service\<name>             — one object per registered service

\Registry\Machine\System\
  CurrentControlSet\Services\
  ├── \Services\Dhcpd           — Registry key with service config
  │   ├── DisplayName   (REG_SZ)   = "DHCP Client"
  │   ├── BinaryPath    (REG_SZ)   = "C:\System\Tools\dhcpd.nxe"
  │   ├── StartType     (REG_DWORD)= 2 (Auto)
  │   ├── RestartPolicy (REG_DWORD)= 1 (OnCrash)
  │   ├── MaxFailures   (REG_DWORD)= 3
  │   ├── Dependencies  (REG_SZ)   = "Network"
  │   └── ImagePath     (REG_SZ)   = "C:\System\Tools\dhcpd.nxe"
  ├── \Services\Ntpd
  │   ├── DisplayName            = "NTP Client"
  │   ├── BinaryPath             = "C:\System\Tools\ntpd.nxe"
  │   ├── StartType              = 3 (Demand)
  │   └── ...
  └── \Services\Syslog
      └── ...
```

### 3.8 State Machine

```text
                        ┌─────────────────────────────────────┐
                        │                                     │
                        v                                     │
  Stopped ──(start)──→ Starting ──(handshake)──→ Running      │
    ^                      │                       │          │
    │                      │                       │          │
    │                      │ (timeout)            (stop)      │
    │                      v                       v          │
    │                   Failed ←────────────── Stopping       │
    │                      │                       │          │
    │                      │ (restart_policy       │ (process  │
    │                      │  = Never)             │  exited)  │
    │                      v                       v          │
    │                   Stopped ───────────────────────────────┘
    │                      ▲
    │                      │ (restart_policy = Always/OnCrash
    │                      │  AND failure_count < max_failures)
    └──────────────────────┘
```

Valid transitions with conditions:

| From | To | Trigger | Condition |
| ------ | ---- | --------- | ----------- |
| Stopped | Starting | `sm_start()` | start_type != Disabled. Security check passes. |
| Starting | Running | Process PID confirmed alive | Process does not exit within 5 seconds |
| Starting | Failed | Process exits before handshake | Process exits immediately |
| Starting | Stopped | `sm_stop()` during startup | Cancel pending start |
| Running | Stopping | `sm_stop()` or `ob_set_info(ServiceStop)` | Security check passes |
| Running | Failed | Process crashes unexpectedly | Exit code != 0 and (restart_policy=Never or failure_count >= max_failures) |
| Running | Starting | Process crashes AND restart allowed | restart_policy=Always or (OnCrash and exit_code != 0) and failure_count < max_failures |
| Stopping | Stopped | Process exits cleanly | Any exit |
| Stopping | Failed | Process does not exit within timeout | `sm_stop()` timeout_ms elapsed |
| Failed | Stopped | `sm_reset()` internal | Manual admin action via `ob_destroy` + re-register |
| Failed | Starting | `sm_start()` (retry) | Admin forces restart of a Failed service |

### 3.9 Thread Safety

- `SERVICE_MANAGER` is `Mutex<ServiceManager>` — acquired for all mutation
- Reads via `ob_query_info` lock only to copy data out, then release
- Process exit monitoring via KWait (already thread-safe)
- Dependency resolution runs once at init (boot), then cached

---

## 4. Alternatives Considered

### Alternative A: User-Mode Service Manager (like `srvany` on Windows)

A user-mode `.NXE` binary (`sm.nxe`) that reads Registry and manages processes.

**Rejected because:**

- A user-mode process cannot enforce security on service operations — any process with admin token could kill services
- Restart policy enforcement on PID 1 crash: if sm.nxe crashes, all services become unmanaged
- Cannot integrate with the Ob namespace at the kernel level — services would be invisible to `ob_enum(\Service\)`
- Race conditions: the sm process managing other processes introduces TOCTOU between query and action
- The kernel already manages NEM driver lifecycle natively; Ring 3 services deserve the same treatment

### Alternative B: Extend NeoInit with Full Service Management

Make NeoInit a full supervisor: track children, implement restart policy, expose status via some IPC.

**Rejected because:**

- NeoInit is PID 1 — if it crashes during complex service management, the entire system goes down
- NeoInit has no special kernel authority — it's just a user process with admin token
- Service security (who can stop a service) would have to be implemented in user space, duplicating the kernel's SeAccessCheck
- No Ob namespace integration without kernel changes anyway
- Violates the principle "the kernel is small" — but in this case the kernel already has the primitives (Ob, KWait, process lifecycle); the Service Manager just connects them

### Alternative C: NEM Drivers for All Services (Ring 3 in isolation slots)

Convert all services to NEM drivers running in Ring 3 isolation slots.

**Rejected because:**

- NEM format is designed for kernel-adjacent code (drivers), not arbitrary user applications
- Services like dhcpd, ntpd, syslogd are standard Ring 3 processes — forcing them into NEM format adds unnecessary complexity (ABI negotiation, certification pipeline, 1 MB slot limit)
- NEM drivers have different lifecycle semantics (driver init/bind/activate) that don't map cleanly to service start/stop
- NEM drivers cannot use `libneodos` syscall wrappers directly

---

## 5. Affected Components

| Subsystem | Nature of Change |
| ----------- | ----------------- |
| **Object Manager** (`src/object/types.rs`) | Add `ObType::Service=20`, 3 new `ObInfoClass` variants, 4 new `ObSetInfoClass` variants |
| **Syscall dispatch** (`src/syscall/mod.rs`) | Add RAX 77 `sys_ob_service` to SSDT, update `MAX_VALID`, update `validate_abi()` |
| **Syscall handlers** (`src/syscall/ob.rs`) | New `handler_ob_service()`, extend `handler_ob_query_info` and `handler_ob_set_info` |
| **Syscall permissions** (`src/syscall/permission.rs`) | Admin-only flag for service operations |
| **Globals** (`src/globals.rs`) | New `SERVICE_MANAGER` global |
| **Boot** (`src/main.rs`) | Phase 3.882 init, Phase 4 auto-start |
| **Scheduler** (no change, but service code calls `sm_spawn_process()` → `sys_ob_create` internally) | Uses existing process creation |
| **Process Management** (`src/scheduler/mod.rs`) | Add `process_by_pid()` public helper (needed by Sm to get process handle/token) |
| **Registry (Cm)** (`src/cm/`) | Service backend reads/writes `Services\<Name>` keys. Already exists. |
| **KWait** (no change) | Sm monitors ChildExit on service process handles. Already supported. |
| **Security** (`src/security/`) | Service operations use `SeAccessCheck` against service Ob object's DACL. Already exists. |
| **NeoInit** (`userbin/neoinit/`) | Remove `spawn_detached()`/`spawn_service()` code. Replace with `sm_register_service()` call or Registry-based passive registration. |
| **libneodos** (`libneodos/src/syscall.rs`) | Add `sys_ob_service()` wrapper, new info/set class constants |
| **Documentation** (`docs/`) | Update `docs/syscalls.md`, `docs/objects.md`, `docs/boot.md` |

### Dependency check (invariant INV-1)

Service Manager depends ON: Ob types, Cm (Registry read/write), Scheduler (process_by_pid), Security (SeAccessCheck), KWait (process monitoring).

Service Manager is used BY: Syscall handlers, Boot (Phase 3.882/4).

No circular dependencies: Sm → Cm, Sm → Scheduler, Sm → Ob. None of those depend on Sm.

---

## 6. API Contract

### 6.1 `sm_init()` — Internal (called at boot Phase 3.882)

```rust
pub fn sm_init()
```

- **Args:** None
- **Returns:** Nothing (panics on OOM)
- **Preconditions:** Registry is initialized. Ob namespace root `\Registry\Machine\System\CurrentControlSet\Services\` exists.
- **Postconditions:** `SERVICE_MANAGER` is initialized. All Registry `Services\*` keys are parsed and `Service` objects created. `\Service\` directory exists in Ob namespace. `ObType::Service` objects registered.
- **Error handling:** If a Registry entry has invalid fields, skip that entry (log warning). Never panic from bad Registry data.

### 6.2 `sm_start_auto_services()` — Internal (called at boot Phase 4)

```rust
pub fn sm_start_auto_services()
```

- **Args:** None
- **Returns:** Nothing
- **Preconditions:** `sm_init()` completed. Scheduler is active.
- **Postconditions:** Services with `StartType::System` and `StartType::Auto` are spawned in dependency order. Each service's `state` is `Running` (or `Failed` if spawn failed).
- **Error handling:** Failed spawn → service state = Failed, continue to next service.

### 6.3 `sys_ob_service(RAX=77)` — Public syscall

```rust
pub fn handler_ob_service(fd: u64, control: u32, buf: u64, buf_len: u64) -> u64
```

**Args:**

- `fd`: Handle to service Ob object (obtained via `ob_open(\Service\<Name>)`)
- `control`: 0=START, 1=STOP, 2=RESTART, 3=QUERY_STATUS, 4=SET_CONFIG
- `buf`: User-space buffer pointer (for QUERY_STATUS output or SET_CONFIG input)
- `buf_len`: Buffer size in bytes

**Returns:**

- `>=0`: Bytes written to `buf` (QUERY_STATUS), or 0 (START/STOP/RESTART/SET_CONFIG success)
- `<0`: Error code

**Error codes:**

| Value | Name | Condition |
| ------- | ------ | ----------- |
| -1 | Inval | Invalid control code, or buf_len too small |
| -2 | NoEnt | fd does not refer to a Service object |
| -4 | Acces | Caller token does not have permission |
| -3 | NoMem | Allocation failure during start |
| -5 | BadF | Invalid fd |
| -7 | NoSys | Control code unknown |
| -15 | Busy | Service is in Starting/Stopping state, cannot accept command now |

**Preconditions:**

- `fd` must be a valid handle obtained via `ob_open(\Service\<Name>)` with appropriate access
- `ob_open`'s `desired_access` must include ACCESS_READ for QUERY_STATUS, ACCESS_WRITE for START/STOP/RESTART/SET_CONFIG
- For START: service start_type must not be Disabled
- For STOP: service state must be Running or Starting
- For RESTART: service state must be Running

**Security check:** `SeAccessCheck(caller_token, service.security, desired_access)` — admin token required for control operations, read access for status query.

### 6.4 `ob_query_info` — Extended for Service classes

#### Class 29 (ServiceState)

```rust
/// Returns [state: u8, pid: u32le, uptime_ticks: u64le]
/// buf_len must be >= 13
```

#### Class 30 (ServiceConfig)

```rust
/// Returns [start_type: u8, restart_policy: u8, max_failures: u32le,
///          display_name: [u8; 128], binary_path: [u8; 256]]
/// buf_len must be >= 394
```

#### Class 31 (ServiceStatus)

```rust
/// Returns [state: u8, pid: u32le, exit_count: u32le,
///          last_exit_code: i64le, failure_count: u32le, uptime_ticks: u64le]
/// buf_len must be >= 29
```

### 6.5 `ob_set_info` — Extended for Service classes

#### Class 33 (ServiceStart)

```rust
/// buf unused (can be null, len=0)
/// Transitions: Stopped→Starting→Running, Failed→Starting→Running
/// Returns 0 on success, -Busy if already running, -Inval if Disabled
```

#### Class 34 (ServiceStop)

```rust
/// buf points to u32 timeout_ms (0 = force kill via ProcessTerminate)
/// Transitions: Running→Stopping→Stopped, Starting→Stopped
/// Returns 0 on success, -Inval if already stopped
```

#### Class 35 (ServiceRestart)

```rust
/// buf points to u32 timeout_ms
/// Combines ServiceStop (with timeout) followed by ServiceStart
/// Returns 0 on success
```

#### Class 36 (ServiceSetConfig)

```rust
/// buf points to [start_type: u8, restart_policy: u8, max_failures: u32le]
/// buf_len must be >= 6
/// Only admin can set config. Returns -Inval on invalid values.
```

---

## 7. Test Plan

### 7.1 Unit Tests (in `src/services/state.rs`)

| Test | Description | Assertion |
| ------ | ------------- | ----------- |
| `sm_state_transition_valid` | Stopped→Starting→Running→Stopping→Stopped | All transitions succeed |
| `sm_state_transition_invalid` | Stopped→Stopping, Running→Starting (without restart) | Returns error |
| `sm_state_start_disabled` | Start a Disabled service | Returns -Inval |
| `sm_state_restart_from_failed` | Failed→Starting when restart policy allows | Succeeds |
| `sm_state_exhaust_failures` | failure_count >= max_failures → stays Failed | State stays Failed |

### 7.2 Dependency Resolution Tests (in `src/services/dependency.rs`)

| Test | Description | Assertion |
| ------ | ------------- | ----------- |
| `sm_dep_no_deps` | Service with empty deps | Starts immediately |
| `sm_dep_simple_chain` | A→B→C, start in order | C starts after B starts after A |
| `sm_dep_cycle_detected` | A→B→A | Cycle detection returns error |
| `sm_dep_missing_service` | A depends on NonExistent | Error reported, A not started |
| `sm_dep_fan_out` | C depends on A,B; A,B independent | A and B start in any order, then C |

### 7.3 Registry Backend Tests (in `src/services/registry_backend.rs`)

| Test | Description | Assertion |
| ------ | ------------- | ----------- |
| `sm_reg_read_service` | Read a service config from Registry | Fields match Registry values |
| `sm_reg_write_service` | Write a service config to Registry | Read back matches written |
| `sm_reg_enum_services` | Enumerate `Services\*` keys | Returns correct count |
| `sm_reg_missing_service` | Read non-existent service | Returns None |

### 7.4 Integration Tests (in `testing.rs`)

| Test | Description | Assertion |
| ------ | ------------- | ----------- |
| `sm_init_creates_namespace` | After `sm_init()`, `\Service\` exists in Ob namespace | `ob_open("\Service\")` succeeds |
| `sm_auto_start_services` | Services with StartType=Auto are running after boot | PID > 0, state = Running |
| `sm_start_demand_service` | Call `ob_set_info(fd, ServiceStart)` on Demand service | PID > 0, state = Running |
| `sm_stop_service` | Call `ob_set_info(fd, ServiceStop)` on Running service | state = Stopped, PID = 0 |
| `sm_restart_service` | Call `ob_set_info(fd, ServiceRestart)` on Running service | New PID, state = Running |
| `sm_query_status` | Call `ob_query_info(fd, ServiceStatus)` | Returns valid binary data |
| `sm_security_admin_required` | User token tries to stop service | Returns -Acces |
| `sm_security_admin_allowed` | Admin token stops service | Returns 0 |
| `sm_restart_on_crash` | Service process exits with code != 0, restart=OnCrash | New PID spawned |
| `sm_restart_always` | Service process exits with code 0, restart=Always | New PID spawned |
| `sm_restart_never` | Service process exits, restart=Never | state = Stopped, no respawn |
| `sm_restart_exhausted` | Service crashes max_failures times | state = Failed |
| `sm_dependency_ordering` | Service B depends on A; start B | A starts first, then B |
| `sm_set_config_syscall` | Change start_type via RAX 77 SET_CONFIG | Registry updated, state reflects change |
| `sm_init_skips_disabled` | Service with StartType=Disabled | Not started, state = Stopped |

### 7.5 User-Mode Tests (`userbin/neoshell/` or dedicated test binary)

| Test | Description | Assertion |
|------|-------------|-----------|
| `sm_neoshell_list_services` | `ob_enum(\Service\)` lists all services | Count matches Registry |
| `sm_neoshell_start_stop` | Open service, set_info(ServiceStart), wait, set_info(ServiceStop) | Process lifecycle works end-to-end |

---

## 8. Implementation Plan

### Step 1: Add enums and types

**Files:** `src/services/state.rs`, `src/object/types.rs`

- Create `ServiceState`, `ServiceStartType`, `ServiceRestartPolicy` enums
- Create `Service` struct
- Add `ObType::Service = 20`
- Add `ObInfoClass::ServiceState(29)`, `::ServiceConfig(30)`, `::ServiceStatus(31)`
- Add `ObSetInfoClass::ServiceStart(33)`, `::ServiceStop(34)`, `::ServiceRestart(35)`, `::ServiceSetConfig(36)`

### Step 2: Create Service Manager skeleton

**Files:** `src/services/mod.rs`, `src/services/manager.rs`, `src/globals.rs`

- Define `ServiceManager` struct with `services: Vec<Service>`
- Declare `SERVICE_MANAGER` global in `src/globals.rs`
- Implement `sm_init()`: creates empty manager, creates `\Service\` in Ob namespace
- Wire call to `sm_init()` in `src/main.rs` Phase 3.882

### Step 3: Implement Registry backend

**Files:** `src/services/registry_backend.rs`

- `sm_reg_load_all()`: enumerate `\Registry\Machine\System\CurrentControlSet\Services\`, parse each subkey into a `Service`
- `sm_reg_save(name, config)`: write service config to Registry
- `sm_reg_delete(name)`: remove service key from Registry
- Integrate Cm syscall wrappers (already exist, reuse)

### Step 4: Implement dependency resolution

**Files:** `src/services/dependency.rs`

- `DependencyGraph` struct with `edges: Vec<(usize, usize)>`
- `build_dependency_graph()`: parse each service's `dependencies` field
- `topological_sort()`: Kahn's algorithm, detect cycles
- `resolve_start_order(start_type: ServiceStartType) -> Vec<usize>`: returns indices in dependency order

### Step 5: Implement state machine

**Files:** `src/services/state.rs` (extend)

- `ServiceState::transition(target: ServiceState) -> Result<(), SmError>`
- `SmError` enum: `InvalidTransition`, `Disabled`, `AlreadyRunning`, `AlreadyStopped`, `Busy`, `SecurityDenied`

### Step 6: Implement process tracker

**Files:** `src/services/tracker.rs`

- `sm_spawn_service(service: &Service) -> Result<u32, SmError>`: creates process via internal `sys_ob_create`, stores PID
- `sm_monitor_service(service_idx: usize)`: registers KWait(ChildExit) on the process handle
- `sm_on_process_exit(service_idx: usize, exit_code: i64)`: called when KWait triggers; applies restart policy
- `sm_stop_service_process(service_idx: usize, timeout_ms: u32)`: sends ProcessTerminate, waits, force-kills on timeout

### Step 7: Implement syscall handler

**Files:** `src/syscall/ob.rs`, `src/syscall/mod.rs`, `src/syscall/permission.rs`

- Add `handler_ob_service()` for RAX 77
- Add arms in `handler_ob_query_info` for classes 29/30/31
- Add arms in `handler_ob_set_info` for classes 33/34/35/36
- Register in SSDT, update `MAX_VALID`, update `validate_abi()`
- Add permission entries (admin for control, read for status)

### Step 8: Wire auto-start at boot

**Files:** `src/main.rs`

- After Phase 3.881 (Registry init), add Phase 3.882: `sm_init()`
- In Phase 4 (before or after NeoInit spawn), call `sm_start_auto_services()`
- `sm_start_auto_services()` starts System services first (dependency-sorted), then Auto services

### Step 9: Update NeoInit

**Files:** `userbin/neoinit/src/main.rs`

- Remove `spawn_detached()`, `spawn_service()`, `services_str` parsing
- Keep `spawn_and_wait()` for the shell loop
- NeoInit no longer manages services — kernel Sm handles it

### Step 10: Add libneodos wrappers

**Files:** `libneodos/src/syscall.rs`

- Add `ObInfoClass::ServiceState`, `::ServiceConfig`, `::ServiceStatus`
- Add `ObSetInfoClass::ServiceStart`, `::ServiceStop`, `::ServiceRestart`, `::ServiceSetConfig`
- Add `sys_ob_service(fd, control, buf, len)` wrapper

### Step 11: Update documentation

**Files:** `docs/kernel/syscalls.md`, `docs/kernel/objects.md`, `docs/boot/boot-flow.md`

- Add RAX 77 to syscall table
- Add ObType::Service=20 to object types
- Add Phase 3.882 to boot phases
- Mark item as in-progress/completed

### Step 12: Write tests

**Files:** `src/services/state.rs`, `src/services/dependency.rs`, `src/services/registry_backend.rs`, `testing.rs`

- Implement all unit tests from §7
- Implement integration tests
- Run `cargo build` + `python3 scripts/auto_test.py` + `scripts/check_deps.py`

---

## 9. Registry Configuration Format

Each service is a key under `\Registry\Machine\System\CurrentControlSet\Services\<ServiceName>`:

| Value Name | Type | Default | Description |
| ------------ | ------ | --------- | ------------- |
| `DisplayName` | REG_SZ | `""` | Human-readable name |
| `BinaryPath` | REG_SZ | (required) | Ob path to the .NXE binary |
| `StartType` | REG_DWORD | 3 (Demand) | 0=Boot, 1=System, 2=Auto, 3=Demand, 4=Disabled |
| `RestartPolicy` | REG_DWORD | 0 (Never) | 0=Never, 1=OnCrash, 2=Always |
| `MaxFailures` | REG_DWORD | 3 | Max consecutive crashes before state→Failed |
| `Dependencies` | REG_SZ | `""` | Semicolon-separated service names required before this one |
| `ImagePath` | REG_SZ | (same as BinaryPath) | Legacy alias |
| `Description` | REG_SZ | `""` | Optional description for admin tools |

Default services provisioned at boot (Phase 3.882):

| Service | StartType | Binary Path |
|---------|-----------|-------------|
| Dhcpd | Auto (2) | `C:\System\Tools\dhcpd.nxe` |
| (future) Ntpd | Demand (3) | `C:\System\Tools\ntpd.nxe` |

---

## 10. Open Questions

1. **Should services be their own ObType or just ObType::Process with a service flag?** Decision: Dedicated ObType::Service because the lifecycle (Start/Stop/Restart) and state machine differ fundamentally from plain processes. A Service is a *managed* process.

2. **Should `sm_stop()` send SIGTERM equivalent first, then SIGKILL?** Decision: Yes. `ServiceStop` with `timeout_ms > 0` sends ProcessTerminate and waits. If `timeout_ms = 0`, it force-kills immediately. This matches NT's `WaitHint` pattern.

3. **Should the service binary itself know it's a service?** No — services are regular `.NXE` binaries. They don't need to link against any special library. The Service Manager tracks them externally.

4. **How does a service signal "I'm ready" (Starting→Running)?** For v1, the transition is automatic: if the process stays alive for ≥ 5 seconds after spawn, it's considered Running. A future v2 could add a named pipe or `ob_set_info(ServiceCheckpoint)` for the service to signal readiness explicitly.

5. **Should `sys_ob_service` be a separate syscall or should all operations go through `ob_set_info`/`ob_query_info`?** Both APIs exist. `ob_set_info`(ServiceStart) works for simple cases. `sys_ob_service` is a convenience multiplexer that reduces the number of syscalls for compound operations (e.g., RESTART = STOP + START atomically).
