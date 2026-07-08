# NeoDOS Registry — Improvements Design Document

**Status:** Draft
**Version:** v0.1
**Target release:** v0.50

---

## 1. Problem Analysis

### Current Architecture

The Registry (`src/cm/`) is an NT-style cell-based hive system:

- **CmManager** (`src/cm/mod.rs`, 882 lines): global state holding up to 8 mounted hives in `Vec<HiveMount>`.
- **Hive** (`src/cm/hive.rs`, 755 lines): flat array of 2048 `Option<Cell>` slots. Cells types: `Free(0)`, `Key(1)`, `Value(2)`, `Security(3)`.
- **Persistence**: NEOH binary format (magic `NEOH`, version 1, `wrapping_add` checksum). Flush serializes entire hive to `C:\System\Registry\<name>.hiv`.
- **Syscalls**: RAX 67-76 (`cm_open_key`, `cm_create_key`, `cm_query_value`, `cm_set_value`, `cm_enum_key`, `cm_enum_value`, `cm_delete_key`, `cm_flush_key`, `cm_load_hive`, `cm_unload_hive`).
- **Ob integration**: ObInfoClass::RegistryKey (21), ObInfoClass::RegistryValue (22), ObSetInfoClass::RegistryCreateKey (23), RegistryDeleteKey (24), RegistrySetValue (25), RegistryDeleteValue (26).
- **Boot**: Phase 3.881 mounts SYSTEM hive, creates defaults (NeoInit shell, DHCP, WaitForNetwork).

### Specific Problems

**Bugs:**

| # | Bug | Location | Severity |
|---|-----|----------|----------|
| 1 | Free list allocation is broken — `free_head` is set via `scan_next_free` but never properly chained | `hive.rs:136-163` | **Critical** — allocation always linear-scans from index 0 |
| 2 | No value deletion — `RegistryDeleteValue` sets data to empty instead of freeing the cell | `syscall/cm.rs` | **High** — cells leak permanently |
| 3 | Unmount doesn't flush dirty data | `mod.rs:unmount()` | **High** — data loss on unload |
| 4 | Recursive `delete_key` without stack depth limit | `hive.rs:delete_key()` | **Medium** — kernel stack overflow on deep trees |
| 5 | Clone-on-flush is O(2048) — temporarily doubles memory | `mod.rs:cm_flush_key()` | **Medium** — wastes time and memory |
| 6 | Checksum is weak `wrapping_add` — false positives possible | `hive.rs:serialize/deserialize` | **Low** — unlikely but possible |

**Missing features:**

| # | Feature | Impact |
|---|---------|--------|
| 1 | **Security descriptors / ACL enforcement** — `sec_desc_cell` always NULL, `src/cm/security.rs` doesn't exist | **Critical** — registry is world-writable |
| 2 | **Proper value deletion** — no `hive.delete_value()` method | **High** — keys accumulate dead values |
| 3 | **Per-cell dirty tracking** — single `dirty: bool` for entire hive | **Medium** — every mutation forces full-hive re-flush |
| 4 | **Multi-hive** — only SYSTEM hive mounted; SOFTWARE, SECURITY, DEFAULT don't exist | **Medium** — no separation of concerns |
| 5 | **Missing libneodos wrappers** — only 3 of 10 syscalls have wrappers | **Medium** — user-mode tools can't use full API |
| 6 | **CellCache unused** — `cache.rs` exists but never wired in | **Low** — wasted code |
| 7 | **WAL (write-ahead logging)** — no crash-safe transactions | **Medium** — power loss corrupts hives |
| 8 | **Registry notifications** — no `RegNotifyChangeKeyValue` equivalent | **Low** — no event on config change |
| 9 | **Registry editor** — `ndreg` is a driver inspector, not a registry editor | **Low** — no user-facing tool |

### Why Existing Abstractions Can't Solve These

