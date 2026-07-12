# NeoDOS Shell — Improvements Design Document

**Status:** Draft
**Version:** v0.1
**Target release:** v0.50

---

## 1. Problem Analysis

### Current Architecture

`neoshell.nxe` (`userbin/neoshell/src/main.rs`, 567 lines) is a Ring 3 binary spawned by NeoInit (PID 1). Its core is a REPL loop:

1. Read kernel version from `\Global\Info\Version`
2. Register TAB completion callback via `console.nxl`
3. Loop: `prompt()` → `readline()` → `execute_line()`

Two dispatch paths:

- **Built-in commands** (5): CWD, SET, EXIT, POWEROFF, CALL — handled internally
- **PATH dispatch**: All other commands probed against `\Programs\*.NXE` via `ob_open`

### Specific Limitations

**Hardcoded limits that constrain users:**

| Limit | Value | Impact |
| ------- | ------- | -------- |
| Line buffer | 256 bytes | Commands with long paths fail silently |
| Environment vars | 16 (array) | Complex scripts run out of slot |
| Env key length | 32 bytes | Descriptive names truncated |
| Env value length | 128 bytes | Long paths/values truncated |
| Pipeline segments | 16 (array) | Reasonable, but hardcoded |
| Completion path buf | 256 bytes | Deep paths truncate |

**Missing user-facing features:**

1. **No quoting/escaping** — `echo "hello world"` gets 3 tokens, not 1. No `^` escape.
2. **No redirection** — `>` (stdout to file), `>>` (append), `<` (stdin from file), `2>&1` (stderr merge). Only `|` pipes exist.
3. **No `;` separator** — `cd ..; dir` is two commands on one line.
4. **No `&&`/`||` chaining** — conditional execution.
5. **No filename/directory completion** — only command name completion.
6. **No Ctrl-A/E/K/U** — no line editing beyond backspace and up/down.
7. **No environment expansion** — `echo %PATH%` doesn't work; only `SET PATH` to view.
8. **No batch scripting** — CALL reads lines but no `IF`, `GOTO`, `FOR`, `SHIFT`.
9. **No command history persistence** — history is in-memory only (console.nxl), lost on reboot.
10. **No i18n** — error messages hardcoded to English.

**Internal design issues:**

1. **Shared memory args at 0x41F000** — fixed physical address, race-prone pattern. Shell writes args there, spawns child, child reads from same address. `CD.NXE` communicates back via the same buffer. Works synchronously but prevents concurrent spawn patterns.

2. **PATH re-scanned every command** — `resolve_path()` calls `ob_open` on each PATH directory for every command. No caching.

3. **Mutable static `COMPL_PATH`** — global mutable `[u8; 256]` written by `run()` and read by `shell_complete()` callback with no synchronization.

4. **Pipeline doesn't wait** — all commands spawned simultaneously; shell doesn't wait for completion or report exit codes. The `rf`/`wf` arrays index by `u8` (cast from `isize`).

5. **History owned by console.nxl** — shell delegates all history management. Shell can't persist history, control buffer size, or implement search (Ctrl-R).

6. **Cursor hack** — uses `\x5F` underscore as cursor, erases with `\x08 \x08`. No real cursor positioning. No ANSI terminal state tracking.

7. **Legacy syscalls in documentation** — RAX 5 (pipe), 7 (spawn), 9 (waitpid) removed from SSDT but still referenced in docs. Shell already uses Ob equivalents — this is purely a doc issue.

### Why Existing Abstractions Can't Solve These

| Problem | Why current abstractions fall short |
| --------- | ------------------------------------- |
| Redirection | No `>` operator : current args split on spaces/TABs only. No file-open-on-spawn semantic. Would need `ob_create` with stdin/stdout override from file fd. |
| Line editing | `console.nxl`'s `read_byte()` is single-key primitive. The shell builds its own line editor on top. A proper line editor needs buffering, cursor position tracking, and escape sequence parsing. |
| Env expansion | Shell parses tokens but has no `%VAR%` substitution phase. Would need a post-tokenization pass. |
| Batch scripting | No control flow in shell. IF/GOTO requires a mini-interpreter for batch files. |
| Persistent history | History lives in `console.nxl`'s ring buffer. Shell would need its own history manager with file I/O. |

---

