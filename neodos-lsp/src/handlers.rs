use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use lsp_types::*;

use crate::cache::DocumentCache;
use crate::config::NeodosLspConfig;
use crate::database::{Database, Symbol};
use crate::indexer::Indexer;
use crate::workspace::WorkspaceManager;

/// Convert a file path to a file:// URI.
pub fn path_to_uri(path: &PathBuf) -> lsp_types::Uri {
    let abs = if path.is_relative() {
        std::env::current_dir().unwrap_or_default().join(path)
    } else {
        path.clone()
    };
    let url = url::Url::from_file_path(&abs).expect("valid file path");
    url.as_str().parse::<lsp_types::Uri>().expect("valid lsp URI")
}

/// Convert a lsp_types::Uri to a file path.
pub fn uri_to_path(uri: &lsp_types::Uri) -> PathBuf {
    // Roundtrip through url::Url for path conversion.
    let url_str = uri.as_str();
    let url = url::Url::parse(url_str).expect("valid URL");
    url.to_file_path().expect("valid file:// URI")
}

/// Central handler for all LSP requests and notifications.
pub struct LspHandlers {
    pub db: Arc<Database>,
    pub cache: Arc<DocumentCache>,
}

impl LspHandlers {
    pub fn new(
        db: Arc<Database>,
        config: Arc<NeodosLspConfig>,
        _workspace: Arc<WorkspaceManager>,
    ) -> Self {
        let cache = Arc::new(DocumentCache::new(config.cache.documents));
        Self {
            db,
            cache,
        }
    }

    /// Handle a textDocument/didOpen notification.
    pub fn on_did_open(&self, params: DidOpenTextDocumentParams) {
        let path = uri_to_path(&params.text_document.uri);
        let version = params.text_document.version as i64;
        let content = &params.text_document.text;

        log::info!("didOpen: {} (v{})", path.display(), version);

        let parsed = Indexer::parse_file(&path, content);
        let parsed_clone = parsed.clone();

        self.cache.insert(path.clone(), content.clone(), version, parsed_clone);

        let file_index = crate::database::FileIndex {
            file: path.clone(),
            symbols: parsed.symbols,
            references: parsed.references,
            neodos_items: parsed.neodos_items,
        };
        self.db.replace_file_index(file_index);
    }

    /// Handle a textDocument/didChange notification.
    pub fn on_did_change(&self, params: DidChangeTextDocumentParams) {
        let path = uri_to_path(&params.text_document.uri);
        let version = params.text_document.version as i64;

        log::trace!("didChange: {} (v{})", path.display(), version);

        if let Some(change) = params.content_changes.into_iter().last() {
            let content = change.text;

            let parsed = Indexer::parse_file(&path, &content);
            let parsed_clone = parsed.clone();

            self.cache.insert(path.clone(), content, version, parsed_clone);

            let file_index = crate::database::FileIndex {
                file: path.clone(),
                symbols: parsed.symbols,
                references: parsed.references,
                neodos_items: parsed.neodos_items,
            };
            self.db.replace_file_index(file_index);
        }
    }

    pub fn on_did_save(&self, _params: DidSaveTextDocumentParams) {
        log::trace!("didSave");
    }

    pub fn on_did_close(&self, params: DidCloseTextDocumentParams) {
        let path = uri_to_path(&params.text_document.uri);
        log::info!("didClose: {}", path.display());
        self.cache.remove(&path);
    }

    // ─── Request Handlers ──────────────────────────────────────────────────