| Problem | Why existing abstractions fall short |
|---------|--------------------------------------|
| Security | `sec_desc_cell` field exists in KeyCell but no code ever populates or checks it. Would need `SeAccessCheck` integration, SID ownership assignment on key creation, and ACE inheritance. |
| Multi-hive | `MAX_HIVES = 8` in `CmManager` is enough, but boot only mounts SYSTEM. No policy for which hives exist, where they live, or how they're organized. |
| WAL | Current persistence writes entire hive atomically. A WAL would require a separate log file, replay logic at mount, and careful ordering of mutations → log before data. |
| Free list | The cell array is `[Option<Cell>; 2048]` — there is no "next free" pointer field in free cells. Fixing the free list requires either adding a `next_free: u32` to free cells (breaking format) or switching to a bitmap allocator. |

---

## 2. Solution Design

### 2.1 Phase 1: Bugfixes (v0.50.0)

Target: fix all bugs without format changes.

#### Fix 1: Free cell allocation

Replace the broken `free_head` + `scan_next_free` with a simple linear scan from last allocation index (next-fit):

```rust
// In Hive:
struct Hive {
    cells: Vec<Option<Cell>>,     // changed from [Option<Cell>; 2048]
    next_alloc_hint: u32,         // next-fit starting point
    cell_count: u16,
    dirty: bool,
}
```

No format change: the free cell walk is internal. `MAX_CELLS = 2048` becomes a soft max — the Vec can grow beyond it (for future expandability). The serializer only writes up to `cell_count`.

#### Fix 2: Value deletion

Add `Hive::delete_value(key_idx, name)`:
1. Walk value linked list to find target
2. Unlink from predecessor or update `values_head`
3. Call `free_cell(target_idx)`
4. Also add `ObSetInfoClass::RegistryDeleteValue` handler that calls this instead of the `REG_NONE` hack

#### Fix 3: Unmount flush

```rust
pub fn cm_unload_hive(mount_path: &str) -> Result<(), ()> {
    // Flush dirty data first
    if let Some(hive) = /* find hive by path */ {
        if hive.hive.is_dirty() {
            cm_flush_key(hive.root_ob_id)?;
        }
    }
    // Then unmount
}
```

#### Fix 4: Iterative delete_key

Replace recursion with explicit stack:

```rust
pub fn delete_key(&mut self, root_idx: u32) {
    let mut stack = Vec::new();  // kernel heap allocation
    stack.push(root_idx);
    while let Some(idx) = stack.pop() {
        if let Some(Cell::Key(key)) = self.slot(idx) {
            // Walk subkeys and push them
            let mut child = key.subkeys_head;
            while child != NULL_CELL {
                stack.push(child);
                if let Some(Cell::Key(k)) = self.slot(child) {
                    child = k.subkeys_sibling;
                } else { break; }
            }
            // Free values
            let mut val = key.values_head;
            while val != NULL_CELL {
                let next = match self.slot(val) {
                    Some(Cell::Value(v)) => v.next,
                    _ => NULL_CELL,
                };
                self.free_cell(val);
                val = next;
            }
        }
        if idx != root_idx {
            self.free_cell(idx);
        }
    }
    // Unlink root_idx from parent
    self.unlink_sibling(root_idx);
}
```

### 2.2 Phase 2: Security (v0.50.1)

New file: `src/cm/security.rs`.

```rust
// Types
pub struct RegistrySecurity {
    pub default_owner_sid: SID,
    pub default_dacl: ACL,     // applied to new keys
}

// Key creation security rules
pub enum KeySecurityInheritance {
    InheritParent,             // copy parent's sec_desc_cell
    Explicit(SecurityDescriptor),
    Default,                   // use process token's default
}

// Access masks (bit flags)
pub const KEY_READ: u32       = 0x0001;
pub const KEY_WRITE: u32      = 0x0002;
pub const KEY_CREATE: u32     = 0x0004;
pub const KEY_ENUMERATE: u32  = 0x0008;
pub const KEY_DELETE: u32     = 0x0010;
pub const KEY_ALL_ACCESS: u32 = 0x001F;
```

**Design:**
1. On key creation (`cm_create_key` / `ObSetInfoClass::RegistryCreateKey`):
   - If parent has `sec_desc_cell != NULL_CELL`, copy the SecurityCell reference (or clone with inheritance rules)
   - If parent has no security, use process token to create a SecurityCell with owner=SID, DACL=default
2. On key/value open/query/set/delete:
   - Call `SeAccessCheck(process_token, sec_desc_cell, requested_access)`
   - Deny if security cell exists and access is not granted
