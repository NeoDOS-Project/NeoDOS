# NeoDOS Users, Groups, and Security — Design Document

> **Status:** Draft v1 — Pre-implementation design review
> **Target Kernel:** v0.50+ (post Registry bugfixes)
> **Filosofía:** NT-like, Object Manager-centric, incremental

---

## 1. Auditoría de la Arquitectura Actual

### 1.1 Security Subsystem — What Exists

The kernel already has a complete NT-style security foundation in `src/security/`:

| Component | File | Status |
|-----------|------|--------|
| SID (Security Identifier) | `sid.rs` | ✅ Complete — `S-1-5-21-*` format, up to 8 sub-authorities |
| Token | `token.rs` | ✅ Complete — SID, groups, 12 privilege bits, session_id |
| ACE / ACL | `acl.rs` | ✅ Complete — Allow/Deny, canonical ordering |
| SecurityDescriptor | `acl.rs` | ✅ Complete — owner, group, dacl |
| SeAccessCheck | `access.rs` | ✅ Complete — two-phase deny-first eval, admin bypass |
| SAM database | `sam.rs` | ✅ Complete — users, SIDs, SHA-256 password hashing, binary persistence |
| ObSecurity store | `object/mod.rs` | ✅ Complete — `OB_SECURITY: BTreeMap<ObId, SecurityDescriptor>` |
| Token inheritance | `scheduler/mod.rs` | ✅ Complete — child processes inherit parent token |
| ObSetInfoClass::Security | `syscall/ob.rs` | ✅ Complete — userspace can set SD on objects |
| Syscall permission table | `syscall/permission.rs` | ✅ Complete — admin bit per syscall |

### 1.2 Security Subsystem — What Is Missing

| Gap | Impact | Since |
|-----|--------|-------|
| SAM not wired to process creation | All processes get DEFAULT_ADMIN_TOKEN | v0.44 |
| No login/authentication flow | No way to authenticate, no sessions | never |
| Group SIDs not checked in SeAccessCheck | `token.groups` unused by ACL evaluation | v0.44 |
| Privileges not enforced | `has_privilege()` exists but never called | v0.44 |
| No security on VFS/NeoFS paths | File operations bypass all access checks | never |
| No security on Registry keys | `sec_desc_cell` always NULL | never |
| No audit logging | SACL field exists but no audit engine | never |
| Empty DACL grants instead of denies | Diverges from NT behavior | v0.44 |

### 1.3 Object Manager — Current Model

