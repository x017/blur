//! Generic LSP 3.17 client implementation.
//!
//! All I/O runs on tokio tasks spawned on the caller-provided
//! [`tokio::runtime::Handle`]. Every public `async` method on [`LspClient`]
//! awaits only a [`futures::channel::oneshot::Receiver`], making it completely
//! executor-agnostic (safe to await from GPUI's executor or any other runtime).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{anyhow, Context, Result};
use futures::channel::{mpsc as futures_mpsc, oneshot};
use lsp_types::{
    ApplyWorkspaceEditParams, CodeActionContext, CodeActionOrCommand, CodeActionParams,
    CompletionItem, CompletionParams, CompletionResponse, ConfigurationParams, Diagnostic,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, DocumentFormattingParams, DocumentSymbolParams,
    DocumentSymbolResponse, FormattingOptions, GotoDefinitionParams, GotoDefinitionResponse,
    Hover, HoverParams, InitializeParams, InitializeResult, InitializedParams, Location,
    LogMessageParams, MessageType, NumberOrString, PartialResultParams, Position, ProgressParams,
    ProgressParamsValue, PublishDiagnosticsParams, Range, ReferenceContext, ReferenceParams,
    Registration, RegistrationParams, RenameParams, ServerCapabilities, ShowMessageParams,
    ShowMessageRequestParams, TextDocumentContentChangeEvent, TextDocumentIdentifier,
    TextDocumentItem, TextDocumentPositionParams, TextDocumentSyncKind, TextEdit,
    UnregistrationParams, VersionedTextDocumentIdentifier, WorkDoneProgress,
    WorkDoneProgressParams, WorkspaceEdit, WorkspaceFolder,
};
use parking_lot::Mutex;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{debug, error, info, warn};
use url::Url;

use crate::presets::ServerConfig;
use crate::protocol::{
    apply_content_changes, build_client_capabilities, flatten_document_symbols, lsp_uri_to_url,
    negotiated_sync_kind, parse_incoming, path_to_url, resolve_config_section,
    serialize_notification, serialize_request, serialize_response_err, serialize_response_ok,
    url_to_lsp_uri, DocumentSymbolItem, IncomingMessage,
};
use crate::transport::{read_message, write_message};

const MAX_WIRE_FRAMES: usize = 200;
const MAX_LATENCY_SAMPLES: usize = 60;
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

/// Lifecycle status of a language server instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerStatus {
    /// Process spawned and `initialize` handshake is in progress.
    Starting,
    /// `initialize` / `initialized` completed and server is ready.
    Running,
    /// Graceful `shutdown` / `exit` in progress.
    Stopping,
    /// Server exited cleanly after shutdown.
    Stopped,
    /// Server exited unexpectedly or failed to initialize.
    Crashed {
        exit_code: Option<i32>,
        message: String,
    },
}

/// Direction of a captured JSON-RPC wire frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireDirection {
    /// Server -> Client (including captured stderr lines).
    In,
    /// Client -> Server.
    Out,
}

/// Captured JSON-RPC frame for the protocol wire inspector.
#[derive(Debug, Clone)]
pub struct WireFrame {
    pub direction: WireDirection,
    pub method: Option<String>,
    pub id: Option<i64>,
    pub json: String,
    pub at: SystemTime,
}

/// Runtime telemetry snapshot for a language server instance.
#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub name: String,
    pub root: PathBuf,
    pub pid: Option<u32>,
    pub status: ServerStatus,
    pub started_at: SystemTime,
    pub capabilities: Option<ServerCapabilities>,
    pub languages: Vec<String>,
    pub avg_latency_ms: f64,
    pub latency_samples_ms: Vec<f64>,
}

/// Events emitted by language servers to subscribed listeners.
#[derive(Debug, Clone)]
pub enum LspEvent {
    Diagnostics {
        server: String,
        uri: Url,
        version: Option<i32>,
        diagnostics: Vec<Diagnostic>,
    },
    Status {
        server: String,
        status: ServerStatus,
    },
    LogMessage {
        server: String,
        level: MessageType,
        message: String,
    },
    ShowMessage {
        server: String,
        level: MessageType,
        message: String,
    },
    Progress {
        server: String,
        token: String,
        title: Option<String>,
        message: Option<String>,
        percentage: Option<u32>,
    },
    ApplyEdit {
        server: String,
        label: Option<String>,
        edit: WorkspaceEdit,
    },
}

/// Multi-subscriber event fan-out hub using `futures::channel::mpsc::UnboundedSender`.
#[derive(Default)]
pub(crate) struct EventHub {
    subscribers: Mutex<Vec<futures_mpsc::UnboundedSender<LspEvent>>>,
}

impl EventHub {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn subscribe(&self) -> futures_mpsc::UnboundedReceiver<LspEvent> {
        let (tx, rx) = futures_mpsc::unbounded();
        self.subscribers.lock().push(tx);
        rx
    }

    pub(crate) fn emit(&self, event: LspEvent) {
        let mut subs = self.subscribers.lock();
        subs.retain(|tx| tx.unbounded_send(event.clone()).is_ok());
    }
}

