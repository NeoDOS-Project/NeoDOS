---
name: shell
description: Add shell commands, NXE user binaries, modify NeoShell behavior
---

# Shell

## When to use

Adding a new shell command or `.NXE` user binary, or modifying NeoShell behavior.

## Goal

Add a Ring 3 `.NXE` binary or extend the shell, following the rule that all
interactive commands live in `userbin/` (AGENTS.md rule 5).

## References

- `docs/userland/shell.md`, `docs/userland/nxe-ecosystem.md`,
  `docs/userland/nxe-format.md`
- `userbin/<name>/` — one directory per binary, each with `src/main.rs`
- `userbin/neoshell/src/` — `shell.rs` (dispatch), `pipeline.rs`, `redir.rs`,
  `tokenizer.rs`, `completion.rs`, `env.rs`
- `libneodos/` — user-mode library (syscalls, console, fs, args, i18n)
- NeoDev discovers `userbin/*` and builds them (`neodev build --userbin` /
  `--image`)

## ABI facts

- Shell binary: `neoshell.nxe`. Built-ins: CWD, SET, EXIT, CALL. All other names
  dispatch via PATH (`\Programs;\System\Tools` + cwd), read at startup from
  `\Registry\Machine\System\CurrentControlSet\Control\Session Manager\Environment\PATH`.
- Ctrl+C: the shell marks the child as the VT's foreground process
  (`ObSetInfoClass::SetForegroundProcess=51`) and blocks in `ObWait`; the keyboard
  IRQ queues a high-priority work item and the kill runs from syscall context.
- Pipeline: up to 16 commands; pipes via `ob_create(Pipe)` (returns
  `[read_fd, write_fd]`), spawns with fd redirection encoded in `attrs`.
- Arguments are copied atomically into `Eprocess.args` at
  `sys_ob_create(PROCESS)`; read via `ob_query_info(ProcessArgs=39)`.

## Steps

1. **Confirm the command belongs in userspace** (rule 5). If it needs privileged
   access, add an Ob-based syscall (`sys_ob_*`, see the syscalls skill) and call it
   from the binary.

2. **Create the binary** (`userbin/mycommand/`), copying an existing binary's
   `Cargo.toml` (depends on `libneodos`):

   ```rust
   // userbin/mycommand/src/main.rs
   #![no_std]
   #![no_main]

   use libneodos::syscall;

   #[no_mangle]
   pub extern "C" fn _start() -> ! {
       // ... implementation ...
       syscall::sys_exit(0);
   }
   ```

3. **Use libneodos** for all syscall access — never invoke `INT 0x80` directly.

4. **Build and include in the image**

   ```bash
   neodev build --userbin
   neodev build --image
   ```

   Binaries land at `\Programs\<name>.nxe`.

5. **If shell internals change**, edit `userbin/neoshell/src/` (`shell.rs`,
   `pipeline.rs`, `redir.rs`, `tokenizer.rs`, `completion.rs`, `env.rs`). Keep the
   shell a launcher, not a kitchen sink.

6. **Test in QEMU**

   ```bash
   neodev build --image && neodev run
   ```

## Best practices

- `#![no_std]` + `#![no_main]`; entry point is `_start() -> !`.
- Keep commands small and statically linked; use `libneodos` patterns.
- Print errors to stderr; return 0 on success, non-zero on error.
- Add a completion candidate if the command should tab-complete.

## Common mistakes

- Implementing a privileged operation in userspace instead of adding a syscall.
- Wrong entry symbol (`main` instead of `_start`).
- Forgetting to build/place the binary (`neodev build --image`).
- Adding a Ring 0 shell command.
- Calling `INT 0x80` directly instead of using `libneodos`.

## Final checklist

- [ ] Binary under `userbin/<name>/`, `#![no_std]` + `#![no_main]`, `_start`
- [ ] Uses `libneodos` only
- [ ] Builds via `neodev build --userbin` and is present in `--image`
- [ ] No new Ring 0 shell command
- [ ] Invokable by name at the shell
- [ ] Tested in QEMU