- **ObId = u64** — monotonically increasing, wraps on overflow
- **ObType** — 18 variants (Process=1, Thread=16, Pipe=4, Event=13, etc.)
- **HandleTable** — per-process `Vec<HandleEntry>`, fds 0/1/2 pre-allocated
- **Namespace** — hierarchical tree rooted at `\`, protected root dirs
- **ObOperations trait** — single `on_destroy()` callback
- **Auto-path mapping** — `\Ob\Process\{pid}`, `\Ob\Socket\{name}`, etc.
- **VFS bridge** — `\Global\FileSystem\` paths delegate entirely to VFS

### 1.4 Process Model — Current

```rust
struct Eprocess {
    pid: u32,
    parent_pid: u32,
    handle_table: HandleTable,
    token: Token,              // <-- security identity lives here
    cwd_drive: u8, cwd_path: String,
    heap_base: u64, heap_break: u64,
    user_slot: Option<u8>,
    mmap_regions: Vec<MmapRegion>,
    thread_count: u32,
    exit_code: i64,
    obj_id: Option<ObId>, ob_id: Option<ObId>,
    address_space: AddressSpace,
    vt_num: u8,
}
```

**Token lifecycle:**
- PID 0 (Idle): `Token::new_admin()`
- PID 1 (NeoInit): `DEFAULT_ADMIN_TOKEN.clone()`
- Ring 3 processes: `DEFAULT_USER_TOKEN` → then overwritten by parent's token via `parent_ep.token.clone()`
- No mechanism to create a process with a different token than the parent's

### 1.5 Filesystem — Current Permission Model

- `DirEntryV2` has a 16-bit `mode` field with: `PERM_R(1)`, `PERM_W(2)`, `PERM_X(4)`, `PERM_S(8)`, `PERM_D(16)`, `MODE_DIR(0x40)`, `MODE_FILE(0x80)`
- **No owner/group/SID/ACL fields** on disk
- **No permission enforcement** anywhere in the VFS or NeoFS code paths
- Bits are serialized/deserialized but never checked

### 1.6 Registry — Current Security Model

- `KeyCell` has `sec_desc_cell: u32` field — **always NULL_CELL in practice**
- No ACL checking on any registry operation
- `CM-SEC` item in IMPROVEMENTS.md plans ACLs (blocked on CM-FIX)

---

## 2. Architectural Analysis

### 2.1 What Model Does NeoDOS Currently Follow?

NeoDOS follows an **NT-inspired hybrid model**:
- **Process creation**: `CreateProcess`-style (spawn + load ELF, no fork)
- **Object Manager**: NT `Ob` namespace with handles, types, security descriptors
- **Security**: NT `Token` + `SID` + `ACL` + `SeAccessCheck`
- **Registry**: NT `Cm` hive-based key/value store
- **Drivers**: NEM format (akin to NT `.sys` with versioned ABI)
- **Scheduler**: Priority-based with aging and work stealing

### 2.2 How Do Processes Interact?

- Via **Ob handles** — open objects in the namespace, pass fds
- Via **pipes** — `ObType::Pipe` with read/write fds
- Via **signals** — `ObType::Event`, `ObType::Semaphore`, `ObType::Timer`
- Via **shared memory** — `ObType::Section` mapped via mmap
- Via **sockets** — `ObType::Socket` for network communication
- Via **Registry** — `Cm` key/value store shared across processes

### 2.3 How Are Objects Created?

1. `sys_ob_create(path, type, fds_out, attrs)` → kernel
2. `handler_ob_create()` in `syscall/ob.rs`:
   - Validates type (user-creatable: Process, Driver, Pipe, Directory, Event, Semaphore, Timer, Thread, Section, Socket)
   - For each type: allocates native resource, creates ObObject, inserts in namespace, allocates fd
3. `ob_open_path(path)` → resolves namespace path, checks SecurityDescriptor via `SeAccessCheck`, creates handle

### 2.4 Where Should User Information Live?

**Decision: Token is the right abstraction** — NeoDOS already has it. User identity is `Token.sid`. What's missing:
1. A name→SID mapping (SAM database — already exists but unwired)
2. A session object to group processes under a login
3. A way to set a process's token at creation time that differs from the parent
4. Login service that creates sessions with user tokens

### 2.5 Components That Must Change

| Component | Change Required |
|-----------|----------------|
| `src/security/sam.rs` | Wire to filesystem, add write-back on mutation |
| `src/security/token.rs` | Add UserProfile reference, creation audit |
| `src/scheduler/mod.rs` | Token-based process creation, session tracking |
| `src/syscall/ob.rs` | Token query on Process/Thread, user creation syscalls |
| `src/object/types.rs` | New ObType::User, ObType::Session |
| `src/fs/neodos_dir.rs` | Extend DirEntryV2 with owner SID field |
| `src/fs/vfs.rs` | Add permission checking in VFS operations |
| `src/cm/` | Wire sec_desc_cell, user profile storage |
| `userbin/neoinit/` | Login prompt or spawn login service |
| `userbin/neoshell/` | Basic identity commands (whoami) |
| `libneodos/src/syscall.rs` | New wrapper functions |

### 2.6 Components That Should NOT Change

| Component | Reason |
|-----------|--------|
| VirtIO drivers | Unrelated to security |
| Network stack | Unrelated (except socket permission checks — future) |
| NEM driver format | Already has capability flags |
| Scheduler priority/aging | Orthogonal to security |
| Memory management | Orthogonal to security |
| Boot sequence (Phase 1-3) | Too early, no userspace yet |
| HAL layer | Unrelated |

---

## 3. Identity Model

### 3.1 Core Types

```rust
// ── User ──
// Represents a human user or system account.
// NOT an Ob object — users are stored in SAM, not in the namespace.
// Users are referenced by SID, not by object handle.
struct User {
    sid: Sid,
    username: String,       // unique, case-insensitive, 1-32 chars
    full_name: String,      // display name, up to 128 chars
    flags: UserFlags,       // ADMIN, DISABLED, LOCKED, PASSWORD_EXPIRED
    password_hash: [u8; 32],// SHA-256(password + salt)
    password_salt: [u8; 32],
    profile_path: String,   // e.g., "\Users\{username}"
    home_path: String,      // e.g., "\Users\{username}\Home"
}

bitflags UserFlags: u32 {
    ADMIN           = 1 << 0,  // Member of Administrators group
    DISABLED        = 1 << 1,  // Cannot log in
    LOCKED          = 1 << 2,  // Account locked after failed attempts
    PASSWORD_EXPIRED = 1 << 3, // Must change password on next login
    SYSTEM          = 1 << 4,  // Built-in system account (not a real user)
}
```

### 3.2 Group

```rust
// ── Group ──
// A collection of users. Groups have a SID and a name.
// Not an Ob object — groups are stored in SAM.
struct Group {
    sid: Sid,
    name: String,           // unique, case-insensitive
    description: String,
    members: Vec<Sid>,      // member user/group SIDs
}
```

### 3.3 Session

```rust
// ── Session ──
// A login session — groups processes under one authenticated identity.
// IS an Ob object (ObType::Session = 19) — lives in the namespace.
// This gives us: ob_open(Session), ob_query_info(Session), ob_enum(Session)
struct Session {
    session_id: u32,        // unique, monotonically increasing
    user_sid: Sid,          // the authenticated user
    token: Token,           // the security token for this session
    login_time: u64,        // timestamp of login
    vt_num: u8,             // associated virtual terminal
    process_count: u32,     // number of processes in this session
    state: SessionState,    // Active, Locked, Disconnected, LoggedOff
}

enum SessionState {
    Active = 0,
    Locked = 1,
    Disconnected = 2,
    LoggedOff = 3,
}
```

### 3.4 Token (Extended)

The existing `Token` struct is enhanced with session context:

```rust
struct Token {
    pub sid: Sid,
    pub is_admin: bool,
    pub groups: Vec<Sid>,
    pub privileges: u64,
    pub session_id: u32,       // already exists
    // NEW:
    pub session_ob_id: Option<ObId>,  // reference to Session object
    pub creation_time: u64,           // when this token was created
    pub integrity_level: IntegrityLevel,  // Mandatory Integrity Control
}

