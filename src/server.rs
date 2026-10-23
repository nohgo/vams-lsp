//! Tower LSP backend adapted from Veridian's server.rs. AMS analysis stays in
//! the reusable Workspace rather than the protocol handlers.
use crate::{
    features,
    project::{self, Project},
    text::{self, Encoding},
    workspace::Workspace,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};
use tower_lsp::{
    jsonrpc::{Error, Result},
    lsp_types::*,
    Client, LanguageServer,
};

pub struct Backend {
    client: Client,
    state: Arc<RwLock<Option<Workspace>>>,
    encoding: RwLock<Encoding>,
    snippets: AtomicBool,
    stopped: Arc<AtomicBool>,
    root_override: Option<PathBuf>,
}
impl Backend {
    pub fn new(client: Client, root_override: Option<PathBuf>, stopped: Arc<AtomicBool>) -> Self {
        Self {
            client,
            state: Arc::new(RwLock::new(None)),
            encoding: RwLock::new(Encoding::Utf16),
            snippets: AtomicBool::new(false),
            stopped,
            root_override,
        }
    }
    fn encoding(&self) -> Encoding {
        *self.encoding.read().unwrap()
    }
    fn read<T>(&self, f: impl FnOnce(&Workspace) -> T) -> Result<T> {
        self.state
            .read()
            .unwrap()
            .as_ref()
            .map(f)
            .ok_or_else(Error::invalid_request)
    }
    async fn publish(&self) {
        publish(&self.client, &self.state, self.encoding()).await;
    }
    async fn refresh(&self) {
        refresh(&self.client, &self.state, self.encoding()).await;
    }
}

async fn publish(client: &Client, state: &Arc<RwLock<Option<Workspace>>>, encoding: Encoding) {
    let diagnostics = {
        let guard = state.read().unwrap();
        let Some(ws) = guard.as_ref() else {
            return;
        };
        ws.open
            .values()
            .map(|doc| (doc.uri.clone(), doc.version, ws.diagnostics(doc, encoding)))
            .collect::<Vec<_>>()
    };
    for (uri, version, diagnostics) in diagnostics {
        let current = state
            .read()
            .unwrap()
            .as_ref()
            .and_then(|ws| ws.open.get(&uri))
            .is_some_and(|d| d.version == version);
        if current {
            client.publish_diagnostics(uri, diagnostics, version).await;
        }
    }
}

