# Security Reference Monitor

NT6-style security subsystem: SID, Token, SAM, ACL, and SeAccessCheck.

## SID

File: `src/security/sid.rs`. Format: `S-R-I-S*` (revision, identifier authority, sub-authorities).

```rust
pub struct Sid {
    pub revision: u8,                    // typically 1 (SID_REVISION)
    pub sub_authority_count: u8,         // number of sub-authorities (1-8)
    pub identifier_authority: [u8; 6],   // big-endian 48-bit authority value
    pub sub_authorities: [u32; 8],       // sub-authority values (RID)
}
```

### Built-in SIDs

| Name | SID String | Usage |
|------|------------|-------|
| `sid_builtin_system()` / `sid_builtin_admin()` | `S-1-5-18` | NT AUTHORITY\SYSTEM (kernel/Idle/NeoInit) |
| `sid_builtin_administrator()` | `S-1-5-21-0-0-0-500` | Built-in Administrator account |
| `sid_builtin_guest()` | `S-1-5-21-0-0-0-501` | Built-in Guest account |
| `sid_builtin_user()` | `S-1-5-21-0-0-0-1000` | Default domain user |

`WELL_KNOWN_SIDS` (the `SeWellKnownSids` table) maps the canonical names
(`SYSTEM`, `Administrator`, `Guest`, `Users`) to SIDs; look them up with
`well_known_sid(name)` (case-insensitive). Domain RIDs are exposed as
`RID_ADMINISTRATOR(500)`, `RID_GUEST(501)`, `RID_USER(1000)`, with
`sid_builtin_domain()` = `S-1-5-21-0-0-0`.

`format_string()` produces the human-readable `S-R-I-S*` format. `from_parts()` constructs a Sid from raw components.

## Token

File: `src/security/token.rs`. Attached to every `EPROCESS` via the `token: Token` field.

```rust
pub struct Token {
    pub sid: Sid,
    pub is_admin: bool,          // admin bypass flag
    pub groups: Vec<Sid>,        // group memberships
    pub privileges: u64,         // 12-bit privilege bitmap
    pub session_id: u32,         // terminal session identifier
    pub integrity_level: IntegrityLevel, // Mandatory Integrity Control
    pub creation_time: u64,      // TSC ticks at creation
}
```

### Integrity levels (`IntegrityLevel`, `#[repr(u8)]`)

| Level | Value | Typical |
| ------- | ------- | --------- |
| `Untrusted` | 0 | untrusted |
| `Low` | 1 | sandboxed |
| `Medium` | 2 | standard user |
| `High` | 3 | elevated |
| `System` | 4 | admin / kernel |

Admin tokens default to `System`, user tokens to `Medium`; `inherit_from` keeps
the parent's level, and each token gets a fresh `creation_time`. (Enforcement in
`SeAccessCheck` is USR-P5a.)

### Factory Methods

| Method | Privileges | Integrity | Description |
| -------- | ----------- | ----------- | ------------- |
| `new_admin()` | `SE_ADMIN_PRIVILEGES` (0xFFFF) | System | Full-privilege token for SYSTEM |
| `new_user()` | `SE_CHANGE_NOTIFY` only | Medium | Restricted user token |
| `new(sid, is_admin)` | by flag | System/Medium | Simple construction |
| `new_full(sid, is_admin, groups, privs, session_id)` | Custom | System/Medium | Complete construction |
| `inherit_from(parent)` | Inherited | Inherited | Copies identity; fresh `creation_time` |

`is_admin_token()` returns `is_admin`.

## SAM (Security Account Manager)

File: `src/security/sam.rs`. Flat-file database of user accounts.

```rust
pub struct SamDatabase {
    pub entries: Vec<SamEntry>,    // max 64 entries
}

pub struct SamEntry {
    pub username: [u8; 32],       // null-terminated, case-insensitive lookup
    pub sid: Sid,                  // user's security identifier
    pub flags: u32,                // SAM_FLAG_ADMIN(1), SAM_FLAG_DISABLED(2), SAM_FLAG_LOCKED(4)
    pub full_name: [u8; 64],      // display name
    pub comment: [u8; 64],        // description
}
```