3. On key create via Ob syscalls:
   - `ObSetInfoClass::Security (3)` can be used to set a key's security descriptor (already exists)

No change to the hive serialization format: `SecurityCell` type 3 already exists in serialization.

### 2.3 Phase 3: Per-cell Dirty Tracking + Incremental Flush (v0.50.2)

```rust
struct Hive {
    cells: Vec<Option<Cell>>,
    dirty_cells: BitVec,          // one bit per slot
    dirty: bool,                  // coarse flag (any dirty)
}
```

- `slot_mut(idx)` sets `dirty_cells[idx] = true` and `dirty = true`
- `cm_flush_key` serializes only dirty cells instead of cloning + serializing the entire hive
- After flush: clear `dirty_cells` and optionally compact (clean) cells
- Backward compatible: serializer writes all non-free cells (clean or dirty). The dirty tracking only optimizes flush.

### 2.4 Phase 4: Multi-hive + Boot (v0.50.3)

Mount additional hives during boot:

| Hive | Mount Point | File | Purpose |
|------|-------------|------|---------|
| SYSTEM | `\Registry\Machine\System` | `SYSTEM.hiv` | ✅ Already mounted |
| SOFTWARE | `\Registry\Machine\Software` | `SOFTWARE.hiv` | App/user settings |
| SECURITY | `\Registry\Machine\Security` | `SECURITY.hiv` | Security policy, SID cache |
| DEFAULT | `\Registry\User\.Default` | `DEFAULT.hiv` | Default user settings |

```rust
// In init_cm(), after mounting SYSTEM:
cm_load_hive("SOFTWARE", "\\Registry\\Machine\\Software");
cm_load_hive("SECURITY", "\\Registry\\Machine\\Security");
cm_load_hive("DEFAULT", "\\Registry\\User\\.Default");
```

### 2.5 Phase 5: WAL (Write-Ahead Logging) (v0.51)

New file: `src/cm/wal.rs`.

```rust
pub struct Wal {
    pub log_path: Vec<u8>,
    pub entries: Vec<WalEntry>,
}

pub enum WalEntry {
    CreateKey { cell_idx: u32, parent: u32, name: String },
    DeleteKey { cell_idx: u32 },
    SetValue { key_idx: u32, name: String, value_type: u32, data: Vec<u8> },
    DeleteValue { key_idx: u32, name: String },
    SetSecurity { key_idx: u32, sec_desc: Vec<u8> },
}
```

**Flow:**
1. Each mutation writes to WAL first (`log entry → fsync log file`)
2. Then applies mutation to in-memory hive
3. Checkpoint: after N mutations or on flush, replay WAL to produce serialized hive, then truncate log
4. On mount: if WAL exists, replay it before loading hive

The WAL file lives alongside the hive: `C:\System\Registry\SYSTEM.wal`.

### 2.6 Phase 6: libneodos Wrappers + Registry Editor (v0.51)

Add missing wrappers to `libneodos/src/syscall.rs`:

```rust
pub fn sys_cm_create_key(parent_fd: u8, name: &str) -> Result<u8, i64>;
pub fn sys_cm_delete_key(fd: u8) -> Result<(), i64>;
pub fn sys_cm_enum_key(fd: u8, index: u32) -> Result<String, i64>;
pub fn sys_cm_enum_value(fd: u8, index: u32) -> Result<Vec<u8>, i64>;  // returns name
pub fn sys_cm_flush_key(fd: u8) -> Result<(), i64>;
pub fn sys_cm_load_hive(name: &str, mount_path: &str) -> Result<(), i64>;
pub fn sys_cm_unload_hive(mount_path: &str) -> Result<(), i64>;
```

New binary: `userbin/regedit/` — full registry editor:

| Command | Action |
|---------|--------|
| `REGEDIT <path>` | Browse key tree |
| `REGEDIT /CREATE <path>` | Create key |
| `REGEDIT /DELETE <path>` | Delete key |
| `REGEDIT /SET <path> <name> <type> <value>` | Set value |
| `REGEDIT /QUERY <path> <name>` | Query value |
| `REGEDIT /FLUSH <path>` | Flush hive to disk |

