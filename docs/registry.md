# Registry Subsystem

## Cm Syscalls (RAX 67-76)

All implemented as `handler_cm_*` in `src/syscall/cm.rs`. Operate on Ob objects of type `ObType::Key` (12).

| RAX | Name | Description | Parameters |
|-----|------|-------------|------------|
| 67 | `cm_open_key` | Open registry key by path | rdi = path_ptr (u64), rsi = path_len (u64) -> returns fd in rax |
| 68 | `cm_create_key` | Create subkey under key handle | rdi = parent_fd (u64), rsi = name_ptr (u64), rdx = name_len (u64) -> returns new fd |
| 69 | `cm_query_value` | Read value by name | rdi = key_fd, rsi = name_ptr, rdx = name_len, r10 = buf_ptr, r8 = buf_len -> returns written size |
| 70 | `cm_set_value` | Set value (type + data) | rdi = key_fd, rsi = name_ptr, rdx = name_len, r10 = val_type, r8 = data_ptr, r9 = data_len |
| 71 | `cm_enum_key` | Enumerate subkeys by index | rdi = key_fd, rsi = index (u32), rdx = buf_ptr, r10 = buf_len -> returns key name |
| 72 | `cm_enum_value` | Enumerate values by index | rdi = key_fd, rsi = index (u32), rdx = buf_ptr, r10 = buf_len -> returns value info |
| 73 | `cm_delete_key` | Delete key and all subkeys | rdi = key_fd (u64) |
| 74 | `cm_flush_key` | Flush hive to disk | rdi = key_fd (u64) |
| 75 | `cm_load_hive` | Load hive from file (admin) | rdi = path_ptr, rsi = path_len, rdx = mount_point_ptr, r10 = mount_len |
| 76 | `cm_unload_hive` | Unload hive (admin) | rdi = hive_root_fd (u64) |

Path format: `\Registry\Machine\System\CurrentControlSet\Services\...`

## Cell-Based Hive Format

File: `src/cm/hive.rs`. Each hive is a contiguous buffer (`HiveBuffer`) of cells indexed by u16 offset. Maximum 2048 cells per hive.

### CellType

| Value | Variant | Description |
|-------|---------|-------------|
| 0 | `Free` | Unallocated cell, part of free list |
| 1 | `Key` | Registry key with subkey/value links |
| 2 | `Value` | Named value with typed data |
| 3 | `Security` | Security descriptor (planned) |

### KeyCell

```
KeyCell {
    name: [u8; 255],       // null-terminated UTF-8, max 254 chars
    name_len: u16,          // actual name length in bytes
    parent_cell: u16,       // offset of parent KeyCell
    subkeys_head: u16,      // offset of first child key sibling
    subkeys_sibling: u16,   // offset of next sibling key
    values_head: u16,       // offset of first ValueCell
    sec_desc_cell: u16,     // offset of security descriptor cell
    last_write_time: u64,   // timestamp (ticks since boot)
}
```

### ValueCell

```
ValueCell {
    name: [u8; 255],        // null-terminated UTF-8
    name_len: u16,
    value_type: u32,        // REG_NONE(0), REG_SZ(1), REG_DWORD(2), REG_BINARY(3)
    data: [u8; 4096],       // inline value data
    data_len: u32,          // actual data size in bytes
    next: u16,              // offset of next ValueCell in chain
}
```

Cell addressing uses `u16` offsets relative to the start of the hive buffer. Cell 0 is reserved as the root cell. Free cells form a singly-linked list for allocation.

## Namespace

Root path: `\Registry\Machine` maps to an `ObType::Key` object in the Ob namespace. All registry operations go through the standard Ob handle/namespace layer.

```
\Registry
  \Machine              -> hive storage root
    \System
      \CurrentControlSet
        \Services
          \NeoInit
            \DefaultShell
    \Network
      \Interfaces
        \0
          DHCPEnabled
    \Control
      \WaitForNetwork
```

### CmManager

File: `src/cm/mod.rs`. `CmManager` maintains up to 8 mounted hives. Each hive is backed by a `HiveBuffer` and registered as an ObObject.

```rust
pub struct CmManager {
    hives: [Option<Hive>; 8],
}
```

Default keys created at boot (Phase 3.881):

- `\Registry\Machine\System\CurrentControlSet\Services\NeoInit\DefaultShell` = `"shell.nxe"`
- `\Registry\Machine\Network\Interfaces\0\DHCPEnabled` = `1` (REG_DWORD)
- `\Registry\Machine\Control\WaitForNetwork` = `0` (REG_DWORD)

## Persistence

`cm_flush_key` serializes the entire hive to `C:\System\Registry\<name>.hiv`. On boot, the hive is loaded from disk if the file exists; otherwise, default keys are created.

Dirty tracking: `cm_set_value` marks the containing cell as dirty via `Cell::set_dirty()`. A future `cm_flush` pass will write only dirty cells.

### Hive File Format (planned for multi-hive architecture)

| Component | File |
|-----------|------|
| SYSTEM hive | `C:\System\Registry\SYSTEM.hiv` |
| SOFTWARE hive | `C:\System\Registry\SOFTWARE.hiv` |
| SECURITY hive | `C:\System\Registry\SECURITY.hiv` |
| DEFAULT hive | `C:\System\Registry\DEFAULT.hiv` |

### Future Enhancements

- Write-ahead logging (WAL) for crash-safe transactions
- Multi-hive architecture splitting SYSTEM/SOFTWARE/SECURITY/DEFAULT
- Key ACL enforcement via `sec_desc_cell` (currently planned in `src/cm/security.rs`)

## Source Files

| File | Responsibility |
|------|---------------|
| `src/cm/mod.rs` | `CmManager` + syscall dispatch |
| `src/cm/hive.rs` | Cell-based hive buffer, key/value CRUD |
| `src/cm/security.rs` | Key ACLs (planned) |
| `src/syscall/cm.rs` | Syscall handlers for RAX 67-76 |
