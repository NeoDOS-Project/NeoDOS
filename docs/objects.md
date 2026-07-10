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
|-------|------|-------------|
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

## Namespace Hierarchy

```
\ (root)
├── \Global\
│   ├── \Global\Info\              — virtual read-only objects
│   │   ├── CpuInfo                — ob_query_info(class=7)
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
│   └── ...
├── \Driver\                       — NEM driver objects
│   ├── Ahci                       — AHCI NEM driver
│   ├── E1000                      — e1000 NIC driver
│   └── ...
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
   Service(20)
2. Call `ob_create_object()` in the global table with given name + type
3. Insert into namespace at the given path
4. Allocate handle entries for any returned fds (pipe creates bidirectional pair)
5. Return fd(s)

### ob_query_info (RAX=62)

Supports 27 info classes:

| Class | Name | Description |
|-------|------|-------------|
| 0 | Basic | Object type + refcount |
| 1 | Name | Object name string |
| 2 | File | File size + attributes (VFS) |
| 3 | Process | PID + parent PID + state |
| 4 | Thread | TID + state + CPU time |
| 5 | Pipe | Pipe buffer size + available |
| 6 | Device | Device type + status |
| 7 | CpuInfo | CPUID leaf values per core |
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
| 20 | NicInfo | NIC MAC, IP, link status |
| 21 | RegistryKey | Key metadata |
| 22 | RegistryValue | Value data |
| 23 | SocketRecv | Receive from socket |
| 29 | ServiceState | Service state (state+pid+uptime) |
| 30 | ServiceConfig | Service configuration (start type, restart policy, max failures) |
| 31 | ServiceStatus | Comprehensive status (state+pid+exit count+exit code+failures+uptime) |

### ob_set_info (RAX=63)

Supports 32 set classes:

| Class | Name | Description |
|-------|------|-------------|
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
|-------|------|---------|
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

```
KObjType::X           ->  ObType::X
kobj_register(t,n,id) ->  ob_create_object(t,n,id,0,None)
kobj_lookup(id)       ->  ob_lookup(id)
```

## Detailed Architecture Reference

For the complete design history, evolution from KOBJ v1 through Ob unification,
namespace design rationale, and future roadmap, see:
**`docs/OBJECT_MANAGER_ARCHITECTURE.md`** (1387 lines) — historical reference
document.
