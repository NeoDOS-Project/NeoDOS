# Object Manager

The Object Manager (`Ob`) is the central abstraction for syscalls, handles,
security, and namespace. It unifies what were previously separate subsystems
(handles, KOBJ, URN, security) into a single kernel object graph. Every
kernel-visible resource is an ObObject, accessible via path in the Ob
namespace or by handle in a process's handle table.

## Core Data Structures

### ObObjectTable

```rust
pub struct ObObjectTable {
    slots: Vec<Option<ObObject>>,
    count: usize,
    next_id: ObId,
}
```

The global object table is protected by `Mutex<ObObjectTable>`. It stores all
live kernel objects. Objects are assigned an `ObId` (u64) on creation and
looked up by ID.

### ObObject

```rust
pub struct ObObject {
    pub id: ObId,
    pub obj_type: ObType,
    pub name: [u8; OB_NAME_LEN],   // 128 bytes
    pub refcount: u32,
    pub flags: u32,
    pub native_id: u64,
    pub ops: Option<&'static dyn ObOperations>,
}
```

- `id`: unique 64-bit identifier, monotonically increasing from 1
- `obj_type`: discriminates object semantics (Process, Pipe, Event, etc.)
- `name`: 128-byte zero-terminated string, human-readable
- `refcount`: number of outstanding references (handles + namespace entries)
- `native_id`: opaque type-specific data (inode for files, PID for processes)
- `flags`: type-specific flags (e.g., drive index for file handles)
- `ops`: optional vtable for polymorphic behavior

### ObOperations Trait

```rust
pub trait ObOperations: Send + Sync {
    fn on_destroy(&self, _id: ObId, _native_id: u64) {}
}
```

Currently provides `on_destroy()` called when the object is removed. This
enables type-specific cleanup (e.g., pipe buffer teardown, semaphore wake).
The `FileHandleOps` struct is a no-op implementation used by VFS handles.

### HandleEntry (Per-Process)

```rust
pub struct HandleEntry {
    pub object_id: ObId,
    pub offset: u64,       // per-handle file offset
}
```

Special sentinel IDs: `HANDLE_CLOSED=0`, `HANDLE_STDIN=ObId::MAX`,
`HANDLE_STDOUT=ObId::MAX-1`, `HANDLE_STDERR=ObId::MAX-2`.

Each process's handle table (`HandleTable`) is a `Vec<HandleEntry>` with fds
0=stdin, 1=stdout, 2=stderr pre-allocated. `alloc_handle()` finds the first
free slot >= 3. The `ob_object()` variant also stores `desired_access` in
offset for SeAccessCheck re-verification.

## ObType Enum

| Value | Type | Description |
| ------- | ------ | ------------- |
| 0 | Unknown | Uninitialized/error |
| 1 | Process | Process object (waitable on ChildExit) |
| 2 | Driver | NEM driver object |
| 3 | Device | Hardware device object |
| 4 | Pipe | Unidirectional data pipe |
| 5 | EventBus | Kernel event bus (kernel only) |
| 6 | BlockDevice | Block storage device (kernel) |
| 7 | Filesystem | Filesystem instance (internal) |
| 8 | MemoryRegion | Physical memory region (kernel) |
| 9 | Symlink | Namespace symbolic link |
| 10 | MountPoint | Filesystem mount point (kernel) |
| 11 | Directory | Namespace directory |
| 12 | Key | Registry key (Cm) |
| 13 | Event | Manual/auto-reset event (waitable) |
| 14 | Semaphore | Counting semaphore (waitable) |
| 15 | Timer | One-shot or periodic timer (waitable) |
| 16 | Thread | Thread object (waitable on join) |
| 17 | Section | Shared memory section (maps to VMA) |
| 18 | Socket | Network socket (Tcp/Udp) |
| 20 | Service | Managed service process (Sm) |
| 21 | PowerManager | Power management (kernel-created, `\System\PowerManager`) |
| 22 | KeyboardDevice | Keyboard device — NeoKBD (kernel-created, `\Device\Keyboard`) |

## Namespace Hierarchy