## 2. Solution Design

### 2.1 New Types and Structures

```rust
// ── Shell configuration ──
pub struct ShellConfig {
    pub line_buf_size: usize,       // default: 4096
    pub max_env: usize,             // default: 64
    pub history_size: usize,        // default: 128
    pub path_cache_ttl: u64,        // default: 5 seconds
    pub history_file_path: &str,    // default: "C:\System\neoshell.hst"
}

// ── Environment store (replaces fixed [EnvVar; 16]) ──
pub struct EnvStore {
    vars: Vec<EnvVar>,              // dynamic: no hard limit
}
pub struct EnvVar {
    key: [u8; 64],                  // 2x current 32-byte limit
    value: [u8; 256],              // 2x current 128-byte limit
}

// ── Token types (for quoting/redirection parser) ──
pub enum ShellToken {
    Word(Vec<u8>),                  // regular argument
    RedirectStdout { target: Vec<u8> },
    RedirectAppend { target: Vec<u8> },
    RedirectStdin { target: Vec<u8> },
    RedirectStderr { target: Vec<u8> },  // 2>
    Pipe,
    And,                            // && (future)
    Or,                             // || (future)
    Semicolon,                      // ;
}

// ── Pipeline result ──
pub struct PipelineResult {
    pub exit_codes: Vec<i64>,
    pub timed_out: bool,
}

// ── Completion context (replaces mutable static) ──
pub struct CompletionCtx {
    pub path: String,
    pub drive: u8,
    pub candidates: Vec<Vec<u8>>,
    pub current_word: Vec<u8>,
}
```

### 2.2 New Files

| Path | Purpose |
| ------ | --------- |
| `userbin/neoshell/src/tokenizer.rs` | Quoting-aware tokenizer: parse `"`, `^`, `%VAR%`, `>`, `>>`, `<`, `\|`, `;` |
| `userbin/neoshell/src/env.rs` | Environment variable store: `EnvStore`, `%VAR%` expansion, SET parsing |
| `userbin/neoshell/src/redir.rs` | I/O redirection logic: open file fd, replace stdin/stdout/stderr before spawn |
| `userbin/neoshell/src/editor.rs` | Line editor: ANSI cursor control, Ctrl-A/E/K/U, insertion mode, search |
| `userbin/neoshell/src/history.rs` | History manager: ring buffer (shell-owned), file persistence on disk |
| `userbin/neoshell/src/completion.rs` | Completion engine: command + filename completion, thread-safe context |
| `userbin/neoshell/src/batch.rs` | Batch script interpreter: IF, GOTO, FOR, SHIFT, labels |
| `userbin/neoshell/src/pipeline.rs` | Pipeline manager: spawn, wait, collect exit codes, error propagation |

### 2.3 Changes to Existing Files

| File | Change |
| ------ | -------- |
| `userbin/neoshell/src/main.rs` | Reduce to minimal entry: `Shell::new(config)` → `shell.run()`. Extract all subsystems to new modules. |
| `userbin/neoshell/Cargo.toml` | No new external dependencies needed. |
| `libneodos/src/args.rs` | Add `write_args_to_addr()` for backward compat with old binaries. |
| `libneodos/src/syscall.rs` | Add `sys_ob_create_with_fds()` helper for spawn+redirection in one call (see API contract). |
| `docs/shell.md` | Update to reflect new features. |
| `scripts/build.sh` | No changes needed (same build pattern). |

### 2.4 No New Syscalls or Ob Types

All improvements are user-mode only. The shell uses existing Ob API:

- `ob_open` / `ob_create` for file redirection
- `ob_create(PROCESS, attrs=fds)` for spawn with fd encoding
- `ob_wait` for pipeline synchronization
- `ob_query_info` / `ob_set_info` for CWD, env

The `ARGS_ADDR` (0x41F000) shared memory is replaced by encoding arguments in the spawn attributes bitfield (extending the `pk` encoding). Old binaries that read from `0x41F000` keep working — the shell writes args there for backward compatibility, but new binaries receive args via a new Ob mechanism (see API contract below).

### 2.5 Tokenizer Design

