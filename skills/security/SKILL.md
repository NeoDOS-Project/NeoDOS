---
name: security
description: Implement security primitives: SID, Token, ACL, SAM, SeAccessCheck
---

# Security Reference Monitor

## When to use

Adding or modifying security primitives (SID, Token, ACL,
SecurityDescriptor), implementing access checks, working with the SAM database,
hooking security into syscalls or Ob operations, or changing token lifecycle.

## Goal

Correctly implement NT6-style security with proper SID construction, token
inheritance, canonical ACL ordering, `SeAccessCheck` semantics, and SAM
persistence.

## References

- `docs/security/security.md` — subsystem documentation
- `src/security/sid.rs` — SID construction/parsing/formatting
- `src/security/token.rs` — `Token`, factories, inheritance, privilege constants
- `src/security/acl.rs` — `Ace`, `Acl`, `SecurityDescriptor`, access constants
- `src/security/access.rs` — `se_access_check`, `se_access_check_sid`
- `src/security/sam.rs` — `SamDatabase`, `SamEntry`, binary serialization
- `src/security/mod.rs` — initialization, `register_security_tests()`
- `src/syscall/mod.rs` — `check_syscall_permission`, `SYSCALL_PERMISSIONS`
- `src/syscall/permission.rs` — `SyscallPermission { caps, ring_min, admin }`

## SID (`src/security/sid.rs`)

Format `S-R-I-S*` (revision, identifier authority, sub-authorities).

| Built-in | SID string | Usage |
| ---------- | ----------- | ------- |
| `sid_builtin_admin()` | `S-1-5-18` | NT AUTHORITY\SYSTEM (kernel/Idle/NeoInit) |
| `sid_builtin_user()` | `S-1-5-21-0-0-0-1000` | Default domain user |

Construct with `Sid::from_parts(revision: u8, authority: &[u8; 6],
sub_authorities: &[u32])`; format with `format_string()`.

## Token (`src/security/token.rs`)

Attached to every `EPROCESS` via `token: Token`.

```rust
pub struct Token {
    pub sid: Sid,
    pub is_admin: bool,
    pub groups: Vec<Sid>,
    pub privileges: u64,   // 12-bit privilege bitmap
    pub session_id: u32,
}
```

| Factory | Privileges | Description |
| --------- | ----------- | ------------- |
| `Token::new_admin()` | `SE_ADMIN_PRIVILEGES` (0xFFFF) | Full-privilege token for SYSTEM |
| `Token::new_user()` | `SE_USER_PRIVILEGES` (`SE_CHANGE_NOTIFY_PRIVILEGE`) | Restricted user token |
| `Token::new_full(sid, is_admin, groups, privileges, session_id)` | custom | Complete construction |
| `Token::inherit_from(parent)` | inherited | Copies sid, is_admin, groups, privileges, session_id |

`is_admin_token()` returns true when `is_admin` is set or the SID is
`sid_builtin_admin()`.

### Privilege bitmap (12 bits, `u64`)

| Bit | Constant |
| ----- | ---------- |
| 0 | `SE_CREATE_TOKEN_PRIVILEGE` |
| 1 | `SE_TCB_PRIVILEGE` |
| 2 | `SE_LOAD_DRIVER_PRIVILEGE` |
| 3 | `SE_SHUTDOWN_PRIVILEGE` |
| 4 | `SE_DEBUG_PRIVILEGE` |
| 5 | `SE_SYSTEM_ENVIRONMENT_PRIVILEGE` |
| 6 | `SE_CHANGE_NOTIFY_PRIVILEGE` (user default) |
| 7 | `SE_BACKUP_PRIVILEGE` |
| 8 | `SE_RESTORE_PRIVILEGE` |
| 9 | `SE_TAKE_OWNERSHIP_PRIVILEGE` |
| 10 | `SE_INCREASE_QUOTA_PRIVILEGE` |
| 11 | `SE_MANAGE_VOLUME_PRIVILEGE` |