### Binary Format

```text
Header:
  magic:    "SAM\0" (4 bytes)
  version:  u32 LE
  count:    u32 LE

Entry (repeated count times):
  username_len:  u16 LE
  username:      [u8; username_len]  + padding to 4 bytes
  sid_revision:  u8
  sid_count:     u8
  sid_auth:      [u8; 6]
  sid_subs:      [u32; sid_count]    + padding to 4 bytes
  flags:         u32 LE
  fullname_len:  u16 LE
  fullname:      [u8; fullname_len]  + padding to 4 bytes
  comment_len:   u16 LE
  comment:       [u8; comment_len]   + padding to 4 bytes
```

`parse_sam(data)` deserializes from bytes. `serialize_sam(db)` produces bytes for disk persistence.

### Built-in accounts

`SamDatabase::with_builtins()` seeds the NT built-in accounts:

| Account | SID | Flags |
| --------- | ----- | ------- |
| `Administrator` | `S-1-5-21-0-0-0-500` | admin, enabled |
| `Guest` | `S-1-5-21-0-0-0-501` | disabled |
| `SYSTEM` | `S-1-5-18` | admin |

At boot, `init_security()` (Phase 2.77) creates the global `SAM_DB`
(`spin::Mutex<SamDatabase>`) with these built-ins. Persistence to
`\Registry\Machine\SAM` is USR-P1c.

## ACL (Access Control List)

File: `src/security/acl.rs`.

```rust
pub struct Ace {
    pub ace_type: u8,         // 0=ALLOW, 1=DENY, 2=SYSTEM_AUDIT
    pub flags: u8,            // inheritance flags
    pub access_mask: u32,     // rights bitmap
    pub sid: Sid,             // trustee
}

pub struct Acl {
    pub revision: u8,         // ACL_REVISION (2)
    pub aces: Vec<Ace>,       // ordered list
}

pub struct SecurityDescriptor {
    pub revision: u8,
    pub owner: Option<Sid>,
    pub group: Option<Sid>,
    pub dacl: Option<Acl>,    // discretionary ACL (None = NULL DACL = full access)
    pub sacl: Option<Acl>,    // system ACL (audit ACEs only; None = no auditing)
}
```

### Access Constants

| Constant | Value | Bit |
| ---------- | ------- | ----- |
| `ACCESS_READ` | 1 | 0 |
| `ACCESS_WRITE` | 2 | 1 |
| `ACCESS_EXECUTE` | 4 | 2 |
| `ACCESS_DELETE` | 8 | 3 |
| `ACCESS_ALL` | 0xFFFF | All lower 16 bits |

`insert_ace_canonical()` enforces NT canonical order: all Deny ACEs before Allow ACEs.

## SeAccessCheck

File: `src/security/access.rs`. Access validation logic.

### Algorithm

`se_access_check(token, sd, desired_access) -> bool`:

1. **Admin bypass**: if `token.is_admin_token()`, grant immediately.
2. **Absent descriptor** (`sd == None`): unprotected → grant.
3. **NULL DACL** (`sd.dacl == None`): full access → grant.
4. **Empty DACL** (`Some` with zero ACEs): **deny all**.
5. **Deny-first**: evaluate all Deny ACEs first. An ACE matches when its trustee
   SID equals `token.sid` **or any SID in `token.groups`**; a matching Deny that
   covers the requested access → deny.
6. **Allow**: a matching Allow ACE covering all requested bits → grant.
7. **Fallback**: no matching Allow → deny.

Signatures:

```rust
pub fn se_access_check(
    token: &Token,
    sd: Option<&SecurityDescriptor>,
    desired_access: u32,
) -> bool

pub fn se_access_check_sid(        // single SID, no group membership
    token_sid: &Sid,
    is_admin: bool,
    dacl: Option<&Acl>,
    desired_access: u32,
) -> bool
```