```text
\ (root)
├── \Global\
│   ├── \Global\Info\              — virtual read-only objects
│   │   ├── CpuInfo                — ob_query_info(class=7); CpuStats (class=24)
│   │   ├── Threads                — ob_query_info(class=25) all-thread snapshot
│   │   ├── DateTime               — RTC date/time (class=9)
│   │   ├── Memory                 — physical + kernel heap stats (class=10)
│   │   ├── Version                — kernel version string (class=8)
│   │   ├── Cwd                    — current working directory per process
│   │   ├── Keyboard               — keyboard layout (set via ob_set_info, class=5)
│   │   ├── Drives                 — mounted drive list (class=11)
│   │   ├── Drivers                — NEM driver registry (class=12)
│   │   └── VtInfo                 — VT information (class=11 sub)
│   ├── \Global\FileSystem\C:\     — VFS mount point for NeoDOS FS
│   └── \Global\FileSystem\A:\     — VFS mount point for FAT32 ESP
├── \Device\                       — device objects
│   ├── Tcp                        — TCP network device
│   ├── Udp                        — UDP network device
│   ├── Harddisk0                  — primary block device
│   ├── NeoDosVolume0              — NeoDOS FS volume
│   ├── EspVolume0                 — ESP FAT32 volume
│   ├── Keyboard                   — keyboard device (NeoKBD, ObType::KeyboardDevice)
│   └── ...
├── \Driver\                       — NEM driver objects
│   ├── Ahci                       — AHCI NEM driver
│   ├── E1000                      — e1000 NIC driver
│   └── ...
├── \System\                       — system objects
│   └── PowerManager               — power management (ObType::PowerManager, selo shutdown/reboot via ob_set_info)
├── \Registry\                     — registry keys (Cm)
├── \Service\                      — registered service objects (ObType::Service)
├── \Ob\Process\                   — PID-indexed process objects
├── \Security\                     — security objects (future)
└── \DosDevices\                   — drive letter symlinks (C:, A:)
```

## Syscall Details

### ob_open (RAX=60)

1. Resolve path via `ob_resolve_path()` (symlink traversal, case-insensitive)
2. Perform `SeAccessCheck` against the object's DACL using the caller's token
3. Allocate a `HandleEntry` in the process handle table with `ob_object(ob_id,
   desired_access)`
4. Return fd (>=3) or error

### ob_create (RAX=61)

1. Validate ObType — only user-creatable types: Process(1), Driver(2), Pipe(4),
   Directory(11), Event(13), Semaphore(14), Timer(15), Thread(16), Section(17),
   Service(20).
   PowerManager(21) and KeyboardDevice(22) are kernel-created only.
2. Call `ob_create_object()` in the global table with given name + type
3. Insert into namespace at the given path
4. Allocate handle entries for any returned fds (pipe creates bidirectional pair)
5. Return fd(s)

### ob_query_info (RAX=62)

Supports the following info classes:

| Class | Name | Description |
| ------- | ------ | ------------- |
| 0 | Basic | Object type + refcount |
| 1 | Name | Object name string |
| 2 | File | File size + attributes (VFS) |
| 3 | Process | PID + parent PID + priority + thread count + aggregated state |
| 4 | Thread | TID + PID + state + priority of the first matching KTHREAD (see 25 for all threads) |
| 5 | Pipe | Pipe buffer size + available |
| 6 | Device | Device type + status |
| 7 | CpuInfo | `CpuInfoFull` for the *calling* CPU (identity, features, address widths) + total `cpu_count` |
| 8 | Version | Kernel version string |
| 9 | DateTime | RTC date/time |
| 10 | Memory | Phys total/usable/free + heap stats |
| 11 | Drives | Mounted drive list |
| 12 | Drivers | NEM driver list + states |
| 13 | Cwd | Current working directory |
| 14 | KeyboardLayout | Current layout ID |
| 15 | ReadContent | File read at handle offset |
| 16 | VolumeLabel | Volume label (get) |
| 17 | SocketInfo | Socket type + state |
| 18 | SocketAddr | Bound/peer address |
| 19 | TcpStatus | TCP connection state |
| 20 | NicInfo | NIC hardware info (MAC, IP, link, vendor/device IDs, name, description) |
| 21 | RegistryKey | Key metadata |
| 22 | RegistryValue | Value data |
| 23 | SocketRecv | Receive from socket |
| 24 | CpuStats | Per-CPU snapshot `[StatsHeader][CpuStatsEntry]` — `\Global\Info\CpuInfo` |
| 25 | ThreadStats | All-thread snapshot `[StatsHeader][ThreadStatsEntry]` — `\Global\Info\Threads` |
| 26 | ProcessSnapshot | Coherent process+thread snapshot `[ProcSnapshotHeader][ProcessInfoRaw][ThreadInfoRaw]` — `\Global\Info\Processes` |
| 29 | ServiceState | Service state (state+pid+uptime) |
| 30 | ServiceConfig | Service configuration (start type, restart policy, max failures) |
| 31 | ServiceStatus | Comprehensive status (state+pid+exit count+exit code+failures+uptime) |
| 33 | FsckStatus | FsckStatsRaw — filesystem integrity check results (read-only) |
| 34 | ProcessId | u32 LE — current process PID |
| 35 | KeyboardInfo | KbdState (modifiers, leds, active_layout_index) — `\Device\Keyboard` |
| 36 | KeyboardCaps | KbdCaps (max_layouts, capabilities, num_layouts) — `\Device\Keyboard` |
| 37 | KeyboardLayouts | [KbdLayoutInfo] — list of loaded layouts on `\Device\Keyboard` |
| 38 | Hostname | System hostname string (null-terminated) — any Ob object, reads from Registry |
| 39 | ProcessArgs | Per-process command-line args (256 bytes, null-terminated) — any valid handle, returns current process args copied atomically at `sys_ob_create(PROCESS)` from `0x41F000` (fixes `0x41F000` data race in pipelines) |
| 32 | PowerState | PowerSystemState u32 (Active/ShuttingDown/Rebooting/Suspending/Hibernating/Off) — `\System\PowerManager` |
| 33 | PowerPlanInfo | Active plan index + name (planned) — `\System\PowerManager` |
| 34 | PowerStatus | Power capabilities bitmask (planned) — `\System\PowerManager` |

### SMP observability (CpuStats = 24, ThreadStats = 25)

Additive ABI v8 extension: no existing class, struct, or syscall number changed.
Both classes return a size-negotiated snapshot:

```text
buffer = [StatsHeader][Entry; returned]
```

`StatsHeader` (16 bytes, `repr(C)`):

| Field | Type | Meaning |
| ------- | ---- | --------- |
| `version` | u32 | Layout version (currently 1) |
| `total` | u32 | Objects available in this snapshot |
| `returned` | u32 | Objects actually written |
| `entry_size` | u32 | `sizeof(Entry)` for forward-compatible parsing |

The syscall returns the number of bytes written. If `returned < total` the
buffer was too small; retry with a larger buffer. There is no silent truncation.

#### CpuStats (24) — `\Global\Info\CpuInfo`

`CpuStatsEntry` (40 bytes):

| Field | Type | Source |
| ------- | ---- | ------ |
| `interrupt_count` | u64 | `KPRCB.interrupt_count` |
| `context_switch_count` | u64 | `KPRCB.context_switch_count` |
| `timer_tick_count` | u64 | `KPRCB.timer_tick_count` |
| `cpu_id` | u32 | `KPRCB.cpu_id` |
| `apic_id` | u32 | `KPRCB.apic_id` |
| `online` | u8 | 1 if the CPU is in the online set |

Semantics:

- One entry per online CPU (`cpu_local::cpu_count()`).
- Reads are lock-free per-CPU reads of `KPRCB_PAGES[cpu]` (the same mechanism
  `is_pid_running_on_any_cpu` uses). Each field is an aligned scalar (no torn
  word), but the *set* of fields is not one atomic instant: that CPU may update
  a counter between two of our reads. Treat it as a best-effort point-in-time
  sample, not a transactionally consistent snapshot.
- `timer_tick_count` counts timer interrupts. It is **not** CPU busy time and
  there is currently no busy/idle accounting, so **CPU% cannot be computed** and
  is deliberately not exposed.
- `interrupt_count` is exposed for ABI completeness, but the kernel currently
  has **no increment site** for it, so it reads 0. `timer_tick_count` and
  `context_switch_count` are maintained.