enum IntegrityLevel {
    Untrusted = 0,
    Low = 1,
    Medium = 2,     // default for users
    High = 3,       // admin
    System = 4,     // kernel
}
```

### 3.5 Justification

| Decision | Why |
|----------|-----|
| User/Group not Ob objects | They are administrative metadata, not runtime resources. SAM is a flat-file database. Making them Ob objects would clutter the namespace with administrative entries. |
| Session IS an Ob object | Sessions are runtime entities that group processes. They need namespace visibility, enumeration (`who` command), and security descriptor support. |
| Token remains per-process | NT model: token is the process security identity. Threads share the process token. No need for per-thread tokens initially. |
| Integrity Level added | NT Mandatory Integrity Control (MIC) provides a simple "can't write up" policy that prevents low-integrity processes from modifying high-integrity ones. Simple to implement, high security value. |
| SID remains primary key | NT compatibility path. S-1-5-21-* format maps cleanly to future domain/network scenarios. |

---

## 4. Integration with Object Manager

### 4.1 New ObType

```rust
// Add to object/types.rs:
Session = 19,
// Future: Job = 20 (for process groups/cgroups)
```

### 4.2 New ObInfoClass Variants

```rust
// For sys_ob_query_info (RAX=62):
SessionInfo = 24,   // session_id, user_sid, state, login_time, process_count
UserInfo = 25,      // username, full_name, flags (queried via session or by SID)
GroupInfo = 26,     // group name, member count, description
IntegrityLevel = 27,// query process integrity level
TokenInfo = 28,     // query current process token (sid, groups, privileges)
```

### 4.3 New ObSetInfoClass Variants

```rust
// For sys_ob_set_info (RAX=63):
SessionLock = 28,        // Lock a session (requires admin)
SessionLogoff = 29,      // Log off a session (requires admin)
SessionDisconnect = 30,  // Disconnect without logging off
ChangePassword = 31,     // Change password for current user
SetIntegrityLevel = 32,  // Lower process integrity level
```

### 4.4 Namespace Layout

```
\Session
  \1              ← session 1 (user "admin")
  \2              ← session 2 (user "jdoe")
\Security
  \SAM            ← SAM database (admin-only)
  \Policies       ← future: security policies
```

### 4.5 API Examples

```
// Create session (login):
ob_handle = ob_create("\Session\2", Session, ...)
→ creates Session object, allocates Token, assigns to process

// Query session info:
ob_query_info(session_fd, SessionInfo, &buf)
→ { session_id: 2, user_sid: S-1-5-21-..., state: Active, ... }

// Lock workstation:
ob_set_info(session_fd, SessionLock, ...)
→ sets session state to Locked, blocks input

// Change password:
ob_set_info(session_fd, ChangePassword, &old_new_pair)
→ SAM verifies old, stores new hash

// Query current token:
ob_query_info(process_fd, TokenInfo, &buf)
→ { sid: ..., groups: [...], privileges: ..., integrity_level: Medium }
```

### 4.6 Advantages vs Disadvantages

| Aspect | Advantage | Disadvantage |
|--------|-----------|--------------|
| Session as Ob object | → Enumeration via `ob_enum(\Session)` works naturally<br>→ Sessions get security descriptors<br>→ Existing handle lifecycle applies | → Slight namespace overhead<br>→ Must manage session lifecycle |
| User NOT as Ob object | → SAM remains the authority<br>→ No namespace clutter<br>→ Backward compatible | → Can't use `ob_open(User)` to reference a user<br>→ User info queries go through session or syscall |

**Decision:** Session=Ob, User/Group=not-Ob. This matches NT (where Session objects exist in the Object Manager namespace but users are SAM entries).

---

## 5. NeoFS Integration

### 5.1 DirEntryV2 Extension

Add an 8-byte owner SID field to DirEntryV2. The on-disk format grows from 128 to 136 bytes.

```rust
// New DirEntryV2 layout (136 bytes):
pub struct DirEntryV2 {
    pub name: Vec<u8>,          // up to 48 bytes (unchanged)
    pub mode: u16,              // unchanged
    pub size: u64,              // unchanged
    pub created: u64,           // unchanged
    pub modified: u64,          // unchanged
    pub checksum: u32,          // unchanged
    pub owner_sid: Sid,         // NEW: 8 bytes, owner SID
    pub inline_len: u32,        // unchanged
    pub inline_data: [u8; 16],  // unchanged
    pub extent_lba: u64,        // unchanged
    pub extent_count: u32,      // unchanged
}
```

**Backward compatibility:** Old NE2 volumes without owner_sid will read `owner_sid = S-1-5-21-0-0-0-1000` (default "unknown" SID). A superblock flag `FEATURE_OWNER_SID` indicates the extended format.

### 5.2 VFS Permission Checking

Add permission checking to the VFS layer. The check happens in `syscall/ob.rs` handlers before delegating to VFS, using the current process token.

```rust
// Permission check matrix:
//                  R   W   X   D   Owner  Admin
// Owner            ✅  ✅  ✅  ✅   -      -
// Group member     ✅  ✅  ✅  -    -      -
// Other            ✅  -   -   -    -      -
// Admin            ✅  ✅  ✅  ✅   -      ✅ (bypass)