`SE_ADMIN_PRIVILEGES = 0xFFFF`. Privilege checks are a bitmap AND:
`token.privileges & required != 0`.

## ACL / SecurityDescriptor (`src/security/acl.rs`)

```rust
pub struct Ace { pub ace_type: u8, pub flags: u8, pub access_mask: u32, pub sid: Sid }
pub struct Acl { pub revision: u8, pub aces: Vec<Ace> }
pub struct SecurityDescriptor {
    pub revision: u8,
    pub owner: Option<Sid>,
    pub group: Option<Sid>,
    pub dacl: Option<Acl>,   // None = NULL DACL = full access
    pub sacl: Option<Acl>,   // audit ACEs only; None = no auditing
}
```

ACE types: `ACE_TYPE_ACCESS_ALLOWED(0)`, `ACE_TYPE_ACCESS_DENIED(1)`,
`ACE_TYPE_SYSTEM_AUDIT(2)`.
Access constants: `ACCESS_READ(1)`, `ACCESS_WRITE(2)`, `ACCESS_EXECUTE(4)`,
`ACCESS_DELETE(8)`, `ACCESS_ALL(0xFFFF)`.

Helpers: `Ace::allow(sid, mask)` / `Ace::deny(sid, mask)`; `Acl::new()`,
`Acl::insert_ace_canonical(ace)` (all Deny before Allow), `Acl::is_empty()`;
`SecurityDescriptor::new().with_dacl(acl).with_sacl(sacl)`. Also
`set_auditing(bool)` / `auditing_enabled()` and `se_audit(...)` for SACL
auditing.

## SeAccessCheck (`src/security/access.rs`)

```rust
pub fn se_access_check(
    token: &Token,
    sd: Option<&SecurityDescriptor>,
    desired_access: u32,
) -> bool;
```

Algorithm (code is truth):

1. **Admin bypass** — if `token.is_admin_token()` → grant.
2. **Absent descriptor** — `None` SD → grant (unprotected).
3. **NULL DACL** — `Some` SD with `dacl == None` → grant (full access).
4. **Empty DACL** — `Some` with no ACEs → **deny all**.
5. **Deny-first** — a matching Deny ACE covering the requested bits → deny.
6. **Allow** — a matching Allow ACE covering all requested bits → grant.
7. **Fallback** — otherwise deny.

ACE trustees match the token's primary SID **and** every SID in `token.groups`.
`se_audit()` emits SACL audit records when audit mode is enabled, without
affecting the decision.

## SAM (`src/security/sam.rs`)

Flat-file account database (max 64 entries), binary format with magic `"SAM\0"`.

```rust
pub struct SamDatabase { pub entries: Vec<SamEntry> }
pub struct SamEntry {
    pub username: [u8; 32],
    pub sid: Sid,
    pub flags: u32,
    pub full_name: [u8; 64],
    pub comment: [u8; 64],
}
```

Flags: `SAM_FLAG_ADMIN(1)`, `SAM_FLAG_DISABLED(2)`, `SAM_FLAG_LOCKED(4)`.
API: `SamDatabase::new()`, `add_user(entry)` / `SamEntry::new(username, sid,
is_admin)`, `remove_user(name)`, `find_by_username(name)` (case-insensitive),
`find_by_sid(sid)`, `serialize_sam(&db)`, `parse_sam(&bytes)`.

## Steps

### 1. Build a SID

```rust
let sid = Sid::from_parts(1, &[0, 0, 0, 0, 0, 5], &[21, 0, 0, 0, 1001]);
let s = sid.format_string(); // "S-1-5-21-0-0-0-1001"
```

### 2. Create or inherit a token

```rust
let admin = Token::new_admin();      // PID 0 / NeoInit
let user  = Token::new_user();       // unprivileged processes
let child = Token::inherit_from(&parent_token); // at process spawn
```

### 3. Build a descriptor with a canonical ACL

```rust
let user = sid_builtin_user();
let mut acl = Acl::new();
acl.insert_ace_canonical(Ace::allow(user, ACCESS_READ | ACCESS_WRITE));
acl.insert_ace_canonical(Ace::deny(user, ACCESS_DELETE));
let sd = SecurityDescriptor::new().with_dacl(acl);
```

