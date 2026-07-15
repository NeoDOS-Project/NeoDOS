---
name: security
description: Implement security policies: SID, Token, ACL, SAM, SeAccessCheck
---

# Security Reference Monitor

## When to use

Adding or modifying security primitives (SID, Token, ACL, SecurityDescriptor), implementing access checks, working with SAM database, hooking security into syscalls or Ob operations, or modifying token lifecycle.

## Goal

Correctly implement NT6-style security with proper SID construction, token inheritance, ACL canonical ordering, SeAccessCheck semantics, and SAM persistence.

## References

- `docs/security.md` — subsystem documentation
- `src/security/sid.rs` — SID construction, parsing, formatting
- `src/security/token.rs` — Token creation, inheritance, privilege checks
- `src/security/acl.rs` — ACE, ACL, SecurityDescriptor, canonical ordering
- `src/security/access.rs` — SeAccessCheck implementation
- `src/security/sam.rs` — SAM database, binary serialization
- `src/security/mod.rs` — initialization, test registration, DEFAULT_ADMIN_TOKEN / DEFAULT_USER_TOKEN
- `src/syscall/mod.rs` — `check_syscall_permission()`, `SYSCALL_PERMISSIONS` table

## Architecture

### SID (`src/security/sid.rs`)

```
S-R-I-S*  format: revision=1, identifier_authority=5 (NT Authority), sub-authorities=RID
```

| Built-in | SID String | Usage |
| ---------- | ----------- | ------- |
| `sid_builtin_admin()` | `S-1-5-18` | NT AUTHORITY\SYSTEM |
| `sid_builtin_user()` | `S-1-5-21-0-0-0-1000` | Default domain user |

### Token (`src/security/token.rs`)

Attached to every `EPROCESS` via `token: Token` field. Created at process spawn.

| Method | Privileges | Description |
| -------- | ----------- | ------------- |
| `new_admin()` | `SE_ADMIN_PRIVILEGES` (0xFFFF) | Full-privilege token for SYSTEM/PID 0/1 |
| `new_user()` | `SE_CHANGE_NOTIFY` only | Restricted user token |
| `inherit_from(parent)` | Inherited | Copies sid, is_admin, groups, privileges, session_id |

Token lifecycle: PID 0 (Idle) → `Token::new_admin()`, PID 1 (NeoInit) → inherits from PID 0, child processes → `inherit_from(parent)`.

### Privilege Bitmap (12 bits, u64)

| Bit | Privilege | Description |
| ----- | ---------- | ------------- |
| 0 | `SE_CREATE_TOKEN` | Create token objects |
| 1 | `SE_TCB` | Act as part of the OS |
| 2 | `SE_LOAD_DRIVER` | Load/unload NEM drivers |
| 3 | `SE_SHUTDOWN` | Shut down system |
| 4 | `SE_DEBUG` | Debug processes |
| 5 | `SE_SYSTEM_ENVIRONMENT` | Modify firmware env |
| 6 | `SE_CHANGE_NOTIFY` (user default) | Directory change notifications |
| 7 | `SE_BACKUP` | Back up files/directories |
| 8 | `SE_RESTORE` | Restore files/directories |
| 9 | `SE_TAKE_OWNERSHIP` | Take ownership of objects |
| 10 | `SE_INCREASE_QUOTA` | Increase working set |
| 11 | `SE_MANAGE_VOLUME` | Manage volume |

`SE_ADMIN_PRIVILEGES = 0xFFFF`, `SE_USER_PRIVILEGES = 1 << 6`.

### ACL / SecurityDescriptor (`src/security/acl.rs`)

```rust
pub struct Ace { ace_type: u8, flags: u8, access_mask: u32, sid: Sid }
pub struct Acl { revision: u8, aces: Vec<Ace> }
pub struct SecurityDescriptor { revision: u8, owner: Sid, group: Sid, dacl: Acl }
```

Access constants: `ACCESS_READ(1)`, `ACCESS_WRITE(2)`, `ACCESS_EXECUTE(4)`, `ACCESS_DELETE(8)`, `ACCESS_ALL(0xFFFF)`.

**Canonical ACE order**: all Deny ACEs first, then all Allow ACEs. Use `insert_ace_canonical()`.

### SeAccessCheck (`src/security/access.rs`)

Algorithm:
1. **Admin bypass**: if `token.is_admin` and requested access has admin-only rights → grant.
2. **Deny-by-default**: empty DACL → deny.
3. **Deny ACEs first**: any matching Deny ACE covering requested access → deny.
4. **Allow ACEs**: matching Allow ACE covering all requested bits → grant.
5. **Fallback**: no matching Allow → deny.

Signature: `pub fn se_access_check(token: &Token, sd: Option<&SecurityDescriptor>, desired_access: u32) -> bool`

### SAM (`src/security/sam.rs`)