fn check_vfs_access(token: &Token, node_mode: u16, owner_sid: Option<&Sid>,
                    desired: AccessMask) -> bool {
    if token.is_admin { return true; }  // admin bypass
    if owner_sid.map_or(false, |o| o == &token.sid) {
        // Owner: full access based on mode bits
        return desired.bits() & node_mode == desired.bits();
    }
    if token.groups.iter().any(|g| owner_sid.map_or(false, |o| o == g)) {
        // Group member: read/write/execute but not delete
        return desired.bits() & (node_mode & !PERM_D) == desired.bits();
    }
    // Other: read-only (world-readable)
    return desired == ACCESS_READ && node_mode & PERM_R != 0;
}
```

### 5.3 Default Permissions by File Extension

Implemented at VFS `create()` and `mkdir()` time:

| Extension | Owner | Group | Other | Notes |
|-----------|-------|-------|-------|-------|
| .NXE | user | users | R-X | Ring 3 executables |
| .NEM | admin | system | R | Kernel drivers |
| .NXL | user | users | R-X | User libraries |
| .SYS | admin | system | R | System config |
| .CFG/.INI | user | users | RW | Config files |
| Directory | user | users | RWXD | Full for owner+group |
| (other) | user | users | RW | Default |

### 5.4 Enforcement Points

| Operation | Where | Check |
|-----------|-------|-------|
| `open(path, flags)` | `handler_ob_open` | `check_vfs_access(token, mode, owner, R or W or X)` |
| `create(dir, name)` | `handler_ob_create → mkdir/create` | `check_vfs_access(token, dir_mode, dir_owner, W)` |
| `read(fd)` | `ob_query_info(ReadContent)` | Check already done at open time |
| `write(fd)` | `ob_set_info(WriteContent)` | Check already done at open time |
| `unlink(path)` | `handler_ob_destroy` | `check_vfs_access(token, mode, owner, D)` |
| `rename(old, new)` | `handler_ob_set_info(VfsRename)` | `check_vfs_access(token, mode, owner, W+D)` |

---

## 6. Registry Integration

### 6.1 User Profile Storage

Store per-user configuration under a dedicated hive:

```
\Registry\User\{sid}
  \Environment          ← environment variables (PATH, TEMP, etc.)
  \Console              ← console settings (colors, buffer size)
  \Network              ← network drive mappings
  \Software\{app}      ← per-user application settings
```

### 6.2 SAM Persistence

The SAM database is serialized to `\Registry\Machine\SAM` (binary format with magic `SAM\0`, version 2). Loaded at boot by NeoInit, flushed on user creation/deletion/password change.

### 6.3 Registry Security

Wire `sec_desc_cell` in `KeyCell`:

1. On key creation: inherit parent's security descriptor (or default)
2. On key open: check `SeAccessCheck(token, sec_desc, desired_access)`
3. On key enumeration: filter keys the user cannot access
4. Default: admin=full access, user=read-only to `\Registry\Machine`, read-write to `\Registry\User\{sid}`

---

## 7. Process Integration

### 7.1 Process Owner

Each EPROCESS has an `owner_sid` field (derived from `token.sid` at creation time). This is queryable via `ObInfoClass::Process` expansion.

### 7.2 Token at Creation Time

The existing token inheritance model is modified:

```
spawn(parent, binary):
  if parent has SE_CREATE_TOKEN_PRIVILEGE:
    token = create_token(sid, groups, privileges)  // NEW: spawn with different identity
  else:
    token = parent.token.clone()                   // existing: inherit

  // Integrity level inheritance:
  if binary has "requireAdministrator" manifest:
    token.integrity_level = High
    token = filter_token(token)  // remove restricted SIDs, enable all privileges

  eproc.token = token
```

### 7.3 Session Assignment

When a process is spawned within an existing session:
```
eproc.session_id = parent.session_id
```
When a login creates a new session:
```
eproc.session_id = new_session.session_id
```

All processes in a session share the same session_id in their tokens. This enables:
- `ob_enum(\Session\{id})` → list all processes in a session
- `SessionLock` → suspend all processes in the session
- Clean session termination on logoff

---

## 8. Shell Integration

### 8.1 New Commands

| Command | Description | Implementation |
|---------|-------------|----------------|
| `WHOAMI` | Print current username and SID | `ob_query_info(process_fd, TokenInfo, &buf)` → extract SID → SAM lookup |
| `LOGIN` | Authenticate and create session | Spawns `neologon.nxe` with current identity |
| `PASSWD` | Change password | `ob_set_info(session_fd, ChangePassword, &data)` |
| `WHO` | List logged-in users | `ob_enum(\Session\)` → query each session |
| `LOGOFF` | End current session | `ob_set_info(session_fd, SessionLogoff, ...)` |

### 8.2 Login Flow (NeoLogon)

```
NeoInit boots
  ↓
Spawns neologon.nxe (PID 2)
  ↓
neologon presents login prompt:
  Username: _______
  Password: _______
  ↓
SAM.authenticate(username, password) → Token
  ↓
ob_create(\Session\{sid}, Session, ...)
  ↓
ob_set_info(session_fd, ..., token)
  ↓
spawn(shell, token, session_id)
  ↓