enum ClientCommand {
    Request {
        method: String,
        params: Value,
        reply: oneshot::Sender<Result<Value>>,
    },
    Notification {
        method: String,
        params: Value,
    },
    DidOpen {
        uri: Url,
        language_id: String,
        version: i32,
        text: String,
    },
    DidChange {
        uri: Url,
        version: i32,
        changes: Vec<TextDocumentContentChangeEvent>,
    },
    DidSave {
        uri: Url,
        text: Option<String>,
    },
    DidClose {
        uri: Url,
    },
    Shutdown {
        reply: Option<oneshot::Sender<Result<()>>>,
    },
}

struct PendingRequest {
    method: String,
    sent_at: Instant,
    reply: oneshot::Sender<Result<Value>>,
}

struct ClientInner {
    name: String,
    root: PathBuf,
    config: ServerConfig,
    started_at: SystemTime,
    pid: Mutex<Option<u32>>,
    status: Mutex<ServerStatus>,
    capabilities: Mutex<Option<ServerCapabilities>>,
    dynamic_registrations: Mutex<HashMap<String, Registration>>,
    latency_samples_ms: Mutex<VecDeque<f64>>,
    wire_log: Mutex<VecDeque<WireFrame>>,
    cmd_tx: tokio_mpsc::UnboundedSender<ClientCommand>,
    events: Arc<EventHub>,
}

impl ClientInner {
    fn set_status(&self, status: ServerStatus) {
        *self.status.lock() = status.clone();
        self.events.emit(LspEvent::Status {
            server: self.name.clone(),
            status,
        });
    }

    fn record_wire(&self, direction: WireDirection, method: Option<String>, id: Option<i64>, json: String) {
        let mut log = self.wire_log.lock();
        if log.len() >= MAX_WIRE_FRAMES {
            log.pop_front();
        }
        log.push_back(WireFrame {
            direction,
            method,
            id,
            json,
            at: SystemTime::now(),
        });
    }

    fn record_latency(&self, elapsed: Duration) {
        let ms = elapsed.as_secs_f64() * 1000.0;
        let mut samples = self.latency_samples_ms.lock();
        if samples.len() >= MAX_LATENCY_SAMPLES {
            samples.pop_front();
        }
        samples.push_back(ms);
    }

    fn info(&self) -> ServerInfo {
        let samples: Vec<f64> = self.latency_samples_ms.lock().iter().copied().collect();
        let avg_latency_ms = if samples.is_empty() {
            0.0
        } else {
            samples.iter().sum::<f64>() / samples.len() as f64
        };
        ServerInfo {
            name: self.name.clone(),
            root: self.root.clone(),
            pid: *self.pid.lock(),
            status: self.status.lock().clone(),
            started_at: self.started_at,
            capabilities: self.capabilities.lock().clone(),
            languages: self.config.languages.clone(),
            avg_latency_ms,
            latency_samples_ms: samples,
        }
    }
}

/// Handle to a single running language server instance for a `(server, root)` pair.
///
/// Cheaply cloneable (`Arc` inside). All `async` methods await only a
/// [`futures::channel::oneshot::Receiver`] so they can be driven by any async
/// executor.
#[derive(Clone)]
pub struct LspClient {
    inner: Arc<ClientInner>,
}

impl std::fmt::Debug for LspClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspClient")
            .field("name", &self.inner.name)
            .field("root", &self.inner.root)
            .field("status", &*self.inner.status.lock())
            .finish()
    }
}

impl LspClient {
    /// Spawn a language server subprocess on `handle` using `config` rooted at `root`.
    pub(crate) fn spawn_process(
        handle: &tokio::runtime::Handle,
        config: ServerConfig,
        executable: PathBuf,
        root: PathBuf,
        events: Arc<EventHub>,
    ) -> Result<Self> {
        let mut cmd = Command::new(&executable);
        cmd.args(&config.args)
            .current_dir(&root)
            .envs(&config.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let _guard = handle.enter();
        let mut child = cmd.spawn().with_context(|| {
            format!(
                "failed to spawn language server '{}' ({:?})",
                config.name, executable
            )
        })?;

        let pid = child.id();
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("failed to take stdin for '{}'", config.name))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("failed to take stdout for '{}'", config.name))?;
        let stderr = child.stderr.take();

        let (cmd_tx, cmd_rx) = tokio_mpsc::unbounded_channel();
        let inner = Arc::new(ClientInner {
            name: config.name.clone(),
            root,
            config,
            started_at: SystemTime::now(),
            pid: Mutex::new(pid),
            status: Mutex::new(ServerStatus::Starting),
            capabilities: Mutex::new(None),
            dynamic_registrations: Mutex::new(HashMap::new()),
            latency_samples_ms: Mutex::new(VecDeque::with_capacity(MAX_LATENCY_SAMPLES)),
            wire_log: Mutex::new(VecDeque::with_capacity(MAX_WIRE_FRAMES)),
            cmd_tx,
            events,
        });

        inner.events.emit(LspEvent::Status {
            server: inner.name.clone(),
            status: ServerStatus::Starting,
        });

