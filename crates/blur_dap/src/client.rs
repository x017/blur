//! Generic Debug Adapter Protocol (DAP) client.
//!
//! All I/O runs on tokio tasks spawned on the caller-provided
//! [`tokio::runtime::Handle`]. Every public `async` request method awaits only a
//! [`futures::channel::oneshot::Receiver`] so it can be awaited from any
//! executor (such as GPUI).

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Context, Result};
use futures::channel::{mpsc as futures_mpsc, oneshot};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{debug, warn};

use crate::transport::{read_message, write_message};
use crate::types::{
    Breakpoint, Capabilities, DapEvent, DebugAdapterConfig, EvaluateResponse, FunctionBreakpoint,
    IncomingEnvelope, Scope, Source, SourceBreakpoint, StackFrame, Thread, Variable, WireDirection,
    WireFrame, WireRequest, WireResponse,
};

const MAX_WIRE_FRAMES: usize = 200;

#[derive(Default)]
struct EventHub {
    subscribers: Mutex<Vec<futures_mpsc::UnboundedSender<DapEvent>>>,
}

impl EventHub {
    fn subscribe(&self) -> futures_mpsc::UnboundedReceiver<DapEvent> {
        let (tx, rx) = futures_mpsc::unbounded();
        self.subscribers.lock().push(tx);
        rx
    }

    fn emit(&self, event: DapEvent) {
        let mut subs = self.subscribers.lock();
        subs.retain(|tx| tx.unbounded_send(event.clone()).is_ok());
    }
}

enum ClientCommand {
    Request {
        command: String,
        arguments: Option<Value>,
        reply: oneshot::Sender<Result<Value>>,
    },
    Shutdown,
}

struct ClientInner {
    name: String,
    capabilities: Mutex<Option<Capabilities>>,
    wire_log: Mutex<VecDeque<WireFrame>>,
    cmd_tx: tokio_mpsc::UnboundedSender<ClientCommand>,
    events: EventHub,
}

impl ClientInner {
    fn record_wire(
        &self,
        direction: WireDirection,
        kind: Option<String>,
        seq: Option<i64>,
        json: String,
    ) {
        let mut log = self.wire_log.lock();
        if log.len() >= MAX_WIRE_FRAMES {
            log.pop_front();
        }
        log.push_back(WireFrame {
            direction,
            kind,
            seq,
            json,
            at: SystemTime::now(),
        });
    }
}

/// Handle to a running Debug Adapter Protocol session.
///
/// Cheaply cloneable (`Arc` inside). All `async` methods are executor-agnostic.
#[derive(Clone)]
pub struct DapClient {
    inner: Arc<ClientInner>,
}

impl std::fmt::Debug for DapClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DapClient")
            .field("name", &self.inner.name)
            .finish()
    }
}