Shell runs as user
```

### 8.3 Privilege Escalation (SUDO concept)

A `RUNAS` command:
```
RUNAS [/USER:admin] command.nxe
  → spawns command.nxe with admin token
  → requires secedit consent or admin password
```

A `SU` command:
```
SU [username]
  → spawns new shell as username
  → requires authentication
```

---

## 9. System Startup Flow

```
UEFI
  ↓
Kernel Phase 1-3 (hardware init)
  ↓
Kernel Phase 4:
  - Creates SAM from built-in defaults (admin user)
  - Creates \Registry\Machine\SAM from built-in data
  - Spawns neoinit.NXE (PID 1) with SYSTEM token
  ↓
NeoInit Phase 1:
  - Loads SAM from \Registry\Machine\SAM
  - Creates built-in users if SAM is empty:
    • Administrator (S-1-5-21-0-0-0-500) — admin, full privileges
    • Guest (S-1-5-21-0-0-0-501) — disabled, no privileges
  - Loads registry hives (SYSTEM, SOFTWARE, SAM, SECURITY)
  ↓
NeoInit Phase 2:
  - Reads AutoStartServices from Registry
  - Auto-starts: netcfg, dhcpd
  ↓
NeoInit Phase 3 — Session 0 (system services):
  - All auto-started services run in Session 0
  - Services have SYSTEM token
  ↓
NeoInit Phase 4 — Interactive session:
  - If EnableVT:
    • If DefaultAutoLogin is set: login as that user
    • Else: spawn neologon.nxe for login prompt
  - After login: spawn DefaultShell (neoshell.nxe)
  ↓
Shell ready — user can run commands, spawn new sessions
```

---

## 10. Security Model

### 10.1 Permission Model

NT-style: **discretionary access control** with owner-managed permissions.

```
Object
  ├── Owner SID (can always change permissions)
  ├── Group SID (used for group-based access)
  ├── DACL (Discretionary ACL)
  │     ├── Ace::Deny(sid, mask)     ← evaluated first
  │     └── Ace::Allow(sid, mask)    ← evaluated second
  ├── SACL (System ACL) — future
  └── Integrity Level (MIC)
```

### 10.2 Access Mask

32-bit access mask per NT convention:

```rust
// Generic rights (bits 0-15):
const ACCESS_READ: u32      = 0x0001;
const ACCESS_WRITE: u32     = 0x0002;
const ACCESS_EXECUTE: u32   = 0x0004;
const ACCESS_DELETE: u32    = 0x0008;
const ACCESS_ALL: u32       = 0x001F;

// Object-specific rights (bits 16-23):
// (reserved for future use — e.g., KEY_CREATE_SUBKEY)

// Standard rights (bits 24-27):
const ACCESS_OWNER: u32     = 0x0100_0000;  // take ownership
const ACCESS_SYSTEM: u32    = 0x0200_0000;  // SACL access
```

### 10.3 Integrity Levels (MIC)

Simple "no write up" policy prevents low-integrity compromise of high-integrity objects:

| Level | Typical Processes | Can Write To |
|-------|-------------------|--------------|
| System | Kernel, drivers, NeoInit | Everything |
| High | Admin processes, login | System, High, Medium, Low |
| Medium | User processes | Medium, Low |
| Low | Sandboxed/Internet | Low |
| Untrusted | Isolated | Nothing |

Implementation: each process token has an `integrity_level`. Each object has an `integrity_level` stored alongside its security descriptor. Writes are denied if `process_il < object_il`.

### 10.4 Privilege Model (12 bits — unchanged from current)

| Bit | Privilege | Description |
|-----|-----------|-------------|
| 0 | `SE_CREATE_TOKEN` | Create Token objects |
| 1 | `SE_TCB` | Act as part of the OS |
| 2 | `SE_LOAD_DRIVER` | Load/unload device drivers |
| 3 | `SE_SHUTDOWN` | Shut down the system |
| 4 | `SE_DEBUG` | Debug processes |
| 5 | `SE_SYSTEM_ENVIRONMENT` | Modify firmware env vars |
| 6 | `SE_CHANGE_NOTIFY` | Receive directory change notifications |
| 7 | `SE_BACKUP` | Back up files/directories |
| 8 | `SE_RESTORE` | Restore files/directories |
| 9 | `SE_TAKE_OWNERSHIP` | Take ownership of objects |
| 10 | `SE_INCREASE_QUOTA` | Increase process working set |
| 11 | `SE_MANAGE_VOLUME` | Manage disk volumes |

**Privilege enforcement:** Every operation that requires privilege must call `token.has_privilege(bit)` before proceeding. The privilege check is added alongside the existing ACL check.

### 10.5 Admin Bypass

Admin bypass (from current `SeAccessCheck`) is preserved: admin tokens skip all DACL checks. This matches NT behavior (administrators have `SeTakeOwnershipPrivilege` and `SeBackupPrivilege` which effectively bypass DACLs).

---

## 11. API Design

### 11.1 ABI Changes

No new syscalls needed. All operations go through the existing Ob API:

| Action | API | Notes |
|--------|-----|-------|
| Create session | `sy_ob_create(Session)` | Creates Session object, returns fd |
| Query session | `sy_ob_query_info(SessionInfo)` | Returns session state/data |
| Query token | `sy_ob_query_info(TokenInfo)` | Returns process token info |
| Change password | `sy_ob_set_info(ChangePassword)` | Changes SAM password |
| Lock session | `sy_ob_set_info(SessionLock)` | Locks interactive session |
| Logoff | `sy_ob_set_info(SessionLogoff)` | Terminates session |
| List sessions | `sy_ob_enum(\Session\)` | Enumerate session objects |

**Rationale:** No need for `sys_login`, `sys_logout`, etc. The Ob API (RAX 60-66) is already a generic "operate on objects" mechanism. Adding ObType::Session and a few ObInfoClass/ObSetInfoClass variants is sufficient.

### 11.2 libneodos Wrappers

```rust
// Session management:
pub fn session_create(sid: &Sid, token: &Token) -> Result<i64, i64>;
pub fn session_query_info(fd: i64) -> Result<SessionInfo, i64>;
pub fn session_lock(fd: i64) -> Result<(), i64>;
pub fn session_logoff(fd: i64) -> Result<(), i64>;