### 2.7 No New Syscalls or Ob Types

All improvements use existing syscalls (RAX 67-76) and ObInfoClass/ObSetInfoClass variants (21-26). No new RAX numbers needed.

---

## 3. Alternatives Considered

### 3.1 Replace cell array with B-tree

**Rejected.** The current flat array with linked-list chains is simple and adequate for the expected scale (<2048 entries). A B-tree would add complexity, require a new serialization format, and is only beneficial when keys/values exceed the current limit. If growth is needed, the cell array can be made dynamic (Vec) without changing the linked-list semantics.

### 3.2 Use an existing embedded database (SQLite, LMDB)

**Rejected.** A raw dependency-free cell format is a deliberate architectural choice (NT influence). Adding a third-party database would introduce complexity, licensing concerns (in `#![no_std]`), and would not align with NeoDOS's philosophy of explicit, rastreable code.

### 3.3 Full per-cell dirty tracking from start

**Rejected for Phase 1.** Per-cell dirty tracking (Phase 3) adds the `BitVec` and incremental flush logic. The critical fix is to make value deletion and free list work correctly first. Per-cell tracking is an optimization and can be layered on later.

### 3.4 Replace `wrapping_add` with CRC32 in-place

**Rejected for Phase 1.** Changing the checksum algorithm breaks the binary format. This can be done when NEOHv2 is introduced (e.g., when adding WAL support in Phase 5), version-negotiated during deserialization.

### 3.5 Full transaction support with rollback

**Deferred.** True transactions (begin/commit/rollback) would require snapshot isolation or undo logs. The WAL in Phase 5 provides crash recovery but not rollback. Full transactions are out of scope for v0.50.

---

## 4. Affected Components

| Component | Nature of Change |
|-----------|-----------------|
| `src/cm/hive.rs` | Fix free list (next-fit), add `delete_value()`, iterative `delete_key`, per-cell dirty tracking, growable `Vec` |
| `src/cm/mod.rs` | Fix unmount flush, add multi-hive boot, add WAL mount/replay, add security checks in CRUD dispatch |
| `src/cm/security.rs` | **New file**: key ACL creation, inheritance, `SeAccessCheck` integration |
| `src/cm/wal.rs` | **New file**: write-ahead log, replay, checkpoint |
| `src/cm/cache.rs` | Wire CellCache into `slot()`/`slot_mut()` |
| `src/syscall/cm.rs` | Fix `RegistryDeleteValue` to call `delete_value()`, add security checks per operation |
| `src/object/types.rs` | No changes needed |
| `libneodos/src/syscall.rs` | Add 7 missing cm_* wrappers |
| `userbin/regedit/` | **New binary**: registry editor |
| `userbin/ndreg/` | No change (driver inspector, unrelated) |
| `docs/registry.md` | Update to reflect new capabilities |
| `scripts/build.sh` | Add `regedit` build rule |

---

## 5. API Contract

### New Hive Methods

```
impl Hive {
    pub fn delete_value(&mut self, key_idx: u32, name: &str) -> Result<(), ()>
        Args: key_idx — cell index of parent key
              name — value name (case-insensitive)
        Returns: Ok(()) or Err if value not found
        Preconditions: key_idx must be a valid Key cell
        Postconditions: ValueCell freed from linked list and cell array

    pub fn set_dirty_cell(&mut self, idx: u32)
        Marks a specific cell as dirty (per-cell tracking)

    pub fn serialize_dirty(&self) -> Vec<u8>
        Serializes only dirty cells + header. Used for incremental flush
}

pub fn cm_delete_value(key_native_id: u64, name: &str) -> Result<(), ()>
    Args: key_native_id — encoded (hive_idx << 24) | cell_idx
          name — value name to delete
    Returns: Ok(()) or Err if not found
```

### Security API (`src/cm/security.rs`)