```rust
/// Parse a command line into tokens, respecting quoting and redirection.
/// Returns (tokens, errors) where errors is a list of parse warnings.
pub fn tokenize(input: &[u8]) -> (Vec<ShellToken>, Vec<&str>);

// State machine states:
enum TokenizerState {
    Normal,         // reading a word
    SingleQuote,    // inside '...' — literal, no expansion
    DoubleQuote,    // inside "..." — expands %VAR%
    Escape,         // after ^ — next char is literal
    Percent,        // after % — reading env var name
}
```

**Rules:**

- `"hello world"` → single token `hello world`
- `'hello %PATH%'` → single literal token `hello %PATH%` (no expansion)
- `echo hello ^| more` → `echo`, `hello |`, `more` (pipe is escaped)
- `cmd1; cmd2` → two commands, sequential
- `%PATH%` → expands to value of PATH env var
- `%%` → literal `%`

### 2.6 Redirection Design

```rust
pub struct Redirection {
    pub kind: RedirKind,
    pub fd: u8,           // 0=stdin, 1=stdout, 2=stderr
    pub path: Vec<u8>,
}

pub enum RedirKind {
    Truncate,  // >  — create or overwrite
    Append,    // >> — create or append
    Read,      // <  — open for reading
}
```

**Semantics:**

1. Tokenizer parses `>` `>>` `<` `2>` tokens
2. Before spawn, `Redirection::apply()`:
   - Opens target file via `ob_open` / `ob_create` with appropriate access
   - Closes inherited fd (0/1/2) and duplicates new fd into its place via `sys_dup2`
3. After child exits, shell closes the redirected fds

**Example:** `dir > listing.txt`

- Tokenize: `[Word("dir"), RedirectStdout { target: "listing.txt" }]`
- Remove redirection tokens from command args
- Open `listing.txt` via `ob_create(file, WriteContent)`, get fd
- `sys_dup2(fd, 1)` — replace stdout
- Spawn `dir` with inherited fd table

### 2.7 Line Editor Design

Replace the current `readline()` character-by-character hack with a stateful `LineEditor`:

```rust
pub struct LineEditor {
    buf: Vec<u8>,           // line buffer (dynamic)
    pos: usize,             // cursor position in buffer
    prompt: Vec<u8>,
    history: Arc<Mutex<History>>,
    insert_mode: bool,       // insert vs overwrite
    search_active: bool,     // Ctrl-R search mode
    search_query: Vec<u8>,
    search_result: Option<usize>,
}
```

**Key sequences (ANSI):**

| Key | Action |
| ----- | -------- |
| Left / Ctrl-B | Cursor left (if pos > 0) |
| Right / Ctrl-F | Cursor right (if pos < len) |
| Home / Ctrl-A | Jump to line start |
| End / Ctrl-E | Jump to line end |
| Ctrl-K | Kill to end of line |
| Ctrl-U | Kill to start of line (or whole line) |
| Ctrl-W | Kill word backward |
| Ctrl-R | Reverse history search |
| Backspace | Delete char before cursor |
| Delete | Delete char at cursor |
| Insert | Toggle insert/overwrite |
| Up / Ctrl-P | History previous |
| Down / Ctrl-N | History next |

**Cursor rendering:**

- Use ANSI escape sequences for real cursor positioning
- `\x1B[G` — move to column 0
- `\x1B[{n}C` — move right n columns
- `\x1B[K` — clear to end of line
- Save/restore cursor position via `\x1B[s` / `\x1B[u`

### 2.8 History Persistence

```rust
pub struct History {
    entries: VecDeque<Vec<u8>>,  // ring buffer, max N entries
    max_entries: usize,
    browse_pos: Option<usize>,    // current position when browsing
    file_path: Vec<u8>,
    dirty: bool,
}
```

- On shell startup: load history from `C:\System\neoshell.hst` if exists
- On each command: append to ring buffer, mark dirty
- Periodic flush (every 10 entries) or on EXIT: write to file
- Format: one line per entry, `\n`-terminated. Lines with `\n` escaped.
- Max 4096 bytes per entry, max 1024 entries.

### 2.9 Environment Variable Expansion

Add a post-tokenization expansion pass:

```rust
impl EnvStore {
    /// Replace %VARNAME% occurrences in a byte slice.
    /// Returns expanded buffer or error on unknown var.
    pub fn expand(&self, input: &[u8]) -> Result<Vec<u8>, ()>;
}
```