impl DapClient {
    /// Spawn a debug adapter subprocess on `handle` using `config`.
    pub fn spawn(handle: tokio::runtime::Handle, config: DebugAdapterConfig) -> Result<Self> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .envs(&config.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let _guard = handle.enter();
        let mut child = cmd.spawn().with_context(|| {
            format!(
                "failed to spawn debug adapter '{}' ({})",
                config.name, config.command
            )
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("failed to take stdin for debug adapter '{}'", config.name))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("failed to take stdout for debug adapter '{}'", config.name))?;
        let stderr = child.stderr.take();

        let (cmd_tx, cmd_rx) = tokio_mpsc::unbounded_channel();
        let inner = Arc::new(ClientInner {
            name: config.name,
            capabilities: Mutex::new(None),
            wire_log: Mutex::new(VecDeque::with_capacity(MAX_WIRE_FRAMES)),
            cmd_tx,
            events: EventHub::default(),
        });

        if let Some(stderr) = stderr {
            let stderr_inner = inner.clone();
            handle.spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    debug!(adapter = %stderr_inner.name, "DAP stderr: {}", line);
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

    /// Create a [`DapClient`] connected to arbitrary async I/O streams (e.g.
    /// `tokio::io::duplex` for in-process testing).
    pub fn from_io<R, W>(
        handle: tokio::runtime::Handle,
        name: impl Into<String>,
        reader: R,
        writer: W,
    ) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (cmd_tx, cmd_rx) = tokio_mpsc::unbounded_channel();
        let inner = Arc::new(ClientInner {
            name: name.into(),
            capabilities: Mutex::new(None),
            wire_log: Mutex::new(VecDeque::with_capacity(MAX_WIRE_FRAMES)),
            cmd_tx,
            events: EventHub::default(),
        });

        let driver_inner = inner.clone();
        let io_handle = handle.clone();
        handle.spawn(async move {
            run_connection(io_handle, driver_inner, reader, writer, cmd_rx, None).await;
        });

        Self { inner }
    }

    /// Adapter name.
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// Subscribe to the [`DapEvent`] stream. Multiple subscribers are supported.
    pub fn events(&self) -> futures_mpsc::UnboundedReceiver<DapEvent> {
        self.inner.events.subscribe()
    }

    /// Negotiated adapter capabilities (populated after [`Self::initialize`]).
    pub fn capabilities(&self) -> Option<Capabilities> {
        self.inner.capabilities.lock().clone()
    }

    /// Last up to 200 captured DAP wire frames.
    pub fn wire_log(&self) -> Vec<WireFrame> {
        self.inner.wire_log.lock().iter().cloned().collect()
    }

    /// Send an arbitrary DAP request and await its `body` JSON value.
    ///
    /// Executor-agnostic: awaits only a [`futures::channel::oneshot::Receiver`].
    pub async fn request_raw(&self, command: &str, arguments: Option<Value>) -> Result<Value> {
        let (tx, rx) = oneshot::channel();
        self.inner
            .cmd_tx
            .send(ClientCommand::Request {
                command: command.to_string(),
                arguments,
                reply: tx,
            })
            .map_err(|_| anyhow!("debug adapter '{}' is no longer running", self.inner.name))?;

        rx.await.map_err(|_| {
            anyhow!(
                "debug adapter '{}' closed before replying to '{}'",
                self.inner.name,
                command
            )
        })?
    }

    /// Send the DAP `initialize` request and store negotiated [`Capabilities`].
    pub async fn initialize(&self) -> Result<Capabilities> {
        let args = json!({
            "clientID": "blur",
            "clientName": "Blur",
            "adapterID": self.inner.name,
            "locale": "en-US",
            "linesStartAt1": true,
            "columnsStartAt1": true,
            "pathFormat": "path",
            "supportsVariableType": true,
            "supportsVariablePaging": false,
            "supportsRunInTerminalRequest": false,
            "supportsArgsCanBeInterpretedByShell": false,
            "supportsStartDebuggingRequest": false,
        });
        let body = self.request_raw("initialize", Some(args)).await?;
        let caps: Capabilities = if body.is_null() {
            Capabilities::default()
        } else {
            serde_json::from_value(body)?
        };
        *self.inner.capabilities.lock() = Some(caps.clone());
        Ok(caps)
    }

    /// Send a `launch` request with adapter-specific JSON arguments.
    pub async fn launch(&self, args: Value) -> Result<()> {
        self.request_raw("launch", Some(args)).await?;
        Ok(())
    }

    /// Send an `attach` request with adapter-specific JSON arguments.
    pub async fn attach(&self, args: Value) -> Result<()> {
        self.request_raw("attach", Some(args)).await?;
        Ok(())
    }

    /// Set source breakpoints for `source_path`.
    pub async fn set_breakpoints(
        &self,
        source_path: &Path,
        breakpoints: Vec<SourceBreakpoint>,
    ) -> Result<Vec<Breakpoint>> {
        let source = Source {
            name: source_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned()),
            path: Some(source_path.to_string_lossy().into_owned()),
            source_reference: None,
        };
        let args = json!({
            "source": source,
            "breakpoints": breakpoints,
        });
        let body = self.request_raw("setBreakpoints", Some(args)).await?;
        let bps = body
            .get("breakpoints")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));
        Ok(serde_json::from_value(bps)?)
    }

    /// Set function breakpoints.
    pub async fn set_function_breakpoints(
        &self,
        breakpoints: Vec<FunctionBreakpoint>,
    ) -> Result<Vec<Breakpoint>> {
        let args = json!({ "breakpoints": breakpoints });
        let body = self
            .request_raw("setFunctionBreakpoints", Some(args))
            .await?;
        let bps = body
            .get("breakpoints")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));
        Ok(serde_json::from_value(bps)?)
    }

    /// Notify the adapter that initial breakpoint/configuration setup is complete.
    pub async fn configuration_done(&self) -> Result<()> {
        self.request_raw("configurationDone", None).await?;
        Ok(())
    }

    /// Retrieve all active threads.
    pub async fn threads(&self) -> Result<Vec<Thread>> {
        let body = self.request_raw("threads", None).await?;
        let threads = body
            .get("threads")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));
        Ok(serde_json::from_value(threads)?)
    }

    /// Retrieve stack frames for `thread_id`.
    pub async fn stack_trace(&self, thread_id: i64) -> Result<Vec<StackFrame>> {
        let body = self
            .request_raw("stackTrace", Some(json!({ "threadId": thread_id })))
            .await?;
        let frames = body
            .get("stackFrames")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));
        Ok(serde_json::from_value(frames)?)
    }

    /// Retrieve variable scopes for `frame_id`.
    pub async fn scopes(&self, frame_id: i64) -> Result<Vec<Scope>> {
        let body = self
            .request_raw("scopes", Some(json!({ "frameId": frame_id })))
            .await?;
        let scopes = body
            .get("scopes")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));
        Ok(serde_json::from_value(scopes)?)
    }

    /// Retrieve child variables for `variables_reference`.
    pub async fn variables(&self, variables_reference: i64) -> Result<Vec<Variable>> {
        let body = self
            .request_raw(
                "variables",
                Some(json!({ "variablesReference": variables_reference })),
            )
            .await?;
        let vars = body
            .get("variables")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));
        Ok(serde_json::from_value(vars)?)
    }

    /// Evaluate an expression in an optional stack frame and context
    /// (e.g. `"watch"`, `"repl"`, `"hover"`).
    pub async fn evaluate(
        &self,
        expression: &str,
        frame_id: Option<i64>,
        context: Option<&str>,
    ) -> Result<EvaluateResponse> {
        let mut args = serde_json::Map::new();
        args.insert("expression".to_string(), json!(expression));
        if let Some(fid) = frame_id {
            args.insert("frameId".to_string(), json!(fid));
        }
        if let Some(ctx) = context {
            args.insert("context".to_string(), json!(ctx));
        }
        let body = self
            .request_raw("evaluate", Some(Value::Object(args)))
            .await?;
        Ok(serde_json::from_value(body)?)
    }

    /// Resume execution of `thread_id`. Returns `allThreadsContinued` (defaults to `true`).
    pub async fn continue_(&self, thread_id: i64) -> Result<bool> {
        let body = self
            .request_raw("continue", Some(json!({ "threadId": thread_id })))
            .await?;
        Ok(body
            .get("allThreadsContinued")
            .and_then(|v| v.as_bool())
            .unwrap_or(true))
    }

    /// Step over (next statement) on `thread_id`.
    pub async fn next(&self, thread_id: i64) -> Result<()> {
        self.request_raw("next", Some(json!({ "threadId": thread_id })))
            .await?;
        Ok(())
    }

    /// Step into function call on `thread_id`.
    pub async fn step_in(&self, thread_id: i64) -> Result<()> {
        self.request_raw("stepIn", Some(json!({ "threadId": thread_id })))
            .await?;
        Ok(())
    }

    /// Step out of current function on `thread_id`.
    pub async fn step_out(&self, thread_id: i64) -> Result<()> {
        self.request_raw("stepOut", Some(json!({ "threadId": thread_id })))
            .await?;
        Ok(())
    }

    /// Pause execution of `thread_id`.
    pub async fn pause(&self, thread_id: i64) -> Result<()> {
        self.request_raw("pause", Some(json!({ "threadId": thread_id })))
            .await?;
        Ok(())
    }

    /// Disconnect from the debug adapter, optionally terminating the debuggee.
    pub async fn disconnect(&self, terminate_debuggee: bool) -> Result<()> {
        let res = self
            .request_raw(
                "disconnect",
                Some(json!({ "terminateDebuggee": terminate_debuggee })),
            )
            .await;
        let _ = self.inner.cmd_tx.send(ClientCommand::Shutdown);
        res.map(|_| ())
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

    let mut next_seq: i64 = 1;
    let mut pending: HashMap<i64, (String, oneshot::Sender<Result<Value>>)> = HashMap::new();

    loop {
        tokio::select! {
            maybe_read = read_rx.recv() => {
                match maybe_read {
                    Some(ReadEvent::Message(raw)) => {
                        match serde_json::from_str::<IncomingEnvelope>(&raw) {
                            Ok(env) => {
                                match env.msg_type.as_str() {
                                    "response" => {
                                        let req_seq = env.request_seq.unwrap_or(-1);
                                        inner.record_wire(WireDirection::In, env.command.clone(), Some(env.seq), raw);
                                        if let Some((cmd_name, tx)) = pending.remove(&req_seq) {
                                            if env.success.unwrap_or(false) {
                                                let _ = tx.send(Ok(env.body.unwrap_or(Value::Null)));
                                            } else {
                                                let msg = env.message.unwrap_or_else(|| "request failed".to_string());
                                                let _ = tx.send(Err(anyhow!("DAP error on '{}': {}", cmd_name, msg)));
                                            }
                                        }
                                    }
                                    "event" => {
                                        let event_name = env.event.clone().unwrap_or_default();
                                        inner.record_wire(WireDirection::In, Some(event_name.clone()), Some(env.seq), raw.clone());
                                        let parsed = parse_dap_event(&event_name, env.body, &raw);
                                        inner.events.emit(parsed);
                                    }
                                    "request" => {
                                        // Reverse request from adapter (e.g. `runInTerminal`).
                                        let cmd = env.command.unwrap_or_else(|| "unknown".to_string());
                                        inner.record_wire(WireDirection::In, Some(cmd.clone()), Some(env.seq), raw);
                                        let resp_seq = next_seq;
                                        next_seq += 1;
                                        let msg = if cmd == "runInTerminal" {
                                            "runInTerminal is not supported"
                                        } else {
                                            "reverse request not supported"
                                        };
                                        let resp = WireResponse {
                                            seq: resp_seq,
                                            msg_type: "response",
                                            request_seq: env.seq,
                                            success: false,
                                            command: &cmd,
                                            message: Some(msg),
                                            body: None,
                                        };
                                        if let Ok(json) = serde_json::to_string(&resp) {
                                            inner.record_wire(WireDirection::Out, Some(cmd), Some(resp_seq), json.clone());
                                            let _ = write_message(&mut writer, &json).await;
                                        }
                                    }
                                    other => {
                                        inner.record_wire(WireDirection::In, Some(other.to_string()), Some(env.seq), raw);
                                        debug!(adapter = %inner.name, msg_type = %other, "ignoring unknown DAP message type");
                                    }
                                }
                            }
                            Err(e) => {
                                inner.record_wire(WireDirection::In, None, None, raw);
                                warn!(adapter = %inner.name, "failed to parse DAP message: {}", e);
                            }
                        }
                    }
                    Some(ReadEvent::Eof) | None => {
                        for (_, (_, tx)) in pending.drain() {
                            let _ = tx.send(Err(anyhow!("debug adapter connection closed")));
                        }
                        break;
                    }
                    Some(ReadEvent::Error(err)) => {
                        for (_, (_, tx)) in pending.drain() {
                            let _ = tx.send(Err(anyhow!("debug adapter read error: {}", err)));
                        }
                        break;
                    }
                }
            }
            maybe_cmd = cmd_rx.recv() => {
                match maybe_cmd {
                    Some(ClientCommand::Request { command, arguments, reply }) => {
                        let seq = next_seq;
                        next_seq += 1;
                        let req = WireRequest {
                            seq,
                            msg_type: "request",
                            command: &command,
                            arguments,
                        };
                        match serde_json::to_string(&req) {
                            Ok(json) => {
                                inner.record_wire(WireDirection::Out, Some(command.clone()), Some(seq), json.clone());
                                if let Err(e) = write_message(&mut writer, &json).await {
                                    let _ = reply.send(Err(anyhow!("failed to write DAP '{}': {}", command, e)));
                                } else {
                                    pending.insert(seq, (command, reply));
                                }
                            }
                            Err(e) => {
                                let _ = reply.send(Err(anyhow!("failed to serialize DAP '{}': {}", command, e)));
                            }
                        }
                    }
                    Some(ClientCommand::Shutdown) | None => {
                        for (_, (_, tx)) in pending.drain() {
                            let _ = tx.send(Err(anyhow!("debug adapter shutting down")));
                        }
                        break;
                    }
                }
            }
        }
    }

    if let Some(c) = child.as_mut() {
        let exited = tokio::time::timeout(Duration::from_secs(1), c.wait()).await;
        if !matches!(exited, Ok(Ok(_))) {
            let _ = c.kill().await;
            let _ = c.wait().await;
        }
    }
}