    /// textDocument/completion
    pub fn completion(&self, params: CompletionParams) -> Option<CompletionResponse> {
        let path = uri_to_path(&params.text_document_position.text_document.uri);
        let pos = params.text_document_position.position;
        log::debug!("completion at {}:{},{}", path.display(), pos.line, pos.character);

        let prefix = self.word_at_position(&path, pos);
        log::trace!("completion prefix: '{:?}'", prefix);

        let mut items: Vec<CompletionItem> = Vec::new();

        // 1. Symbols matching the prefix.
        if let Some(ref p) = prefix {
            for sym in self.db.find_by_prefix(p) {
                items.push(Self::sym_to_completion(&sym, p));
            }
        }

        // 2. NeoDOS syscall completions.
        if prefix.as_deref() == Some("sys_") || prefix.as_deref().map_or(false, |p| p.starts_with("sys_")) {
            for entry in self.db.syscalls.iter() {
                let (num, item) = entry.pair();
                items.push(CompletionItem {
                    label: format!("sys_{}", item.name),
                    detail: Some(format!("syscall #{num} — {}", item.detail)),
                    kind: Some(CompletionItemKind::FUNCTION),
                    insert_text: Some(format!("sys_{}", item.name)),
                    ..Default::default()
                });
            }
        }

        // 3. Shell commands.
        if prefix.as_deref() == Some("") || prefix.as_deref().map_or(false, |p| p.len() <= 3) {
            for entry in self.db.shell_commands.iter() {
                items.push(CompletionItem {
                    label: entry.key().clone(),
                    detail: Some(entry.value().detail.clone()),
                    kind: Some(CompletionItemKind::FUNCTION),
                    insert_text: Some(entry.key().clone()),
                    ..Default::default()
                });
            }
        }

        // 4. Capability constants.
        if prefix.as_deref().map_or(false, |p| p.starts_with("CAP_")) {
            for sym in self.db.find_by_prefix("CAP_") {
                items.push(Self::sym_to_completion(&sym, &"CAP_".to_string()));
            }
        }

        // 5. Snippets.
        if prefix.as_deref().map_or(false, |p| p.starts_with("unsafe") || p.is_empty()) {
            items.push(unsafe_snippet());
        }

        // 6. Module paths (use statements).
        if prefix.as_deref().map_or(false, |p| p.contains("::")) {
            let parts: Vec<&str> = prefix.as_ref().unwrap().split("::").collect();
            if let Some(last) = parts.last() {
                for sym in self.db.find_by_prefix(last) {
                    items.push(Self::sym_to_completion(&sym, last));
                }
            }
        }

        // Deduplicate.
        items.sort_by(|a, b| {
            let a_exact = prefix.as_ref().map_or(false, |p| a.label.eq_ignore_ascii_case(p));
            let b_exact = prefix.as_ref().map_or(false, |p| b.label.eq_ignore_ascii_case(p));
            a_exact.cmp(&b_exact).reverse().then_with(|| a.label.cmp(&b.label))
        });
        items.dedup_by(|a, b| a.label == b.label);

        log::debug!("completion: {} items", items.len());
        Some(CompletionResponse::Array(items.into_iter().take(50).collect()))
    }

    /// textDocument/definition
    pub fn goto_definition(&self, params: GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
        let path = uri_to_path(&params.text_document_position_params.text_document.uri);
        let pos = params.text_document_position_params.position;
        log::debug!("goto-def at {}:{},{}", path.display(), pos.line, pos.character);

        let word = self.word_at_position(&path, pos)?;
        let mut results: Vec<Location> = Vec::new();

        for sym in self.db.find_by_name(&word) {
            results.push(Location {
                uri: path_to_uri(&sym.file),
                range: sym.selection_range,
            });
        }

        if results.is_empty() && word.len() >= 2 {
            for sym in self.db.find_by_prefix(&word).iter().take(5) {
                results.push(Location {
                    uri: path_to_uri(&sym.file),
                    range: sym.selection_range,
                });
            }
        }

        log::debug!("goto-def: {} results", results.len());
        if results.is_empty() { None } else { Some(GotoDefinitionResponse::Array(results)) }
    }

