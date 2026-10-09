---
name: object-manager
description: Add Ob types, extend ObInfoClass/ObSetInfoClass, modify namespace or handle management
---

# Object Manager

## When to use

Adding a new `ObType`, extending the Ob API (new `ObInfoClass` /
`ObSetInfoClass`), modifying namespace resolution, or changing handle management.

## Goal

Extend the Object Manager correctly — the central abstraction for all kernel
objects, handles, security, and namespace.

## References

- `docs/kernel/objects.md` — subsystem documentation
- `docs/architecture/source-of-truth.md` — enforceable invariants
- `src/object/types.rs` — `ObType`, `ObInfoClass`, `ObSetInfoClass`
- `src/object/table.rs` — `ObObjectTable`, `ObObject`, `ObOperations`,
  `ob_create_object`, `ob_lookup`
- `src/object/namespace/` — path resolution and mutation (`mod.rs`, `path.rs`,
  `mutate.rs`, `query.rs`)
- `src/infra/handle.rs` — per-process `HandleTable` / `HandleEntry`
- `src/syscall/ob/` — `ob_*` handlers (RAX 40-48)

## ABI facts (v8)

- Ob syscalls: `ob_open=40`, `ob_create=41`, `ob_query_info=42`,
  `ob_set_info=43`, `ob_enum=44`, `ob_wait=45`, `ob_destroy=46`,
  `ob_service=47`, `ob_snapshot=48`.
- `ObType` (0-22): 0 Unknown, 1 Process, 2 Driver, 3 Device, 4 Pipe,
  5 EventBus, 6 BlockDevice, 7 Filesystem, 8 MemoryRegion, 9 Symlink,
  10 MountPoint, 11 Directory, 12 Key, 13 Event, 14 Semaphore, 15 Timer,
  16 Thread, 17 Section, 18 Socket, 20 Service, 21 PowerManager,
  22 KeyboardDevice. `19` is free; the next unused number is `23`.
- User-creatable via `ob_create`: Process, Driver, Pipe, Directory, Event,
  Semaphore, Timer, Thread, Section, Service. Kernel-created only:
  `PowerManager(21)`, `KeyboardDevice(22)`.
- `ObInfoClass` (0-42) and `ObSetInfoClass` (0-51) — see
  `src/object/types.rs` / `docs/kernel/objects.md`.

## Steps

1. **Read the architecture docs**
   `docs/kernel/objects.md` and `docs/architecture/source-of-truth.md`.

2. **Add a new `ObType` variant (if needed)**
   Edit `src/object/types.rs` and assign the next free number. Keep exactly one
   variant per resource type.

3. **Implement `ObOperations`** (`src/object/table.rs`)

   ```rust
   pub trait ObOperations: Send + Sync {
       fn on_destroy(&self, id: ObId, native_id: u64) {}
   }
   ```

   `on_destroy` is the type-specific cleanup hook (pipe teardown, semaphore wake,
   …). Attach it to the object via `ObObject.ops = Some(&STATIC_OPS)`. The trait
   is the extension point — grow it deliberately and document additions.

4. **Add `ObInfoClass` / `ObSetInfoClass` variants (if needed)** in
   `src/object/types.rs`. Each class maps to a fixed-size, `#[repr(C)]` payload.

5. **Namespace integration** (if the object is nameable)
   Objects are created with a name and inserted at a path via the namespace
   helpers in `src/object/namespace/`; directories are `ObType::Directory` and
   act as path components. Paths use `\` and are case-insensitive.

6. **Handle lifecycle**
   - `ob_create` (41) inserts into the namespace and allocates a `HandleEntry`
     (fd) in the caller's `HandleTable` (`src/infra/handle.rs`); pipes create a
     read/write pair.
   - `ob_destroy` (46) removes from the namespace + object table and triggers
     `on_destroy` when the refcount reaches zero. It fails with `-Busy`
     (`RefCountHeld`) while handles/children remain.

7. **Register related syscalls** following the syscalls skill — all new syscalls
   are `sys_ob_*` and operate on Ob objects.

8. **Write tests** with `test_case!` in the module's `register_*_tests()`:
   create + query, open-by-name (if nameable), set-info, enumeration,
   destroy + confirm cleanup, error cases (invalid handle, wrong type, bad class).

9. **Update docs** — `docs/kernel/objects.md` (type, classes, semantics).

## Best practices

- Every resource is exactly one `ObType`; never reuse a number.
- `on_destroy()` must be idempotent (called at most once per object).
- Access objects through the global table; a handle is an opaque `u64` — never
  dereference it.
- Security is integrated: `SeAccessCheck` runs on `ob_open`; every object may
  carry a DACL.
- URN is a frontend of Ob — resolve URIs via `ob_lookup_path`.

## Common mistakes

- Implementing the wrong trait shape — `ObOperations` currently exposes only
  `on_destroy`.
- Reusing an `ObType` number (or type-punning via `native_id`).
- Leaking a handle, or dropping a handle that is still referenced.
- Exposing internal or dynamic-lifetime pointers through an info class — copy
  fixed-size data structures only.
- Relying on `Drop` for cleanup instead of `on_destroy`.

## Final checklist

- [ ] `ObType` variant added with a unique number
- [ ] `ObOperations` impl attached where cleanup is needed
- [ ] `ObInfoClass`/`ObSetInfoClass` variants added (if applicable)
- [ ] Namespace integration correct (if nameable)
- [ ] Lifecycle: create → query/set → destroy works end-to-end
- [ ] Kernel tests added and passing (`neodev test`)
- [ ] `docs/kernel/objects.md` updated
- [ ] `cargo build` succeeds; `neodev check-deps` passes
