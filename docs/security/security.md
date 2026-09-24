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
| `sid_builtin_admin()` | `S-1-5-18` | NT AUTHORITY\SYSTEM (kernel/Idle/NeoInit) |
| `sid_builtin_user()` | `S-1-5-21-0-0-0-1000` | Default domain user |

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
}
```

### Factory Methods

| Method | Privileges | Description |
| -------- | ----------- | ------------- |
| `new_admin()` | `SE_ADMIN_PRIVILEGES` (0xFFFF) | Full-privilege token for SYSTEM |
| `new_user()` | `SE_CHANGE_NOTIFY` only | Restricted user token |
| `new_full(sid, is_admin, groups, privs, sid)` | Custom | Complete construction |
| `inherit_from(parent)` | Inherited | Copies sid, is_admin, groups, privileges, session_id |

`is_admin_token()` returns true when `is_admin` is set or SID equals `sid_builtin_admin()`.

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

## ACL (Access Control List)

File: `src/security/acl.rs`.

```rust
pub struct Ace {
    pub ace_type: u8,         // 0=ALLOW, 1=DENY
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
    pub owner: Sid,
    pub group: Sid,
    pub dacl: Acl,            // discretionary ACL
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

1. **Admin bypass**: if `token.is_admin` and the requested access includes admin-only rights, grant access immediately.
2. **Deny-by-default**: an empty DACL (or one with no matching Allow ACE) returns denied.
3. **Iteration order**: evaluate all Deny ACEs first. If any Deny ACE matches the token's SID or any group SID and covers the requested access, deny.
4. **Allow ACEs**: if a matching Allow ACE covers all requested access bits, grant.
5. **Fallback**: if no Allow ACE matches, deny.

Signature:

```rust
pub fn se_access_check(
    token: &Token,
    sd: &SecurityDescriptor,
    desired_access: u32,
) -> Result<(), ()>
```

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

23 tests covering:

- SID format/parse, builtin construction, equality
- Token: new_admin, new_user, inherit, group membership
- ACL: insert canonical order, allow/deny evaluation
- SeAccessCheck: admin bypass, empty DACL, specific ACE matching, group-based access
- SAM database: 12 tests including create, add user, find by username, find by SID, remove user, flag manipulation, parse roundtrip, magic number validation, truncation detection, max entries enforcement