    /// textDocument/references
    pub fn find_references(&self, params: ReferenceParams) -> Option<Vec<Location>> {
        let path = uri_to_path(&params.text_document_position.text_document.uri);
        let pos = params.text_document_position.position;
        log::debug!("find-references at {}:{},{}", path.display(), pos.line, pos.character);

        let word = self.word_at_position(&path, pos)?;
        let defs = self.db.find_by_name(&word);
        if defs.is_empty() {
            return Some(Vec::new());
        }

        let target_id = defs[0].id;
        let ref_ids = self.db.references_for(&target_id);

        let mut locations: Vec<Location> = Vec::new();
        for ref_id in ref_ids {
            if let Some(sym) = self.db.lookup(&ref_id) {
                locations.push(Location {
                    uri: path_to_uri(&sym.file),
                    range: sym.range,
                });
            }
        }

        if params.context.include_declaration {
            locations.push(Location {
                uri: path_to_uri(&defs[0].file),
                range: defs[0].selection_range,
            });
        }

        log::debug!("find-references: {} locations", locations.len());
        Some(locations)
    }

    /// textDocument/hover
    pub fn hover(&self, params: HoverParams) -> Option<Hover> {
        let path = uri_to_path(&params.text_document_position_params.text_document.uri);
        let pos = params.text_document_position_params.position;
        log::debug!("hover at {}:{},{}", path.display(), pos.line, pos.character);

        let sym = self.db.find_innermost_at_position(&path, pos)?;

        let mut contents: Vec<MarkedString> = Vec::new();

        // Signature.
        if let Some(ref sig) = sym.signature {
            contents.push(MarkedString::LanguageString(LanguageString {
                language: "rust".into(),
                value: sig.clone(),
            }));
        } else {
            let kind_label = symbol_kind_label(sym.kind);
            contents.push(MarkedString::String(format!("**{kind_label}** `{}`", sym.name)));
        }

        // NeoDOS-specific info.
        if let Some(ref ndk) = sym.neodos_kind {
            contents.push(MarkedString::String(format!(
                "**NeoDOS {}**",
                ndk.label()
            )));
            if let Some(num) = sym.syscall_number {
                contents.push(MarkedString::String(format!("Syscall #{num}")));
            }
            if let Some(ref caps) = sym.capabilities {
                contents.push(MarkedString::String(format!("Capabilities: 0x{caps:x}")));
            }
        }

        // Visibility.
        if let Some(ref vis) = sym.visibility {
            contents.push(MarkedString::String(format!("**Visibility:** {vis}")));
        }

        // Documentation.
        if let Some(ref doc) = sym.documentation {
            contents.push(MarkedString::String(doc.clone()));
        }

        // Location.
        contents.push(MarkedString::String(format!(
            "📄 {}:{}:{}",
            sym.file.file_name().unwrap_or_default().to_string_lossy(),
            sym.range.start.line + 1,
            sym.range.start.character + 1,
        )));

        Some(Hover {
            contents: HoverContents::Array(contents),
            range: Some(sym.range),
        })
    }

    /// textDocument/rename
    pub fn rename(&self, params: RenameParams) -> Option<WorkspaceEdit> {
        let path = uri_to_path(&params.text_document_position.text_document.uri);
        let pos = params.text_document_position.position;
        let new_name = &params.new_name;

        log::info!("rename at {}:{},{} -> '{}'", path.display(), pos.line, pos.character, new_name);

        let sym = self.db.find_innermost_at_position(&path, pos)?;
        let target_id = sym.id;
        let ref_ids = self.db.references_for(&target_id);

        let mut changes: HashMap<lsp_types::Uri, Vec<TextEdit>> = HashMap::new();

        // Rename definition.
        let def_uri = path_to_uri(&sym.file);
        changes.entry(def_uri).or_default().push(TextEdit {
            range: sym.selection_range,
            new_text: new_name.clone(),
        });

        // Rename references.
        for ref_id in ref_ids {
            if let Some(ref_sym) = self.db.lookup(&ref_id) {
                let u = path_to_uri(&ref_sym.file);
                changes.entry(u).or_default().push(TextEdit {
                    range: ref_sym.selection_range,
                    new_text: new_name.clone(),
                });
            }
        }

        Some(WorkspaceEdit {
            changes: Some(changes.into_iter().collect()),
            document_changes: None,
            change_annotations: None,
        })
    }