- `cpu_id` is a scheduler CPU index; `apic_id` identifies hardware. They are not
  assumed equal.
- Online CPUs are currently `0..cpu_count()`: NeoDOS brings APs online
  sequentially and does not support CPU hot-remove.

#### ThreadStats (25) — `\Global\Info\Threads`

`ThreadStatsEntry` (24 bytes):

| Field | Type | Source |
| ------- | ---- | ------ |
| `tid` | u32 | `Kthread.tid` |
| `pid` | u32 | `Kthread.pid` |
| `cpu_id` | u32 | `Kthread.cpu` — CPU the thread is enqueued/running on; follows migration |
| `priority` | u8 | `Kthread.priority` (0 HIGH … 3 IDLE) |
| `state` | u8 | `ThreadState::to_u8` (0 Ready, 1 Running, 2 Blocked, 3 Suspended, 4 Terminated) |
| `cpu_ticks` | u64 | `Kthread.cpu_ticks` — timer ticks charged to this thread |

Semantics:

- Enumerates **all** live KTHREADs, including kernel and per-CPU idle threads
  (idle threads have `pid = 0`). It does not use the Ob namespace, so it is not
  limited to user threads and needs no per-thread Ob object.
- The global scheduler lock is held only while copying thread fields into a
  staging buffer; user memory is written after the lock is released. No
  `Kthread` pointer ever reaches user space (no UAF window).
- Best-effort snapshot: a thread created/terminated concurrently may or may not
  appear, and `Kthread.cpu` can change immediately after the copy (migration).
  At most one snapshot is in flight; a concurrent query returns `-Again` and
  may be retried.
- A snapshot is capped at 128 entries; `total > returned` signals truncation.
- `cpu_ticks` is a **tick counter**, not calibrated time and not a percentage.
  It increments once per timer tick while the thread is the running thread.

#### ProcessSnapshot (26) — `\Global\Info\Processes`

Phase 15-A. A single coherent process+thread inspection snapshot built from
`Scheduler::snapshot_into` (Phase 14-B). Unlike `ThreadStats`, processes and
threads come from **one** scheduler-consistent capture and names are included.

```text
buffer = [ProcSnapshotHeader]
         [ProcessInfoRaw; process_returned]
         [ThreadInfoRaw;  thread_returned]
```

`ProcSnapshotHeader` (32 bytes): `version` (1), `process_total`,
`process_returned`, `thread_total`, `thread_returned`, `process_entry_size`
(40), `thread_entry_size` (48), `flags` (bit0 = truncated).

`ProcessInfoRaw` (40 bytes): `pid: u32`, `name: [u8; 32]`, `thread_count: u32`.

`ThreadInfoRaw` (48 bytes): `tid: u32`, `pid: u32`, `name: [u8; 32]`,
`state: u8` (0 Ready, 1 Running, 2 Blocked, 3 Suspended, 4 Terminated),
`idle: u8`, `is_current: u8`, `cpu: u32`.

Semantics:

- Sourced from the logical registries (`Scheduler.eprocesses` /
  `Scheduler.kthreads`), not the run queues. A reaped object does not appear; a
  `Terminated`-but-not-yet-reaped thread does.
- Copied under the global `SCHEDULER` mutex; the lock is released before any
  user-memory write. Read-only, no kernel pointer escapes.
- `cpu` for a Running thread is the `KPRCB.current_thread` owner; otherwise the
  scheduler's `Kthread.cpu` assignment. `is_current` marks the authoritative
  owner (never inferred from `state`).
- `idle` marks per-CPU idle threads (distinct names `idle/0..n`).
- Deterministic ordering: processes by `pid`, threads by `tid`.
- Truncation is explicit (`flags` bit0 and/or `*_returned < *_total`); there is
  no silent loss. At most one snapshot is in flight (`-Again` on contention).
- Names are the bounded Phase 14-A `KernelName` (ASCII, `NAME_MAX = 32`).

#### Process state semantics (`ObProcessInfo.state`)

Class 3 (`Process`) now aggregates its threads' states under the scheduler lock
instead of `thread_count == 0 ? Running : Ready`:

| Value | Meaning |
| ------- | --------- |
| 0 | Ready — no Running thread, at least one Ready |
| 1 | Running — at least one thread Running on some CPU |
| 2 | Blocked — live threads exist, all Blocked/Suspended |
| 3 | Terminated — no live thread |

The struct layout is unchanged.

#### Known limitations (no compatible ABI workaround)

- **Process names**: `Eprocess.name` (Phase 14-A) is the name source; use
  `ProcessSnapshot` (class 26). `ProcessArgs` still returns the *calling*
  process's args, not the queried process's.
- **CPU% / busy time**: only tick counters exist; no idle/busy accounting.
- **CPU hotplug**: `online` is derived from the contiguous online range.
- **`sys_ob_enum` truncation**: directory enumeration still has no pagination.
  The new stats classes avoid silent loss via `total`/`returned`; `sys_ob_enum`
  itself is unchanged to preserve ABI v8.

### ob_set_info (RAX=63)

Supports 38 set classes:

| Class | Name | Description |
| ------- | ------ | ------------- |
| 0 | ProcessPriority | Set process priority |
| 1 | ThreadPriority | Set thread priority |
| 2 | ObjectName | Rename object |
| 3 | Security | Set DACL |
| 4 | ProcessTerminate | Kill process |
| 5 | KeyboardLayout | Set layout (1=en, 2=es, 3=de) |
| 6 | VfsRename | Rename VFS file/dir |
| 7 | WriteContent | Write to VFS file at handle offset |
| 8 | SetCwd | Change working directory |
| 9 | SetVolumeLabel | Set volume label |
| 10 | TimerStart | Start timer (oneshot/periodic) |
| 11 | TimerCancel | Cancel running timer |
| 12 | SemaphoreRelease | Increment semaphore count |
| 13 | MapView | Map Section into process address space |
| 14 | UnmapView | Unmap Section view |
| 15 | FileCreate | Create VFS file |
| 16 | FileDelete | Delete VFS file |
| 17 | SetProcessVt | Switch virtual terminal |
| 18 | SocketConnect | Connect TCP socket |
| 19 | SocketBind | Bind UDP/TCP socket |
| 20 | SocketListen | Listen on TCP socket |
| 21 | SocketSend | Send data on socket |
| 22 | SocketClose | Close socket |
| 23 | RegistryCreateKey | Create registry key |
| 24 | RegistryDeleteKey | Delete registry key |
| 25 | RegistrySetValue | Set registry value |
| 26 | RegistryDeleteValue | Delete registry value |
| 27 | SetNicIp | Set NIC IP address and subnet mask |
| 33 | ServiceStart | Start a service (Stopped/Failed → Starting → Running) |
| 34 | ServiceStop | Stop a running service (Running → Stopping → Stopped) |
| 35 | ServiceRestart | Restart a service (stop + start atomically) |
| 36 | ServiceSetConfig | Modify service configuration (start type, restart policy, max failures) |
| 37 | PowerShutdown | Initiate coordinated system shutdown |
| 38 | PowerReboot | Initiate coordinated system reboot |
| 39 | PowerSuspend | Suspend to RAM (planned) — `\System\PowerManager` |
| 40 | PowerHibernate | Hibernate to disk (planned) — `\System\PowerManager` |
| 41 | PowerSetPlan | Set active power plan by index (planned) — `\System\PowerManager` |
| 42 | PowerSetPolicy | Set individual power policy value (planned) — `\System\PowerManager` |
| 39 | FsckRepair | Run fsck with repair flag (buf[0] != 0 = repair) |
| 43 | KeyboardSetLayout | Set layout by name (string) — `\Device\Keyboard` |
| 44 | KeyboardSetRepeatDelay | Set repeat delay in ms (u32 LE) — `\Device\Keyboard` |
| 45 | KeyboardSetRepeatRate | Set repeat rate in cps (u32 LE) — `\Device\Keyboard` |
| 46 | KeyboardSetLeds | Set LED state byte — `\Device\Keyboard` |
| 47 | KeyboardSetModifier | Set modifier byte (admin) — `\Device\Keyboard` |
| 49 | SetHostname | Set system hostname (REG_SZ) — any Ob object, admin only |

### ob_enum (RAX=64)

Enumerate a directory fd. Writes `ObEnumEntry` structs (52 bytes each) into the
user buffer. Returns entry count.

