# Shell Subsystem

## Architecture

Ring 3 binary `neoshell.nxe` in `userbin/neoshell/`. Spawned by NeoInit (PID 1) during Phase 4 boot. Communicates with kernel exclusively via Ob API and foundation syscalls. Runs as a regular user-mode process; no special kernel privileges.

## Command Dispatch

Two dispatch paths:

- **Built-in commands**: CWD, SET, EXIT, CALL. Handled internally by neoshell without spawning a child process.
- **PATH dispatch**: All other command names scanned against PATH directories. PATH is semicolon-delimited (`;`), read from Registry at startup. Default PATH:

  ```text
  \Programs;\System\Tools
  ```

  Search order:
  1. `{drive}:\Programs\{cmd}.NXE`
  2. `{drive}:\System\Tools\{cmd}.NXE`
  3. Current working directory

  Relative entries (starting with `\`) are resolved against the current drive. Absolute entries (`X:\...`) are used as-is. If found, neoshell spawns the binary via `sys_ob_create(PROCESS)` with stdin/stdout inherited. PATH search is fallback after built-in check.

Built-in commands are not pipeable. Only .NXE binaries can appear in pipelines.

## Process Interrupts (Ctrl+C)

While a foreground command runs, NeoShell declares the child as the foreground process of the active VT (`ObSetInfoClass::SetForegroundProcess`) and then blocks in `ObWait`. The designation is explicit: the kernel does **not** infer it from `ObWait`, because NeoInit also waits on NeoShell and that would make Ctrl+C at a fresh prompt kill the shell. Pressing Ctrl+C makes the keyboard handler (IRQ context) queue an IRQ-safe, high-priority work item, and the termination runs later from syscall context (`kill_pid` + `wake_waiters`) so `ObWait` returns and the shell reaches the prompt again. When the child exits or is terminated, the foreground marker is cleared.

This is a default terminate action, not a POSIX signal API: no handler, job control or suspend/resume. `ping /t` (continuous) is the first consumer.

## PATH Configuration

PATH is stored in the Registry at:

```text
\Registry\Machine\System\CurrentControlSet\Control\Session Manager\Environment\PATH
```

The shell reads this value at startup. If the registry key is unavailable or empty, the shell falls back to `\Programs`. The user can override PATH at runtime with `SET PATH=...`, which modifies the in-memory environment (not persisted to Registry). Future versions may persist runtime SET changes to the Registry.

## TAB Autocomplete

Callback registered via `register_completion()` against `console.nxl` (NXL slot 2, lazy-loaded by console module). The shell scans all PATH directories for case-insensitive prefix match on `.NXE` filenames. Matching candidates are printed to stdout on each TAB press. If exactly one match, the name is auto-completed in the input buffer. Built-in commands are also offered as completion candidates.

## History

`console.nxl` provides a circular buffer of 32 entries. Up arrow sends sentinel byte `0x01` to stdin; down arrow sends `0x02`. The shell interprets these sentinels and requests the previous/next entry via `history_prev()`, `history_next()` API. Entries are added via `history_add_raw()` after each command execution. History is in-memory only, not persisted to disk.

API exposed by console module:

- `history_add_raw(line: &str)`
- `history_prev() -> Option<&str>`
- `history_next() -> Option<&str>`
- `history_reset()`
- `history_get_count() -> u32`
- `history_get_entry(index: u32) -> Option<&str>`

## Pipeline Support

The `|` operator chains commands. Up to 16 commands in a single pipeline.

Pipeline flow:

1. For each `|`, neoshell calls `sys_ob_create("\Pipe/pN", PIPE)` which allocates a pipe and returns `[read_fd, write_fd]` via the `fds_out` parameter.
2. Left command spawned via `sys_ob_create(PROCESS)` with `stdout_fd` redirected to pipe write end (`attrs` encodes stdin/stdout/stderr as `si | so<<8 | se<<16`, `0xFF` = inherit).
3. Right command spawned with `stdin_fd` redirected to pipe read end.
4. Shell waits for all processes in the pipeline via `sys_ob_wait`.

Handle management: `rf`/`wf` arrays are initialized to `0xFF` (invalid). If pipe creation fails, previously created pipes are closed before returning. After each spawn the used pipe ends are closed and marked `0xFF` to avoid double-close; on pipeline error the remaining open ends are closed.

Argument passing: arguments are no longer passed solely via the shared `0x41F000` buffer. The shell still writes to `0x41F000` for backward compatibility, but the kernel copies the buffer atomically into `Eprocess.args` at `sys_ob_create(PROCESS)` time; children retrieve their args via `sys_ob_query_info(fd, ProcessArgs=39)` (`libneodos::args::read_args()`), eliminating the data race when pipeline stages are spawned concurrently.

Built-in commands (CWD, SET, EXIT, CALL) are not pipeable and produce an error if used in a pipeline.

## File Management Commands

| Command | Implementation | ABI |
| --------- | --------------- | ----- |
| DEL `<path>` | `ob_destroy` (RAX 46) on file ObObject | `sys_ob_destroy(path)` |
| REN `<src> <dst>` | `ob_set_info` with `VfsRename` info class | `sys_ob_set_info(src, VfsRename, &dst)` |
| RD `<dir>` | `ob_destroy` (RAX 46) on directory ObObject | `sys_ob_destroy(path)` |
| COPY `<src> <dst>` | `ob_query_info(ReadContent)` → buffer → `ob_set_info(WriteContent)` | read content, create/overwrite dst, write content |
| TYPE `<path>` | `ob_query_info(ReadContent)` → `sys_write` stdout | read file content to buffer, print to stdout |
| DIR `<path>` | `ob_enum` (RAX 44) on directory | `sys_ob_enum(dir, &entries)` → tabular output |
| TREE `<path>` | `ob_enum` recursive (depth-first) | `sys_ob_enum` called per subdirectory |
| CD `<path>` | `ob_set_info(SetCwd)` via `ARGS_ADDR` field; the kernel VFS path resolver canonicalizes `.` / `..`, validates existence + directory type, then commits atomically | `sys_ob_set_info(cwd_handle, SetCwd, &path)` |
| CLS | ANSI escape: `\x1B[2J\x1B[H` | `sys_write(1, escape, 7)` |
| ECHO `<text>` | `sys_write` (RAX 20) to stdout | direct write |
| MD `<dir>` | `ob_create(Directory)` (RAX 41) | `sys_ob_create(path, Directory)` |

## System Commands

| Command | Implementation |
| --------- | --------------- |
| FSCK `<drive>` | `ob_query_info(FsckStatus=33)` / `ob_set_info(FsckRepair=39)` on the Filesystem handle |
| LOADLIB `<nxl>` | `sys_loadlib` (RAX 25) — loads NXL into slot region |
| PS | `ob_enum(\Ob\Process)` then `ob_query_info` per process for name/pid/state |
| KILL `<pid>` | `ob_set_info(Process, ProcessTerminate)` on target process object |
| PRI `<pid> <level>` | `ob_set_info(Process, ProcessPriority, &level)` |
| KEYB `<layout>` | Legacy — use `NEOKEY layout <name>` instead. `ob_set_info(KeyboardLayout)` on keyboard device object. |
| POWEROFF | `ob_open(\\System\\PowerManager)` + `ob_set_info(PowerShutdown)` — via Object Manager |
| REBOOT | `ob_open(\\System\\PowerManager)` + `ob_set_info(PowerReboot)` — via Object Manager |
| VER | `ob_open(\Global\Info\Version)` → `ob_query_info` → print version string |
| VOL `<drive>` | `ob_query_info(VolumeLabel)` on volume object |
| DATE | `ob_query_info(LocalDateTime)` → formatted local print; `/U` uses `DateTime` (UTC). `datetime /S <date> <time>` (or `date`/`time /S`) sets the authoritative UTC clock via `ob_set_info(DateTime)`; admin-only, formats `DD/MM/YY HH:MM[:SS]`. Local timezone/DST is configured in `Control\TimeZoneInformation` (see registry doc) |
| TIME | Same as DATE |
| NEOMEM | `ob_open(\Global\Info\Memory)` → `ob_query_info` → print memory stats |
| DRIVES | `ob_open(\Global\Info\Drives)` → `ob_query_info` → list mounted drives |
| LABEL `<drive> <label>` | `ob_query_info(VolumeLabel)` to read, `ob_set_info(VolumeLabel)` to write |
| FSCHECK | `fsck.nxe` — user-mode wrapper invoking the Ob FsckStatus/FsckRepair classes |

## userbin/.NXE Binaries

All 42 user-mode binaries, each a standalone `.NXE` ELF file in `userbin/<name>/`:

| Binary | Category | Description |
| -------- | ---------- | ------------- |
| neoshell | core | Interactive command shell |
| neoinit | core | PID 1 — system initialization |
| neomem | monitor | Memory usage display |
| neotop | monitor | `neotop v0.2`: dynamic process/thread monitor with real CPU% from two kernel snapshots (PID/TID, names, state, CPU, idle/current, CPU% per process; `ObInfoClass::ProcessSnapshot`). Controls: `q` quit, `r` refresh |
| neotrace | monitor | System call trace viewer |
| cmdtest | test | Command dispatch test utility |
| ipconfig | network | Network interface configuration |
| ping | network | ICMP echo test |
| neologon | security | Login prompt, password auth |
| sudo | security | Privilege escalation |
| consent | security | UAC-style consent dialog |
| samutil | security | SAM database utility |
| whoami | security | Current user/SID display |
| runas | security | Run as different user |
| secedit | security | Security policy editor |
| neoedit | editor | Text editor |
| neopkg | package | Package manager |
| netcfg | network | Network configuration |
| dhcp | network | DHCP client |
| cpuinfo | system | CPU information display |
| datetime | system | Date/time display and set |
| ver | system | Version information |
| echo | utility | Print text |
| cls | utility | Clear screen |
| copy | utility | Copy files |
| del | utility | Delete files |
| ren | utility | Rename files |
| md | utility | Create directories |
| rd | utility | Remove directories |
| tree | utility | Recursive directory listing |
| type | utility | Display file contents |
| drives | utility | List mounted drives |
| keyb | utility | Keyboard layout control (legacy — use neokey) |
| neokey | utility | Keyboard management: show state, switch layout, list layouts, set repeat rate/delay, show LEDs |
| kill | system | Terminate processes |
| pri | system | Set process priority |
| label | utility | Volume label management |
| fsck | system | Filesystem check |
| progress | utility | Progress bar utility |
| kobj | debug | Kernel object tree viewer |
| cd | utility | Change directory |

## User Window Layout

Address range `0x400000..0x2400000` (32 MB total). Divided into 128 slots of 256 KB each (192 KB binary + 64 KB stack).

ASLR v1: Random slot selection via `RDRAND` instruction with `RDTSC` fallback if RDRAND unavailable. The slot index determines code base address: `0x400000 + slot * 0x40000`.

Each slot layout:

- Code section at slot base
- Stack grows downward from slot top
- Heap region within slot boundaries

## User Heap

Address range `0x10000000..0x12000000` (32 MB total). Divided into 16 slots of 2 MB each. Demand-paged at 4 KB granularity via `sys_mmap`/`sys_munmap`.

Heap managed by user-mode brk/sbrk:

- `brk(addr)`: set program break
- `sbrk(increment)`: increment program break
- Backed by `sys_brk` (RAX 10) or `sys_mmap` (RAX 11)

## Adding a New Command

1. Create `userbin/<name>/` directory with a `Cargo.toml` depending on `libneodos`
2. Implement `#![no_std]` entry point with `pub extern "C" fn _start() -> !`
3. Use `libneodos` wrappers for I/O and syscalls
4. Add the binary to NeoDev's image list (`neodev/src/image.rs`) for the new `.nxe`
5. Verify:
   - `cargo build` in `neodos-kernel/`
   - `neodev test`
6. The binary is available at `\Programs\<name>.nxe` in the built image
