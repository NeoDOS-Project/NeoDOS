---
name: registry
description: Add registry keys/values, hive persistence, Cm syscalls, hive security
---

# Registry (Cm — Configuration Manager)

## When to use

Adding a registry key/value, modifying hive persistence, extending the
cell-based hive format, working with `src/cm/`, or touching the Cm syscalls.

## Goal

Implement registry operations following the NT-style cell-based hive
architecture: cell allocation, sibling/value chains, dirty tracking, and
persistence.

## References

- `docs/registry/registry.md` — subsystem documentation
- `src/cm/mod.rs` — `CmManager`, mount/unmount, dispatch
- `src/cm/api.rs` — `cm_open_key`, `cm_create_key`, `cm_query_value`,
  `cm_set_value`, `cm_enum_key`, `cm_enum_value`, `cm_delete_key`,
  `cm_flush_key`, `cm_flush_all_hives`, `cm_load_hive`, `cm_unload_hive`
- `src/cm/hive/` — cell buffer and CRUD (`core.rs`, `keys.rs`, `values.rs`,
  `serialize.rs`, `types.rs`)
- `src/cm/manager.rs` — `CmManager { hives: [Option<Hive>; 8] }`,
  `encode_cell` / `decode_cell`
- `src/cm/init.rs` — boot init and default values (Phase 3.881)
- `src/cm/timezone.rs`, `src/cm/cache.rs`, `src/cm/tests.rs`
- `src/syscall/cm.rs` — syscall handlers for RAX 50-59
- `src/object/types.rs` — `ObType::Key = 12`; `ObInfoClass::RegistryKey=21`,
  `RegistryValue=22`; `ObSetInfoClass::RegistryCreateKey=23`,
  `RegistryDeleteKey=24`, `RegistrySetValue=25`, `RegistryDeleteValue=26`
- `libneodos/src/syscall/cm.rs` — user-mode wrappers
- `tools/gen-hiv` — offline SYSTEM.HIV generator (NEOHv1) → `data/system.hiv`

## Cm syscalls (RAX 50-59)

Handlers are `handler_cm_*` in `src/syscall/cm.rs`; they operate on Ob objects of
type `ObType::Key (12)`. Register convention: `RBX`=arg0, `RCX`=arg1,
`RDX`=arg2, `R8`=arg3, `R9`=arg4.

| RAX | Syscall | Purpose | Parameters |
| ----- | -------- | --------- | ------------ |
| 50 | `cm_open_key` | Open a key by path | rbx=path → fd |
| 51 | `cm_create_key` | Create subkey under a key handle | rbx=parent_fd, rcx=name → fd |
| 52 | `cm_query_value` | Read a value by name | rbx=key_fd, rcx=name, rdx=buf, r8=len → size |
| 53 | `cm_set_value` | Set a value (type + data) | rbx=key_fd, rcx=name, rdx=type, r8=data, r9=len |
| 54 | `cm_enum_key` | Enumerate subkeys by index | rbx=key_fd, rcx=index, rdx=buf |
| 55 | `cm_enum_value` | Enumerate values by index | rbx=key_fd, rcx=index, rdx=buf |
| 56 | `cm_delete_key` | Delete a key and its subkeys | rbx=key_fd |
| 57 | `cm_flush_key` | Flush hive to disk | rbx=key_fd |
| 58 | `cm_load_hive` (admin) | Load a hive file | rbx=name, rcx=mount |
| 59 | `cm_unload_hive` (admin) | Unload a hive (flushes if dirty) | rbx=mount |

Path format: `\Registry\Machine\System\CurrentControlSet\Services\...`

## Cell-based hive format

Source: `src/cm/hive/`. Each hive is a contiguous buffer (`HiveBuffer`) of cells
indexed by `u32` offset; `MAX_CELLS = 2048`. Cell 0 is always the root.

| Value | Variant | Contents |
| ------- | --------- | ---------- |
| 0 | `Free` | Unallocated cell |
| 1 | `Key` | `KeyCell`: `name`, `parent_cell`, `subkeys_head`, `subkeys_sibling`, `values_head`, `sec_desc_cell`, `last_write_time` |
| 2 | `Value` | `ValueCell`: `name`, `value_type`, `data`, `data_len`, `next` |
| 3 | `Security` | `SecurityCell`: serialized security descriptor (present in the format; not yet enforced) |

Value types: `REG_NONE(0)`, `REG_SZ(1)`, `REG_DWORD(2)`, `REG_BINARY(3)`.
Subkeys form a singly-linked sibling chain; values form a singly-linked chain.
Cell allocation is **next-fit** from `next_alloc_hint` (`core.rs::alloc_cell`),
and `free_cell` returns a cell to the pool.

