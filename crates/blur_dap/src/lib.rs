//! Generic Debug Adapter Protocol (DAP) client for Blur.
//!
//! Pure Rust, GPUI-free, and adapter-agnostic. Uses Content-Length framed JSON
//! over stdio (or any [`tokio::io::AsyncRead`]/[`tokio::io::AsyncWrite`] pair),
//! runs I/O on tokio tasks spawned on a caller-supplied
//! [`tokio::runtime::Handle`], and exposes executor-agnostic async methods.

pub mod client;
pub mod transport;
pub mod types;

pub use client::DapClient;
pub use types::{
    load_launch_configs, parse_launch_toml, Breakpoint, Capabilities, DapEvent, DebugAdapterConfig,
    EvaluateResponse, FunctionBreakpoint, LaunchConfig, LaunchRequestKind, Scope, Source,
    SourceBreakpoint, StackFrame, Thread, Variable, WireDirection, WireFrame,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use futures::StreamExt;
    use serde_json::json;
    use tokio::io::{AsyncWriteExt, BufReader};

    use crate::transport::{encode_message, read_message, write_message};

    #[tokio::test]
    async fn dap_framing_partial_and_multi() {
        let m1 = r#"{"seq":1,"type":"request","command":"initialize"}"#;
        let m2 = r#"{"seq":2,"type":"event","event":"initialized"}"#;
        let mut wire = encode_message(m1);
        wire.extend_from_slice(&encode_message(m2));

        let (mut w, r) = tokio::io::duplex(64);
        let t = tokio::spawn(async move {
            for chunk in wire.chunks(7) {
                w.write_all(chunk).await.unwrap();
                w.flush().await.unwrap();
            }
        });

        let mut br = BufReader::new(r);
        assert_eq!(read_message(&mut br).await.unwrap().unwrap(), m1);
        assert_eq!(read_message(&mut br).await.unwrap().unwrap(), m2);
        assert!(read_message(&mut br).await.unwrap().is_none());
        t.await.unwrap();
    }

    #[test]
    fn parse_launch_toml_configs() {
        let toml_src = r#"
[[configuration]]
name = "Debug binary"
adapter = "codelldb"
request = "launch"
[configuration.args]
program = "${workspaceFolder}/target/debug/app"
stopOnEntry = true

[[configuration]]
name = "Attach PID"
adapter = "codelldb"
request = "attach"
[configuration.args]
pid = 1234
"#;
        let cfgs = parse_launch_toml(toml_src).unwrap();
        assert_eq!(cfgs.len(), 2);
        assert_eq!(cfgs[0].name, "Debug binary");
        assert_eq!(cfgs[0].adapter, "codelldb");
        assert_eq!(cfgs[0].request, LaunchRequestKind::Launch);
        assert_eq!(cfgs[0].args["stopOnEntry"], json!(true));
        assert_eq!(cfgs[1].request, LaunchRequestKind::Attach);
        assert_eq!(cfgs[1].args["pid"], json!(1234));
    }

    #[tokio::test]
    async fn dap_mock_adapter_requests_events_and_reverse_request() {
        let (client_io, adapter_io) = tokio::io::duplex(16 * 1024);
        let (client_r, client_w) = tokio::io::split(client_io);
        let (adapter_r, mut adapter_w) = tokio::io::split(adapter_io);

        let client = DapClient::from_io(
            tokio::runtime::Handle::current(),
            "mock-dap",
            client_r,
            client_w,
        );
        let mut events = client.events();

        let adapter_task = tokio::spawn(async move {
            let mut reader = BufReader::new(adapter_r);
            let mut seq = 1i64;
            let mut saw_run_in_terminal_err = false;

            while let Some(raw) = read_message(&mut reader).await.unwrap() {
                let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
                match v["type"].as_str().unwrap() {
                    "request" => {
                        let req_seq = v["seq"].as_i64().unwrap();
                        let cmd = v["command"].as_str().unwrap();
                        match cmd {
                            "initialize" => {
                                let resp = json!({
                                    "seq": seq,
                                    "type": "response",
                                    "request_seq": req_seq,
                                    "success": true,
                                    "command": "initialize",
                                    "body": {
                                        "supportsConfigurationDoneRequest": true,
                                        "supportsConditionalBreakpoints": true
                                    }
                                });
                                seq += 1;
                                write_message(&mut adapter_w, &resp.to_string()).await.unwrap();

                                // Emit `initialized` event + a reverse `runInTerminal` request.
                                let ev = json!({
                                    "seq": seq,
                                    "type": "event",
                                    "event": "initialized"
                                });
                                seq += 1;
                                write_message(&mut adapter_w, &ev.to_string()).await.unwrap();

                                let rev = json!({
                                    "seq": seq,
                                    "type": "request",
                                    "command": "runInTerminal",
                                    "arguments": { "cwd": "/tmp", "args": ["./app"] }
                                });
                                seq += 1;
                                write_message(&mut adapter_w, &rev.to_string()).await.unwrap();
                            }
                            "setBreakpoints" => {
                                let resp = json!({
                                    "seq": seq,
                                    "type": "response",
                                    "request_seq": req_seq,
                                    "success": true,
                                    "command": "setBreakpoints",
                                    "body": {
                                        "breakpoints": [{
                                            "id": 1,
                                            "verified": true,
                                            "line": 42
                                        }]
                                    }
                                });
                                seq += 1;
                                write_message(&mut adapter_w, &resp.to_string()).await.unwrap();
                            }
                            "threads" => {
                                let resp = json!({
                                    "seq": seq,
                                    "type": "response",
                                    "request_seq": req_seq,
                                    "success": true,
                                    "command": "threads",
                                    "body": {
                                        "threads": [{ "id": 1, "name": "main" }]
                                    }
                                });
                                seq += 1;
                                write_message(&mut adapter_w, &resp.to_string()).await.unwrap();

                                // Emit stopped event
                                let stopped = json!({
                                    "seq": seq,
                                    "type": "event",
                                    "event": "stopped",
                                    "body": {
                                        "reason": "breakpoint",
                                        "threadId": 1,
                                        "allThreadsStopped": true
                                    }
                                });
                                seq += 1;
                                write_message(&mut adapter_w, &stopped.to_string()).await.unwrap();
                            }
                            "disconnect" => {
                                let resp = json!({
                                    "seq": seq,
                                    "type": "response",
                                    "request_seq": req_seq,
                                    "success": true,
                                    "command": "disconnect"
                                });
                                write_message(&mut adapter_w, &resp.to_string()).await.unwrap();
                                break;
                            }
                            _ => {}
                        }
                    }
                    "response" => {
                        if v["command"] == "runInTerminal" {
                            assert_eq!(v["success"], json!(false));
                            saw_run_in_terminal_err = true;
                        }
                    }
                    _ => {}
                }
            }
            assert!(
                saw_run_in_terminal_err,
                "adapter did not receive unsupported reply for runInTerminal"
            );
        });

        let caps = client.initialize().await.unwrap();
        assert!(caps.supports_configuration_done_request);
        assert!(caps.supports_conditional_breakpoints);

        let bps = client
            .set_breakpoints(
                Path::new("/tmp/main.rs"),
                vec![SourceBreakpoint {
                    line: 42,
                    column: None,
                    condition: None,
                    hit_condition: None,
                    log_message: None,
                }],
            )
            .await
            .unwrap();
        assert_eq!(bps.len(), 1);
        assert!(bps[0].verified);
        assert_eq!(bps[0].line, Some(42));

        let threads = client.threads().await.unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].name, "main");

        // Verify `Initialized` and `Stopped` events arrived.
        let mut saw_init = false;
        let mut saw_stopped = false;
        while let Some(ev) = events.next().await {
            match ev {
                DapEvent::Initialized => saw_init = true,
                DapEvent::Stopped {
                    reason,
                    thread_id,
                    all_threads_stopped,
                    ..
                } => {
                    assert_eq!(reason, "breakpoint");
                    assert_eq!(thread_id, Some(1));
                    assert!(all_threads_stopped);
                    saw_stopped = true;
                }
                _ => {}
            }
            if saw_init && saw_stopped {
                break;
            }
        }
        assert!(saw_init && saw_stopped);

        client.disconnect(true).await.unwrap();
        adapter_task.await.unwrap();
    }
}