```
pub fn cm_check_access(
    token: &Token,
    key_native_id: u64,
    access_mask: u32,
) -> Result<(), ()>
    Args: token — calling process security token
          key_native_id — encoded key identifier
          access_mask — requested access (KEY_READ|KEY_WRITE|KEY_CREATE|...)
    Returns: Ok(()) if access granted, Err if denied
    Preconditions: key must exist and be a Key cell
    Error states: key not found → Err; no security cell → admin check only

pub fn cm_ensure_security(
    key_native_id: u64,
    token: &Token,
) -> Result<(), ()>
    Creates a SecurityCell for the key if one doesn't exist,
    using the process token's SID as owner and default DACL.
    Returns Ok(()) or Err on cell allocation failure

pub fn cm_inherit_security(
    key_native_id: u64,
    parent_native_id: u64,
) -> Result<(), ()>
    Copies or inherits security from parent to new key.
    If parent has sec_desc_cell, clone it for the child.
    Returns Ok(()) or Err if parent not found
```

### WAL API (`src/cm/wal.rs`)

```
pub fn wal_ensure_path(name: &str) -> Result<Vec<u8>, ()>
    Returns the WAL file path: "C:\\System\\Registry\\{name}.wal"

pub fn wal_replay(name: &str, hive: &mut Hive) -> Result<(), ()>
    If WAL file exists, read entries, apply to hive, delete WAL file.
    Called during hive load.

pub fn wal_log(
    name: &str,
    entry: &WalEntry,
) -> Result<(), ()>
    Append entry to WAL file, fsync
    Error: disk full, I/O error

pub fn wal_checkpoint(name: &str, hive: &Hive) -> Result<(), ()>
    Called after successful hive flush.
    Serializes hive, writes to .hiv file, deletes .wal file.
```

### Fixed `RegistryDeleteValue` Syscall (existing, unchanged ABI)

```
RAX 63 (ob_set_info), info_class = ObSetInfoClass::RegistryDeleteValue (26)
  Args: RBX = key_fd, RCX = buf (value name)
  Returns: 0 on success, negative error
  Errors: -NoEnt if value not found
          -Acces if security check fails
  Preconditions: fd must be an open Key object
  Postconditions: ValueCell freed from key's linked list, cell reclaimed
```

---

## 6. Test Plan

### Phase 1: Bugfixes

| # | Test | Description |
|---|------|-------------|
| 1 | Free list next-fit | Create 100 keys, delete every other, verify new allocations reuse freed slots |
| 2 | Value deletion | `set_value("foo", REG_SZ, "bar")` → `delete_value(key, "foo")` → `query_value("foo")` returns error |
| 3 | Value deletion persistence | Delete value, flush, reload hive — verify value is gone |
| 4 | Unmount flush | Set value, unmount hive, remount — verify value persists |
| 5 | Deep key deletion | Create 500 nested keys, delete root — verify no stack overflow and all cells freed |
| 6 | Key deletion count | Delete key with 10 subkeys and 20 values — verify all cells freed |
| 7 | Key deletion preserves siblings | Delete middle key in sibling chain — verify next/prev siblings still linked |

### Phase 2: Security

| # | Test | Description |
|---|------|-------------|
| 1 | Key creation assigns owner | Create key with admin token — verify SecurityCell exists, owner SID = admin |
| 2 | Access granted | Admin token opens key with KEY_READ — succeeds |
| 3 | Access denied | Non-admin token opens admin-only key with KEY_WRITE — fails |
| 4 | Security inheritance | Create child key — verify child has same SecurityCell reference or clone |
| 5 | Security explicit set | Use `ObSetInfoClass::Security` on existing key — verify sec_desc_cell updated |
| 6 | Admin bypass | Admin token accesses any key — succeeds regardless of DACL |
| 7 | Default DACL on new key | Key created without explicit security — verify default DACL grants owner full access |

### Phase 3: Per-cell Dirty Tracking

| # | Test | Description |
|---|------|-------------|
| 1 | Cell dirty on write | `set_value()` → verify `dirty_cells[cell_idx]` is set |
| 2 | Cell clean after flush | Flush → verify `dirty_cells` cleared |
| 3 | Only dirty cells serialized | Modify 1 of 100 values — verify serialized output is small |
| 4 | Full flush still correct | `cm_flush_key` after dirty tracking — verify all data survives round-trip |
| 5 | Hive dirty flag | Any mutation sets `hive.dirty = true`; flush clears it |

### Phase 4: Multi-hive