- `%%` → literal `%`
- `%VARNAME%` → value, error if undefined
- Case-insensitive matching (uppercased key comparison)
- Expanded before redirection target resolution (so `> %OUTFILE%` works)

### 2.10 Completion Improvements

```rust
pub struct CompletionEngine {
    path_cache: Vec<(Vec<u8>, u64)>,  // (dir_path, last_checked_tick)
    cache_ttl_ticks: u64,
}
```

- Cache `ob_enum` results per PATH directory with TTL
- Add filename completion after command name: if current word contains `\` or `/`, complete file paths in CWD
- Thread-safe: `CompletionCtx` per invocation, no mutable statics

---

## 3. Alternatives Considered

### 3.1 Keep shared memory args (0x41F000) but make it dynamic

**Rejected.** The fixed physical address prevents multiple concurrent argument readers and is inherently racy. New Ob mechanism (encoding args in spawn attributes) is cleaner and follows the existing `pk` (fd encoding) pattern. Backward compatibility maintained by having the shell also write to 0x41F000.

### 3.2 Use console.nxl for full line editing

**Rejected.** Console.nxl provides `read_byte()` only. Adding full line editing to console.nxl would couple the shell to a specific NXL library. Keeping the editor in neoshell itself allows independent evolution and makes the shell self-contained.

### 3.3 Implement a shell grammar (lex/yacc style)

**Rejected.** A formal grammar parser (like POSIX shell grammar) would be ~2000+ lines of parser code for features that can be handled by a simpler state machine tokenizer. The tokenizer described above is ~300 lines and covers all planned features.

### 3.4 Add new syscall for spawn-with-redirection

**Rejected.** The existing `ob_create(PROCESS, attrs=fd_encoding)` pattern already supports fd override. File redirection is purely a user-mode concern: open the file, `dup2` the fd, then spawn. No kernel changes needed.

---

## 4. Affected Components

| Component | Nature of Change |
| ----------- | ----------------- |
| `userbin/neoshell/` | Major refactor: extract modules, add tokenizer, editor, history, redirection, completion, environment, batch interpreter |
| `libneodos/` | Minor: new `sys_ob_create_with_fds()` helper in syscall.rs; backward compat in args.rs |
| `userbin/cd/` | Update to use new arg mechanism (optional — can keep reading from 0x41F000) |
| `userbin/corecopy/` | No change (redirection handled by shell) |
| `userbin/coretype/` | No change (TYPE < file.txt uses shell redirection) |

**No changes to kernel, VFS, scheduler, Ob, drivers, or HAL.**

---

## 5. API Contract

### Shell Configuration

```rust
fn ShellConfig::default() -> ShellConfig
  Returns: ShellConfig { line_buf_size: 4096, max_env: 64, history_size: 128,
            path_cache_ttl: 5_000_000_000, history_file_path: "C:\\System\\neoshell.hst" }
```

### Tokenizer (`tokenizer.rs`)

```rust
pub fn tokenize(input: &[u8]) -> (Vec<ShellToken>, Vec<&str>)
  Args: input — raw command line bytes
  Returns: tokens — parsed token stream
           errors — parse warnings (e.g., unmatched quote)
  Preconditions: input must not be empty
  Error states: unmatched quote → error, tokenization continues
```

### Environment (`env.rs`)

```rust
impl EnvStore {
    pub fn new() -> Self
    pub fn set(&mut self, key: &[u8], value: &[u8]) -> Result<(), ()>
        Returns: Ok(()) or Err(()) if key is empty or contains '='
    pub fn get(&self, key: &[u8]) -> Option<&[u8]>
    pub fn unset(&mut self, key: &[u8])
    pub fn expand(&self, input: &[u8]) -> Result<Vec<u8>, ()>
        Returns: expanded bytes, or Err if %VAR% not found
    pub fn iter(&self) -> impl Iterator<Item = (&[u8], &[u8])>
}
```

### Redirection (`redir.rs`)

```rust
pub fn parse_redirects(tokens: &[ShellToken], cwd: &[u8])
    -> (Vec<Vec<u8>>, Vec<Redirection>, Vec<&str>)
  Args: tokens — from tokenizer
        cwd — current working directory for relative paths
  Returns: (args, redirects, errors)
           args — tokens to pass to the command (redirections removed)
           redirects — redirections to apply before spawn

