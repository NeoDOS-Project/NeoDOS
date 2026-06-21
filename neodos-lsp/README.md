# NeoDOS LSP — Language Server for NeoDOS Development

LSP server for the [NeoDOS](https://github.com/anomalyco/neodos) operating system.
Provides IDE features for kernel, driver, user-mode, and system library code.

## Features

| Feature | Status | Description |
|---------|--------|-------------|
| **Completion** | ✅ | Symbol completion with NeoDOS-specific syscall, shell command, and capability entries |
| **Go to Definition** | ✅ | Navigate to symbol declarations across the workspace |
| **Find References** | ✅ | Find all references to a symbol |
| **Hover** | ✅ | Type signatures, documentation, NeoDOS-kind annotations |
| **Diagnostics** | ✅ | Unbalanced delimiters, missing semicolons, deprecated items |
| **Rename** | ✅ | Safe renaming with workspace edit |
| **Document Symbols** | ✅ | Hierarchical outline (functions, structs, enums, traits, modules) |
| **NeoDOS syscall analysis** | ✅ | `#[syscall(n)]` attribute and `sys_*` naming convention detection |
| **NeoDOS shell commands** | ✅ | `CommandEntry` pattern recognition |
| **NEM driver analysis** | ✅ | Capability constants (`CAP_*`), driver states, `DriverState` enum |
| **Kernel service analysis** | ✅ | Boot phase functions, filesystem trait methods, HAL exports |

## Architecture

```
┌──────────────────────────────────────────────────┐
│                  Editor (LSP Client)             │
│         VS Code / Neovim / Helix / Emacs         │
└───────────────┬──────────────────────┬───────────┘
                │  JSON-RPC 2.0        │
                │  Content-Length      │
                ▼  stdio               ▼
┌──────────────────────────────────────────────────┐
│                neodos-lsp (server)                │
│                                                   │
│  ┌──────────┐  ┌────────────┐  ┌──────────────┐  │
│  │  server  │──│  handlers  │──│  indexer     │  │
│  │ (stdio)  │  │ (LSP reqs) │  │ (parser)     │  │
│  └──────────┘  └────────────┘  └──────┬───────┘  │
│                                       │          │
│  ┌──────────┐  ┌────────────┐  ┌──────┴───────┐  │
│  │ workspace│  │   cache    │  │  database    │  │
│  │ (files)  │  │ (LRU docs) │  │ (DashMap)    │  │
│  └──────────┘  └────────────┘  └──────────────┘  │
│                                                   │
│  ┌──────────────────────────────────────────┐     │
│  │  NeoDOS Analyzers                        │     │
│  │  syscalls · drivers · shell · modules    │     │
│  └──────────────────────────────────────────┘     │
└──────────────────────────────────────────────────┘
```

### Module responsibilities

| Module | Lines | Responsibility |
|--------|-------|---------------|
| `server.rs` | 518 | JSON-RPC/stdio transport, message framing, lifecycle management |
| `handlers.rs` | 778 | All LSP request/notification handlers, diagnostics engine |
| `indexer.rs` | 900 | Rust source parser, symbol extraction, NeoDOS pattern detection |
| `database.rs` | 536 | In-memory symbol database (DashMap-backed), queries by name/file/position |
| `workspace.rs` | 280 | File discovery, polling-based change detection, version tracking |
| `cache.rs` | 143 | LRU document cache for parsed AST and source text |
| `config.rs` | 131 | Server configuration (env vars, workspace roots, capacity) |
| `main.rs` | 37 | Entry point, logger setup |

### Data flow

1. **Initialization**: Client sends `initialize` → server responds with capabilities
2. **Indexing**: Background thread discovers `.rs` files, parses in parallel (rayon), populates the database
3. **File changes**: `didOpen`/`didChange` trigger incremental re-parsing and database updates
4. **Requests**: Handlers query the database directly (lock-free reads via DashMap)
5. **Diagnostics**: Computed on `didChange` and published asynchronously

### NeoDOS-specific analysis

The indexer recognizes these NeoDOS constructs:

```
#[syscall(42)]                            → SyscallHandler item
pub fn sys_write(...)                     → SyscallHandler (naming convention)
SyscallNum { Exit = 0, Read = 4, ... }   → SyscallNum variants
CommandEntry { name: "DIR", ... }        → ShellCommand item
pub const CAP_IRQ: u64 = 1 << 0;         → CapabilityFlag item
impl FileSystem for NeoDosFs { ... }     → FileSystemImpl methods
fn PHASE_3_init()                        → BootPhase function
DriverState::Loaded → Active → Faulted   → DriverState variants
```

## Usage

### Build

```bash
cd neodos/neodos-lsp
cargo build --release
```

Binary at `target/release/neodos-lsp`.

### Integration with editors

**VS Code** (`.vscode/settings.json`):
```json
{
  "rust-analyzer.rustfmt.overrideCommand": null,
  "neodos-lsp.enable": true,
  "LSP": {
    "neodos-lsp": {
      "command": ["path/to/neodos-lsp"],
      "filetypes": ["rust"],
      "rootPatterns": ["neodos-kernel/Cargo.toml"]
    }
  }
}
```

**Neovim** (lspconfig):
```lua
require('lspconfig').neodos_lsp = {
  cmd = { 'neodos-lsp' },
  filetypes = { 'rust' },
  root_dir = require('lspconfig').util.root_pattern('neodos-kernel/Cargo.toml', 'AGENTS.md'),
}
```

**Helix**:
```toml
[language-server.neodos-lsp]
command = "neodos-lsp"

[[language]]
name = "rust"
language-servers = ["neodos-lsp", "rust-analyzer"]
```

### Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `NEODOS_LSP_ROOT` | `.` | Workspace root directory |
| `NEODOS_LSP_MAX_FILES` | `10000` | Max files to index |
| `NEODOS_LSP_CACHE_SIZE` | `256` | Max cached documents |
| `NEODOS_LSP_THREADS` | `auto` | Indexing thread count |
| `NEODOS_LSP_WATCH` | `true` | Enable file change polling |
| `RUST_LOG` | `neodos_lsp=info` | Log level (`trace` for full LSP message dump) |

Run with verbose logging:
```bash
RUST_LOG=neodos_lsp=trace neodos-lsp
```

## Design decisions

### Why a custom parser instead of `syn`?
- `syn` parses valid Rust only; NeoDOS source has `#[feature(...)]`, incomplete modules, and nightly-only constructs
- Linear-scan parsing is O(n) and trivially parallelizable
- NeoDOS-specific patterns (syscalls, shell commands, drivers) are structural, not semantic

### Why DashMap for the database?
- Lock-free concurrent reads from multiple handlers
- Background indexing writes without blocking LSP requests
- O(1) symbol lookup by ID, O(k) by name prefix

### Why polling instead of inotify?
- Cross-platform (Linux, macOS, Windows)
- Simplifies implementation (no platform-specific dependencies)
- 2-second polling interval is sufficient for a kernel codebase

### Incremental updates
- `didChange` notifications trigger per-file re-parsing
- `replace_file_index` atomically swaps old → new symbols in the database
- Document cache maintains LRU eviction to bound memory

## Tests

```bash
cargo test
# 34 tests: database (6), cache (3), handlers (6), indexer (14), workspace (4)
```

## License

Same as NeoDOS.
