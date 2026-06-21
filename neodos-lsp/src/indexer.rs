use std::path::PathBuf;
use std::sync::Arc;

use lsp_types::{Position, Range, SymbolKind};
use walkdir::WalkDir;

use crate::config::NeodosLspConfig;
use crate::database::{
self, Database, FileIndex, ImportInfo, NeodosItem, Symbol,
};

/// Result of parsing a single file.
#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub symbols: Vec<database::Symbol>,
    pub references: Vec<database::Reference>,
    pub imports: Vec<database::ImportInfo>,
    pub neodos_items: Vec<database::NeodosItem>,
}

struct SyscallNumVariant {
    name: String,
    number: u64,
}

/// The workspace indexer: walks files, parses them, and populates the Database.
pub struct Indexer {
    db: Arc<Database>,
    config: Arc<NeodosLspConfig>,
}

impl Indexer {
    pub fn new(db: Arc<Database>, config: Arc<NeodosLspConfig>) -> Self {
        Self { db, config }
    }

    /// Discover all `.rs` files in the workspace roots, respecting exclusions.
    pub fn discover_files(&self) -> Vec<PathBuf> {
        let exclude = &self.config.workspace.exclude_patterns;
        let max = self.config.workspace.max_files;
        let mut files = Vec::new();

        for root in self.config.workspace.roots.read().iter() {
            if !root.exists() {
                log::warn!("workspace root does not exist: {}", root.display());
                continue;
            }
            for entry in WalkDir::new(root)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| {
                    let name = e.file_name().to_str().unwrap_or("");
                    if name.starts_with('.') && e.depth() == 1 {
                        return false;
                    }
                    let path = e.path().to_string_lossy();
                    !exclude.iter().any(|pat| {
                        if pat.ends_with("/**") {
                            path.contains(&pat[..pat.len() - 3])
                        } else {
                            path.contains(pat)
                        }
                    })
                })
                .filter_map(|e| e.ok())
            {
                if entry.file_type().is_file()
                    && entry
                        .path()
                        .extension()
                        .map(|e| e == "rs")
                        .unwrap_or(false)
                {
                    files.push(entry.path().to_path_buf());
                    if files.len() >= max {
                        log::warn!("hit max_files limit ({})", max);
                        return files;
                    }
                }
            }
        }