        if let Some(stderr) = stderr {
            let stderr_inner = inner.clone();
            handle.spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    debug!(server = %stderr_inner.name, "LSP stderr: {}", line);
                    stderr_inner.record_wire(
                        WireDirection::In,
                        Some("stderr".to_string()),
                        None,
                        line,
                    );
                }
            });
        }

        let driver_inner = inner.clone();
        let io_handle = handle.clone();
        handle.spawn(async move {
            run_connection(io_handle, driver_inner, stdout, stdin, cmd_rx, Some(child)).await;
        });

        Ok(Self { inner })
    }

    /// Create an [`LspClient`] connected to arbitrary async I/O streams (e.g.
    /// `tokio::io::duplex` for in-process testing).
    pub fn from_io<R, W>(
        handle: &tokio::runtime::Handle,
        config: ServerConfig,
        root: PathBuf,
        reader: R,
        writer: W,
    ) -> (Self, futures_mpsc::UnboundedReceiver<LspEvent>)
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let events = Arc::new(EventHub::new());
        let rx = events.subscribe();
        let (cmd_tx, cmd_rx) = tokio_mpsc::unbounded_channel();
        let inner = Arc::new(ClientInner {
            name: config.name.clone(),
            root,
            config,
            started_at: SystemTime::now(),
            pid: Mutex::new(None),
            status: Mutex::new(ServerStatus::Starting),
            capabilities: Mutex::new(None),
            dynamic_registrations: Mutex::new(HashMap::new()),
            latency_samples_ms: Mutex::new(VecDeque::with_capacity(MAX_LATENCY_SAMPLES)),
            wire_log: Mutex::new(VecDeque::with_capacity(MAX_WIRE_FRAMES)),
            cmd_tx,
            events,
        });

        inner.events.emit(LspEvent::Status {
            server: inner.name.clone(),
            status: ServerStatus::Starting,
        });

        let driver_inner = inner.clone();
        let io_handle = handle.clone();
        handle.spawn(async move {
            run_connection(io_handle, driver_inner, reader, writer, cmd_rx, None).await;
        });

        (Self { inner }, rx)
    }

    /// Server name from [`ServerConfig::name`].
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// Workspace root directory this client is serving.
    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    /// Current lifecycle status.
    pub fn status(&self) -> ServerStatus {
        self.inner.status.lock().clone()
    }

    /// Negotiated [`ServerCapabilities`], available once initialization completes.
    pub fn capabilities(&self) -> Option<ServerCapabilities> {
        self.inner.capabilities.lock().clone()
    }

    /// Currently registered dynamic capabilities (from `client/registerCapability`).
    pub fn dynamic_registrations(&self) -> Vec<Registration> {
        self.inner
            .dynamic_registrations
            .lock()
            .values()
            .cloned()
            .collect()
    }

    /// Snapshot of runtime telemetry for this server.
    pub fn info(&self) -> ServerInfo {
        self.inner.info()
    }

    /// Last up to 200 wire frames exchanged with this server.
    pub fn wire_log(&self) -> Vec<WireFrame> {
        self.inner.wire_log.lock().iter().cloned().collect()
    }

    /// Subscribe to events emitted by this client.
    pub fn subscribe(&self) -> futures_mpsc::UnboundedReceiver<LspEvent> {
        self.inner.events.subscribe()
    }

    /// Notify the server that a document was opened.
    pub fn did_open(&self, uri: Url, language_id: &str, version: i32, text: String) {
        let _ = self.inner.cmd_tx.send(ClientCommand::DidOpen {
            uri,
            language_id: language_id.to_string(),
            version,
            text,
        });
    }

    /// Notify the server that a document changed.
    ///
    /// Callers should prefer passing incremental [`TextDocumentContentChangeEvent`]s
    /// (e.g. from `EditDelta`). The client maintains the latest full text per open
    /// document and automatically downgrades incremental edits to a full-document
    /// change if the server negotiated [`TextDocumentSyncKind::FULL`], or drops them
    /// if the server negotiated [`TextDocumentSyncKind::NONE`].
    pub fn did_change(
        &self,
        uri: Url,
        version: i32,
        changes: Vec<TextDocumentContentChangeEvent>,
    ) {
        let _ = self.inner.cmd_tx.send(ClientCommand::DidChange {
            uri,
            version,
            changes,
        });
    }

    /// Convenience wrapper around [`Self::did_change`] when the caller already has
    /// the full updated document text.
    pub fn did_change_full(&self, uri: Url, version: i32, text: String) {
        self.did_change(
            uri,
            version,
            vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text,
            }],
        );
    }

    /// Notify the server that a document was saved.
    pub fn did_save(&self, uri: Url, text: Option<String>) {
        let _ = self
            .inner
            .cmd_tx
            .send(ClientCommand::DidSave { uri, text });
    }

    /// Notify the server that a document was closed.
    pub fn did_close(&self, uri: Url) {
        let _ = self.inner.cmd_tx.send(ClientCommand::DidClose { uri });
    }

    /// Send an arbitrary JSON-RPC notification to the server.
    pub fn notify_raw(&self, method: &str, params: Value) {
        let _ = self.inner.cmd_tx.send(ClientCommand::Notification {
            method: method.to_string(),
            params,
        });
    }

    /// Send an arbitrary JSON-RPC request and await its result.
    ///
    /// Executor-agnostic: awaits only a [`futures::channel::oneshot::Receiver`].
    pub async fn request_raw(&self, method: &str, params: Value) -> Result<Value> {
        let (tx, rx) = oneshot::channel();
        self.inner
            .cmd_tx
            .send(ClientCommand::Request {
                method: method.to_string(),
                params,
                reply: tx,
            })
            .map_err(|_| anyhow!("language server '{}' is no longer running", self.inner.name))?;

        rx.await
            .map_err(|_| anyhow!("language server '{}' closed before replying", self.inner.name))?
    }

    /// Request completion items at `(uri, pos)`.
    pub async fn completion(&self, uri: Url, pos: Position) -> Result<Vec<CompletionItem>> {
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_lsp_uri(&uri)?,
                },
                position: pos,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        };
        let val = self
            .request_raw("textDocument/completion", serde_json::to_value(params)?)
            .await?;
        if val.is_null() {
            return Ok(Vec::new());
        }
        let resp: CompletionResponse = serde_json::from_value(val)?;
        Ok(match resp {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        })
    }

    /// Request hover information at `(uri, pos)`.
    pub async fn hover(&self, uri: Url, pos: Position) -> Result<Option<Hover>> {
        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_lsp_uri(&uri)?,
                },
                position: pos,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let val = self
            .request_raw("textDocument/hover", serde_json::to_value(params)?)
            .await?;
        if val.is_null() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_value(val)?))
    }

    /// Request go-to-definition locations at `(uri, pos)`.
    pub async fn definition(&self, uri: Url, pos: Position) -> Result<Vec<Location>> {
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_lsp_uri(&uri)?,
                },
                position: pos,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let val = self
            .request_raw("textDocument/definition", serde_json::to_value(params)?)
            .await?;
        if val.is_null() {
            return Ok(Vec::new());
        }
        let resp: GotoDefinitionResponse = serde_json::from_value(val)?;
        Ok(match resp {
            GotoDefinitionResponse::Scalar(loc) => vec![loc],
            GotoDefinitionResponse::Array(locs) => locs,
            GotoDefinitionResponse::Link(links) => links
                .into_iter()
                .map(|l| Location {
                    uri: l.target_uri,
                    range: l.target_selection_range,
                })
                .collect(),
        })
    }

    /// Request all references to the symbol at `(uri, pos)`.
    pub async fn references(&self, uri: Url, pos: Position) -> Result<Vec<Location>> {
        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_lsp_uri(&uri)?,
                },
                position: pos,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        };
        let val = self
            .request_raw("textDocument/references", serde_json::to_value(params)?)
            .await?;
        if val.is_null() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_value(val)?)
    }

    /// Request a workspace-wide rename of the symbol at `(uri, pos)` to `new_name`.
    pub async fn rename(
        &self,
        uri: Url,
        pos: Position,
        new_name: String,
    ) -> Result<Option<WorkspaceEdit>> {
        let params = RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_lsp_uri(&uri)?,
                },
                position: pos,
            },
            new_name,
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let val = self
            .request_raw("textDocument/rename", serde_json::to_value(params)?)
            .await?;
        if val.is_null() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_value(val)?))
    }

    /// Request code actions for `range` with accompanying `diagnostics`.
    pub async fn code_actions(
        &self,
        uri: Url,
        range: Range,
        diagnostics: Vec<Diagnostic>,
    ) -> Result<Vec<CodeActionOrCommand>> {
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: url_to_lsp_uri(&uri)?,
            },
            range,
            context: CodeActionContext {
                diagnostics,
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let val = self
            .request_raw("textDocument/codeAction", serde_json::to_value(params)?)
            .await?;
        if val.is_null() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_value(val)?)
    }

    /// Request full-document formatting edits.
    pub async fn formatting(
        &self,
        uri: Url,
        tab_size: u32,
        insert_spaces: bool,
    ) -> Result<Vec<TextEdit>> {
        let params = DocumentFormattingParams {
            text_document: TextDocumentIdentifier {
                uri: url_to_lsp_uri(&uri)?,
            },
            options: FormattingOptions {
                tab_size,
                insert_spaces,
                properties: HashMap::new(),
                trim_trailing_whitespace: None,
                insert_final_newline: None,
                trim_final_newlines: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let val = self
            .request_raw("textDocument/formatting", serde_json::to_value(params)?)
            .await?;
        if val.is_null() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_value(val)?)
    }

    /// Request document symbols, flattened into a pre-order list of [`DocumentSymbolItem`].
    pub async fn document_symbols(&self, uri: Url) -> Result<Vec<DocumentSymbolItem>> {
        let params = DocumentSymbolParams {
            text_document: TextDocumentIdentifier {
                uri: url_to_lsp_uri(&uri)?,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let val = self
            .request_raw("textDocument/documentSymbol", serde_json::to_value(params)?)
            .await?;
        if val.is_null() {
            return Ok(Vec::new());
        }
        let resp: DocumentSymbolResponse = serde_json::from_value(val)?;
        Ok(flatten_document_symbols(resp))
    }

    /// Initiate a graceful shutdown (`shutdown` + `exit`, then kill after timeout)
    /// without waiting for completion.
    pub fn shutdown_nowait(&self) {
        let _ = self
            .inner
            .cmd_tx
            .send(ClientCommand::Shutdown { reply: None });
    }

    /// Gracefully shut down the language server and await completion.
    pub async fn shutdown(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        if self
            .inner
            .cmd_tx
            .send(ClientCommand::Shutdown { reply: Some(tx) })
            .is_err()
        {
            return Ok(());
        }
        let _ = rx.await;
        Ok(())
    }
}

enum ReadEvent {
    Message(String),
    Eof,
    Error(String),
}

async fn run_connection<R, W>(
    handle: tokio::runtime::Handle,
    inner: Arc<ClientInner>,
    reader: R,
    mut writer: W,
    mut cmd_rx: tokio_mpsc::UnboundedReceiver<ClientCommand>,
    mut child: Option<Child>,
) where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (read_tx, mut read_rx) = tokio_mpsc::unbounded_channel();
    handle.spawn(async move {
        let mut buf_reader = BufReader::new(reader);
        loop {
            match read_message(&mut buf_reader).await {
                Ok(Some(msg)) => {
                    if read_tx.send(ReadEvent::Message(msg)).is_err() {
                        break;
                    }
                }
                Ok(None) => {
                    let _ = read_tx.send(ReadEvent::Eof);
                    break;
                }
                Err(e) => {
                    let _ = read_tx.send(ReadEvent::Error(e.to_string()));
                    break;
                }
            }
        }
    });

    let mut next_id: i64 = 1;
    let mut pending: HashMap<i64, PendingRequest> = HashMap::new();
    let mut open_docs: HashMap<Url, String> = HashMap::new();
    let mut progress_titles: HashMap<String, String> = HashMap::new();
    let mut pre_init_queue: VecDeque<ClientCommand> = VecDeque::new();
    let mut initialized = false;

    // 1. Send `initialize` request immediately.
    let init_id = next_id;
    next_id += 1;
    let init_params = build_initialize_params(&inner.root, &inner.config);
    match serde_json::to_value(&init_params)
        .map_err(anyhow::Error::from)
        .and_then(|v| serialize_request(init_id, "initialize", v))
    {
        Ok(json) => {
            inner.record_wire(
                WireDirection::Out,
                Some("initialize".to_string()),
                Some(init_id),
                json.clone(),
            );
            if let Err(e) = write_message(&mut writer, &json).await {
                error!(server = %inner.name, "failed to send initialize: {}", e);
                inner.set_status(ServerStatus::Crashed {
                    exit_code: None,
                    message: format!("failed to send initialize: {}", e),
                });
                return;
            }
        }
        Err(e) => {
            error!(server = %inner.name, "failed to serialize initialize: {}", e);
            inner.set_status(ServerStatus::Crashed {
                exit_code: None,
                message: format!("failed to serialize initialize: {}", e),
            });
            return;
        }
    }
    let init_sent_at = Instant::now();

    loop {
        tokio::select! {
            maybe_read = read_rx.recv() => {
                match maybe_read {
                    Some(ReadEvent::Message(raw)) => {
                        match parse_incoming(&raw) {
                            Ok(IncomingMessage::Response { id, result, error }) => {
                                if !initialized && id == init_id {
                                    inner.record_wire(WireDirection::In, Some("initialize".to_string()), Some(id), raw);
                                    inner.record_latency(init_sent_at.elapsed());
                                    if let Some(err) = error {
                                        let msg = format!("initialize failed ({}): {}", err.code, err.message);
                                        error!(server = %inner.name, "{}", msg);
                                        fail_queued(&mut pre_init_queue, &msg);
                                        inner.set_status(ServerStatus::Crashed { exit_code: None, message: msg });
                                        break;
                                    }
                                    let val = result.unwrap_or(Value::Null);
                                    match serde_json::from_value::<InitializeResult>(val) {
                                        Ok(init_res) => {
                                            *inner.capabilities.lock() = Some(init_res.capabilities);
                                            if let Ok(notif_json) = serialize_notification(
                                                "initialized",
                                                serde_json::to_value(InitializedParams {}).unwrap_or(Value::Object(Default::default())),
                                            ) {
                                                inner.record_wire(WireDirection::Out, Some("initialized".to_string()), None, notif_json.clone());
                                                let _ = write_message(&mut writer, &notif_json).await;
                                            }
                                            initialized = true;
                                            inner.set_status(ServerStatus::Running);
                                            info!(server = %inner.name, root = %inner.root.display(), "LSP server initialized");

                                            // Drain queued commands that arrived while starting.
                                            while let Some(cmd) = pre_init_queue.pop_front() {
                                                if let Err(e) = handle_client_command(
                                                    cmd,
                                                    &inner,
                                                    &mut writer,
                                                    &mut next_id,
                                                    &mut pending,
                                                    &mut open_docs,
                                                ).await {
                                                    warn!(server = %inner.name, "failed to dispatch queued command: {}", e);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            let msg = format!("invalid InitializeResult: {}", e);
                                            error!(server = %inner.name, "{}", msg);
                                            fail_queued(&mut pre_init_queue, &msg);
                                            inner.set_status(ServerStatus::Crashed { exit_code: None, message: msg });
                                            break;
                                        }
                                    }
                                    continue;
                                }

                                if let Some(req) = pending.remove(&id) {
                                    inner.record_wire(WireDirection::In, Some(req.method.clone()), Some(id), raw);
                                    inner.record_latency(req.sent_at.elapsed());
                                    let res = match error {
                                        Some(err) => Err(anyhow!("LSP error {} on {}: {}", err.code, req.method, err.message)),
                                        None => Ok(result.unwrap_or(Value::Null)),
                                    };
                                    let _ = req.reply.send(res);
                                } else {
                                    inner.record_wire(WireDirection::In, None, Some(id), raw);
                                }
                            }
                            Ok(IncomingMessage::Request { id, method, params }) => {
                                let id_i64 = id.as_i64();
                                inner.record_wire(WireDirection::In, Some(method.clone()), id_i64, raw);
                                let resp_json = handle_server_request(&inner, id, &method, params);
                                inner.record_wire(WireDirection::Out, Some(method), id_i64, resp_json.clone());
                                if let Err(e) = write_message(&mut writer, &resp_json).await {
                                    warn!(server = %inner.name, "failed to reply to server request: {}", e);
                                }
                            }
                            Ok(IncomingMessage::Notification { method, params }) => {
                                inner.record_wire(WireDirection::In, Some(method.clone()), None, raw);
                                handle_server_notification(&inner, &method, params, &mut progress_titles);
                            }
                            Err(e) => {
                                inner.record_wire(WireDirection::In, None, None, raw);
                                warn!(server = %inner.name, "failed to parse incoming LSP message: {}", e);
                            }
                        }
                    }
                    Some(ReadEvent::Eof) | None => {
                        let exit_code = wait_child_exit_code(&mut child).await;
                        let msg = match exit_code {
                            Some(code) => format!("language server exited unexpectedly with code {}", code),
                            None => "language server connection closed unexpectedly".to_string(),
                        };
                        warn!(server = %inner.name, "{}", msg);
                        fail_pending(&mut pending, &msg);
                        fail_queued(&mut pre_init_queue, &msg);
                        inner.set_status(ServerStatus::Crashed { exit_code, message: msg });
                        break;
                    }
                    Some(ReadEvent::Error(err_msg)) => {
                        let exit_code = wait_child_exit_code(&mut child).await;
                        warn!(server = %inner.name, "LSP transport read error: {}", err_msg);
                        fail_pending(&mut pending, &err_msg);
                        fail_queued(&mut pre_init_queue, &err_msg);
                        inner.set_status(ServerStatus::Crashed { exit_code, message: err_msg });
                        break;
                    }
                }
            }
            maybe_cmd = cmd_rx.recv() => {
                match maybe_cmd {
                    Some(ClientCommand::Shutdown { reply }) => {
                        inner.set_status(ServerStatus::Stopping);
                        fail_pending(&mut pending, "language server shutting down");
                        fail_queued(&mut pre_init_queue, "language server shutting down");

                        let id = next_id;
                        if let Ok(json) = serialize_request(id, "shutdown", Value::Null) {
                            inner.record_wire(WireDirection::Out, Some("shutdown".to_string()), Some(id), json.clone());
                            if write_message(&mut writer, &json).await.is_err() {
                                finish_shutdown(&inner, &mut child, reply).await;
                                break;
                            }
                        }

                        // Wait up to SHUTDOWN_TIMEOUT for the `shutdown` response while
                        // still draining incoming frames so the server doesn't block on stdout.
                        let deadline = tokio::time::Instant::now() + SHUTDOWN_TIMEOUT;
                        loop {
                            match tokio::time::timeout_at(deadline, read_rx.recv()).await {
                                Ok(Some(ReadEvent::Message(raw))) => {
                                    if let Ok(IncomingMessage::Response { id: resp_id, .. }) = parse_incoming(&raw) {
                                        if resp_id == id {
                                            inner.record_wire(WireDirection::In, Some("shutdown".to_string()), Some(resp_id), raw);
                                            if let Ok(exit_json) = serialize_notification("exit", Value::Null) {
                                                inner.record_wire(WireDirection::Out, Some("exit".to_string()), None, exit_json.clone());
                                                let _ = write_message(&mut writer, &exit_json).await;
                                            }
                                            break;
                                        }
                                    }
                                }
                                _ => {
                                    // Timeout or EOF; proceed to terminate process.
                                    break;
                                }
                            }
                        }
                        finish_shutdown(&inner, &mut child, reply).await;
                        break;
                    }
                    Some(cmd) => {
                        if !initialized {
                            pre_init_queue.push_back(cmd);
                        } else if let Err(e) = handle_client_command(
                            cmd,
                            &inner,
                            &mut writer,
                            &mut next_id,
                            &mut pending,
                            &mut open_docs,
                        ).await {
                            warn!(server = %inner.name, "LSP write error: {}", e);
                        }
                    }
                    None => {
                        // All client handles dropped.
                        finish_shutdown(&inner, &mut child, None).await;
                        break;
                    }
                }
            }
        }
    }
}

async fn handle_client_command<W: AsyncWrite + Unpin>(
    cmd: ClientCommand,
    inner: &ClientInner,
    writer: &mut W,
    next_id: &mut i64,
    pending: &mut HashMap<i64, PendingRequest>,
    open_docs: &mut HashMap<Url, String>,
) -> Result<()> {
    match cmd {
        ClientCommand::Request {
            method,
            params,
            reply,
        } => {
            let id = *next_id;
            *next_id += 1;
            let json = serialize_request(id, &method, params)?;
            inner.record_wire(WireDirection::Out, Some(method.clone()), Some(id), json.clone());
            if let Err(e) = write_message(writer, &json).await {
                let _ = reply.send(Err(anyhow!("failed to write LSP request '{}': {}", method, e)));
                return Err(e);
            }
            pending.insert(
                id,
                PendingRequest {
                    method,
                    sent_at: Instant::now(),
                    reply,
                },
            );
        }
        ClientCommand::Notification { method, params } => {
            let json = serialize_notification(&method, params)?;
            inner.record_wire(WireDirection::Out, Some(method), None, json.clone());
            write_message(writer, &json).await?;
        }
        ClientCommand::DidOpen {
            uri,
            language_id,
            version,
            text,
        } => {
            open_docs.insert(uri.clone(), text.clone());
            let params = DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: url_to_lsp_uri(&uri)?,
                    language_id,
                    version,
                    text,
                },
            };
            let json = serialize_notification("textDocument/didOpen", serde_json::to_value(params)?)?;
            inner.record_wire(
                WireDirection::Out,
                Some("textDocument/didOpen".to_string()),
                None,
                json.clone(),
            );
            write_message(writer, &json).await?;
        }
        ClientCommand::DidChange {
            uri,
            version,
            changes,
        } => {
            let full_text = {
                let entry = open_docs.entry(uri.clone()).or_default();
                apply_content_changes(entry, &changes);
                entry.clone()
            };

            let sync_kind = {
                let caps = inner.capabilities.lock();
                negotiated_sync_kind(caps.as_ref())
            };

            let wire_changes = match sync_kind {
                TextDocumentSyncKind::NONE => return Ok(()),
                TextDocumentSyncKind::FULL => vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: full_text,
                }],
                _ => changes,
            };

            let params = DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: url_to_lsp_uri(&uri)?,
                    version,
                },
                content_changes: wire_changes,
            };
            let json =
                serialize_notification("textDocument/didChange", serde_json::to_value(params)?)?;
            inner.record_wire(
                WireDirection::Out,
                Some("textDocument/didChange".to_string()),
                None,
                json.clone(),
            );
            write_message(writer, &json).await?;
        }
        ClientCommand::DidSave { uri, text } => {
            let params = DidSaveTextDocumentParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_lsp_uri(&uri)?,
                },
                text,
            };
            let json =
                serialize_notification("textDocument/didSave", serde_json::to_value(params)?)?;
            inner.record_wire(
                WireDirection::Out,
                Some("textDocument/didSave".to_string()),
                None,
                json.clone(),
            );
            write_message(writer, &json).await?;
        }
        ClientCommand::DidClose { uri } => {
            open_docs.remove(&uri);
            let params = DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_lsp_uri(&uri)?,
                },
            };
            let json =
                serialize_notification("textDocument/didClose", serde_json::to_value(params)?)?;
            inner.record_wire(
                WireDirection::Out,
                Some("textDocument/didClose".to_string()),
                None,
                json.clone(),
            );
            write_message(writer, &json).await?;
        }
        ClientCommand::Shutdown { .. } => {}
    }
    Ok(())
}