```rust
#[repr(C)]
pub struct ObEnumEntry {
    pub id: ObId,
    pub obj_type: u32,
    pub name: [u8; 32],
    pub mode: u16,
    pub _pad: [u8; 2],
    pub size: u32,
}
```

### ob_wait (RAX=65)

Wait on up to N handles, with wait type (0=ANY, 1=ALL) and timeout in ms.

- **Process**: signaled when child exits (exit code in native_id)
- **Pipe**: signaled when data is available (pipe read end)
- **Event**: signaled on `set` (manual-reset stays set until cleared)
- **Timer**: signaled on expiry
- **Thread**: signaled on termination (join)
- **Semaphore**: signaled when count > 0

Non-blocking check: Pipe, Semaphore, and Timer perform an immediate check
before entering the KWait block path. This prevents unnecessary context
switches.

### ob_destroy (RAX=66)

Delete an object from the namespace and object table. Calls `on_destroy()` if
the object has a custom `ObOperations` impl. Fails with `-RefCountHeld` if the
refcount indicates open handles or namespace children.

## URN Integration

URN is a frontend of Ob — all URI schemes (`file://`, `device://`,
`registry://`, `kobj://`) resolve via `ob_open_path()` internally.

```rust
// URN resolving:
//   "file://C:/foo.txt"  ->  ob_open_path("\\Global\\FileSystem\\C:\\foo.txt")
//   "device://Tcp"       ->  ob_open_path("\\Device\\Tcp")
//   "registry://..."     ->  ob_open_path("\\Registry\\...")
```

`UrnHandle` wraps a kernel fd returned by `ob_open_path()`. There are 19
dedicated URN tests.

## ObError Codes

| Value | Name | Meaning |
| ------- | ------ | --------- |
| 0 | Success | Operation succeeded |
| -1 | NotFound | Object path not found |
| -2 | AlreadyExists | Object already exists at path |
| -3 | InvalidParam | Bad argument |
| -4 | RefCountHeld | Object has active references |
| -5 | OutOfMemory | Allocation failure |
| -6 | AccessDenied | Security check failed |
| -7 | NotSupported | Operation not supported for this type |
| -8 | InvalidType | Wrong ObType for operation |
| -9 | TableFull | Object table capacity exhausted |

## Migration from KOBJ

The legacy `kobj/` subsystem was eliminated in v0.46. All objects now use the
Ob architecture. The mapping is straightforward:

```text
KObjType::X           ->  ObType::X
kobj_register(t,n,id) ->  ob_create_object(t,n,id,0,None)
kobj_lookup(id)       ->  ob_lookup(id)
```

## Detailed Architecture Reference

For the complete design history, evolution from KOBJ v1 through Ob unification,
namespace design rationale, and future roadmap, see:
**`docs/kernel/obj-arch.md`** — historical design companion document.

### Frozen Design Decisions

The following are locked decisions for the Ob architecture (archived from
`obj-arch.md` §15):

1. **ObObject is mandatory.** Every kernel resource MUST be an `ObObject` in the
   global object table. No parallel resource tracking outside Ob.
2. **ObId is unique and never reused.** `next_id` increments monotonically.
3. **ObType enum is the unique discriminator.** No second enum, no type punning
   via native_id. Adding a new resource type means adding one variant to
   `ObType`.
4. **All handles are ObHandles.** HandleEntry stores only `object_id: ObId` and
   `offset: u64`. The old per-type kind field is eliminated.
5. **sys_close works via Ob.** Closes the handle, calls `ob_release_object`,
   triggers `on_destroy` if refcount reaches zero.
6. **Ob namespace is hierarchical.** Paths like `\Global\FileSystem\C:\...`.
   No flat namespace.
7. **Security is integrated into Ob.** `SeAccessCheck` is called on
   `ob_open_path`. Every object has a DACL.
8. **URN is a frontend of Ob.** All URI schemes resolve via `ob_open_path()`.
9. **Migration is incremental.** Legacy syscalls coexist with Ob syscalls during
   migration. No flag day.
10. **ObOperations trait is the extension point.** Custom `on_destroy` for
    type-specific cleanup. The trait can grow new methods as needed.