    /// textDocument/documentSymbol
    pub fn document_symbols(&self, params: DocumentSymbolParams) -> Option<DocumentSymbolResponse> {
        let path = uri_to_path(&params.text_document.uri);
        log::debug!("document-symbols for {}", path.display());

        let symbols = self.db.document_symbols(&path);
        if symbols.is_empty() {
            return Some(DocumentSymbolResponse::Flat(Vec::new()));
        }

        let top_level: Vec<Symbol> = symbols.iter().filter(|s| s.parent.is_none()).cloned().collect();
        let result: Vec<DocumentSymbol> = top_level.into_iter()
            .map(|s| self.symbol_to_document_symbol(&s, &symbols))
            .collect();

        Some(DocumentSymbolResponse::Nested(result))
    }

    // ─── Diagnostics ──────────────────────────────────────────────────────

    /// Compute diagnostics for a file.
    pub fn diagnostics(&self, path: &PathBuf) -> Vec<Diagnostic> {
        log::trace!("diagnostics for {}", path.display());
        let mut diags: Vec<Diagnostic> = Vec::new();

        let content = self.cache.get_source(path)
            .or_else(|| std::fs::read_to_string(path).ok());

        let content = match content {
            Some(c) => c,
            None => return diags,
        };

        // Check 1: Unbalanced braces/parens.
        let mut open_braces: i32 = 0;
        let mut open_parens: i32 = 0;
        for ch in content.chars() {
            match ch {
                '{' => open_braces += 1,
                '}' => open_braces -= 1,
                '(' => open_parens += 1,
                ')' => open_parens -= 1,
                _ => {}
            }
        }

        let last_line = content.lines().count().saturating_sub(1) as u32;

        if open_braces != 0 {
            diags.push(Diagnostic {
                range: Range {
                    start: Position { line: last_line, character: 0 },
                    end: Position { line: last_line, character: 0 },
                },
                severity: Some(DiagnosticSeverity::ERROR),
                message: format!("unbalanced braces: {open_braces} unclosed"),
                source: Some("neodos-lsp".into()),
                ..Default::default()
            });
        }
        if open_parens != 0 {
            diags.push(Diagnostic {
                range: Range {
                    start: Position { line: last_line, character: 0 },
                    end: Position { line: last_line, character: 0 },
                },
                severity: Some(DiagnosticSeverity::ERROR),
                message: format!("unbalanced parentheses: {open_parens} unclosed"),
                source: Some("neodos-lsp".into()),
                ..Default::default()
            });
        }

        // Check 2: Possible missing semicolons.
        let lines: Vec<&str> = content.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed == "}" && i > 0 {
                let prev = lines[i - 1].trim();
                if !prev.ends_with(';') && !prev.ends_with('{') && !prev.ends_with('}')
                    && !prev.ends_with(')') && !prev.starts_with("//") && !prev.starts_with("/*")
                    && !prev.is_empty() && !prev.ends_with(',') && !prev.ends_with("->")
                    && !prev.ends_with(':')
                {
                    diags.push(Diagnostic {
                        range: Range {
                            start: Position { line: i as u32, character: 0 },
                            end: Position { line: i as u32, character: line.len() as u32 },
                        },
                        severity: Some(DiagnosticSeverity::WARNING),
                        message: "possible missing semicolon".into(),
                        source: Some("neodos-lsp".into()),
                        ..Default::default()
                    });
                }
            }
        }

