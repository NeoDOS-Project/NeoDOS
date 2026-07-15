---
name: syscalls
description: Add or modify syscall handlers, SSDT dispatch, libneodos wrappers
---

# Syscalls

## When to use

You are adding a new syscall, modifying an existing syscall handler, or changing the syscall dispatch mechanism.

## Goal

Correctly implement a syscall with proper dispatch, argument handling, Ob integration, and documentation.

## Steps

1. **Check AGENTS.md rule 6**
   New syscalls with RAX >= 77 MUST be `sys_ob_*` — operate on Ob objects, receive/return Ob handles.

2. **Assign syscall number**
   Open `src/syscall/mod.rs` and add the entry to the SSDT dispatch table. Add to `SyscallNum` enum if it uses named constants.

3. **Implement handler**
   Create the handler function in `src/syscall/`. Signature pattern:

   ```rust
   pub fn sys_xxx(args: &mut SyscallArgs) -> Result<u64, Status>
   ```

   Arguments arrive in RBX, RCX, RDX, RSI, R8, R9 (mapped to `SyscallArgs` fields).
   Return value in RAX. Return `Ok(0)` on success, `Err(Status::xxx)` on failure.

4. **For Ob syscalls (RAX 60-66 or new)**
   - Handler goes in `src/object/dispatch.rs` or alongside the relevant Ob type.
   - Use `ob_open`, `ob_create`, `ob_query_info`, `ob_set_info`, `ob_enum`, `ob_wait`, `ob_destroy` patterns.
   - ObInfoClass / ObSetInfoClass enums in `src/object/types.rs` control what info can be queried/set.
   - New Ob types go in `src/object/types.rs` (`ObType` enum) and need an `ObOperation` impl.

5. **Update public API docs**
   - Add entry to `docs/syscalls.md` syscall table.
   - If a new ObInfoClass variant: update `docs/objects.md`.
   - If ABI change: update AGENTS.md version and ABI constants.

6. **Add libneodos wrapper** (if userspace-accessible)
   Add wrapper function in `libneodos/src/` following existing patterns.

7. **Write kernel tests**
   Add `fn test_xxx_syscall()` to the appropriate group in `src/testing.rs`. Test success and error paths.

8. **Build and test**

   ```bash
   cargo build && python3 scripts/auto_test.py
   ```

## Best practices

- Validate every argument: null pointers, invalid handles, out-of-range enums. Return `STATUS_INVALID_PARAMETER`.
- Ob syscalls should check handle validity via `ObObjectTable` before dereferencing.
- Keep handlers short — delegate to subsystem functions.
- Use `SyscallArgs` helper methods for safe argument extraction.

## Common mistakes

- Forgetting to update the SSDT table count after adding a syscall.
- Returning `()` instead of `Result<u64, Status>` — the dispatcher expects a u64 in RAX.
- Not adding the wrapper to `libneodos/` — userspace programs can't call it.
- Modifying the dispatch mechanism (INT 0x80 handler in `src/interrupts/syscall_handler.rs`) without updating the SSDT.

## Final checklist

- [ ] RAX number assigned, no conflicts (check `SyscallNum` and SSDT)
- [ ] Handler implemented with argument validation
- [ ] Ob syscalls follow `sys_ob_*` naming (RAX >= 77)
- [ ] Handler registered in `src/syscall/mod.rs` SSDT
- [ ] `docs/syscalls.md` updated
- [ ] libneodos wrapper added (if applicable)
- [ ] Kernel tests added and pass
- [ ] `scripts/check_deps.py` passes