fn handle_server_request(inner: &ClientInner, id: Value, method: &str, params: Value) -> String {
    match method {
        "workspace/configuration" => {
            let result = match serde_json::from_value::<ConfigurationParams>(params) {
                Ok(cfg) => {
                    let values: Vec<Value> = cfg
                        .items
                        .iter()
                        .map(|item| {
                            resolve_config_section(
                                inner.config.settings.as_ref(),
                                item.section.as_deref(),
                            )
                        })
                        .collect();
                    Value::Array(values)
                }
                Err(_) => Value::Array(Vec::new()),
            };
            serialize_response_ok(id, result).unwrap_or_default()
        }
        "client/registerCapability" => {
            if let Ok(reg_params) = serde_json::from_value::<RegistrationParams>(params) {
                let mut map = inner.dynamic_registrations.lock();
                for reg in reg_params.registrations {
                    map.insert(reg.id.clone(), reg);
                }
            }
            serialize_response_ok(id, Value::Null).unwrap_or_default()
        }
        "client/unregisterCapability" => {
            if let Ok(unreg_params) = serde_json::from_value::<UnregistrationParams>(params) {
                let mut map = inner.dynamic_registrations.lock();
                for unreg in unreg_params.unregisterations {
                    map.remove(&unreg.id);
                }
            }
            serialize_response_ok(id, Value::Null).unwrap_or_default()
        }
        "window/workDoneProgress/create" => {
            serialize_response_ok(id, Value::Null).unwrap_or_default()
        }
        "workspace/applyEdit" => {
            if let Ok(apply_params) = serde_json::from_value::<ApplyWorkspaceEditParams>(params) {
                inner.events.emit(LspEvent::ApplyEdit {
                    server: inner.name.clone(),
                    label: apply_params.label,
                    edit: apply_params.edit,
                });
            }
            serialize_response_ok(
                id,
                serde_json::json!({
                    "applied": false,
                    "failureReason": "workspace/applyEdit is surfaced as an event and not auto-applied"
                }),
            )
            .unwrap_or_default()
        }
        "window/showMessageRequest" => {
            if let Ok(msg) = serde_json::from_value::<ShowMessageRequestParams>(params) {
                inner.events.emit(LspEvent::ShowMessage {
                    server: inner.name.clone(),
                    level: msg.typ,
                    message: msg.message,
                });
            }
            serialize_response_ok(id, Value::Null).unwrap_or_default()
        }
        _ => {
            debug!(server = %inner.name, method = %method, "unhandled server->client LSP request");
            serialize_response_err(id, -32601, &format!("Method not found: {}", method))
                .unwrap_or_default()
        }
    }
}