### SACL and auditing (`ACE_TYPE_SYSTEM_AUDIT = 2`)

A descriptor may carry a **SACL** (`SecurityDescriptor::sacl`) whose entries are
audit ACEs. `se_audit(token, sd, desired_access, granted)` emits a record when
auditing is enabled (`set_auditing(true)` / `auditing_enabled()`) and a
`SYSTEM_AUDIT` ACE matches the trustee and requested access. Auditing is a
logging hook only — it never changes the access decision, and an absent SACL is a
no-op.

## Token Lifecycle

| PID | Process | Token Source | Privileges |
| ----- | --------- | ------------- | ----------- |
| 0 | Idle (kernel) | `Token::new_admin()` | Full (0xFFFF) |
| 1 | NeoInit | Inherits from PID 0 via `add_ring3_process()` | Full (0xFFFF) |
| N | Child processes | `inherit_from(parent)` at spawn | Parent's privileges |

Group membership: `add_group(sid)` appends to the `groups` vector. `is_in_group(sid)` performs a linear scan.

## Privilege Constants (12 flags, u64 bitmap)

| Bit | Constant | Description |
| ----- | ---------- | ------------- |
| 0 | `SE_CREATE_TOKEN_PRIVILEGE` | Create token objects |
| 1 | `SE_TCB_PRIVILEGE` | Act as part of the OS |
| 2 | `SE_LOAD_DRIVER_PRIVILEGE` | Load/unload device drivers |
| 3 | `SE_SHUTDOWN_PRIVILEGE` | Shut down the system |
| 4 | `SE_DEBUG_PRIVILEGE` | Debug processes |
| 5 | `SE_SYSTEM_ENVIRONMENT_PRIVILEGE` | Modify firmware environment |
| 6 | `SE_CHANGE_NOTIFY_PRIVILEGE` | Receive directory change notifications |
| 7 | `SE_BACKUP_PRIVILEGE` | Back up files/directories |
| 8 | `SE_RESTORE_PRIVILEGE` | Restore files/directories |
| 9 | `SE_TAKE_OWNERSHIP_PRIVILEGE` | Take ownership of objects |
| 10 | `SE_INCREASE_QUOTA_PRIVILEGE` | Increase process working set |
| 11 | `SE_MANAGE_VOLUME_PRIVILEGE` | Manage volume (defrag, etc.) |

Combined: `SE_ADMIN_PRIVILEGES = 0xFFFF` (all 12 bits set). `SE_USER_PRIVILEGES = 1 << 6` (SE_CHANGE_NOTIFY only).

## Source Files

| File | Responsibility |
| ------ | --------------- |
| `src/security/sid.rs` | SID construction, parsing, formatting |
| `src/security/token.rs` | Token creation, inheritance, privilege checks |
| `src/security/sam.rs` | SAM database, binary serialization |
| `src/security/acl.rs` | ACE, ACL, SecurityDescriptor, canonical ordering |
| `src/security/access.rs` | SeAccessCheck implementation |

## Tests

36 tests covering:

- SID format/parse, builtin construction, equality, **well-known SIDs**
  (`SYSTEM`, `Administrator`, `Guest`, `Users`)
- Token: new_admin, new_user, inherit, group membership, **integrity level**
  (admin = System, user = Medium) and **creation_time**
- ACL: insert canonical order, allow/deny evaluation
- SeAccessCheck: admin bypass, empty DACL (deny), NULL DACL / absent SD (grant),
  specific ACE matching, deny-first ordering, **group-SID** allow and deny
- SACL: audit hook is a no-op without a SACL and does not affect the decision
- SAM database: create, add/remove user, find by username/SID, flags, parse
  roundtrip, magic/truncation validation, max entries, **built-in accounts**
  (`Administrator`/`Guest`/`SYSTEM`) and the global `SAM_DB`