pub fn apply_redirect(redir: &Redirection, drive: u8) -> Result<u8, ()>
  Returns: the new fd number (replacing 0, 1, or 2)
  Errors: file not found, permission denied, out of fds
```

### Line Editor (`editor.rs`)

```rust
impl LineEditor {
    pub fn new(prompt: &[u8], history: Arc<Mutex<History>>) -> Self
    pub fn read_line(&mut self) -> Result<Vec<u8>, ()>
        Returns: the entered line (without trailing newline)
                 Err(()) on Ctrl-D / EOF
    pub fn clear(&mut self)
    pub fn set_prompt(&mut self, prompt: &[u8])
}
```

### History (`history.rs`)

```rust
impl History {
    pub fn new(max_entries: usize, file_path: &[u8]) -> Self
    pub fn add(&mut self, line: &[u8])
    pub fn prev(&mut self) -> Option<&[u8]>
    pub fn next(&mut self) -> Option<&[u8]>
    pub fn reset_browse(&mut self)
    pub fn search(&self, query: &[u8]) -> Option<usize>   // returns entry index
    pub fn get_entry(&self, index: usize) -> Option<&[u8]>
    pub fn count(&self) -> usize
    pub fn load(&mut self) -> Result<(), ()>
    pub fn flush(&mut self) -> Result<(), ()>
}
```

### Completion (`completion.rs`)

```rust
impl CompletionEngine {
    pub fn new(path_cache_ttl: u64) -> Self
    pub fn complete_command(&mut self, word: &[u8], drive: u8, path_dirs: &[Vec<u8>])
        -> Vec<Vec<u8>>
    pub fn complete_filename(&mut self, word: &[u8], drive: u8, cwd: &[u8])
        -> Vec<Vec<u8>>
    pub fn invalidate_cache(&mut self)
}
```

### Pipeline (`pipeline.rs`)

```rust
pub fn execute_pipeline(
    commands: &[ParsedCommand],
    drive: u8,
    env: &EnvStore,
) -> PipelineResult
  Args: commands — each parsed command with args + redirects
        drive — current drive letter
        env — environment for %VAR% expansion
  Returns: PipelineResult { exit_codes, timed_out }
  Errors: pipe creation failure, spawn failure
```

### Batch (`batch.rs`)

```rust
impl BatchInterpreter {
    pub fn new(file_path: &[u8]) -> Self
    pub fn run(&mut self) -> Result<(), ()>
        // Reads lines from batch file, tokenizes, executes.
        // Handles: IF, GOTO, FOR, SHIFT, :label, @, REM, PAUSE
}
```

---

## 6. Test Plan

### Tokenizer Tests

| # | Test | Input → Expected |
| --- | ------ | ----------------- |
| 1 | Simple words | `echo hello` → `[Word("echo"), Word("hello")]` |
| 2 | Double quotes | `echo "hello world"` → `[Word("echo"), Word("hello world")]` |
| 3 | Single quotes literal | `echo 'hello %PATH%'` → `[Word("echo"), Word("hello %PATH%")]` no expansion |
| 4 | Escape char | `echo hello ^` \| `more` → `[Word("echo"), Word("hello` \| `"), Word("more")]` |
| 5 | Pipe token | `cmd1` \| `cmd2` → `[Word("cmd1"), Pipe, Word("cmd2")]` |
| 6 | Redirect stdout | `dir > out.txt` → `[Word("dir"), RedirectStdout("out.txt")]` |
| 7 | Redirect append | `echo hi >> log.txt` → `[Word("echo"), Word("hi"), RedirectAppend("log.txt")]` |
| 8 | Redirect stdin | `sort < input.txt` → `[Word("sort"), RedirectStdin("input.txt")]` |
| 9 | Semicolon separator | `cd src; dir` → `[Word("cd"), Word("src"), Semicolon, Word("dir")]` |
| 10 | Multiple redirects | `cmd < in.txt > out.txt` → `[Word("cmd"), RedirectStdin("in.txt"), RedirectStdout("out.txt")]` |
| 11 | Unmatched quote | `echo "hello` → error "unmatched double quote" |
| 12 | Empty string | `` → empty token list |
| 13 | Nested redirect | `cmd 2>err.txt` → `[Word("cmd"), RedirectStderr("err.txt")]` |