fn handle_server_notification(
    inner: &ClientInner,
    method: &str,
    params: Value,
    progress_titles: &mut HashMap<String, String>,
) {
    match method {
        "textDocument/publishDiagnostics" => {
            if let Ok(diag_params) = serde_json::from_value::<PublishDiagnosticsParams>(params) {
                if let Ok(url) = lsp_uri_to_url(&diag_params.uri) {
                    inner.events.emit(LspEvent::Diagnostics {
                        server: inner.name.clone(),
                        uri: url,
                        version: diag_params.version,
                        diagnostics: diag_params.diagnostics,
                    });
                }
            }
        }
        "$/progress" => {
            if let Ok(prog) = serde_json::from_value::<ProgressParams>(params) {
                let token = match prog.token {
                    NumberOrString::Number(n) => n.to_string(),
                    NumberOrString::String(s) => s,
                };
                match prog.value {
                    ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(begin)) => {
                        progress_titles.insert(token.clone(), begin.title.clone());
                        inner.events.emit(LspEvent::Progress {
                            server: inner.name.clone(),
                            token,
                            title: Some(begin.title),
                            message: begin.message,
                            percentage: begin.percentage,
                        });
                    }
                    ProgressParamsValue::WorkDone(WorkDoneProgress::Report(report)) => {
                        let title = progress_titles.get(&token).cloned();
                        inner.events.emit(LspEvent::Progress {
                            server: inner.name.clone(),
                            token,
                            title,
                            message: report.message,
                            percentage: report.percentage,
                        });
                    }
                    ProgressParamsValue::WorkDone(WorkDoneProgress::End(end)) => {
                        let title = progress_titles.remove(&token);
                        inner.events.emit(LspEvent::Progress {
                            server: inner.name.clone(),
                            token,
                            title,
                            message: end.message,
                            percentage: Some(100),
                        });
                    }
                }
            }
        }
        "window/logMessage" => {
            if let Ok(log) = serde_json::from_value::<LogMessageParams>(params) {
                debug!(server = %inner.name, level = ?log.typ, "LSP log: {}", log.message);
                inner.events.emit(LspEvent::LogMessage {
                    server: inner.name.clone(),
                    level: log.typ,
                    message: log.message,
                });
            }
        }
        "window/showMessage" => {
            if let Ok(show) = serde_json::from_value::<ShowMessageParams>(params) {
                info!(server = %inner.name, level = ?show.typ, "LSP message: {}", show.message);
                inner.events.emit(LspEvent::ShowMessage {
                    server: inner.name.clone(),
                    level: show.typ,
                    message: show.message,
                });
            }
        }
        _ => {
            debug!(server = %inner.name, method = %method, "unhandled LSP notification");
        }
    }
}