// User management:
pub fn user_create(username: &str, password: &str, flags: UserFlags) -> Result<(), i64>;
pub fn user_delete(sid: &Sid) -> Result<(), i64>;
pub fn user_list() -> Result<Vec<UserEntry>, i64>;
pub fn user_change_password(old: &str, new: &str) -> Result<(), i64>;

// Process token query:
pub fn token_query_info() -> Result<TokenInfo, i64>;
pub fn token_set_integrity_level(fd: i64, level: IntegrityLevel) -> Result<(), i64>;
```

### 11.3 Error Codes

| Error | Value | Meaning |
|-------|-------|---------|
| `EACCES` | -4 | Access denied |
| `ENOENT` | -2 | User/session not found |
| `EEXIST` | -5 | User already exists |
| `EPERM` | -3 | Insufficient privilege |
| `EAUTH` | -16 | Authentication failed |
| `EPWDMIN` | -17 | Password too short |
| `EACCOUNT` | -18 | Account disabled/locked |
| `ESESSION` | -19 | Session limit reached |

---

## 12. Test Plan

### 12.1 SAM & Authentication

| Test | Description |
|------|-------------|
| `sam_create_user_admin` | Create admin user, verify SID |
| `sam_create_user_duplicate` | Duplicate username returns EEXIST |
| `sam_authenticate_correct` | Correct password returns Ok |
| `sam_authenticate_wrong` | Wrong password returns EAUTH |
| `sam_authenticate_disabled` | Disabled user returns EACCOUNT |
| `sam_password_change` | Change password, verify new hash |
| `sam_serialize_roundtrip` | Save/load SAM, verify all entries |
| `sam_max_users_64` | Create 64 users, 65th fails |
| `sam_delete_user` | Delete user, authenticate fails |

### 12.2 Session Management

| Test | Description |
|------|-------------|
| `session_create` | Create session, verify session_id |
| `session_create_duplicate` | Same user, new session, different id |
| `session_query_info` | Query via ObInfoClass::SessionInfo |
| `session_lock_unlock` | Lock session, verify state change |
| `session_logoff_kills_processes` | Logoff terminates session processes |
| `session_enum_all` | `ob_enum(\Session\)` lists all sessions |

### 12.3 NeoFS Permission Enforcement

| Test | Description |
|------|-------------|
| `neofs_owner_read_own_file` | Owner can read own file |
| `neofs_owner_write_own_file` | Owner can write own file |
| `neofs_owner_delete_own_file` | Owner can delete own file |
| `neofs_other_read_public_file` | Other can read world-readable file |
| `neofs_other_write_denied` | Other cannot write without permission |
| `neofs_admin_bypass` | Admin can read any file |
| `neofs_create_default_permissions` | New file gets correct default perms |
| `neofs_dir_owner_can_create_child` | Owner can create in own dir |

### 12.4 Registry Security

| Test | Description |
|------|-------------|
| `cm_sec_key_creation_assigns_owner` | New key gets creator's SID |
| `cm_sec_access_granted` | User can read their own key |
| `cm_sec_access_denied` | User cannot write admin key |
| `cm_sec_admin_bypass` | Admin can write any key |
| `cm_sec_inheritance` | Child key inherits parent SD |

### 12.5 Integrity Levels

| Test | Description |
|------|-------------|
| `integrity_medium_writes_low` | Medium can write to Low object |
| `integrity_medium_writes_high_denied` | Medium cannot write to High object |
| `integrity_high_writes_medium` | High can write to Medium object |
| `integrity_low_reads_high` | Low can read High object (no "no read up") |
| `integrity_drop_level` | Process can lower its own IL |

### 12.6 User Commands

| Test | Description |
|------|-------------|
| `whoami_prints_username` | WHOAMI returns current username |
| `login_success_creates_session` | LOGIN with correct password creates session |
| `login_failure_no_session` | LOGIN with wrong password returns EAUTH |
| `passwd_change_and_verify` | PASSWD changes password, new one works |
| `runas_different_user` | RUNAS spawns process with target token |
| `sudo_requires_password` | Privilege escalation requires auth |

---

## 13. Alternatives Considered

### 13.1 Unix-style uid/gid model

| Aspect | Unix | NeoDOS (NT-like) | Decision |
|--------|------|-------------------|----------|
| Identity | u16/u32 integer | SID (variable-length) | SID chosen — already exists, extensible |
| Groups | gid_t, max 16 groups | Vec<Sid>, unlimited | Vec chosen — NT model, SAM already supports |
| Filesystem | uid/gid in inode | SID in DirEntryV2 | SID chosen — consistent with rest of system |
| Permissions | rwxrwxrwx (9 bits) | DACL with ACEs | DACL chosen — more expressive, NT standard |
| Elevation | setuid bit | Token with privileges | Token chosen — SE_* privileges already exist |

**Why not Unix:** NeoDOS already has NT-style Token/SID/ACL/SeAccessCheck. Adding a parallel uid/gid system would create an incompatible hybrid. The investment is in wiring what exists.

### 13.2 Flat user database vs Ob namespace for users

| Approach | Pros | Cons |
|----------|------|------|
| SAM (flat) | Simple, already exists, binary persistence | No handles/refcounting, no namespace queries |
| ObType::User | ob_open/query/enum work naturally | Cluttered namespace, lifecycle complexity |

**Decision:** SAM for storage, `ob_open(User)` → query SAM by SID. Users are not persistent namespace objects.

### 13.3 Per-thread vs per-process token

| Approach | Pros | Cons |
|----------|------|------|
| Per-process (chosen) | Simpler, matches NT | Thread impersonation requires explicit API |
| Per-thread | Enables fine-grained impersonation | Complexity, most threads don't need it |

**Decision:** Per-process token. Thread impersonation can be added later via `ObSetInfoClass::ThreadImpersonate` if needed.

### 13.4 Capability-based vs ACL-based security

| Approach | Pros | Cons |
|----------|------|------|
| ACL-based (chosen) | Matches NT, discretionary, owner-managed | Complex ACL evaluation, canonical ordering required |
| Capability-based | Simpler delegation, no ambient authority | Requires all objects to carry caps, incompatible with existing code |

**Decision:** ACL-based. NeoDOS already has ACE/ACL/SecurityDescriptor code. Capabilities could be added as an additional layer (e.g., for driver capabilities) but the primary security model is ACL-based.

---

## 14. Affected Components

| Subsystem | Change | Complexity |
|-----------|--------|------------|
| `src/security/sam.rs` | Wire to Registry persistence, add user CRUD syscalls, password policy | M |
| `src/security/token.rs` | Add integrity_level, session_ob_id, creation_time | S |
| `src/security/access.rs` | Add group checking to SeAccessCheck, fix empty DACL semantics | S |
| `src/security/acl.rs` | Add integrity level to SecurityDescriptor | S |
| `src/security/mod.rs` | Add USR-001..024 test registration | S |
| `src/object/types.rs` | Add ObType::Session=19, ObInfoClass variants 24-28, ObSetInfoClass variants 28-32 | S |
| `src/syscall/ob.rs` | Add Session handler, extend Process/Thread info with token data, VFS permission checks, token query, integrity level lowering | XL |
| `src/fs/neodos_dir.rs` | Extend DirEntryV2 with owner_sid, backward compat | M |
| `src/fs/neodos_v2.rs` | Write/read owner_sid, superblock feature flag | M |
| `src/fs/vfs.rs` | Add permission check in VFS operations, default perms by extension | L |
| `src/cm/hive.rs` | Wire sec_desc_cell, default SD on key creation | M |
| `src/cm/security.rs` | New file: Registry ACL checking | M |
| `src/cm/mod.rs` | Add user profile hive, SAM persistence | M |
| `src/scheduler/mod.rs` | Token-based spawn, session tracking, integrity inheritance | M |
| `src/syscall/permission.rs` | Add SE_* privilege enforcement to admin-only syscalls | S |
| `src/globals.rs` | Add SESSION_MANAGER global | S |
| `src/main.rs` | Init SESSION_MANAGER, default SAM creation | S |
| `libneodos/src/syscall.rs` | Add wrappers for session/user/token operations | M |
| `userbin/neologon/` | New: login binary | M |
| `userbin/neoshell/` | Add WHOAMI, basic identity display | S |

---

## 15. Risks

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| DirEntryV2 format change breaks existing volumes | Medium | High | Superblock feature flag, backward compat read |
| SAM password hashing security insufficient | Low | Medium | SHA-256 + 32-byte salt is adequate for v0.5x; migrate to Argon2 in v1.0 |
| Session management adds complexity to process lifecycle | Medium | Medium | Sessions are lightweight wrappers; processes can exist without sessions (backward compat) |
| ACL evaluation performance on file-heavy workloads | Low | Medium | ACL evaluation is O(n_aces); most files have 2-3 ACEs. Cache if needed. |
| Permission enforcement breaks existing scripts/tools | Medium | Medium | Default: files created with world-read/write during transition. Gradual lockdown. |
| NeoInit becomes a security boundary | Low | High | NeoInit runs as SYSTEM. Services are SYSTEM. Only interactive shell runs as user. |

---

## 16. Implementation Roadmap

### Phase 1 — Foundation (v0.50.x)

**Goal:** SAM wired, tokens work, no permission enforcement yet.

| Step | Files | Description |
|------|-------|-------------|
| 1.1 | `src/security/sam.rs` | Wire SAM persistence to `\Registry\Machine\SAM` via VFS |
| 1.2 | `src/security/access.rs` | Fix empty DACL semantics (empty = deny), add group SID checking |
| 1.3 | `src/security/token.rs` | Add `integrity_level`, `creation_time` fields |
| 1.4 | `src/security/mod.rs` | Add built-in user creation (Administrator, Guest) in `init_security()` |
| 1.5 | `src/object/types.rs` | Add `Session = 19` to ObType |
| 1.6 | `src/main.rs` | Init default users before spawning NeoInit |

### Phase 2 — Sessions (v0.51.x)

**Goal:** Sessions work, login possible.

| Step | Files | Description |
|------|-------|-------------|
| 2.1 | `src/syscall/ob.rs` | Handler for `ob_create(Session)` — create session object, allocate session_id |
| 2.2 | `src/syscall/ob.rs` | Handler for `ObInfoClass::SessionInfo` (24) |
| 2.3 | `src/syscall/ob.rs` | Handler for `ObSetInfoClass::SessionLock/Logoff` (28-29) |
| 2.4 | `src/syscall/ob.rs` | Handler for `ObSetInfoClass::ChangePassword` (31) |
| 2.5 | `src/globals.rs` | Add `SESSION_MANAGER: Mutex<SessionManager>` |
| 2.6 | `libneodos/src/syscall.rs` | Add session/CRUD wrappers |
| 2.7 | `userbin/neologon/` | New binary: login prompt, SAM authentication, session creation |
| 2.8 | `userbin/neoinit/` | Spawn neologon instead of shell directly |

### Phase 3 — Filesystem Security (v0.52.x)

**Goal:** Files have owners, permissions enforced.

| Step | Files | Description |
|------|-------|-------------|
| 3.1 | `src/fs/neodos_dir.rs` | Add `owner_sid: Sid` to DirEntryV2 |
| 3.2 | `src/fs/neodos_v2.rs` | Superblock `FEATURE_OWNER_SID` flag, backward compat read |
| 3.3 | `src/fs/vfs.rs` | Add `check_vfs_access()` function |
| 3.4 | `src/syscall/ob.rs` | Wire permission checks in VFS operations |
| 3.5 | `src/fs/vfs.rs` | Default permissions by file extension |

### Phase 4 — Registry Security (v0.53.x)

**Goal:** Registry keys have owners, ACLs enforced.

| Step | Files | Description |
|------|-------|-------------|
| 4.1 | `src/cm/hive.rs` | Default `sec_desc_cell` on key creation |
| 4.2 | `src/cm/security.rs` | New: `cm_check_access()` for registry ACL checking |
| 4.3 | `src/cm/mod.rs` | Wire ACL check in all Cm syscall handlers |
| 4.4 | `src/cm/mod.rs` | User profile hive at `\Registry\User\{sid}` |

### Phase 5 — Integrity & Privilege (v0.54.x)

**Goal:** MIC and privilege enforcement active.

| Step | Files | Description |
|------|-------|-------------|
| 5.1 | `src/security/access.rs` | Add integrity level check to SeAccessCheck |
| 5.2 | `src/syscall/ob.rs` | Handler for `ObSetInfoClass::SetIntegrityLevel` (32) — can only lower |
| 5.3 | `src/syscall/permission.rs` | Wire `has_privilege()` checks in all admin-only syscalls |
| 5.4 | `src/syscall/ob.rs` | Handler for `ObInfoClass::TokenInfo` (28) |

### Phase 6 — Tools (v0.55.x)

**Goal:** User-facing commands work.

| Step | Files | Description |
|------|-------|-------------|
| 6.1 | `userbin/neoshell/` | Add `WHOAMI` command |
| 6.2 | `userbin/neoshell/` | Add `PASSWD` command |
| 6.3 | `userbin/neoshell/` | Add `WHO` command |
| 6.4 | `userbin/neoshell/` | Add `LOGOFF` command |
| 6.5 | `userbin/neoshell/` | Add `SU` command |
| 6.6 | `userbin/neoshell/` | Add `RUNAS` command (requires admin password) |

---

## 17. Recommendations for Clean Architecture

1. **SAM is the authority.** Never cache user data outside SAM. SAM is serialized to `\Registry\Machine\SAM` on every mutation.

2. **Session is the runtime boundary.** Processes belong to sessions. Sessions have tokens. Tokens have SIDs. SIDs are resolved through SAM.

3. **No new syscalls.** All operations go through the existing Ob API (RAX 60-66). Session management is `ob_create(Session)`, `ob_set_info(SessionLock)`, etc.

4. **Backward compatibility by default.** Old processes without sessions get a default "Session 0" (SYSTEM). Old NE2 volumes without owner_sid get a default "unknown" owner. Permissions default to world-access until explicitly locked down.

5. **Integrity Levels before ACLs for most objects.** MIC is simpler to implement and provides better protection against common attacks (e.g., a compromised text editor can't modify system files). ACLs are for fine-grained user administration.

6. **Token filtering at login.** When creating a user token from SAM, only enable the privileges the user needs. Admin users get all privileges. Standard users get `SE_CHANGE_NOTIFY` only.

7. **Session 0 isolation.** System services run in Session 0 with SYSTEM token. Interactive user sessions run in Session 1+. No process in Session 1+ can send to or modify Session 0 without explicit privilege.

8. **No setuid.** NT-style token elevation is more secure: create a new process with the target token rather than changing the identity of the current process.