async fn refresh(client: &Client, state: &Arc<RwLock<Option<Workspace>>>, encoding: Encoding) {
    let input = {
        let mut guard = state.write().unwrap();
        let Some(ws) = guard.as_mut() else {
            return;
        };
        if ws.scanning || ws.project.reason == "awaiting-document" {
            return;
        }
        ws.scanning = true;
        (ws.project.clone(), ws.disk.clone())
    };
    let scan = tokio::task::spawn_blocking(move || input.0.scan(input.1)).await;
    let notice = {
        let mut guard = state.write().unwrap();
        let Some(ws) = guard.as_mut() else {
            return;
        };
        match scan {
            Ok(scan) => {
                let changed = ws.project.config_error != scan.project.config_error;
                let notice = changed.then(|| scan.project.config_error.clone()).flatten();
                ws.merge(scan);
                notice
            }
            Err(e) => {
                ws.scanning = false;
                Some(format!("Background indexing failed: {e}"))
            }
        }
    };
    if let Some(message) = notice {
        client.log_message(MessageType::WARNING, message).await;
    }
    publish(client, state, encoding).await;
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let encoding = if params
            .capabilities
            .general
            .as_ref()
            .and_then(|g| g.position_encodings.as_ref())
            .is_some_and(|e| e.contains(&PositionEncodingKind::UTF8))
        {
            Encoding::Utf8
        } else {
            Encoding::Utf16
        };
        *self.encoding.write().unwrap() = encoding;
        self.snippets.store(
            params
                .capabilities
                .text_document
                .as_ref()
                .and_then(|d| d.completion.as_ref())
                .and_then(|c| c.completion_item.as_ref())
                .and_then(|i| i.snippet_support)
                .unwrap_or(false),
            Ordering::Relaxed,
        );
        #[allow(deprecated)]
        let root = self
            .root_override
            .clone()
            .or_else(|| {
                params
                    .initialization_options
                    .as_ref()
                    .and_then(|o| o.get("root_dir"))
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from)
            })
            .or_else(|| {
                params
                    .workspace_folders
                    .as_ref()
                    .and_then(|f| f.first())
                    .and_then(|f| f.uri.to_file_path().ok())
            })
            .or_else(|| params.root_uri.and_then(|u| u.to_file_path().ok()));
        let reason = if root.is_some() {
            "client"
        } else {
            "awaiting-document"
        };
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let root = project::absolute(&root.unwrap_or(cwd.clone()), &cwd);
        *self.state.write().unwrap() = Some(Workspace::new(Project::new(root, reason.into())));
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "vams-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            capabilities: ServerCapabilities {
                position_encoding: Some(if encoding == Encoding::Utf8 {
                    PositionEncodingKind::UTF8
                } else {
                    PositionEncodingKind::UTF16
                }),
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::INCREMENTAL),
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                            include_text: Some(false),
                        })),
                        ..Default::default()
                    },
                )),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![".".into(), "$".into()]),
                    ..Default::default()
                }),
                definition_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                document_highlight_provider: Some(OneOf::Left(true)),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".into(), ",".into()]),
                    ..Default::default()
                }),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                })),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec!["vams.showProjectInfo".into(), "vams.reindex".into()],
                    ..Default::default()
                }),
                // vamsfmt/Conform and cadence.nvim keep their own responsibilities.
                ..Default::default()
            },
        })
    }
    async fn initialized(&self, _: InitializedParams) {
        let client = self.client.clone();
        let state = self.state.clone();
        let stopped = self.stopped.clone();
        let encoding = self.encoding();
        tokio::spawn(async move {
            loop {
                if stopped.load(Ordering::Relaxed) {
                    break;
                }
                refresh(&client, &state, encoding).await;
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
        self.client
            .log_message(
                MessageType::INFO,
                "vams-lsp ready; use vams.showProjectInfo to inspect source discovery",
            )
            .await;
    }
    async fn shutdown(&self) -> Result<()> {
        self.stopped.store(true, Ordering::Relaxed);
        Ok(())
    }
    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let item = params.text_document;
        {
            let mut guard = self.state.write().unwrap();
            if let Some(ws) = guard.as_mut() {
                if ws.project.reason == "awaiting-document" {
                    if let Ok(path) = item.uri.to_file_path() {
                        let (root, reason) = project::find_root(&path);
                        ws.project = Project::new(root, reason);
                    }
                }
                if item.text.len() as u64 <= ws.project.config.max_file_bytes {
                    ws.update(item.uri, item.text, item.version);
                } else {
                    ws.warnings
                        .push("Open document exceeds max_file_bytes".into());
                }
            }
        }
        self.publish().await;
    }
    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let encoding = self.encoding();
        {
            let mut guard = self.state.write().unwrap();
            if let Some(ws) = guard.as_mut() {
                let uri = params.text_document.uri;
                let version = params.text_document.version;
                if let Some(doc) = ws.open.get(&uri) {
                    if doc.version.is_some_and(|v| v >= version) {
                        return;
                    }
                    let mut value = doc.text.clone();
                    for change in params.content_changes {
                        if let Some(range) = change.range {
                            let (Some(start), Some(end)) = (
                                text::offset(&value, range.start, encoding),
                                text::offset(&value, range.end, encoding),
                            ) else {
                                return;
                            };
                            if start > end {
                                return;
                            }
                            value.replace_range(start..end, &change.text);
                        } else {
                            value = change.text;
                        }
                    }
                    if value.len() as u64 <= ws.project.config.max_file_bytes {
                        ws.update(uri, value, version);
                    }
                }
            }
        }
        self.publish().await;
    }
    async fn did_save(&self, _: DidSaveTextDocumentParams) {
        self.refresh().await;
    }
    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        {
            let mut guard = self.state.write().unwrap();
            if let Some(ws) = guard.as_mut() {
                ws.open.remove(&params.text_document.uri);
            }
        }
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
        self.refresh().await;
    }
    async fn did_change_watched_files(&self, _: DidChangeWatchedFilesParams) {
        self.refresh().await;
    }
    async fn completion(&self, p: CompletionParams) -> Result<Option<CompletionResponse>> {
        let pos = p.text_document_position;
        self.read(|w| {
            Some(CompletionResponse::Array(w.completion(
                &pos.text_document.uri,
                pos.position,
                self.encoding(),
                self.snippets.load(Ordering::Relaxed),
            )))
        })
    }
    async fn goto_definition(
        &self,
        p: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let pos = p.text_document_position_params;
        self.read(|w| {
            Some(GotoDefinitionResponse::Array(w.definitions(
                &pos.text_document.uri,
                pos.position,
                self.encoding(),
            )))
        })
    }
    async fn hover(&self, p: HoverParams) -> Result<Option<Hover>> {
        let pos = p.text_document_position_params;
        self.read(|w| w.hover(&pos.text_document.uri, pos.position, self.encoding()))
    }
    async fn document_symbol(
        &self,
        p: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        self.read(|w| {
            Some(DocumentSymbolResponse::Nested(
                w.document_symbols(&p.text_document.uri, self.encoding()),
            ))
        })
    }
    async fn symbol(&self, p: WorkspaceSymbolParams) -> Result<Option<Vec<SymbolInformation>>> {
        self.read(|w| Some(w.workspace_symbols(&p.query, self.encoding())))
    }
    async fn references(&self, p: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let pos = p.text_document_position;
        self.read(|w| {
            Some(w.references(
                &pos.text_document.uri,
                pos.position,
                self.encoding(),
                p.context.include_declaration,
            ))
        })
    }
    async fn document_highlight(
        &self,
        p: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let pos = p.text_document_position_params;
        self.read(|w| {
            Some(
                w.references(&pos.text_document.uri, pos.position, self.encoding(), true)
                    .into_iter()
                    .filter(|l| l.uri == pos.text_document.uri)
                    .map(|l| DocumentHighlight {
                        range: l.range,
                        kind: Some(DocumentHighlightKind::TEXT),
                    })
                    .collect(),
            )
        })
    }
    async fn signature_help(&self, p: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let pos = p.text_document_position_params;
        self.read(|w| w.signature(&pos.text_document.uri, pos.position, self.encoding()))
    }
    async fn prepare_rename(
        &self,
        p: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        self.read(|w| {
            w.rename_target(&p.text_document.uri, p.position, self.encoding())
                .map(|target| PrepareRenameResponse::RangeWithPlaceholder {
                    range: features::location(&target, self.encoding()).range,
                    placeholder: target.0.symbols[target.1].name.clone(),
                })
        })
    }
    async fn rename(&self, p: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let pos = p.text_document_position;
        let result = self.read(|w| {
            w.rename(
                &pos.text_document.uri,
                pos.position,
                self.encoding(),
                &p.new_name,
            )
        })?;
        result.map(Some).ok_or_else(||Error::invalid_params("Rename is limited to unambiguous non-macro function-local symbols, without name capture"))
    }
    async fn execute_command(&self, p: ExecuteCommandParams) -> Result<Option<serde_json::Value>> {
        match p.command.as_str() {
            "vams.showProjectInfo" => self.read(|w| Some(w.info())),
            "vams.reindex" => {
                self.refresh().await;
                self.read(|w| Some(w.info()))
            }
            _ => Err(Error::invalid_params("Unknown command")),
        }
    }
}
