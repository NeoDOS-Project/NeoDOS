---
name: syscalls
description: Add or modify syscall handlers, SSDT dispatch, permission table, libneodos wrappers
---

# Syscalls

## When to use

You are adding a new syscall, modifying an existing syscall handler, or changing
the SSDT dispatch mechanism.

## Goal

Correctly implement a syscall with proper dispatch, argument handling, Ob
integration, permissions, and documentation.

## ABI facts

- ABI **v8**. `RAX` = syscall number; args `RBX, RCX, RDX, R8, R9`; return in
  `RAX` (`>= 0` success, `< 0` error via `err_to_u64(SyscallError::…)`).
- `SyscallNum` (`#[repr(u64)]`) and the SSDT live in `src/syscall/mod.rs`.
  Handler type is `SyscallFn = fn(Registers) -> u64` (`src/syscall/table.rs`).
- Current numbering (frozen): Process `0-4`, Memory `10-12`, I/O `20-25`,
  Console `30`, Driver `35-36`, Object Manager `40-48`, Registry `50-59`,
  Debug `99`. `MAX_VALID = HIGHEST_ASSIGNED = 99`.
- **Architecture rule: every new syscall MUST be `sys_ob_*`** — it operates on Ob
  objects, receives/returns Ob handles, and uses `ob_query_info`/`ob_set_info`
  for data. There is **no numeric threshold** (`docs/kernel/syscalls.md`,
  AGENTS.md rule 6).

## Steps

1. **Assign the number and extend `SyscallNum`**
   Edit `src/syscall/mod.rs`: add the variant to the `#[repr(u64)] enum
   SyscallNum` and its arm in `SyscallNum::from_u64`. Pick the next free slot in
   the correct category range.

2. **Implement the handler** in the right module:
   - Ob syscalls → `src/syscall/ob/` (`open.rs`, `create/`, `query/`, `set/`,
     `enum.rs`, `wait.rs`, `destroy.rs`).
   - Registry → `src/syscall/cm.rs`.
   - Generic/foundation → `src/syscall/handlers.rs`.

   Signature:

   ```rust
   pub fn handler_ob_xxx(regs: crate::syscall::Registers) -> u64 {
       let arg0 = regs.rbx;
       // Validate args, then delegate to the subsystem.
       match subsystem::do_work() {
           Ok(v) => v,
           Err(e) => crate::syscall::err_to_u64(crate::syscall::SyscallError::Inval),
       }
   }
   ```

   Validate every argument: user pointers via `util::is_user_ptr_valid`, valid
   handles via the process handle table, enum ranges. Keep handlers short —
   delegate to subsystem functions.

3. **Register in the SSDT** — add to the `SYSCALL_TABLE` lazy_static:

   ```rust
   t[N] = Some(handler_ob_xxx as SyscallFn);
   ```

4. **Set the permission** in the `SYSCALL_PERMISSIONS` lazy_static:

   ```rust
   t[N] = SyscallPermission::user();   // or ::admin() for admin-only, or leave free()
   ```

   (`SyscallPermission` lives in `src/syscall/permission.rs`.) Admin syscalls are
   checked by `check_syscall_permission` before dispatch.

5. **Add the number to `validate_abi()`** — append `N` to the `ASSIGNED` slice so
   boot Phase 3.9 fails fast if the handler is missing.

6. **Ob integration** — use `ob_open` / `ob_create` / `ob_query_info` /
   `ob_set_info` / `ob_enum` / `ob_wait` / `ob_destroy`. Add `ObInfoClass` /
   `ObSetInfoClass` variants in `src/object/types.rs` when you need a new payload.

7. **Add a libneodos wrapper** in `libneodos/src/syscall/` following existing
   patterns, so Ring 3 programs can call it.

8. **Update public API docs**
   - `docs/kernel/syscalls.md` (index, numbering, migration history).
   - `docs/kernel/objects.md` for new Ob classes.
   - Bump the ABI in `AGENTS.md` on a breaking change.

9. **Write kernel tests** with `test_case!` in the module's `register_*_tests()`
   (e.g. `register_ob_set_tests`). Cover success and error paths.

10. **Build and test**

    ```bash
    cd neodos-kernel && cargo build
    neodev build --quick --image && neodev test
    neodev check-deps
    ```

## Best practices

- Validate every argument: null pointers, invalid handles, out-of-range enums.
- Ob syscalls must check handle validity through the process handle table before
  dereferencing.
- Prefer reusing existing `ObInfoClass`/`ObSetInfoClass` classes before adding new
  ones.
- Free syscall numbers in the category range; never reuse a retired number.

## Common mistakes

- Adding a handler but forgetting `SYSCALL_TABLE`, `SYSCALL_PERMISSIONS`, or the
  `ASSIGNED` slice in `validate_abi()`.
- Using the historical "RAX >= 77" heuristic — the rule is Ob-based, not numeric.
- Returning `Result<...>` instead of `u64` (`SyscallFn = fn(Registers) -> u64`).
- Forgetting the libneodos wrapper, so userspace cannot call it.
- Touching the INT 0x80 entry (`syscall_handler_asm` in
  `src/arch/x64/idt/mod.rs`) without updating the SSDT.

## Final checklist

- [ ] `SyscallNum` variant + `from_u64` arm added
- [ ] Handler implemented with argument validation
- [ ] Registered in `SYSCALL_TABLE`
- [ ] Permission set in `SYSCALL_PERMISSIONS`
- [ ] Number listed in `validate_abi()` `ASSIGNED`
- [ ] New syscall is Ob-based (`sys_ob_*` naming)
- [ ] libneodos wrapper added
- [ ] `docs/kernel/syscalls.md` (+ `objects.md`) updated
- [ ] Kernel tests added; `cargo build`, `neodev test`, `neodev check-deps` pass
