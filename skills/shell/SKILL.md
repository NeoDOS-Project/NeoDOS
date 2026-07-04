# Shell

## When to use
Adding a new shell command, modifying shell behavior, or creating a new user binary.

## Goal
Add a new .NXE user binary or extend the shell with a command, following the rule that all interactive commands go to userbin/ as Ring 3 binaries.

## Steps

1. **Confirm the command belongs in userspace**
   AGENTS.md rule 5: "No new Ring 0 shell commands. All interactive commands go to userbin/ as .NXE Ring 3 binaries."
   If the command needs privileged access, add an Ob-based syscall (RAX >= 77) and call it from the .NXE binary.

2. **Create the user binary**
   ```bash
   mkdir -p userbin/mycommand/src
   ```
   Create `userbin/mycommand/src/main.rs`:
   ```rust
   #![no_std]
   #![no_main]

   use libneodos::*;

   #[no_mangle]
   pub extern "C" fn main() -> i32 {
       // ... implementation ...
       0
   }
   ```
   Use libneodos wrappers for syscalls (ob_open, ob_create, console_write, etc.).

3. **Add to build system**
   Edit `userbin/Makefile` or the build script to compile the new binary.
   For `--neodos-image` builds: ensure your binary is included in the image.

4. **Register with shell** (if it should be invokable by name)
   The shell (`userbin/neoshell/src/main.rs`) resolves commands by searching for `.NXE` binaries in `\System\Bin`. Just placing the binary in the image at `\System\Bin\mycommand.nxe` makes it available.
   Optionally add tab completion by extending the shell's completer.

5. **If shell internals change** (`userbin/neoshell/src/`)
   - Pipeline support: `userbin/neoshell/src/pipe.rs`
   - Argument parsing: `userbin/neoshell/src/args.rs`
   - TAB completion: `userbin/neoshell/src/completer.rs`
   Keep the shell itself minimal — it's a launcher, not a kitchen sink.

6. **Build with user binaries**
   ```bash
   bash scripts/build.sh --neodos-image
   ```
   Test in QEMU: run `mycommand` at the shell prompt.

## Best practices
- Use libneodos for all syscall access — never invoke INT 0x80 directly.
- Follow existing command patterns (e.g., `ls`, `cat`, `ps` in `userbin/`).
- Print errors to stderr using libneodos console_write or logging.
- Command return codes: 0 for success, non-zero for errors.
- Keep binaries small and statically linked — no dynamic linking in userspace yet.

## Common mistakes
- Implementing privileged operations in userspace — add a syscall instead.
- Forgetting `#![no_std]` and `#![no_main]` — there's no libc or standard main.
- Not linking libneodos correctly — check the binary's Makefile/cargo config.
- Placing the binary in the wrong image directory — `\System\Bin` for commands, `\System\Drivers` for NEM drivers.
- Hardcoding paths instead of using the shell path resolution.

## Final checklist
- [ ] Binary is in `userbin/` as a Ring 3 `.NXE` executable
- [ ] `#![no_std]` and `#![no_main]` set correctly
- [ ] Build system updated to compile the new binary
- [ ] Binary included in `--neodos-image`
- [ ] No Ring 0 shell commands added
- [ ] Shell can invoke the command by name
- [ ] Tested in QEMU