fn build_initialize_params(root: &Path, config: &ServerConfig) -> InitializeParams {
    let root_url = path_to_url(root).ok();
    let root_uri = root_url.as_ref().and_then(|u| url_to_lsp_uri(u).ok());
    let folder_name = root
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "workspace".to_string());

    let workspace_folders = root_uri.clone().map(|uri| {
        vec![WorkspaceFolder {
            uri,
            name: folder_name,
        }]
    });

    #[allow(deprecated)]
    InitializeParams {
        process_id: Some(std::process::id()),
        root_path: Some(root.to_string_lossy().into_owned()),
        root_uri,
        initialization_options: config.initialization_options.clone(),
        capabilities: build_client_capabilities(),
        trace: None,
        workspace_folders,
        client_info: Some(lsp_types::ClientInfo {
            name: "Blur".to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
        }),
        locale: None,
        work_done_progress_params: WorkDoneProgressParams::default(),
    }
}

fn fail_pending(pending: &mut HashMap<i64, PendingRequest>, message: &str) {
    for (_, req) in pending.drain() {
        let _ = req.reply.send(Err(anyhow!("{}", message)));
    }
}

fn fail_queued(queue: &mut VecDeque<ClientCommand>, message: &str) {
    while let Some(cmd) = queue.pop_front() {
        if let ClientCommand::Request { reply, .. } = cmd {
            let _ = reply.send(Err(anyhow!("{}", message)));
        }
    }
}

async fn wait_child_exit_code(child: &mut Option<Child>) -> Option<i32> {
    let Some(c) = child.as_mut() else {
        return None;
    };
    match tokio::time::timeout(Duration::from_millis(500), c.wait()).await {
        Ok(Ok(status)) => status.code(),
        _ => None,
    }
}

async fn finish_shutdown(
    inner: &ClientInner,
    child: &mut Option<Child>,
    reply: Option<oneshot::Sender<Result<()>>>,
) {
    if let Some(c) = child.as_mut() {
        let exited = tokio::time::timeout(Duration::from_secs(1), c.wait()).await;
        if !matches!(exited, Ok(Ok(_))) {
            let _ = c.kill().await;
            let _ = c.wait().await;
        }
    }
    inner.set_status(ServerStatus::Stopped);
    if let Some(r) = reply {
        let _ = r.send(Ok(()));
    }
}
