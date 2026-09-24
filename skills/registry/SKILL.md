---
name: registry
description: Add registry keys, modify hive persistence, implement registry security
---

# Registry (Cm — Configuration Manager)

## When to use

Adding a registry key/value, modifying hive persistence, implementing registry security, working with `src/cm/` or `src/syscall/cm.rs`, or extending the cell-based hive format.

## Goal

Correctly implement registry operations following the NT-style cell-based hive architecture, with proper cell allocation, sibling chains, dirty tracking, and security.

## References

- `docs/registry/registry.md` — subsystem documentation
- `docs/design/registry-improvements.md` — planned improvements design
- `src/cm/hive.rs` — cell-based hive buffer, key/value CRUD
- `src/cm/mod.rs` — CmManager, mount/unmount, persistence, default values
- `src/cm/security.rs` — key ACLs (planned, not yet implemented)
- `src/cm/cache.rs` — cell cache (unused, intended for optimization)
- `src/cm/wal.rs` — WAL (planned, not yet implemented)
- `src/syscall/cm.rs` — syscall handlers for RAX 67-76
- `src/object/types.rs` — `ObType::Key (12)`, ObInfoClass/ObSetInfoClass variants (21-26)
- `libneodos/src/syscall.rs` — user-mode wrappers (only 3 of 10 exist)
- `scripts/gen_system_hiv.py` — offline SYSTEM.HIV generator for build-time
- `scripts/mcp_server/parsers/registry_hive.py` — offline NEOH hive parser for MCP tools

## Architecture

```text
RAX 67-76 (Cm syscalls)
    │
    ├── cm_open_key (67)      ─── ObType::Key open by path
    ├── cm_create_key (68)     ─── Create subkey under parent handle
    ├── cm_query_value (69)    ─── Read value by name
    ├── cm_set_value (70)      ─── Set value (type + data)
    ├── cm_enum_key (71)       ─── Enumerate subkeys by index
    ├── cm_enum_value (72)     ─── Enumerate values by index
    ├── cm_delete_key (73)     ─── Delete key and all subkeys
    ├── cm_flush_key (74)      ─── Flush hive to disk
    ├── cm_load_hive (75)      ─── Mount hive file (admin)
    └── cm_unload_hive (76)    ─── Unmount hive (admin)

ObSetInfoClass (RAX 63):
    RegistryCreateKey (23)
    RegistryDeleteKey (24)
    RegistrySetValue (25)
    RegistryDeleteValue (26)
    RegistryDeleteValue actually frees the cell (v0.50+)

ObInfoClass (RAX 62):
    RegistryKey (21)    → [subkey_count: u32, value_count: u32]
    RegistryValue (22)  → [type: u32, data_len: u32, data...]
```

## Steps

### 1. Understand the cell-based hive format

The hive is a flat array of `Option<Cell>` (max 2048). Cells are 4 types:

| Type | Value | Struct | Key fields |
| ------ | ------- | -------- | ------------ |
| Free | 0 | — | part of free list |
| Key | 1 | `KeyCell` | `name`, `parent_cell`, `subkeys_head`, `subkeys_sibling`, `values_head`, `sec_desc_cell`, `last_write` |
| Value | 2 | `ValueCell` | `name`, `value_type`, `data`, `next` |
| Security | 3 | `SecurityCell` | `sd_data`, `next` |

Cell 0 is always the root key. Subkeys form a singly-linked list via `subkeys_head`/`subkeys_sibling`. Values form a singly-linked list via `values_head`/`next`.

The free list uses a `free_head` pointer, but **the current implementation is broken** — `scan_next_free` does a linear scan instead of proper chaining. Fix: use next-fit from `next_alloc_hint`.

### 2. Add a new key/value

In `src/cm/hive.rs`:

```rust
// Create a subkey under parent
let child = hive.create_key(parent_idx, "NewKey");

// Set a value
hive.set_value(key_idx, "MyValue", REG_DWORD, &42u32.to_le_bytes());
```

In `src/cm/mod.rs` (via syscall dispatch):

```rust
// Open key by path, creating intermediate keys if needed
if let Some(key) = ensure_key_path(&mut hm.hive, root, "Path\\To\\Key") {
    hm.hive.set_value(key, "ValueName", REG_SZ, b"data");
}
```

### 3. Navigate keys

```rust
// Find subkey by name (case-insensitive)
let child = hive.find_key(parent_idx, "SubKey");

// Walk the sibling chain
let mut idx = key.subkeys_head;
while idx != NULL_CELL {
    if let Some(Cell::Key(child)) = hive.slot(idx) {
        // process child
        idx = child.subkeys_sibling;
    }
}

// Walk the value chain
let mut idx = key.values_head;
while idx != NULL_CELL {
    if let Some(Cell::Value(val)) = hive.slot(idx) {
        // process val
        idx = val.next;
    }
}

// Open by full path from root
let key = hive.open_key_by_path(root, "CurrentControlSet\\Services\\NeoInit");
```

### 4. Delete a key or value

```rust
// Delete a value (v0.50+)
hive.delete_value(key_idx, "ValueName");
// This unlinks from the value chain and calls free_cell().

// Delete a key and all subkeys (iterative, not recursive)
hive.delete_key(key_idx);
// Uses an explicit Vec stack to avoid kernel stack overflow.
// Unlinks from parent's sibling chain, frees all cells.
```

### 5. Flush a hive to disk

```rust
// Flush a specific key's hive
cm_flush_key(key_native_id);

// Flush all dirty hives (called on poweroff)
cm_flush_all_hives();

// At boot, hives are loaded from C:\System\Registry\<name>.hiv
// If the file doesn't exist, a fresh hive is created with defaults.
// Defaults are created in cm_ensure_default_values() at Phase 3.881.
```