| # | Test | Description |
|---|------|-------------|
| 1 | SOFTWARE hive mounted | After init — `cm_open_key("\Registry\Machine\Software")` succeeds |
| 2 | Hive isolation | Create key in SYSTEM, verify not visible in SOFTWARE |
| 3 | Cross-hive path fails | `cm_open_key("\Registry\Machine\Software\..\System")` — fails |
| 4 | Unload/reload SOFTWARE | Unload SOFTWARE hive, remount — verify defaults created |

### Phase 5: WAL

| # | Test | Description |
|---|------|-------------|
| 1 | WAL created on mutation | `set_value()` → WAL file exists with matching entry |
| 2 | WAL replay on load | Corrupt .hiv, keep .wal — replay recovers data |
| 3 | WAL truncated after flush | Flush → .wal file deleted |
| 4 | Power loss recovery | Simulate: write 5 mutations, "crash", replay — all 5 mutations present |
| 5 | Empty WAL no-op | Mount with no .wal file — normal load, no replay |

### Phase 6: libneodos + regedit

| # | Test | Description |
|---|------|-------------|
| 1 | `sys_cm_create_key` wrapper | Create key via wrapper, verify with `cm_open_key` |
| 2 | `sys_cm_enum_key` wrapper | Create 3 subkeys, enumerate — all 3 returned |
| 3 | `sys_cm_enum_value` wrapper | Set 3 values on key, enumerate — all 3 returned |
| 4 | `sys_cm_flush_key` wrapper | Set value, flush, reload — value persists |
| 5 | regedit browse | `REGEDIT \Registry\Machine\System` — prints key tree |
| 6 | regedit create/delete | `REGEDIT /CREATE \Registry\Machine\Software\Test` → key exists |

### Integration

| # | Test | Description |
|---|------|-------------|
| 1 | Boot with persistent hives | Create key, flush, reboot — key exists after mount |
| 2 | Security enforced end-to-end | Non-admin process tries `RegistrySetValue` — denied |
| 3 | WAL crash recovery | Set 10 values without flush, "crash" (drop hive), mount — all 10 recovered |
| 4 | Full round-trip | Create 20 keys with 100 values across 3 hives, flush, reload, verify |

---

## 7. Implementation Plan

### Phase 1: Bugfixes (v0.50.0)

| Step | Files | Description |
|------|-------|-------------|
| 1.1 | `src/cm/hive.rs` | Fix free list: replace `free_head`/`scan_next_free` with next-fit linear scan from `next_alloc_hint`. Change `cells` to `Vec<Option<Cell>>`. |
| 1.2 | `src/cm/hive.rs` | Add `delete_value()`: unlink from value linked list, call `free_cell()`. |
| 1.3 | `src/cm/hive.rs` | Rewrite `delete_key()` as iterative with explicit Vec stack. |
| 1.4 | `src/syscall/cm.rs` | Fix `RegistryDeleteValue` handler to call `cm_delete_value()` (via a new `cm_delete_value()` in `mod.rs`) instead of the `REG_NONE` hack. |
| 1.5 | `src/cm/mod.rs` | Fix `cm_unload_hive()`: flush dirty data before unmounting. |
| 1.6 | `src/cm/mod.rs` | Fix `cm_flush_key()` deadlock: acquire lock once for clone, flush, mark_clean. |

**Test gate:** Phase 1 tests 1-7 pass.

### Phase 2: Security (v0.50.1)

| Step | Files | Description |
|------|-------|-------------|
| 2.1 | `src/cm/security.rs` | Create file with `RegistrySecurity` type, `cm_check_access()`, `cm_ensure_security()`, `cm_inherit_security()`. |
| 2.2 | `src/cm/mod.rs` | Hook `cm_check_access()` into `cm_open_key`, `cm_create_key`, `cm_set_value`, `cm_query_value`, `cm_delete_key`, `cm_enum_key`, `cm_enum_value`. |
| 2.3 | `src/cm/mod.rs` | Hook `cm_ensure_security()` into `cm_create_key` (on first key creation in a path). |
| 2.4 | `src/cm/mod.rs` | Hook `cm_inherit_security()` into `cm_create_key` (child inherits parent). |
| 2.5 | `src/syscall/cm.rs` | Add `SeAccessCheck` to all syscall handlers, return `-Acces` on failure. |