### Environment Expansion Tests

| # | Test | Input → Expected |
| --- | ------ | ----------------- |
| 1 | Simple expansion | `%PATH%` with PATH=`\Programs` → `\Programs` |
| 2 | Multiple expansions | `%A%%B%` → concatenated values |
| 3 | Unknown var | `%UNDEFINED%` → error |
| 4 | Literal percent | `%%` → `%` |
| 5 | In quoted string (double) | `"hello %NAME%"` → `"hello World"` |
| 6 | In quoted string (single) | `'hello %NAME%'` → no expansion |
| 7 | In redirect target | `> %OUTFILE%` → redirect to expanded path |

### Editor Tests

| # | Test | Description |
| --- | ------ | ------------- |
| 1 | Basic input | Type `hello\n` → line = `hello` |
| 2 | Backspace | Type `helloo\x08\n` → line = `hello` |
| 3 | Left/Right | Type `hel\xc2ll\xc2o\n` → line = `hello` (left arrows) |
| 4 | Home/End | Type `hel\x01o\x05\n` → line = `ohel` (Ctrl-A, type o, Ctrl-E) |
| 5 | Ctrl-K kill | Type `hello\x01\x0b\n` → line = `` (Ctrl-A, Ctrl-K clears line) |
| 6 | Ctrl-U kill | Type `hello\x15\n` → line = `` (Ctrl-U clears whole line) |
| 7 | History up | Type `first\n` then `up` → retrieves `first` |
| 8 | History search | Type `first\n` then `\x12f` → retrieves `first` (Ctrl-R) |
| 9 | Insert toggle | Type characters in insert vs overwrite mode |

### Redirection Tests

| # | Test | Description |
| --- | ------ | ------------- |
| 1 | Stdout to file | `echo hello > test.txt` → file contains `hello` |
| 2 | Append to file | `echo line1 > log.txt; echo line2 >> log.txt` → file contains both lines |
| 3 | Stdin from file | `sort < input.txt` → sorted output |
| 4 | Stderr redirect (future) | `cmd 2>err.txt` → errors to file |
| 5 | File not found | `sort < nonexistent.txt` → error "File not found" |
| 6 | Permission denied | `echo > \System\protected.txt` → error "Access denied" |

### Pipeline Tests

| # | Test | Description |
| --- | ------ | ------------- |
| 1 | Simple pipe | `dir` \| `sort` → sorted directory listing |
| 2 | Three-stage pipe | `dir` \| `sort` \| `more` → sorted, paged |
| 3 | Pipe with redirection | `dir` \| `sort > out.txt` → sorted output to file |
| 4 | Empty command in pipe | `dir` \| \| `sort` → error "Invalid pipe syntax" |
| 5 | Pipeline wait | Shell waits for all processes, reports exit codes |
| 6 | Pipe with built-in | `echo` \| `more` → error "Cannot pipe built-in" |
| 7 | Exit code propagation | Pipeline fails if any command returns non-zero (future `&&`) |

### Batch Tests

| # | Test | Description |
| --- | ------ | ------------- |
| 1 | Simple batch | `CALL test.bat` → executes lines |
| 2 | IF EXIST | `IF EXIST file.txt ECHO found` |
| 3 | GOTO label | `GOTO :end` → jumps to `:end` |
| 4 | FOR loop | `FOR %%F IN (*.txt) DO ECHO %%F` |
| 5 | SHIFT | Process positional args with SHIFT |
| 6 | Comment | `REM this is a comment` → ignored |
| 7 | PAUSE | `PAUSE` → "Press any key..." |

### Integration Tests

| # | Test | Description |
| --- | ------ | ------------- |
| 1 | Environment SET/use | `SET NAME=World` then `ECHO %NAME%` → prints "World" |
| 2 | CWD change + command | `cd \Programs; dir` → lists Programs directory |
| 3 | Complex pipeline | `dir` \| `find ".nxe"` \| `sort > nxe_list.txt` |
| 4 | Quoting with spaces in path | `cd "C:\Program Files"` → changes to dir with space |
| 5 | Environment in path | `SET TARGET=C:\Out; type %TARGET%\file.txt` |
| 6 | Semicolon commands | `cls; ver; mem` → three commands sequentially |
| 7 | History persistence | Run commands, exit, restart shell → history loaded from disk |
| 8 | PATH caching | First `dir` probes PATH, second `dir` uses cached result |

