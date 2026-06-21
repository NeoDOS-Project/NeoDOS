use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use crossbeam::channel::{Receiver, Sender};

use lsp_types::*;

use crate::config::NeodosLspConfig;
use crate::database::Database;
use crate::handlers::{self, LspHandlers};
use crate::indexer::Indexer;
use crate::workspace::{FileEvent, WorkspaceManager};

/// JSON-RPC message from the client.
#[derive(Debug)]
struct JsonRpcMessage {
    id: Option<serde_json::Value>,
    method: String,
    params: Option<serde_json::Value>,
}

/// The main LSP server.
pub struct LspServer {
    config: Arc<NeodosLspConfig>,
    db: Arc<Database>,
    handlers: LspHandlers,
    workspace: Arc<WorkspaceManager>,
    /// Channel for sending diagnostics to be published.
    diag_tx: Sender<(PathBuf, Vec<Diagnostic>)>,
    diag_rx: Receiver<(PathBuf, Vec<Diagnostic>)>,
    /// Client capabilities (set during initialize).
    _client_caps: ClientCapabilities,
}

impl LspServer {
    pub fn new(config: Arc<NeodosLspConfig>) -> Self {
        let db = Arc::new(Database::new());
        let workspace = Arc::new(WorkspaceManager::new(config.clone()));
        let (dtx, drx) = crossbeam::channel::unbounded();

        Self {
            db: db.clone(),
            handlers: LspHandlers::new(db, config.clone(), workspace.clone()),
            workspace,
            config,
            diag_tx: dtx,
            diag_rx: drx,
            _client_caps: ClientCapabilities::default(),
        }
    }

