# File Seek / Read-At-Offset — Design Document

> **Version:** v0.1-draft
> **Status:** Design
> **Target Release:** v0.52+
> **Issue:** [#468](https://github.com/NeoDOS-Project/NeoDOS/issues/468)
> **ABI Impact:** New `ObSetInfoClass` (57), new `ObInfoClass` (48)

---

## 1. Problem Analysis

### 1.1 Current Limitation

File reads are strictly sequential. The per-handle offset lives in the process
handle table and only ever advances:

```rust
// neodos-kernel/src/syscall/ob/query/fs.rs (ReadContent path)
ep.handle_table[fd as usize].offset += bytes_read as u64;
```

`libneodos/src/fs.rs` exposes only `open`/`read`/`write`/`close`; there is no
`seek`. A new syscall exists neither in the SSDT nor in `ObSetInfoClass`.

### 1.2 Why It Matters

Doom's `W_ReadLump` seeks to a lump offset and reads it; the WAD directory is
an index, not a stream. The same applies to any format with an index (ZIP, ELF,
databases). Without seek, a client must read the whole WAD into memory (viable
with the 32 MB mmap region, but wasteful and slow) or reopen the file and
discard bytes.

### 1.3 Scope

Design **positioning** for file handles (`seek`) and a convenience
`read_at`. Directory/pipe/device handles are explicitly out of scope.

---

## 2. Design

### 2.1 Handle Offset Is Already Per-fd

Source-of-truth Rule 4.2.4 guarantees each file handle carries its own
`offset: u64`. Seek therefore only needs to **set** that field; the subsequent
`ReadContent`/`WriteContent` paths already use it.

### 2.2 API

#### ObSetInfoClass — Seek (57)

Input buffer:

```rust
#[repr(C)]
pub struct SeekInfo {
    pub offset: i64,   // signed delta or absolute
    pub whence: u32,   // 0 = SET, 1 = CUR, 2 = END
    pub _pad: u32,
}
```

Behavior:

1. Resolve the handle; require a file object (`-InvalidType` otherwise).
2. Compute the target from `whence`:
   - `SET`: `offset`
   - `CUR`: `current + offset`
   - `END`: `size + offset`
3. Reject negative results, `> i64::MAX`, or beyond `size` for `SET`/`END`
   (`-InvalidParam`). Seeking past EOF for `CUR`/`SET` is allowed (sparse) only
   if writes are allowed; reads past EOF return 0.
4. Store the new offset; return the new absolute offset.

#### ObInfoClass — ReadAt (48)

Input buffer:

```rust
#[repr(C)]
pub struct ReadAtQuery {
    pub offset: u64,
    pub len: u32,
    pub _pad: u32,
    // bytes follow in the same user buffer
}
```

Reads `len` bytes at `offset` **without changing the handle offset**. Returns
bytes read (0 at EOF). Useful for indexed readers that do not want to perturb a
sequential cursor.

### 2.3 libneodos

`libneodos/src/fs.rs`:

```rust
pub const SEEK_SET: u32 = 0;
pub const SEEK_CUR: u32 = 1;
pub const SEEK_END: u32 = 2;

impl File {
    pub fn seek(&mut self, offset: i64, whence: u32) -> Result<u64, i64>;
    pub fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, i64>;
}
```

`read_at` may be emulated as `seek` + `read` + restore in userland for old
kernels; the kernel-side class is the preferred path.

---

## 3. Alternatives Considered

- **A new `sys_lseek` syscall**: rejected — new syscalls MUST be `sys_ob_*`;
  `ob_set_info` already targets file handles.
- **Reopen + skip**: O(offset) per access and cannot express `SEEK_END`.
  Rejected.
- **Whole-file mmap**: works for the WAD but does not generalize and consumes
  the 32 MB mmap budget. Kept as a client-side option, not the API.

---

## 4. Affected Components

| Subsystem | Change | Impact |
|-----------|--------|--------|
| `syscall/ob/set/mod.rs` | `Seek` handler | Low |
| `syscall/ob/query/fs.rs` | `ReadAt` handler | Low |
| `object/types.rs` | Classes 48, 57 | Low |
| VFS handle table | Reuse existing `offset` | None |
| `libneodos/src/fs.rs` | `seek`, `read_at`, constants | Low |
| `docs/filesystem/overview.md` | Document positioning | Low |

---

## 5. API Contract

| Operation | Preconditions | Returns / Errors |
|-----------|---------------|------------------|
| `Seek(SET, off)` | file fd, WRITE if writing | new offset; `-InvalidParam` if < 0 |
| `Seek(CUR, d)` | file fd | new offset; `-InvalidParam` if result < 0 |
| `Seek(END, d)` | file fd | new offset; `-InvalidParam` if result < 0 |
| `ReadAt(off, len)` | file fd, READ | bytes read (0 at EOF); `-Fault` on bad buffer |

Seek on a pipe/directory/device fd MUST return `-InvalidType`.

---

## 6. Test Plan

| Test | Expected |
|------|----------|
| `seek_set_reads_from_offset` | Read after `SET` returns the right bytes |
| `seek_cur_relative` | `CUR` advances relative to current |
| `seek_end` | `END` positions at size |
| `seek_negative_rejected` | Negative target → `-InvalidParam` |
| `seek_past_eof_read_zero` | Read past EOF returns 0 |
| `seek_per_handle_independent` | Two fds on one file keep separate offsets |
| `read_at_does_not_move_cursor` | `read_at` leaves the sequential offset intact |
| `seek_on_pipe_invalid` | Pipe fd → `-InvalidType` |
| `seek_write_then_read` | Write at seek position updates content |

---

## 7. Files and Modules

### Modified

| Path | Change |
|------|--------|
| `neodos-kernel/src/syscall/ob/set/mod.rs` | `Seek` |
| `neodos-kernel/src/syscall/ob/query/fs.rs` | `ReadAt` |
| `neodos-kernel/src/object/types.rs` | `ReadAt=48`, `Seek=57` |
| `libneodos/src/fs.rs` | `seek`/`read_at` + constants |
| `docs/filesystem/overview.md` | Positioning semantics |

---

## 8. Implementation Plan

1. Add the enum variants.
2. Implement `Seek` with bounds validation + tests.
3. Implement `ReadAt` reusing the `ReadContent` copy path with an explicit
   offset + tests.
4. libneodos wrappers.
5. Docs + tests + markdownlint.

---

## 9. Open Questions

1. Should `Seek` return the new offset (proposed) or just 0? Returning the
   offset removes a follow-up query.
2. Sparse writes past EOF: supported or rejected? (Proposed: reject for v1.)
3. Should `ReadAt` be one class that copies bytes, or a separate mmap-view
   class? (Proposed: copy; mmap already exists.)

---

## 10. Dependencies

- VFS handle table + `ReadContent`/`WriteContent` (`syscall/ob`).
- Object Manager info/set classes.
- No scheduler/block-driver dependencies (INV-1).