        diags
    }

    // ─── Utility functions ────────────────────────────────────────────────

    fn word_at_position(&self, path: &PathBuf, pos: Position) -> Option<String> {
        let content = self.cache.get_source(path)
            .or_else(|| std::fs::read_to_string(path).ok())?;

        let line = content.lines().nth(pos.line as usize)?;
        let col = pos.character as usize;

        if col >= line.len() {
            return None;
        }

        let before: String = line[..col]
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':' || *c == '#' || *c == '!')
            .collect();
        let after: String = line[col..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':' || *c == '!' || *c == '?')
            .collect();

        let word = format!("{}{}", before.chars().rev().collect::<String>(), after);
        if word.is_empty() { None } else { Some(word) }
    }

    fn sym_to_completion(sym: &Symbol, _prefix: &str) -> CompletionItem {
        CompletionItem {
            label: sym.name.clone(),
            kind: Some(sym.completion_item_kind()),
            detail: sym.detail.clone().or_else(|| {
                sym.neodos_kind.map(|k| format!("[{}]", k.label()))
            }),
            documentation: sym.documentation.clone().map(Documentation::String),
            insert_text: Some(sym.name.clone()),
            insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
            ..Default::default()
        }
    }

    fn symbol_to_document_symbol(&self, sym: &Symbol, all_symbols: &[Symbol]) -> DocumentSymbol {
        let children: Vec<DocumentSymbol> = all_symbols
            .iter()
            .filter(|s| s.parent == Some(sym.id))
            .map(|s| self.symbol_to_document_symbol(s, all_symbols))
            .collect();

        let tags = if sym.is_deprecated {
            Some(vec![SymbolTag::DEPRECATED])
        } else {
            None
        };

        #[allow(deprecated)]
        DocumentSymbol {
            name: sym.name.clone(),
            detail: sym.detail.clone().or_else(|| sym.neodos_kind.as_ref().map(|k| k.label().to_string())),
            kind: sym.kind,
            tags,
            deprecated: None,
            range: sym.range,
            selection_range: sym.selection_range,
            children: if children.is_empty() { None } else { Some(children) },
        }
    }
}