    /// Run the LSP server. Reads JSON-RPC messages from stdin and writes responses to stdout.
    pub fn run(&mut self) -> Result<(), String> {
        let stdin = std::io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        let mut stdout = std::io::stdout().lock();

        log::info!("LSP server running, waiting for initialize...");

        // Main message loop.
        loop {
            // Read one JSON-RPC message.
            let msg = match read_message(&mut reader) {
                Ok(Some(msg)) => msg,
                Ok(None) => {
                    log::info!("client closed connection (EOF)");
                    break;
                }
                Err(e) => {
                    log::error!("error reading message: {e}");
                    break;
                }
            };

            log::trace!(">>> {} {:?}", msg.method, msg.id);

            // Handle lifecycle.
            match msg.method.as_str() {
                "exit" => {
                    log::info!("received exit notification, shutting down");
                    break;
                }

                "initialize" => {
                    let result = self.handle_initialize(msg.params);
                    let caps = serde_json::to_value(result).unwrap_or_default();
                    let response = make_response(msg.id, caps);
                    write_message(&mut stdout, &response).map_err(|e| e.to_string())?;
                }

                "initialized" => {
                    log::info!("client initialized");
                    // Start background indexing.
                    self.start_background_indexing();
                }

                "shutdown" => {
                    log::info!("received shutdown request");
                    let response = make_response(msg.id, serde_json::Value::Null);
                    write_message(&mut stdout, &response).map_err(|e| e.to_string())?;
                }

                // ── Notifications ──
                "textDocument/didOpen" => {
                    if let Some(params) = msg.params {
                        if let Ok(p) = serde_json::from_value::<DidOpenTextDocumentParams>(params) {
                            self.handlers.on_did_open(p);
                        }
                    }
                }

                "textDocument/didChange" => {
                    if let Some(params) = msg.params {
                        if let Ok(p) = serde_json::from_value::<DidChangeTextDocumentParams>(params) {
                            let change_uri = p.text_document.uri.clone();
                            self.handlers.on_did_change(p);
                            let path = handlers::uri_to_path(&change_uri);
                            self.request_diagnostics(path);
                        }
                    }
                }

                "textDocument/didSave" => {
                    if let Some(params) = msg.params {
                        if let Ok(p) = serde_json::from_value::<DidSaveTextDocumentParams>(params) {
                            self.handlers.on_did_save(p);
                        }
                    }
                }

                "textDocument/didClose" => {
                    if let Some(params) = msg.params {
                        if let Ok(p) = serde_json::from_value::<DidCloseTextDocumentParams>(params) {
                            self.handlers.on_did_close(p);
                        }
                    }
                }

                "workspace/didChangeWatchedFiles" => {
                    log::debug!("workspace/didChangeWatchedFiles (handled via polling)");
                }

                // ── Requests ──
                "textDocument/completion" => {
                    let result = msg.params.and_then(|params| {
                        serde_json::from_value::<CompletionParams>(params).ok()
                            .and_then(|p| self.handlers.completion(p))
                    });
                    let response = make_response(msg.id, serde_json::to_value(result).unwrap_or(serde_json::Value::Null));
                    write_message(&mut stdout, &response).map_err(|e| e.to_string())?;
                }

                "textDocument/definition" => {
                    let result = msg.params.and_then(|params| {
                        serde_json::from_value::<GotoDefinitionParams>(params).ok()
                            .and_then(|p| self.handlers.goto_definition(p))
                    });
                    let response = make_response(msg.id, serde_json::to_value(result).unwrap_or(serde_json::Value::Null));
                    write_message(&mut stdout, &response).map_err(|e| e.to_string())?;
                }

                "textDocument/references" => {
                    let result = msg.params.and_then(|params| {
                        serde_json::from_value::<ReferenceParams>(params).ok()
                            .and_then(|p| self.handlers.find_references(p))
                    });
                    let response = make_response(msg.id, serde_json::to_value(result).unwrap_or(serde_json::Value::Null));
                    write_message(&mut stdout, &response).map_err(|e| e.to_string())?;
                }

                "textDocument/hover" => {
                    let result = msg.params.and_then(|params| {
                        serde_json::from_value::<HoverParams>(params).ok()
                            .and_then(|p| self.handlers.hover(p))
                    });
                    let response = make_response(msg.id, serde_json::to_value(result).unwrap_or(serde_json::Value::Null));
                    write_message(&mut stdout, &response).map_err(|e| e.to_string())?;
                }

                "textDocument/rename" => {
                    let result = msg.params.and_then(|params| {
                        serde_json::from_value::<RenameParams>(params).ok()
                            .and_then(|p| self.handlers.rename(p))
                    });
                    let response = make_response(msg.id, serde_json::to_value(result).unwrap_or(serde_json::Value::Null));
                    write_message(&mut stdout, &response).map_err(|e| e.to_string())?;
                }

                "textDocument/documentSymbol" => {
                    let result = msg.params.and_then(|params| {
                        serde_json::from_value::<DocumentSymbolParams>(params).ok()
                            .and_then(|p| self.handlers.document_symbols(p))
                    });
                    let response = make_response(msg.id, serde_json::to_value(result).unwrap_or(serde_json::Value::Null));
                    write_message(&mut stdout, &response).map_err(|e| e.to_string())?;
                }

                _ => {
                    log::warn!("unhandled method: {} (id: {:?})", msg.method, msg.id);
                    // Respond with method not found for requests (messages with id).
                    if msg.id.is_some() {
                        let error = serde_json::json!({
                            "code": -32601,
                            "message": format!("method not found: {}", msg.method),
                        });
                        let response = make_error_response(msg.id, error);
                        write_message(&mut stdout, &response).map_err(|e| e.to_string())?;
                    }
                }
            }

            // Flush any pending diagnostics.
            self.flush_diagnostics(&mut stdout)
                .map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    /// Handle the initialize request.
    fn handle_initialize(&mut self, params: Option<serde_json::Value>) -> InitializeResult {
        if let Some(ref params) = params {
            if let Ok(p) = serde_json::from_value::<InitializeParams>(params.clone()) {
                log::info!(
                    "client: {} {}",
                    p.client_info.as_ref().map(|i| i.name.as_str()).unwrap_or("unknown"),
                    p.client_info.as_ref().and_then(|i| i.version.as_deref()).unwrap_or(""),
                );

                // Extract workspace roots.
                #[allow(deprecated)]
                let root_from_deprecated: Option<lsp_types::Uri> = p.root_uri.clone().or_else(|| {
                    p.root_path.as_ref().and_then(|rp| {
                        url::Url::from_file_path(rp).ok()
                            .and_then(|u| u.as_str().parse::<lsp_types::Uri>().ok())
                    })
                });
                if let Some(folders) = p.workspace_folders {
                    let roots: Vec<PathBuf> = folders
                        .iter()
                        .filter_map(|f| {
                            url::Url::parse(f.uri.as_str())
                                .ok()
                                .and_then(|u| u.to_file_path().ok())
                        })
                        .collect();
                    if !roots.is_empty() {
                        *self.config.workspace.roots.write() = roots;
                    }
                } else if let Some(uri) = root_from_deprecated {
                    let path = url::Url::parse(uri.as_str())
                        .ok()
                        .and_then(|u| u.to_file_path().ok());
                    if let Some(path) = path {
                        *self.config.workspace.roots.write() = vec![path];
                    }
                }

                self._client_caps = p.capabilities;
            }
        }

        let server_caps = ServerCapabilities {
            text_document_sync: Some(TextDocumentSyncCapability::Options(
                TextDocumentSyncOptions {
                    open_close: Some(true),
                    change: Some(TextDocumentSyncKind::INCREMENTAL),
                    will_save: None,
                    will_save_wait_until: None,
                    save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                },
            )),
            completion_provider: Some(CompletionOptions {
                trigger_characters: Some(vec![
                    ".".into(), "::".into(), "_".into(),
                ]),
                all_commit_characters: None,
                resolve_provider: None,
                work_done_progress_options: Default::default(),
                completion_item: None,
            }),
            definition_provider: Some(OneOf::Left(true)),
            references_provider: Some(OneOf::Left(true)),
            hover_provider: Some(HoverProviderCapability::Simple(true)),
            rename_provider: Some(OneOf::Left(true)),
            document_symbol_provider: Some(OneOf::Left(true)),
            workspace_symbol_provider: Some(OneOf::Left(true)),
            ..Default::default()
        };

        InitializeResult {
            capabilities: server_caps,
            server_info: Some(ServerInfo {
                name: "neodos-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            ..Default::default()
        }
    }

    /// Start background indexing in a separate thread.
    fn start_background_indexing(&self) {
        let db = self.db.clone();
        let config = self.config.clone();
        let handlers = LspHandlers::new(
            db.clone(),
            config.clone(),
            self.workspace.clone(),
        );

        let diag_tx = self.diag_tx.clone();
        let workspace = self.workspace.clone();

        thread::Builder::new()
            .name("neodos-lsp-indexer".into())
            .spawn(move || {
                log::info!("background indexing started");

                // Phase 1: Discover files.
                let indexer = Indexer::new(db.clone(), config.clone());
                let files = indexer.discover_files();
                workspace.register_files(&files);

                // Phase 2: Index all files (parallel).
                let count = indexer.index_workspace(&files);

                log::info!(
                    "indexed {} symbols in {} files ({} suites, ~431 kernel tests expected)",
                    count,
                    files.len(),
                    "TODO",
                );

                // Register workspace file list.
                *db.all_files.write() = files.clone();

                // Request initial diagnostics for all open files (none yet at startup).
                log::info!("background indexing complete");

                // Poll for file changes.
                loop {
                    thread::sleep(std::time::Duration::from_secs(2));

                    let events = workspace.poll_for_changes();
                    for (_path, event) in events {
                        match event {
                            FileEvent::Created(ref p) | FileEvent::Modified(ref p) => {
                                if let Ok(content) = std::fs::read_to_string(p) {
                                    let parsed = Indexer::parse_file(p, &content);
                                    let fi = crate::database::FileIndex {
                                        file: p.clone(),
                                        symbols: parsed.symbols,
                                        references: parsed.references,
                                        neodos_items: parsed.neodos_items,
                                    };
                                    db.replace_file_index(fi);

                                    // Send diagnostics for the changed file.
                                    let diags = handlers.diagnostics(p);
                                    diag_tx.send((p.clone(), diags)).ok();
                                }
                            }
                            FileEvent::Deleted(p) => {
                                db.replace_file_index(crate::database::FileIndex {
                                    file: p.clone(),
                                    symbols: vec![],
                                    references: vec![],
                                    neodos_items: vec![],
                                });
                            }
                            FileEvent::FullRescan => {
                                let files = indexer.discover_files();
                                indexer.index_workspace(&files);
                            }
                        }
                    }
                }
            })
            .expect("failed to spawn indexer thread");
    }

    /// Request diagnostics for a file (called after didChange).
    fn request_diagnostics(&self, path: PathBuf) {
        let diags = self.handlers.diagnostics(&path);
        self.diag_tx.send((path, diags)).ok();
    }

    /// Send pending diagnostics to the client.
    fn flush_diagnostics(&self, writer: &mut impl Write) -> Result<(), std::io::Error> {
        while let Ok((path, diags)) = self.diag_rx.try_recv() {
            let params = PublishDiagnosticsParams {
                uri: handlers::path_to_uri(&path),
                diagnostics: diags,
                version: None,
            };
            let notification = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": params,
            });
            write_message_inner(writer, &notification)?;
        }
        Ok(())
    }
}

// ─── JSON-RPC transport ──────────────────────────────────────────────────

/// Read one JSON-RPC message from stdin (Content-Length framed).
fn read_message(reader: &mut impl BufRead) -> Result<Option<JsonRpcMessage>, String> {
    let mut content_length: Option<usize> = None;

    loop {
        let mut header = String::new();
        let bytes = reader
            .read_line(&mut header)
            .map_err(|e| format!("read header: {e}"))?;
        if bytes == 0 {
            return Ok(None); // EOF
        }

        let header = header.trim();
        if header.is_empty() {
            // End of headers.
            break;
        }

        if let Some(val) = header
            .to_lowercase()
            .strip_prefix("content-length:")
        {
            content_length = Some(val.trim().parse().map_err(|e| format!("invalid Content-Length: {e}"))?);
        }
    }

    let len = content_length.ok_or_else(|| "missing Content-Length header")?;

    // Read exactly `len` bytes.
    let mut body = vec![0u8; len];
    reader
        .read_exact(&mut body)
        .map_err(|e| format!("read body ({len} bytes): {e}"))?;

    let body_str =
        String::from_utf8(body).map_err(|e| format!("invalid UTF-8: {e}"))?;

    let json: serde_json::Value =
        serde_json::from_str(&body_str).map_err(|e| format!("invalid JSON: {e} ({body_str:?})"))?;

    let id = json.get("id").cloned();
    let method = json
        .get("method")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing method field")?
        .to_string();
    let params = json.get("params").cloned();

    log::trace!("<<< {} (id={:?})", method, id);
    if log::log_enabled!(log::Level::Trace) {
        if let Some(ref p) = params {
            let s = serde_json::to_string(p).unwrap_or_default();
            if s.len() < 200 {
                log::trace!("    params: {s}");
            }
        }
    }

    Ok(Some(JsonRpcMessage { id, method, params }))
}

/// Write a JSON-RPC response to the writer.
fn write_message(writer: &mut impl Write, value: &serde_json::Value) -> Result<(), std::io::Error> {
    write_message_inner(writer, value)
}

fn write_message_inner(writer: &mut impl Write, value: &serde_json::Value) -> Result<(), std::io::Error> {
    let body = serde_json::to_string(value).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::Other, format!("serialize: {e}"))
    })?;

    if log::log_enabled!(log::Level::Trace) {
        let s = if body.len() < 300 {
            body.clone()
        } else {
            format!("{}...({} bytes)", &body[..200], body.len())
        };
        log::trace!(">>> {}", s);
    }

    write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    writer.flush()?;
    Ok(())
}

fn make_response(id: Option<serde_json::Value>, result: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn make_error_response(
    id: Option<serde_json::Value>,
    error: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": error,
    })
}