        log::info!("discovered {} .rs files in workspace", files.len());
        files
    }

    /// Full workspace index: parse all files in parallel.
    pub fn index_workspace(&self, files: &[PathBuf]) -> usize {
        log::info!("indexing {} files ({} threads)...", files.len(), self.parallelism());

        use rayon::prelude::*;
        let results: Vec<_> = files
            .par_iter()
            .with_max_len(8) // batch files per thread
            .map(|path| {
                let content = match std::fs::read_to_string(path) {
                    Ok(c) => c,
                    Err(e) => {
                        log::warn!("cannot read {}: {e}", path.display());
                        return None;
                    }
                };
                let parsed = Self::parse_file(path, &content);
                Some((path.clone(), content, parsed))
            })
            .collect();

        let mut count = 0;
        for result in results.into_iter().flatten() {
            let (path, _content, parsed) = result;
            let fi = FileIndex {
                file: path,
                symbols: parsed.symbols,
                references: parsed.references,
                neodos_items: parsed.neodos_items,
            };
            let sc = fi.symbols.len();
            self.db.replace_file_index(fi);
            count += sc;
        }

        // Store the file list.
        *self.db.all_files.write() = files.to_vec();

        log::info!("indexing complete: {} symbols in {} files", count, files.len());
        count
    }

    /// Re-index a single file (for incremental updates).
    #[cfg(test)]
    pub fn reindex_file(&self, path: &PathBuf, content: &str, version: i64) -> usize {
        log::trace!("re-indexing {} (v{})", path.display(), version);
        let parsed = Self::parse_file(path, content);
        let sc = parsed.symbols.len();

        let fi = FileIndex {
            file: path.clone(),
            symbols: parsed.symbols,
            references: parsed.references,
            neodos_items: parsed.neodos_items,
        };
        self.db.replace_file_index(fi);
        sc
    }

    fn parallelism(&self) -> usize {
        let c = self.config.indexing.threads;
        if c == 0 {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
        } else {
            c
        }
    }

    // ─── PARSING ────────────────────────────────────────────────────────────

    /// Parse a single `.rs` file: extract all symbols and NeoDOS items.
    pub fn parse_file(path: &PathBuf, content: &str) -> ParsedFile {
        let mut symbols: Vec<database::Symbol> = Vec::new();
        let references: Vec<database::Reference> = Vec::new();
        let mut imports: Vec<database::ImportInfo> = Vec::new();
        let mut neodos_items: Vec<database::NeodosItem> = Vec::new();

        let lines: Vec<&str> = content.lines().collect();
        let _line_count = lines.len();
        let mut current_comment: Option<String> = None;
        let mut pending_attrs: Vec<String> = Vec::new();
        let mut in_impl_block: Option<String> = None;
        let mut impl_brace: u32 = 0;

        // Track the position of `mod` declarations for module hierarchy.
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            let line_trimmed_start = line.len() - line.trim_start().len();
            let col = line_trimmed_start as u32;

            // Collect doc comments.
            if trimmed.starts_with("///") || trimmed.starts_with("//!") {
                let doc = trimmed.trim_start_matches("///").trim_start_matches("//!").trim();
                let prev = current_comment.take().unwrap_or_default();
                current_comment = Some(if prev.is_empty() { doc.to_string() } else { format!("{prev}\n{doc}") });
                continue;
            }
            if trimmed.starts_with("/*") && trimmed.contains("*/") {
                // Single-line block comment — ignore for doc purposes.
                continue;
            }

            // Extract attributes.
            if trimmed.starts_with("#[") && trimmed.ends_with(']') {
                pending_attrs.push(trimmed[2..trimmed.len() - 1].to_string());
                continue;
            }
            // Multi-line attribute (skip).
            if trimmed.starts_with("#[") {
                pending_attrs.push(trimmed[2..].to_string());
                continue;
            }
            if trimmed.ends_with(']') && !pending_attrs.is_empty() {
                let last = pending_attrs.last_mut().unwrap();
                last.push_str(trimmed.trim_end_matches(']'));
                last.push_str(trimmed.trim_end_matches(']'));
                continue;
            }

            let i = i as u32;
            let pos_start = Position { line: i, character: col as u32 };
            let pos_end = Position { line: i, character: (line.len()) as u32 };

            // ── mod declaration ──
            if let Some(mod_name) = Self::parse_mod_decl(trimmed) {
                let sym = Self::make_sym(
                    &mod_name, SymbolKind::MODULE, path, pos_start, pos_end,
                    &mut current_comment, &pending_attrs,
                );
                symbols.push(sym);
                pending_attrs.clear();
                continue;
            }

            // ── use / import ──
            if let Some(imp) = Self::parse_use(trimmed, i) {
                imports.push(imp);
                continue;
            }

            // ── Methods inside impl blocks ──
            // Check this BEFORE standalone fn, so methods get SymbolKind::METHOD.
            if in_impl_block.is_some() {
                impl_brace += trimmed.matches('{').count() as u32;
                impl_brace = impl_brace.saturating_sub(trimmed.matches('}').count() as u32);

                if trimmed.starts_with("pub fn") || trimmed.starts_with("fn ")
                    || trimmed.starts_with("pub unsafe fn") || trimmed.starts_with("unsafe fn")
                {
                    if let Some(name) = Self::extract_name(trimmed, "fn") {
                        let mut sym = Self::make_sym(
                            &name, SymbolKind::METHOD, path, pos_start, pos_end,
                            &mut current_comment, &pending_attrs,
                        );
                        sym.signature = Some(Self::extract_signature(trimmed, &lines, i as usize));

                        if name == "read" || name == "write" || name == "open" || name == "close" {
                            neodos_items.push(NeodosItem {
                                name: name.clone(),
                                detail: format!("impl {}::{}",
                                    in_impl_block.as_ref().unwrap(), name),
                            });
                        }

                        symbols.push(sym);
                    }
                    pending_attrs.clear();
                    continue;
                }

                if impl_brace == 0 {
                    in_impl_block = None;
                }
            }

            // ── pub fn / fn ──
            if trimmed.starts_with("pub fn") || trimmed.starts_with("fn ")
                || trimmed.starts_with("pub(crate) fn") || trimmed.starts_with("pub(super) fn")
                || trimmed.starts_with("pub unsafe fn") || trimmed.starts_with("unsafe fn")
                || trimmed.starts_with("pub async fn") || trimmed.starts_with("async fn")
            {
                let is_pub = trimmed.contains("pub ");
                let fn_name = Self::extract_name(trimmed, "fn");
                if let Some(name) = fn_name {
                    // Detect special NeoDOS patterns in function names.
                    let kind = SymbolKind::FUNCTION;
                    let mut sym = Self::make_sym(
                        &name, kind, path, pos_start, pos_end,
                        &mut current_comment, &pending_attrs,
                    );
                    sym.visibility = Some(if is_pub { "pub".into() } else { "private".into() });
                    sym.signature = Some(Self::extract_signature(trimmed, &lines, i as usize));
                    sym.is_test = trimmed.contains("#[test]") || pending_attrs.iter().any(|a| a == "test");

                    // Detect syscall handlers.
                    if let Some(num) = Self::detect_syscall_handler(&name, &pending_attrs) {
                        sym.neodos_kind = Some(database::NeodosKind::Syscall(num));
                        sym.syscall_number = Some(num);
                    }
                    // Detect boot phase functions.
                    if name.starts_with("PHASE_") || name.starts_with("phase_") {
                        sym.neodos_kind = Some(database::NeodosKind::BootPhase);
                    }

                    let ndk = sym.neodos_kind.clone();
                    symbols.push(sym);

                    // Register as NeodosItem for special handling.
                    match ndk {
                        Some(database::NeodosKind::Syscall(num)) => {
                            neodos_items.push(NeodosItem {
                                name: name.clone(),
                                detail: format!("syscall #{num}: {name}"),
                            });
                        }
                        Some(database::NeodosKind::BootPhase) => {
                            neodos_items.push(NeodosItem {
                                name: name.clone(),
                                detail: format!("Boot phase: {name}"),
                            });
                        }
                        _ => {}
                    }
                }
                pending_attrs.clear();
                continue;
            }

            // ── pub struct / struct ──
            if trimmed.starts_with("pub struct") || trimmed.starts_with("struct ") {
                let name = Self::extract_name(trimmed, "struct");
                if let Some(name) = name {
                    let mut sym = Self::make_sym(
                        &name, SymbolKind::STRUCT, path, pos_start, pos_end,
                        &mut current_comment, &pending_attrs,
                    );
                    // Check for #[repr(C)] or other key NeoDOS attributes.
                    if pending_attrs.iter().any(|a| a.contains("repr(C)") || a.contains("repr(packed)")) {
                        sym.attributes = pending_attrs.clone();
                    }
                    symbols.push(sym);
                }
                pending_attrs.clear();
                continue;
            }

            // ── pub enum / enum ──
            if trimmed.starts_with("pub enum") || trimmed.starts_with("enum ") {
                if let Some(name) = Self::extract_name(trimmed, "enum") {
                    symbols.push(Self::make_sym(
                        &name, SymbolKind::ENUM, path, pos_start, pos_end,
                        &mut current_comment, &pending_attrs,
                    ));
                }
                pending_attrs.clear();
                continue;
            }

            // ── pub trait / trait ──
            if trimmed.starts_with("pub trait") || trimmed.starts_with("trait ") {
                if let Some(name) = Self::extract_name(trimmed, "trait") {
                    symbols.push(Self::make_sym(
                        &name, SymbolKind::INTERFACE, path, pos_start, pos_end,
                        &mut current_comment, &pending_attrs,
                    ));
                }
                pending_attrs.clear();
                continue;
            }

            // ── pub type / type ──
            if trimmed.starts_with("pub type") || trimmed.starts_with("type ") {
                if let Some(name) = Self::extract_name(trimmed, "type") {
                    symbols.push(Self::make_sym(
                        &name, SymbolKind::TYPE_PARAMETER, path, pos_start, pos_end,
                        &mut current_comment, &pending_attrs,
                    ));
                }
                pending_attrs.clear();
                continue;
            }

            // ── pub const / const (but not `const fn`) ──
            if (trimmed.starts_with("pub const") || trimmed.starts_with("const ")) && !trimmed.contains(" fn ") {
                if let Some(name) = Self::extract_name(trimmed, "const") {
                    symbols.push(Self::make_sym(
                        &name, SymbolKind::CONSTANT, path, pos_start, pos_end,
                        &mut current_comment, &pending_attrs,
                    ));

                    // NeoDOS-specific: CAP_* capability constants.
                    if name.starts_with("CAP_") {
                        let val = trimmed.split('=').nth(1).unwrap_or("?").trim().trim_end_matches(';').to_string();
                        neodos_items.push(NeodosItem {
                            name: name.clone(),
                            detail: format!("Capability: {name} = {val}"),
                        });
                    }
                }
                pending_attrs.clear();
                continue;
            }

            // ── pub static / static ──
            if trimmed.starts_with("pub static") || trimmed.starts_with("static ") {
                if let Some(name) = Self::extract_name(trimmed, "static") {
                    symbols.push(Self::make_sym(
                        &name, SymbolKind::CONSTANT, path, pos_start, pos_end,
                        &mut current_comment, &pending_attrs,
                    ));
                }
                pending_attrs.clear();
                continue;
            }

            // ── macro_rules! ──
            if trimmed.starts_with("macro_rules!") {
                if let Some(name) = Self::extract_macro_name(trimmed) {
                    symbols.push(Self::make_sym(
                        &name, SymbolKind::FUNCTION, path, pos_start, pos_end,
                        &mut current_comment, &pending_attrs,
                    ));
                }
                pending_attrs.clear();
                continue;
            }

            // ── impl block ──
            if trimmed.starts_with("impl ") {
                let target = Self::extract_impl_target(trimmed);
                if let Some(target) = target {
                    // Mark that we're entering an impl block.
                    in_impl_block = Some(target.clone());
                    impl_brace = 1;

                    symbols.push(Self::make_sym(
                        &target, SymbolKind::OBJECT, path, pos_start, pos_end,
                        &mut current_comment, &pending_attrs,
                    ));
                }
                pending_attrs.clear();
                continue;
            }

            // ── NeoDOS-specific pattern: SyscallNum enum variants ──
            if let Some(var) = Self::parse_syscallnum_variant(trimmed) {
                neodos_items.push(NeodosItem {
                    name: format!("SyscallNum::{} ({})", var.name, var.number),
                    detail: format!("syscall #{}: {}", var.number, var.name),
                });
                continue;
            }

            // ── NeoDOS-specific pattern: CommandEntry ──
            if trimmed.contains("CommandEntry {") || trimmed.contains("CommandEntry::new") {
                if let Some(cmd) = Self::parse_command_entry(trimmed, &lines, i as usize) {
                    neodos_items.push(NeodosItem {
                        name: cmd.name,
                        detail: cmd.detail,
                    });
                }
                continue;
            }

            // ── NeoDOS-specific pattern: DriverState enum ──
            if Self::is_driver_state_enum(trimmed) {
                neodos_items.push(NeodosItem {
                    name: trimmed.trim().trim_end_matches(',').to_string(),
                    detail: "DriverState variant".into(),
                });
                continue;
            }

            // Clear pending attributes if line is not an attribute continuation.
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                pending_attrs.clear();
            }
        }

        ParsedFile { symbols, references, imports, neodos_items }
    }

    // ─── Helper parsing functions ──────────────────────────────────────────

    fn parse_mod_decl(trimmed: &str) -> Option<String> {
        if let Some(rest) = trimmed.strip_prefix("pub mod ").or_else(|| trimmed.strip_prefix("mod ")) {
            let name = rest.split([';', '{', ' ']).next().unwrap_or("").trim();
            if !name.is_empty() && !name.contains("//") {
                return Some(name.to_string());
            }
        }
        None
    }

    fn parse_use(trimmed: &str, _line: u32) -> Option<ImportInfo> {
        if trimmed.starts_with("use ") || trimmed.starts_with("pub use ") {
            let use_str = if trimmed.starts_with("pub use ") { &trimmed[8..] } else { &trimmed[4..] };
            let use_str = use_str.trim_end_matches(';').trim();
            let path: Vec<String> = use_str.split("as ")
                .next()
                .unwrap_or("")
                .trim()
                .split("::")
                .map(|s| s.to_string())
                .collect();
            Some(ImportInfo { path })
        } else {
            None
        }
    }

    fn extract_name(trimmed: &str, keyword: &str) -> Option<String> {
        // Find position of the keyword.
        let kw_pos = trimmed.find(keyword)?;
        let after_kw = &trimmed[kw_pos + keyword.len()..];
        let name = after_kw
            .split(|c: char| c.is_whitespace() || c == '<' || c == '(' || c == ';' || c == '{' || c == '!')
            .filter(|s| !s.is_empty())
            .next()
            .unwrap_or("")
            .trim();
        if name.is_empty() || name.contains("//") {
            return None;
        }
        // Filter out things like "mut", "ref", "self", "&"
        if matches!(name, "mut" | "ref" | "self" | "&" | "unsafe" | "async") {
            return None;
        }
        Some(name.to_string())
    }

    fn extract_macro_name(trimmed: &str) -> Option<String> {
        let rest = trimmed.strip_prefix("macro_rules!")?.trim();
        let name = rest.split('{').next().unwrap_or("").trim().trim_matches('!');
        if name.is_empty() {
            return None;
        }
        Some(name.to_string())
    }

    fn extract_impl_target(trimmed: &str) -> Option<String> {
        let rest = trimmed.strip_prefix("impl ")?;
        // Handle "impl Trait for Type", "impl Type", "impl<T> Type"
        let target = rest.split("for ")
            .last()
            .unwrap_or(rest)
            .split('<')
            .next()
            .unwrap_or("")
            .split('{')
            .next()
            .unwrap_or("")
            .split("where")
            .next()
            .unwrap_or("")
            .trim();
        if target.is_empty() || target == " " {
            return None;
        }
        Some(target.to_string())
    }

    fn extract_signature(line: &str, lines: &[&str], line_idx: usize) -> String {
        let mut sig = line.to_string();
        // If the function signature spans multiple lines, collect until '{'.
        if !sig.contains('{') {
            for j in (line_idx + 1)..lines.len().min(line_idx + 10) {
                let l = lines[j];
                sig.push_str(l);
                if l.contains('{') {
                    break;
                }
            }
        }
        // Clean up: remove the body.
        if let Some(pos) = sig.find('{') {
            sig.truncate(pos);
        }
        sig.trim().to_string()
    }

    fn make_sym(
        name: &str,
        kind: SymbolKind,
        path: &PathBuf,
        start: Position,
        end: Position,
        doc: &mut Option<String>,
        attrs: &[String],
    ) -> Symbol {
        let doc_comment = doc.take();
        let is_deprecated = attrs.iter().any(|a| a.contains("deprecated"));
        Symbol {
            id: database::fresh_symbol_id(),
            name: name.to_string(),
            kind,
            neodos_kind: None,
            file: path.clone(),
            range: Range { start, end },
            selection_range: Range { start, end },
            parent: None,
            children: Vec::new(),
            documentation: doc_comment,
            detail: None,
            signature: None,
            visibility: None,
            attributes: attrs.to_vec(),
            is_deprecated,
            is_test: false,
            syscall_number: None,
            capabilities: None,
        }
    }

    // ─── NeoDOS-specific detection ─────────────────────────────────────────

    fn detect_syscall_handler(name: &str, attrs: &[String]) -> Option<u64> {
        // syscall handlers are functions named `sys_*` or with #[syscall(num)] attribute.
        if let Some(rest) = name.strip_prefix("sys_") {
            // Look up the syscall number from known syscall mapping.
            if let Ok(num) = rest.parse::<u64>() {
                return Some(num);
            }
            // Match by name convention.
            Some(match rest {
                "exit" => 0, "write" => 1, "yield" => 2, "getpid" => 3,
                "read" => 4, "pipe" => 5, "dup2" => 6, "spawn" => 7,
                "readdir" => 8, "waitpid" => 9, "open" => 10,
                "readfile" => 11, "writefile" => 12, "close" => 13,
                "chdir" => 16, "getcwd" => 17, "brk" => 18, "mmap" => 19,
                "munmap" => 20, "loadlib" => 21, "thread_create" => 22,
                "thread_join" => 23, "getcpuinfo" => 24, "mkdir" => 25,
                "unlink" => 26, "rmdir" => 27, "rename" => 28,
                "wait_alertable" => 40, "sleep_ex" => 41, "poweroff" => 42,
                "get_version" => 43, "get_datetime" => 44, "get_meminfo" => 45,
                "get_volume_label" => 46, "chdir_parent" => 47, "kobj_enum" => 48,
                _ => return None,
            })
        } else {
            // Also check for #[syscall(num)] attribute.
            for attr in attrs {
                if attr.starts_with("syscall(") {
                    let num_str = attr.trim_start_matches("syscall(").trim_end_matches(')');
                    return num_str.parse::<u64>().ok();
                }
            }
            None
        }
    }

    fn parse_syscallnum_variant(trimmed: &str) -> Option<SyscallNumVariant> {
        let t = trimmed.trim();
        // Match "Exit = 0," or "Exit," patterns (SyscallNum variants).
        if !t.ends_with(',') {
            return None;
        }
        let t = t.trim_end_matches(',');
        // Check if it's in a SyscallNum enum context by checking if the name starts with
        // a Rust identifier and optionally has "= <number>".
        if !t.contains('=') {
            return None;
        }
        let parts: Vec<&str> = t.splitn(2, '=').collect();
        if parts.len() != 2 {
            return None;
        }
        let name = parts[0].trim();
        let num_str = parts[1].trim();
        let number = num_str.parse::<u64>().ok()?;
        if name.is_empty() || !name.chars().next()?.is_uppercase() {
            return None;
        }
        Some(SyscallNumVariant {
            name: name.to_string(),
            number,
        })
    }

    fn parse_command_entry(_trimmed: &str, lines: &[&str], line_idx: usize) -> Option<NeodosItem> {
        // Look for `name: "CMDNAME"` pattern in CommandEntry construction.
        for j in line_idx..lines.len().min(line_idx + 5) {
            let l = lines[j];
            if let Some(pos) = l.find("name: \"") {
                let rest = &l[pos + 7..];
                if let Some(end) = rest.find('"') {
                    let cmd_name = rest[..end].to_string();
                    let description = l.split("description: ")
                        .nth(1)
                        .unwrap_or("Shell command")
                        .trim_matches('"')
                        .to_string();
                    return Some(NeodosItem {
                        name: cmd_name,
                        detail: description,
                    });
                }
            }
        }
        None
    }

    fn is_driver_state_enum(trimmed: &str) -> bool {
        let t = trimmed.trim();
        (t.starts_with("Loaded") || t.starts_with("Initialized") || t.starts_with("Registered")
            || t.starts_with("Bound") || t.starts_with("Active") || t.starts_with("Faulted")
            || t.starts_with("Unloaded") || t.starts_with("Unloading"))
            && (t.ends_with(',') || t.ends_with('}'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index_code(code: &str) -> ParsedFile {
        let f = PathBuf::from("test.rs");
        Indexer::parse_file(&f, code)
    }

    #[test]
    fn test_parse_function() {
        let p = index_code("pub fn sys_write(fd: u64, buf: &[u8]) -> u64 { 0 }");
        assert_eq!(p.symbols.len(), 1);
        assert_eq!(p.symbols[0].name, "sys_write");
        assert_eq!(p.symbols[0].kind, SymbolKind::FUNCTION);
    }

    #[test]
    fn test_parse_struct() {
        let p = index_code("#[repr(C)]\npub struct BootInfo {\n    magic: u32,\n}");
        assert_eq!(p.symbols.len(), 1);
        assert_eq!(p.symbols[0].name, "BootInfo");
        assert_eq!(p.symbols[0].kind, SymbolKind::STRUCT);
        assert!(p.symbols[0].attributes.iter().any(|a| a.contains("repr(C)")));
    }

    #[test]
    fn test_parse_enum() {
        let p = index_code("pub enum ThreadState { Ready, Running, Blocked, Terminated }");
        assert_eq!(p.symbols.len(), 1);
        assert_eq!(p.symbols[0].kind, SymbolKind::ENUM);
    }

    #[test]
    fn test_parse_trait() {
        let p = index_code("pub trait FileSystem { fn read(&self) -> Result<(), ()>; }");
        assert_eq!(p.symbols.len(), 1);
        assert_eq!(p.symbols[0].kind, SymbolKind::INTERFACE);
    }

    #[test]
    fn test_parse_impl() {
        let p = index_code("impl FileSystem for NeoDosFs {\n    fn read(&self) -> Result<(), ()> { Ok(()) }\n}");
        assert!(p.symbols.iter().any(|s| s.kind == SymbolKind::OBJECT));
        assert!(p.symbols.iter().any(|s| s.kind == SymbolKind::METHOD));
    }

    #[test]
    fn test_parse_mod_decl() {
        let p = index_code("pub mod scheduler;\nmod drivers;\npub mod fs;");
        assert_eq!(p.symbols.len(), 3);
        assert!(p.symbols.iter().all(|s| s.kind == SymbolKind::MODULE));
    }

    #[test]
    fn test_parse_const() {
        let p = index_code("pub const KERNEL_VERSION: &str = \"v0.39.3\";\nconst MAX_CPUS: usize = 16;");
        assert_eq!(p.symbols.len(), 2);
        assert!(p.symbols.iter().all(|s| s.kind == SymbolKind::CONSTANT));
    }

    #[test]
    fn test_parse_macro() {
        let p = index_code("macro_rules! println {\n    ($($arg:tt)*) => ({ /* ... */ });\n}");
        assert_eq!(p.symbols.len(), 1);
        assert_eq!(p.symbols[0].kind, SymbolKind::FUNCTION);
    }

    #[test]
    fn test_doc_comment_collection() {
        let p = index_code("/// Writes to the console.\n/// Returns number of bytes written.\npub fn sys_write(fd: u64, buf: &[u8]) -> u64 { 0 }");
        assert_eq!(p.symbols.len(), 1);
        let doc = p.symbols[0].documentation.as_deref().unwrap_or("");
        assert!(doc.contains("Writes to the console"));
        assert!(doc.contains("Returns number of bytes"));
    }

    #[test]
    fn test_syscall_detection() {
        // Test via naming convention.
        let p = index_code("pub fn sys_exit(code: u64) -> ! { loop {} }");
        assert_eq!(p.neodos_items.len(), 1);

        // Test via attribute (detect_syscall_handler also checks #[syscall(num)]).
        let p2 = index_code("#[syscall(42)]\npub fn poweroff_handler() -> ! { loop {} }");
        assert_eq!(p2.neodos_items.len(), 1, "should detect #[syscall(42)] attribute");
    }

    #[test]
    fn test_parse_use() {
        let p = index_code("use crate::scheduler;\npub use core::fmt::Write;\nuse alloc::boxed::Box as MyBox;");
        assert_eq!(p.imports.len(), 3);
        assert_eq!(p.imports[0].path, vec!["crate", "scheduler"]);
        assert_eq!(p.imports[1].path, vec!["core", "fmt", "Write"]);
        assert_eq!(p.imports[2].path, vec!["alloc", "boxed", "Box"]);
    }

    #[test]
    fn test_cap_constants() {
        let p = index_code("pub const CAP_IRQ: u64 = 1 << 0;\npub const CAP_DMA: u64 = 1 << 1;\nconst CAP_MMIO: u64 = 1 << 2;");
        assert_eq!(p.neodos_items.len(), 3);
        assert!(p.neodos_items.iter().all(|i| i.name.starts_with("CAP_")));
    }

    #[test]
    fn test_multiple_files() {
        let db = Arc::new(Database::new());
        let idx = Indexer::new(db.clone(), Arc::new(NeodosLspConfig::default()));

        let files = vec![
            PathBuf::from("a.rs"),
            PathBuf::from("b.rs"),
        ];

        // Index file A.
        idx.reindex_file(&files[0], "pub fn foo() {}", 0);
        assert!(db.find_by_name("foo").len() == 1);

        // Index file B.
        idx.reindex_file(&files[1], "pub fn bar() {}", 0);
        assert!(db.find_by_name("bar").len() == 1);
        assert!(db.find_by_name("foo").len() == 1);

        // Re-index file A with changes.
        idx.reindex_file(&files[0], "pub fn foo_updated() {}", 1);
        assert!(db.find_by_name("foo").is_empty());
        assert!(db.find_by_name("foo_updated").len() == 1);
    }

    #[test]
    fn test_discover_files_filters_target() {
        use std::fs;

        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        // Create some .rs files.
        fs::write(root.join("good.rs"), "fn a() {}").ok();
        fs::write(root.join("bad.txt"), "not rust").ok();
        fs::create_dir_all(root.join("target")).ok();
        fs::write(root.join("target").join("ignored.rs"), "fn b() {}").ok();

        let mut cfg = NeodosLspConfig::default();
        *cfg.workspace.roots.write() = vec![root.to_path_buf()];
        cfg.workspace.max_files = 100;

        let db = Arc::new(Database::new());
        let idx = Indexer::new(db, Arc::new(cfg));
        let files = idx.discover_files();

        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("good.rs"));
    }
}