### 4. Run an access check

```rust
if se_access_check(&process_token, Some(&sd), ACCESS_READ) {
    // grant
}
```

### 5. Group membership

```rust
token.add_group(sid_builtin_admin());
assert!(token.is_in_group(sid_builtin_admin()));
```

Group SIDs are consulted by `se_access_check`: an Allow/Deny ACE whose trustee is
any of `token.groups` matches, so a group Allow can grant and a group Deny wins
over a primary-SID Allow.

### 5b. SACL / auditing

```rust
let mut sacl = Acl::new();
sacl.add_ace(Ace { ace_type: ACE_TYPE_SYSTEM_AUDIT, flags: 0,
                   access_mask: ACCESS_READ, sid: sid_builtin_user() });
let sd = SecurityDescriptor::new().with_dacl(acl).with_sacl(sacl);

set_auditing(true);   // enable audit logging
// se_access_check(...) now emits an AUDIT record for matching audit ACEs
```

Auditing never changes the access decision and an absent SACL is a no-op.

### 6. Hook security into syscalls

Admin syscalls are gated by `SYSCALL_PERMISSIONS` and checked centrally:

```rust
// src/syscall/mod.rs
pub fn check_syscall_permission(num: u64, is_admin: bool) -> Result<(), u64>;
```

Set `t[N] = SyscallPermission::admin()` when adding an admin-only syscall (see the
syscalls skill).

### 7. Integrate with the Object Manager

`ob_open` runs `se_access_check` against the object's DACL using the caller's
token (`current_process().token`). When adding a new object type/operation, make
sure the access check is reached before the operation executes.

### 8. Work with the SAM database

```rust
let mut sam = SamDatabase::new();
let entry = SamEntry::new("admin", sid_builtin_admin(), true);
sam.add_user(entry).unwrap();
let bytes = serialize_sam(&sam).unwrap();
let parsed = parse_sam(&bytes).unwrap();
```

## Best practices

- Always use `insert_ace_canonical()` — manual `push`/`add_ace` breaks Deny-first
  ordering.
- Empty DACL denies all; NULL DACL (and an absent descriptor) grants full access.
  Be explicit about which one you want.
- ACEs are matched against the primary SID and all group SIDs.
- Tokens are copied at spawn; mutating a parent token does not affect children.
- SAM usernames are case-insensitive and zero-padded to 32 bytes.
- Session IDs isolate terminal sessions (per-VT login context).

## Common mistakes

- Using `Acl::add_ace()` instead of `insert_ace_canonical()`.
- Assuming `se_access_check` takes `&SecurityDescriptor` — it takes
  `Option<&SecurityDescriptor>`.
- Assuming group SIDs do not participate — `se_access_check` matches
  `token.sid` **and** `token.groups`.
- Expecting an empty DACL to grant — an empty DACL denies all (use a NULL DACL
  for full access).
- Forgetting `inherit_from()` at process spawn.
- Adding an admin syscall without setting its `SyscallPermission::admin()`.
- SAM (de)serialization dropping the 4-byte alignment padding.

## Final checklist

- [ ] SID constructed correctly (revision, authority, sub-authorities)
- [ ] Token created with the proper privileges (admin vs. user)
- [ ] Token inherited at process spawn
- [ ] ACL built with `insert_ace_canonical()`
- [ ] `SeAccessCheck` behavior matches the code (admin bypass; absent SD / NULL
      DACL grant; empty DACL denies; deny-first; allow; fallback deny)
- [ ] Group SIDs and (if used) the SACL audit hook behave as expected
- [ ] SAM serialize/parse round-trips
- [ ] Admin syscalls flagged in `SYSCALL_PERMISSIONS`
- [ ] Ob integration: access check on open/create
- [ ] Tests added; `register_security_tests()` wired; `neodev test` passes
- [ ] `docs/security/security.md` updated
