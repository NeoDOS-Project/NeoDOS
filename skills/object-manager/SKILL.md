---
name: object-manager
description: Add Ob types, extend ObInfoClass, modify namespace or handle management
---

# Object Manager

## When to use

Adding a new Ob type, extending the Ob API (new ObInfoClass/ObSetInfoClass variant), modifying namespace resolution, or changing handle management.

## Goal

Extend the Object Manager correctly — the central abstraction for all kernel objects, handles, security, and namespace.

## Steps

1. **Read the architecture docs**
   Open `docs/objects.md` and `docs/ARCHITECTURE_SOURCE_OF_TRUTH.md` to understand Ob design invariants.

2. **Add new ObType variant (if needed)**
   Edit `src/object/types.rs`:
   - Add variant to `ObType` enum (e.g. `MyType(24)` — next available number).
   - Assign a descriptive name for debug output if there's a name mapping function.

3. **Implement ObOperation**
   Create or extend a struct in the relevant subsystem that implements `ObOperation`:

   ```rust
   impl ObOperation for MyObject {
       fn obj_type(&self) -> ObType { ObType::MyType }
       fn close(&self) -> Status { /* cleanup */ }
       fn query_info(&self, class: ObInfoClass, buf: &mut [u8]) -> Result<(), Status> { /* ... */ }
       fn set_info(&mut self, class: ObSetInfoClass, buf: &[u8]) -> Result<(), Status> { /* ... */ }
   }
   ```

   Place in e.g. `src/object/my_object.rs` or the owning subsystem.

4. **Add ObInfoClass / ObSetInfoClass variants (if needed)**
   Edit the enums in `src/object/types.rs`. Each variant maps to a fixed-size data structure.

5. **Integration with namespace** (if object is nameable)
   In `src/object/namespace.rs`, the Ob namespace is rooted at `\`. Objects are created via `ob_create` with an `ObjectAttributes` containing the name. The namespace supports directories (`ObType::Directory`) as path components.

6. **Handle lifecycle**
   - Objects are reference-counted via handles in `ObObjectTable` (`src/object/mod.rs`).
   - `ob_create` returns a handle, `ob_destroy` drops the handle reference.
   - `ob_open` looks up by name in the namespace and returns a handle.
   - Implement `close()` in `ObOperation` to release resources when the last handle drops.

7. **Register related syscalls**
   If the new type needs dedicated syscalls, follow the syscalls skill (RAX >= 77, `sys_ob_*` naming).

8. **Write tests**
   Add tests in `src/testing.rs` for:
   - Create and query info
   - Open by name (if nameable)
   - Set info
   - Enumeration
   - Destroy and confirm cleanup
   - Error cases (invalid handles, wrong type, bad info class)

9. **Update docs**
   Update `docs/objects.md` with the new type, its ObInfoClass variants, and operation semantics.

## Best practices

- Every Ob type must have exactly one `ObType` variant — no sharing.
- `ObOperation::close()` must be idempotent (called at most once per object).
- Use `ObObjectTable::with_handle()` for safe handle-to-object access.
- Namespace paths use `\` separators, case-insensitive.
- Handle values are opaque u64 — never dereference them directly.

## Common mistakes

- Forgetting to implement `ObOperation` — the object can't be managed by the Ob infrastructure.
- Using the same ObType number twice.
- Not calling `close()` or relying on Drop for cleanup — Ob uses explicit reference counting.
- Exposing internal pointers via ObInfoClass — only copy fixed-size data structures.
- Allowing handle leak (creating without tracking) or use-after-free (dropping handle while still referenced).

## Final checklist

- [ ] `ObType` variant added with unique number
- [ ] `ObOperation` implemented for the new type
- [ ] `ObInfoClass`/`ObSetInfoClass` variants added (if applicable)
- [ ] Namespace integration correct (if nameable)
- [ ] Lifecycle: create → query/set → destroy works end-to-end
- [ ] Kernel tests added and pass
- [ ] `docs/objects.md` updated
- [ ] `cargo build` succeeds, `scripts/check_deps.py` passes