Flat-file database, max 64 entries. Binary format with magic `"SAM\0"`.

```rust
pub struct SamDatabase { entries: Vec<SamEntry> }
pub struct SamEntry { username: [u8; 32], sid: Sid, flags: u32, full_name: [u8; 64], comment: [u8; 64] }
```

Flags: `SAM_FLAG_ADMIN(1)`, `SAM_FLAG_DISABLED(2)`, `SAM_FLAG_LOCKED(4)`.

API: `create()`, `add_user()`, `remove_user()`, `find_by_username()` (case-insensitive), `find_by_sid()`, `serialize_sam()`, `parse_sam()`.

## Steps

### 1. Add a new SID

```rust
use crate::security::sid::*;

let new_sid = Sid::from_parts(1, 5, &[21, 0, 0, 0, 1001]);
let s = new_sid.format_string();  // "S-1-5-21-0-0-0-1001"
```

### 2. Create or inherit a Token

```rust
// Admin token (for PID 0, NeoInit)
let admin = Token::new_admin();

// User token (for unprivileged processes)
let user = Token::new_user();

// Inherit from parent (at process spawn)
let child = Token::inherit_from(&parent_token);
```

### 3. Build a SecurityDescriptor with ACL

```rust
let user = sid_builtin_user();
let mut acl = Acl::new();
acl.insert_ace_canonical(Ace::allow(user, ACCESS_READ | ACCESS_WRITE));
acl.insert_ace_canonical(Ace::deny(user, ACCESS_DELETE));
let sd = SecurityDescriptor::new().with_dacl(acl);
```

### 4. Perform an access check

```rust
if se_access_check(&process_token, Some(&sd), ACCESS_READ) {
    // grant access
} else {
    // deny access
}
```

### 5. Add group membership

```rust
let admin_sid = sid_builtin_admin();
token.add_group(admin_sid);
assert!(token.is_in_group(admin_sid));
```

### 6. Hook security into syscalls

In syscall handlers, check permissions before proceeding:

```rust
// Check if caller is admin (for privileged syscalls)
let result = crate::syscall::check_syscall_permission(syscall_num, is_admin);
if result.is_err() {
    return Err(SyscallError::Perm);
}
```

The `SYSCALL_PERMISSIONS` table in `src/syscall/mod.rs` maps each RAX to `(admin_required, min_ring)`.

### 7. Integrate security with Object Manager

Ob objects can carry a `SecurityDescriptor` via their `ObOperation` impl. The `ob_open` / `ob_create` flow should call `se_access_check` against the object's security descriptor using the caller's token (from `current_process().token`).

### 8. Work with SAM database

```rust
let mut sam = SamDatabase::create();
sam.add_user(b"admin", sid_builtin_admin(), SAM_FLAG_ADMIN, b"Administrator", b"Built-in admin");
let bytes = sam.serialize_sam();
let parsed = SamDatabase::parse_sam(&bytes);
```

## Best practices

- Always use `insert_ace_canonical()` — manual `push` breaks NT canonical order.
- Admin bypass is a convenience, not a substitute for proper ACLs.
- Tokens are copied at process spawn; mutations to parent token do NOT affect children.
- Empty DACL = no access (deny-by-default). Null DACL (no SecurityDescriptor) = full access.
- SAM usernames are case-insensitive, zero-padded to 32 bytes.
- Session IDs isolate terminal sessions — each VT gets its own login context.
- Privilege checks are bitmap AND: `token.privileges & required_privilege != 0`.

## Common mistakes

- Using `Acl::new().aces.push()` instead of `insert_ace_canonical()` — breaks deny-first ordering.
- Forgetting that `se_access_check` takes `Option<&SecurityDescriptor>` — `None` means no security (grant).
- Modifying `DEFAULT_ADMIN_TOKEN` or `DEFAULT_USER_TOKEN` — they are lazy_static singletons.
- Not calling `inherit_from()` at process spawn — child gets no token.
- SAM serialization: forgetting 4-byte alignment padding between variable-length fields.
- Adding syscalls without updating `SYSCALL_PERMISSIONS` — they default to admin-only.

## Final checklist

- [ ] SID constructed correctly (revision, authority, sub-authorities)
- [ ] Token created with proper privileges (admin vs user)
- [ ] Token inherited at process spawn via `inherit_from()`
- [ ] ACL uses `insert_ace_canonical()` for correct ACE ordering
- [ ] SeAccessCheck covers: admin bypass, deny-first, allow matching, fallback deny
- [ ] SAM serialization/deserialization round-trips correctly
- [ ] Syscall permission table updated for new privileged operations
- [ ] Ob integration: access check on open/create
- [ ] Tests registered via `register_security_tests()` and pass
- [ ] `docs/security.md` updated
- [ ] `cargo build` succeeds, `scripts/check_deps.py` passes