---

## 7. Implementation Plan

### Phase 1: Infrastructure (v0.50.0)

| Step | Files | Description |
| ------ | ------- | ------------- |
| 1.1 | `userbin/neoshell/src/tokenizer.rs` | Implement tokenizer with quoted strings, escape, pipe, redirection, semicolon tokens. |
| 1.2 | `userbin/neoshell/src/env.rs` | Implement `EnvStore` with dynamic Vec, `%VAR%` expansion, SET parsing. |
| 1.3 | `userbin/neoshell/src/completion.rs` | Implement `CompletionEngine` with PATH cache and filename completion. |
| 1.4 | `userbin/neoshell/src/redir.rs` | Implement `parse_redirects()`, `apply_redirect()` with ob_open/dup2. |
| 1.5 | `userbin/neoshell/src/main.rs` | Integrate tokenizer + new dispatch into `execute_line()`. Keep old readline. |

**Test gate:** Phase 1 tests 1-13 (tokenizer), 1-7 (env), 1-3 (redirection), 1-3 (completion).

### Phase 2: Line Editor (v0.50.1)

| Step | Files | Description |
| ------ | ------- | ------------- |
| 2.1 | `userbin/neoshell/src/editor.rs` | Implement `LineEditor` with ANSI cursor control, Ctrl keys, insert/overwrite. |
| 2.2 | `userbin/neoshell/src/history.rs` | Implement `History` with file persistence, Ctrl-R search. |
| 2.3 | `userbin/neoshell/src/main.rs` | Replace old `readline()` with new `LineEditor`. Wire Ctrl-R. |

**Test gate:** Phase 2 tests 1-9 (editor), 1-3 (history), integration test 7.

### Phase 3: Pipeline and Scripting (v0.50.2)

| Step | Files | Description |
| ------ | ------- | ------------- |
| 3.1 | `userbin/neoshell/src/pipeline.rs` | Refactor pipeline execution: wait for all processes, collect exit codes. |
| 3.2 | `userbin/neoshell/src/batch.rs` | Implement batch interpreter: labels, GOTO, IF, FOR, SHIFT. |
| 3.3 | `userbin/neoshell/src/main.rs` | Wire batch interpreter into `cmd_call()`. Wire `;` command separator. |
| 3.4 | `libneodos/src/syscall.rs` | Add `sys_ob_create_with_fds()` helper. |

**Test gate:** Phase 3 tests 1-7 (pipeline), 1-7 (batch), integration tests 1-8.

### Phase 4: Polish and Docs (v0.50.3)

| Step | Files | Description |
| ------ | ------- | ------------- |
| 4.1 | `docs/shell.md` | Update documentation to cover new features. |
| 4.2 | `userbin/neoshell/src/main.rs` | Code cleanup: remove dead code, deprecated methods. |
| 4.3 | All Phase 1-3 files | Address edge cases: fuzz test tokenizer, stress test editor, large env sets. |

**Test gate:** All tests pass. Full integration test suite run.

---

## 8. Backward Compatibility

- All existing `.NXE` binaries continue to work unchanged.
- Old binaries reading `0x41F000` for args keep working (shell writes args there AND uses new mechanism).
- `CD.NXE` continues to use `0x41F000` for returning new CWD.
- Pipeline behavior changes: shell now waits for all processes (previously fire-and-forget). Exit codes are printed. This is a visible behavior change but positive.
- History file format is simple text. Old history in console.nxl is not migrated — console.nxl entries are separate from shell-owned history.

---

## 9. Out of Scope (Future)

| Feature | Why not now |
| --------- | ------------- |
| `&&` / \| \| conditional chaining | Requires exit code tracking in pipeline, straightforward add-on |
| Tab autocomplete for filenames with wildcard matching | Depends on Phase 2 completion engine |
| Ctrl-R incremental history search (like bash) | Added to Phase 2 in editor design |
| Job control (`bg`, `fg`, `jobs`) | No kernel job object support yet |
| Aliases | Simple: store in env, expand at token level |
| Command substitution `` `cmd` `` | Requires recursive shell execution |
| Unicode path support | Depends on kernel UTF-8 readiness |