fn parse_dap_event(event: &str, body: Option<Value>, raw: &str) -> DapEvent {
    let body_val = body.unwrap_or(Value::Null);
    match event {
        "initialized" => DapEvent::Initialized,
        "stopped" => DapEvent::Stopped {
            reason: body_val
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            description: body_val
                .get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            thread_id: body_val.get("threadId").and_then(|v| v.as_i64()),
            all_threads_stopped: body_val
                .get("allThreadsStopped")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            text: body_val
                .get("text")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        },
        "continued" => DapEvent::Continued {
            thread_id: body_val
                .get("threadId")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            all_threads_continued: body_val
                .get("allThreadsContinued")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
        },
        "exited" => DapEvent::Exited(
            body_val
                .get("exitCode")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
        ),
        "terminated" => DapEvent::Terminated {
            restart: body_val.get("restart").cloned(),
        },
        "output" => DapEvent::Output {
            category: body_val
                .get("category")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            output: body_val
                .get("output")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        },
        "breakpoint" => {
            let reason = body_val
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("changed")
                .to_string();
            let bp = body_val
                .get("breakpoint")
                .cloned()
                .and_then(|v| serde_json::from_value::<Breakpoint>(v).ok())
                .unwrap_or_default();
            DapEvent::Breakpoint {
                reason,
                breakpoint: bp,
            }
        }
        "thread" => DapEvent::Thread {
            reason: body_val
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("started")
                .to_string(),
            thread_id: body_val
                .get("threadId")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
        },
        _ => DapEvent::Raw(serde_json::from_str(raw).unwrap_or(body_val)),
    }
}