fn unsafe_snippet() -> CompletionItem {
    CompletionItem {
        label: "unsafe { }".into(),
        kind: Some(CompletionItemKind::SNIPPET),
        insert_text: Some("unsafe { $0 }".into()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        detail: Some("unsafe block".into()),
        ..Default::default()
    }
}

fn symbol_kind_label(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::FILE => "file",
        SymbolKind::MODULE => "module",
        SymbolKind::NAMESPACE => "namespace",
        SymbolKind::PACKAGE => "package",
        SymbolKind::CLASS => "class",
        SymbolKind::METHOD => "method",
        SymbolKind::PROPERTY => "property",
        SymbolKind::FIELD => "field",
        SymbolKind::CONSTRUCTOR => "constructor",
        SymbolKind::ENUM => "enum",
        SymbolKind::INTERFACE => "trait",
        SymbolKind::FUNCTION => "function",
        SymbolKind::VARIABLE => "variable",
        SymbolKind::CONSTANT => "constant",
        SymbolKind::STRING => "string",
        SymbolKind::NUMBER => "number",
        SymbolKind::BOOLEAN => "boolean",
        SymbolKind::ARRAY => "array",
        SymbolKind::OBJECT => "object",
        SymbolKind::KEY => "key",
        SymbolKind::NULL => "null",
        SymbolKind::ENUM_MEMBER => "enum member",
        SymbolKind::STRUCT => "struct",
        SymbolKind::EVENT => "event",
        SymbolKind::OPERATOR => "operator",
        SymbolKind::TYPE_PARAMETER => "type parameter",
        _ => "symbol",
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_handlers() -> LspHandlers {
        let cfg = Arc::new(NeodosLspConfig::default());
        let db = Arc::new(Database::new());
        let wm = Arc::new(WorkspaceManager::new(cfg.clone()));
        LspHandlers::new(db, cfg, wm)
    }

    #[test]
    fn test_completion_basic() {
        let h = make_handlers();
        let path = PathBuf::from("test.rs");
        h.db.insert_symbol(Symbol::simple(
            "sys_write", SymbolKind::FUNCTION,
            path.clone(), Range::default(), Range::default(),
        ));

        h.on_did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: path_to_uri(&path),
                language_id: "rust".into(),
                version: 1,
                text: "fn test() { sys_write }".into(),
            },
        });

        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: path_to_uri(&PathBuf::from("test.rs")) },
                position: Position { line: 0, character: 5 },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        };

        let result = h.completion(params);
        assert!(result.is_some());
    }

    #[test]
    fn test_goto_definition_finds_symbol() {
        let h = make_handlers();
        let range = Range {
            start: Position { line: 10, character: 0 },
            end: Position { line: 10, character: 10 },
        };
        h.db.insert_symbol(Symbol::simple(
            "MyStruct", SymbolKind::STRUCT,
            PathBuf::from("lib.rs"), range, range,
        ));

        let found = h.db.find_by_name("MyStruct");
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn test_hover_with_syscall_info() {
        let h = make_handlers();
        let path = PathBuf::from("hover.rs");
        let range = Range {
            start: Position { line: 5, character: 0 },
            end: Position { line: 5, character: 8 },
        };

        let mut sym = Symbol::simple("sys_read", SymbolKind::FUNCTION, path.clone(), range, range);
        sym.neodos_kind = Some(crate::database::NeodosKind::Syscall(4));
        sym.syscall_number = Some(4);
        sym.signature = Some("pub fn sys_read(fd: u64, buf: &[u8]) -> i64".into());
        sym.documentation = Some("Reads from a file descriptor.".into());
        h.db.insert_symbol(sym);

        let stored = h.db.find_by_name("sys_read");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].syscall_number, Some(4));
    }

    #[test]
    fn test_diagnostics_unbalanced_braces() {
        let h = make_handlers();
        let path = PathBuf::from("bad.rs");
        let content = "fn main() { let x = 1;".to_string();
        let parsed = Indexer::parse_file(&path, &content);
        h.cache.insert(path.clone(), content.clone(), 1, parsed);

        let diags = h.diagnostics(&path);
        assert!(!diags.is_empty());
        assert!(diags.iter().any(|d| d.message.contains("unbalanced")));
    }

    #[test]
    fn test_diagnostics_balanced_ok() {
        let h = make_handlers();
        let path = PathBuf::from("good.rs");
        let content = "fn main() { let x = 1; }".to_string();
        let parsed = Indexer::parse_file(&path, &content);
        h.cache.insert(path.clone(), content.clone(), 1, parsed);

        let diags = h.diagnostics(&path);
        assert!(diags.is_empty());
    }

    #[test]
    fn test_word_at_position() {
        let h = make_handlers();
        let rel_path = PathBuf::from("word.rs");
        let abs_path = std::env::current_dir().unwrap_or_default().join(&rel_path);
        h.on_did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: path_to_uri(&rel_path),
                language_id: "rust".into(),
                version: 1,
                text: "fn foo_bar() { let x = sys_write(0, buf); }".into(),
            },
        });

        let word = h.word_at_position(&abs_path, Position { line: 0, character: 30 });
        assert_eq!(word, Some("sys_write".into()));
    }

    #[test]
    fn test_document_symbols_multiple_items() {
        let h = make_handlers();
        let path = PathBuf::from("ds.rs");
        h.on_did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: path_to_uri(&path),
                language_id: "rust".into(),
                version: 1,
                text: "pub fn foo() {}\npub struct Bar {}\npub enum Baz {}\npub trait Qux {}\npub mod my_mod;".into(),
            },
        });

        let result = h.document_symbols(DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri: path_to_uri(&path) },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        });

        assert!(result.is_some());
    }

    #[test]
    fn test_rename_finds_symbol() {
        let h = make_handlers();
        let path = PathBuf::from("rename.rs");
        h.on_did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: path_to_uri(&path),
                language_id: "rust".into(),
                version: 1,
                text: "fn old_name() {}\nfn caller() { old_name(); }".into(),
            },
        });

        let found = h.db.find_by_name("old_name");
        assert_eq!(found.len(), 1);
    }
}