### 6. Implement registry security (planned)

`KeyCell.sec_desc_cell` points to a `SecurityCell` containing a serialized `SecurityDescriptor`.

```rust
// On key creation, ensure a SecurityCell exists
cm_ensure_security(key_native_id, process_token);

// On access, check permissions
cm_check_access(process_token, key_native_id, KEY_READ | KEY_WRITE);
```

The `SecurityCell` type 3 already exists in the serialization format but no code creates or checks it. Security enforcement requires:

- `src/cm/security.rs` with `cm_check_access()`, `cm_ensure_security()`, `cm_inherit_security()`
- Hooks in all cm_* syscall handlers in `src/syscall/cm.rs`
- `SeAccessCheck` integration from `src/security/access.rs`

### 7. Offline hive inspection (MCP tools)

```bash
# Query a hive from the build image
bash scripts/mcp-server.sh --tool registry_list \
    key_path='\CurrentControlSet\Services\NeoInit' \
    hive=SYSTEM

bash scripts/mcp-server.sh --tool registry_query \
    key_path='\CurrentControlSet\Services\NeoInit' \
    value_name=DefaultShell hive=SYSTEM

bash scripts/mcp-server.sh --tool registry_tree \
    hive=SYSTEM

bash scripts/mcp-server.sh --tool registry_hive_info \
    hive=SYSTEM
```

### 8. Generate a hive offline (for build-time inclusion)

```bash
python3 scripts/gen_system_hiv.py
# Produces scripts/system.hiv (NEOHv1)
# Embedded in neodos_image.img during build.sh --neodos-image
# at C:\System\Registry\SYSTEM.HIV
```

## Known issues

| Issue | Status | Workaround |
| ------- | -------- | ------------ |
| Free list allocation is broken | **BUG** | `scan_next_free` doesn't properly chain freed cells. Fix: use linear next-fit scan from `next_alloc_hint`. |
| No `delete_value` method | **v0.50+** | `RegistryDeleteValue` (26) currently sets data to empty instead of freeing the cell. Use `hive.delete_value()` once implemented. |
| Unmount doesn't flush dirty data | **BUG** | `cm_unload_hive()` removes hive without persisting. Call `cm_flush_key()` first. |
| Recursive `delete_key` | **BUG** | Can overflow kernel stack on deep trees. Use iterative version with `Vec<u32>` stack. |
| `cm_flush_key` double-lock | **BUG** | `cm_flush_key()` and `cm_flush_all_hives()` have deadlock potential. Hold lock once for clone+flush+mark_clean. |
| Checksum is `wrapping_add` | **Weak** | Can have false positives. Will be replaced with CRC32 in NEOHv2 (planned). |
| Security descriptors not enforced | **Missing** | `sec_desc_cell` always NULL. Key ACLs need `security.rs` implementation. |
| `CellCache` not wired | **Unused** | `cache.rs` exists but is never instantiated. Needs integration in `slot()`/`slot_mut()`. |
| Missing libneodos wrappers | **Missing** | Only `sys_cm_open_key`, `sys_cm_query_value`, `sys_cm_set_value` have wrappers. 7 missing. |
| No WAL | **Missing** | Write-ahead logging planned for crash recovery (NEOHv2). |

## Best practices

- Always case-insensitive matching for key/value names (uppercase comparison).
- Cell 0 (root) is protected from deletion — check before calling `delete_key`.
- Use `encode_cell(hive_idx, cell_idx)` to pack hive+cell index into `native_id`.
- Dirty tracking is per-cell (v0.50+) — always set dirty bit when mutating.
- Hive operations hold `CM_MANAGER.lock()` — avoid long operations while holding it.
- `ensure_key_path()` creates intermediate keys if they don't exist — use for boot defaults.
- New hives mount at `\Registry\Machine\<name>` in the Ob namespace.
- When adding new default values, modify `cm_ensure_default_values()` in `src/cm/mod.rs`.

## Common mistakes

- Using `delete_key` recursively — causes stack overflow on deep trees. Use the iterative version.
- Forgetting `mark_clean()` after `flush_to_io()` — hive stays dirty, flushes repeatedly.
- Using `slot(idx)` without checking bounds — `idx` must be a valid cell index.
- Mutating the value linked list without updating both `prev.next` and `values_head` for head insertion.
- Adding values without checking for existing value by name first — `set_value` handles this, but manual linked-list manipulation doesn't.
- Not regenerating `system.hiv` after changing default values — the offline hive in `scripts/system.hiv` must match `cm_ensure_default_values()`.

## Test checklist

When modifying registry code, add tests in `src/cm/mod.rs` (registered via `register_cm_tests()`):

- [ ] Create key + verify with find_key
- [ ] Set value + query + verify type and data
- [ ] Case-insensitive lookup works
- [ ] Enumeration of subkeys (multiple)
- [ ] Enumeration of values (multiple)
- [ ] Key deletion frees all subkey cells
- [ ] Value deletion frees cell and unlinks from chain
- [ ] Serialize → deserialize → verify all data survives round-trip
- [ ] Flush + reload persists data
- [ ] Default values created and idempotent
- [ ] Multi-hive isolation (create in SYSTEM, not visible in SOFTWARE)
- [ ] Security: access granted/denied correctly
- [ ] Free list reuses freed cells

## Build and test

```bash
cargo build  # in neodos-kernel/
python3 scripts/auto_test.py
scripts/check_deps.py

# Regenerate offline hive if default values changed
python3 scripts/gen_system_hiv.py

# Rebuild image with new hive
bash scripts/build.sh --neodos-image

# Verify with MCP tools
bash scripts/mcp-server.sh --tool registry_hive_info hive=SYSTEM
```