## Steps

### 1. Add a key or value

```rust
// In src/cm/hive/ (values.rs / keys.rs) or via src/cm/api.rs
hive.create_key(parent_idx, "NewKey");
hive.set_value(key_idx, "MyValue", REG_DWORD, &42u32.to_le_bytes());
```

### 2. Navigate keys

```rust
let child = hive.find_key(parent_idx, "SubKey");      // case-insensitive
let key   = hive.open_key_by_path(root, "CurrentControlSet\\Services\\NeoInit");

// Walk subkeys
let mut idx = key.subkeys_head;
while idx != NULL_CELL {
    if let Some(Cell::Key(child)) = hive.slot(idx) { /* ... */ idx = child.subkeys_sibling; }
}
```

### 3. Delete a key or value

```rust
hive.delete_value(key_idx, "ValueName");  // unlinks the value and frees the cell
hive.delete_key(key_idx);                 // deletes the key and all subkeys
```

### 4. Flush / persistence

```rust
cm_flush_key(key_native_id);   // serialize one hive
cm_flush_all_hives();          // called on poweroff
```

Hives persist to `C:\System\Registry\<name>.hiv`. On boot (Phase 3.881) a hive is
loaded if the file exists; otherwise defaults are created. `cm_unload_hive`
flushes a dirty hive before unmounting.

### 5. Registry security (planned)

`KeyCell.sec_desc_cell` exists in the format but is always `NULL`; there is no
enforcement code yet. Planned work: a `src/cm/security.rs` with
`ensure_security` / `check_access` / `inherit_security`, hooks in the `cm_*`
handlers, and `SeAccessCheck` integration from `src/security/access.rs`.

### 6. Offline hive

```bash
cd tools/gen-hiv && cargo run --release -- ../../data/system.hiv
```

Generates `data/system.hiv` (NEOHv1), embedded in the image during the NeoDev
build. There is no offline registry-parser CLI; inspect the running registry
through the Ob syscalls or the `neodos-mcp` tools (`kernel_index`,
`search_symbol`, `check_consistency`).

### 7. Tests and build

Add tests in `src/cm/tests.rs` (registered by `register_cm_tests()`), then:

```bash
cd neodos-kernel && cargo build
neodev build --quick --image && neodev test
neodev check-deps
```

## Known limitations

| Area | Status |
| ------ | -------- |
| Security descriptors | Format supports `SecurityCell`, but key ACLs are not enforced |
| `CellCache` | Defined in `cache.rs`, not wired into `slot()`/`slot_mut()` |
| WAL / crash-safe transactions | Not implemented (planned for NEOHv2) |
| Multi-hive split | SYSTEM/SOFTWARE/SECURITY/DEFAULT split is planned; currently SYSTEM is mounted |
| Checksum | `wrapping_add` over cell fields (weak, planned CRC32) |

## Best practices

- Case-insensitive comparison for key/value names.
- Cell 0 (root) is protected from deletion.
- `encode_cell(hive_idx, cell_idx)` packs a hive+cell reference into `native_id`.
- Mark the cell dirty on mutation so flush persists only changed cells.
- Hive operations hold `CM_MANAGER`; avoid long work while holding it.
- New hives mount under `\Registry\Machine\<name>` in the Ob namespace.
- When adding default values, update `cm_ensure_default_values()` and regenerate
  `data/system.hiv` so offline and kernel defaults match.

## Test checklist (in `src/cm/tests.rs`)

- [ ] Create key + verify with `find_key`
- [ ] Set value + query + verify type and data
- [ ] Case-insensitive lookup
- [ ] Subkey and value enumeration (multiple)
- [ ] Key deletion frees all subkey cells
- [ ] Value deletion frees the cell and unlinks the chain
- [ ] Serialize → deserialize round-trip
- [ ] Flush + reload persistence
- [ ] Default values created and idempotent
- [ ] Multi-hive isolation (SYSTEM vs. SOFTWARE)
- [ ] Free-cell reuse via next-fit

## Final checklist

- [ ] Cell allocation/free keeps the hive consistent
- [ ] Dirty tracking set on mutation; flush persists
- [ ] `data/system.hiv` regenerated if defaults changed (`tools/gen-hiv`)
- [ ] Tests added; `cargo build`, `neodev test`, `neodev check-deps` pass
- [ ] `docs/registry/registry.md` updated for new syscalls/keys/classes