**Test gate:** Phase 2 tests 1-7 pass.

### Phase 3: Per-cell Dirty Tracking (v0.50.2)

| Step | Files | Description |
|------|-------|-------------|
| 3.1 | `src/cm/hive.rs` | Add `dirty_cells: BitVec` field. Modify `slot_mut()` to set dirty bit. |
| 3.2 | `src/cm/hive.rs` | Add `serialize_dirty()` for incremental flush. |
| 3.3 | `src/cm/cache.rs` | Wire CellCache into `slot()`/`slot_mut()` — check cache before linear scan. |
| 3.4 | `src/cm/mod.rs` | Update `cm_flush_key()/cm_flush_all_hives()` to use per-cell dirty tracking. |

**Test gate:** Phase 3 tests 1-5 pass.

### Phase 4: Multi-hive (v0.50.3)

| Step | Files | Description |
|------|-------|-------------|
| 4.1 | `src/cm/mod.rs` | Add `cm_load_hive("SOFTWARE", ...)` call in `init_cm()`. |
| 4.2 | `src/cm/mod.rs` | Add `cm_load_hive("SECURITY", ...)` call in `init_cm()`. |
| 4.3 | `src/cm/mod.rs` | Add `cm_load_hive("DEFAULT", ...)` call in `init_cm()`. |
| 4.4 | `src/cm/mod.rs` | Ensure each hive creates its root directory in Ob namespace during mount. |

**Test gate:** Phase 4 tests 1-4 pass.

### Phase 5: WAL (v0.51.0)

| Step | Files | Description |
|------|-------|-------------|
| 5.1 | `src/cm/wal.rs` | Create file with `WalEntry`, `wal_replay()`, `wal_log()`, `wal_checkpoint()`. |
| 5.2 | `src/cm/mod.rs` | Integrate `wal_log()` into `cm_set_value`, `cm_create_key`, `cm_delete_key`, `cm_delete_value`. |
| 5.3 | `src/cm/mod.rs` | Integrate `wal_replay()` into `cm_load_hive` (run after deserialize, before mount). |
| 5.4 | `src/cm/mod.rs` | Integrate `wal_checkpoint()` into `cm_flush_key()`. |
| 5.5 | `src/cm/hive.rs` | Change serializer to use the full/wrapping_add checksum, add version header for `NEOHv2`. |

**Test gate:** Phase 5 tests 1-5 pass.

### Phase 6: libneodos + regedit (v0.51.1)

| Step | Files | Description |
|------|-------|-------------|
| 6.1 | `libneodos/src/syscall.rs` | Add `sys_cm_create_key`, `sys_cm_delete_key`, `sys_cm_enum_key`, `sys_cm_enum_value`, `sys_cm_flush_key`, `sys_cm_load_hive`, `sys_cm_unload_hive` wrappers. |
| 6.2 | `userbin/regedit/src/main.rs` | Create registry editor binary with browse, create, delete, set, query, flush commands. |
| 6.3 | `scripts/build.sh` | Add `regedit` to build list. |
| 6.4 | `docs/registry.md` | Update documentation: security, WAL, multi-hive, new wrappers, regedit. |

**Test gate:** Phase 6 tests 1-6 pass. All integration tests pass.

---

## 8. Backward Compatibility

- **Hive format**: Phases 1-4 are format-compatible with NEOHv1. Phase 5 (WAL) adds a `.wal` sidecar file; the `.hiv` format remains the same but gains a version bump to NEOHv2. Old hives are loaded via version detection in `deserialize()`.
- **Syscall ABI**: No changes to RAX numbers, argument order, or return types. All existing user binaries continue working unmodified.
- **Ob API**: No new ObInfoClass/ObSetInfoClass variants. `RegistryDeleteValue` changes behavior (now actually deletes instead of zeroing data), but the old behavior was effectively a bug.
- **Security**: Existing processes without SIDs (legacy) are treated as admin (full access). Only processes with security tokens will have restricted registry access.
- **Cell cache**: Optional — `slot()` falls back to linear scan if cache is not wired. No semantic change.
