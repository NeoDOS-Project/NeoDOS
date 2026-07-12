# Shell Subsystem

## Architecture

Ring 3 binary `neoshell.nxe` in `userbin/neoshell/`. Spawned by NeoInit (PID 1) during Phase 4 boot. Communicates with kernel exclusively via Ob API and foundation syscalls. Runs as a regular user-mode process; no special kernel privileges.

## Command Dispatch

Two dispatch paths:

- **Built-in commands**: CWD, SET, POWEROFF, EXIT, CALL. Handled internally by neoshell without spawning a child process.
- **PATH dispatch**: All other command names scanned against `\Programs\*.NXE` path. If found, neoshell spawns the binary via `sys_spawn` (RAX 7) with stdin/stdout inherited. PATH search is fallback after built-in check.

Built-in commands are not pipeable. Only .NXE binaries can appear in pipelines.

## TAB Autocomplete

Callback registered via `register_completion()` against `console.nxl` (NXL slot 2, lazy-loaded by console module). The shell scans PATH directories for case-insensitive prefix match on `.NXE` filenames. Matching candidates are printed to stdout on each TAB press. If exactly one match, the name is auto-completed in the input buffer.

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

1. For each `|`, neoshell calls `sys_pipe` (RAX 5) which allocates a 4 KB kernel pipe buffer and returns `[read_fd, write_fd]`.
2. Left command spawned via `sys_spawn` with `stdout_fd` redirected to pipe write end.
3. Right command spawned with `stdin_fd` redirected to pipe read end.
4. Shell waits for all processes in the pipeline via `sys_waitpid` per process.

Built-in commands (CWD, SET, CD, etc.) are not pipeable and produce an error if used in a pipeline.

## File Management Commands

| Command | Implementation | ABI |
| --------- | --------------- | ----- |
| DEL `<path>` | `ob_destroy` (RAX 66) on file ObObject | `sys_ob_destroy(path)` |
| REN `<src> <dst>` | `ob_set_info` with `VfsRename` info class | `sys_ob_set_info(src, VfsRename, &dst)` |
| RD `<dir>` | `ob_destroy` (RAX 66) on directory ObObject | `sys_ob_destroy(path)` |
| COPY `<src> <dst>` | `ob_query_info(ReadContent)` → buffer → `ob_set_info(WriteContent)` | read content, create/overwrite dst, write content |
| TYPE `<path>` | `ob_query_info(ReadContent)` → `sys_write` stdout | read file content to buffer, print to stdout |
| DIR `<path>` | `ob_enum` (RAX 64) on directory | `sys_ob_enum(dir, &entries)` → tabular output |
| TREE `<path>` | `ob_enum` recursive (depth-first) | `sys_ob_enum` called per subdirectory |
| CD `<path>` | `ob_set_info(SetCwd)` via `ARGS_ADDR` field | `sys_ob_set_info(cwd_handle, SetCwd, &path)` |
| CLS | ANSI escape: `\x1B[2J\x1B[H` | `sys_write(1, escape, 7)` |
| ECHO `<text>` | `sys_write` (RAX 1) to stdout | direct write |
| MD `<dir>` | `ob_create(Directory)` (RAX 61) | `sys_ob_create(path, Directory)` |

## System Commands

| Command | Implementation |
| --------- | --------------- |
| FSCK `<drive>` | `sys_fsck` (RAX 55) — invokes kernel fsck on specified drive |
| LOADLIB `<nxl>` | `sys_loadlib` (RAX 21) — loads NXL into slot region |
| NDREG | Opens `\Global\Info\Drivers` — reads driver registration info |
| PS | `ob_enum(\Ob\Process)` then `ob_query_info` per process for name/pid/state |
| KILL `<pid>` | `ob_set_info(Process, ProcessTerminate)` on target process object |
| PRI `<pid> <level>` | `ob_set_info(Process, ProcessPriority, &level)` |
| KEYB `<layout>` | Legacy — use `NEOKEY layout <name>` instead. `ob_set_info(KeyboardLayout)` on keyboard device object. |
| POWEROFF | `ob_open(\\System\\PowerManager)` + `ob_set_info(PowerShutdown)` — via Object Manager |
| REBOOT | `ob_open(\\System\\PowerManager)` + `ob_set_info(PowerReboot)` — via Object Manager |
| VER | `ob_open(\Global\Info\Version)` → `ob_query_info` → print version string |
| VOL `<drive>` | `ob_query_info(VolumeLabel)` on volume object |
| DATE | `ob_open(\Global\Info\DateTime)` → `ob_query_info` → formatted print |
| TIME | Same as DATE |
| NEOMEM | `ob_open(\Global\Info\Memory)` → `ob_query_info` → print memory stats |
| DRIVES | `ob_open(\Global\Info\Drives)` → `ob_query_info` → list mounted drives |
| LABEL `<drive> <label>` | `ob_query_info(VolumeLabel)` to read, `ob_set_info(VolumeLabel)` to write |
| FSCHECK | `fsck.nxe` — user-mode wrapper invoking `sys_fsck` |

## userbin/.NXE Binaries

All 42 user-mode binaries, each a standalone `.NXE` ELF file in `userbin/<name>/`:

| Binary | Category | Description |
| -------- | ---------- | ------------- |
| neoshell | core | Interactive command shell |
| neoinit | core | PID 1 — system initialization |
| neomem | monitor | Memory usage display |
| neotop | monitor | Process list (top-like) |
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
| ndreg | system | Driver registration viewer |
| loadnem | system | Load NEM drivers |
| progress | utility | Progress bar utility |
| kobj | debug | Kernel object tree viewer |
| cd | utility | Change directory |

## User Window Layout

Address range `0x400000..0x2400000` (32 MB total). Divided into 32 slots of 128 KB each.

ASLR v1: Random slot selection via `RDRAND` instruction with `RDTSC` fallback if RDRAND unavailable. The slot index determines code base address: `0x400000 + slot * 0x20000`.

Each slot layout:

- Code section at slot base
- Stack grows downward from slot top
- Heap region within slot boundaries

## User Heap

Address range `0x10000000..0x12000000` (32 MB total). Divided into 16 slots of 2 MB each. Demand-paged at 4 KB granularity via `sys_mmap`/`sys_munmap`.

Heap managed by user-mode brk/sbrk:

- `brk(addr)`: set program break
- `sbrk(increment)`: increment program break
- Backed by `sys_brk` (RAX 18) or `sys_mmap` (RAX 19)

## Adding a New Command

1. Create `userbin/<name>/` directory with a `Cargo.toml` depending on `libneodos`
2. Implement `#![no_std]` entry point with `pub extern "C" fn _start() -> !`
3. Use `libneodos` wrappers for I/O and syscalls
4. Add build rule in `scripts/build.sh` for the new `.NXE`
5. Verify:
   - `cargo build` in `neodos-kernel/`
   - `python3 scripts/auto_test.py`
6. The binary is available at `\Programs\<name>.NXE` in the built image
