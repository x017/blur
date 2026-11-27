//! Generic Language Server Protocol (LSP 3.17) client for Blur.
//!
//! Pure Rust, GPUI-free, and server-agnostic. All I/O runs on tokio tasks
//! spawned on a caller-supplied [`tokio::runtime::Handle`]; every public `async`
//! request method awaits only a [`futures::channel::oneshot::Receiver`] so it
//! can be driven from any executor.

pub mod client;
pub mod manager;
pub mod presets;
pub mod protocol;
pub mod transport;

pub use client::{LspClient, LspEvent, ServerInfo, ServerStatus, WireDirection, WireFrame};
pub use lsp_types::{
    self, CodeActionOrCommand, CompletionItem, Diagnostic, Hover, Location, MessageType, Position,
    Range, ServerCapabilities, SymbolKind, TextDocumentContentChangeEvent, TextEdit, Uri,
    WorkspaceEdit,
};
pub use manager::LspManager;
pub use presets::{builtin_server_presets, detect_root, find_executable, ServerConfig};
pub use protocol::{
    apply_content_changes, lsp_uri_to_url, path_to_url, url_to_lsp_uri, url_to_path,
    DocumentSymbolItem,
};
pub use url::Url;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    use futures::StreamExt;
    use tokio::io::{AsyncWriteExt, BufReader};

    use crate::transport::{encode_message, read_message, write_message};

    #[tokio::test]
    async fn framing_encode_decode_partial_and_multi() {
        let msg1 = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
        let msg2 = r#"{"jsonrpc":"2.0","id":2,"method":"pong","params":{"emoji":"🦀"}}"#;
        let mut wire = encode_message(msg1);
        wire.extend_from_slice(encode_message(msg2).as_slice());

        let (mut writer, reader) = tokio::io::duplex(64);
        let write_task = tokio::spawn(async move {
            // Write in tiny 5-byte chunks to exercise partial header & body reads.
            for chunk in wire.chunks(5) {
                writer.write_all(chunk).await.unwrap();
                writer.flush().await.unwrap();
            }
        });

        let mut buf_reader = BufReader::new(reader);
        let got1 = read_message(&mut buf_reader).await.unwrap().unwrap();
        let got2 = read_message(&mut buf_reader).await.unwrap().unwrap();
        let eof = read_message(&mut buf_reader).await.unwrap();
        write_task.await.unwrap();

        assert_eq!(got1, msg1);
        assert_eq!(got2, msg2);
        assert!(eof.is_none());
    }

    #[test]
    fn root_detection_walks_up_to_marker() {
        let base = std::env::temp_dir().join(format!("blur_lsp_root_test_{}", std::process::id()));
        let nested = base.join("a").join("b").join("c");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(base.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let file = nested.join("main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();

        let markers = vec!["Cargo.toml".to_string()];
        assert_eq!(detect_root(&file, &markers), Some(base.clone()));
        assert_eq!(detect_root(&nested, &markers), Some(base.clone()));
        assert_eq!(
            detect_root(&file, &["nonexistent.marker".to_string()]),
            None
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn incremental_sync_helper_handles_utf16() {
        let mut text = "hello 🦀 world\nsecond line".to_string();
        // "hello " = 6 UTF-16 units; "🦀" = 2 UTF-16 units (surrogate pair) -> col 6..8
        apply_content_changes(
            &mut text,
            &[TextDocumentContentChangeEvent {
                range: Some(Range::new(Position::new(0, 6), Position::new(0, 8))),
                range_length: None,
                text: "rust".to_string(),
            }],
        );
        assert_eq!(text, "hello rust world\nsecond line");
    }

    #[tokio::test]
    async fn mock_server_correlation_diagnostics_and_reverse_requests() {
        let (client_io, server_io) = tokio::io::duplex(16 * 1024);
        let (client_r, client_w) = tokio::io::split(client_io);
        let (server_r, mut server_w) = tokio::io::split(server_io);

        let config = ServerConfig {
            name: "mock-lsp".to_string(),
            command: "mock-lsp".to_string(),
            args: vec![],
            env: HashMap::new(),
            languages: vec!["rust".to_string()],
            root_markers: vec![],
            initialization_options: None,
            settings: Some(serde_json::json!({
                "mock": { "checkOnSave": true }
            })),
        };

        let (client, mut events) = LspClient::from_io(
            &tokio::runtime::Handle::current(),
            config,
            PathBuf::from("/tmp"),
            client_r,
            client_w,
        );

        // In-process mock LSP server task.
        let server_task = tokio::spawn(async move {
            let mut reader = BufReader::new(server_r);

            // 1. Expect `initialize`
            let init_raw = read_message(&mut reader).await.unwrap().unwrap();
            let init_val: serde_json::Value = serde_json::from_str(&init_raw).unwrap();
            assert_eq!(init_val["method"], "initialize");
            let init_id = init_val["id"].as_i64().unwrap();

            // Advertise Full sync so we can verify incremental->full conversion too.
            let init_resp = serde_json::json!({
                "jsonrpc": "2.0",
                "id": init_id,
                "result": {
                    "capabilities": {
                        "textDocumentSync": 1,
                        "hoverProvider": true
                    }
                }
            });
            write_message(&mut server_w, &init_resp.to_string())
                .await
                .unwrap();

            // 2. Expect `initialized` notification
            let initialized_raw = read_message(&mut reader).await.unwrap().unwrap();
            let initialized_val: serde_json::Value =
                serde_json::from_str(&initialized_raw).unwrap();
            assert_eq!(initialized_val["method"], "initialized");

            // 3. Send server -> client `workspace/configuration` request
            let cfg_req = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 99,
                "method": "workspace/configuration",
                "params": {
                    "items": [
                        { "section": "mock.checkOnSave" },
                        { "section": "mock.missing" }
                    ]
                }
            });
            write_message(&mut server_w, &cfg_req.to_string())
                .await
                .unwrap();

            // 4. Send server -> client `client/registerCapability`
            let reg_req = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 100,
                "method": "client/registerCapability",
                "params": {
                    "registrations": [{
                        "id": "watch-1",
                        "method": "workspace/didChangeWatchedFiles"
                    }]
                }
            });
            write_message(&mut server_w, &reg_req.to_string())
                .await
                .unwrap();

            // 5. Send `textDocument/publishDiagnostics` notification
            let diag_notif = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": {
                    "uri": "file:///tmp/test.rs",
                    "version": 2,
                    "diagnostics": [{
                        "range": {
                            "start": { "line": 0, "character": 0 },
                            "end": { "line": 0, "character": 5 }
                        },
                        "severity": 1,
                        "message": "unused variable"
                    }]
                }
            });
            write_message(&mut server_w, &diag_notif.to_string())
                .await
                .unwrap();

            // Collect incoming messages from client and answer hover + shutdown.
            let mut saw_cfg_reply = false;
            let mut saw_reg_reply = false;
            let mut saw_full_did_change = false;
            while let Some(raw) = read_message(&mut reader).await.unwrap() {
                let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
                if v.get("id") == Some(&serde_json::json!(99)) {
                    assert_eq!(v["result"], serde_json::json!([true, null]));
                    saw_cfg_reply = true;
                } else if v.get("id") == Some(&serde_json::json!(100)) {
                    assert_eq!(v["result"], serde_json::Value::Null);
                    saw_reg_reply = true;
                } else if v.get("method") == Some(&serde_json::json!("textDocument/didChange")) {
                    // Server advertised Full sync (1), so incremental edit must have been converted to full text!
                    let changes = v["params"]["contentChanges"].as_array().unwrap();
                    assert_eq!(changes.len(), 1);
                    assert!(changes[0].get("range").is_none() || changes[0]["range"].is_null());
                    assert_eq!(changes[0]["text"], "fn bar() {}");
                    saw_full_did_change = true;
                } else if v.get("method") == Some(&serde_json::json!("textDocument/hover")) {
                    let id = v["id"].as_i64().unwrap();
                    let resp = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "contents": {
                                "kind": "markdown",
                                "value": "```rust\nfn bar()\n```"
                            }
                        }
                    });
                    write_message(&mut server_w, &resp.to_string())
                        .await
                        .unwrap();
                } else if v.get("method") == Some(&serde_json::json!("shutdown")) {
                    let id = v["id"].as_i64().unwrap();
                    let resp = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": null
                    });
                    write_message(&mut server_w, &resp.to_string())
                        .await
                        .unwrap();
                } else if v.get("method") == Some(&serde_json::json!("exit")) {
                    break;
                }
            }
            assert!(saw_cfg_reply, "server did not receive workspace/configuration reply");
            assert!(saw_reg_reply, "server did not receive client/registerCapability reply");
            assert!(saw_full_did_change, "server did not receive Full-converted didChange");
        });

        let uri = Url::parse("file:///tmp/test.rs").unwrap();
        client.did_open(uri.clone(), "rust", 1, "fn foo() {}".to_string());
        // Pass an incremental edit; server only supports Full so client must convert it.
        client.did_change(
            uri.clone(),
            2,
            vec![TextDocumentContentChangeEvent {
                range: Some(Range::new(Position::new(0, 3), Position::new(0, 6))),
                range_length: None,
                text: "bar".to_string(),
            }],
        );

        let hover = client
            .hover(uri.clone(), Position::new(0, 4))
            .await
            .unwrap()
            .expect("expected hover result");
        match hover.contents {
            lsp_types::HoverContents::Markup(m) => assert!(m.value.contains("fn bar()")),
            other => panic!("unexpected hover contents: {:?}", other),
        }

        // Wait for diagnostics event.
        let mut saw_diag = false;
        while let Some(ev) = events.next().await {
            if let LspEvent::Diagnostics {
                server,
                uri: d_uri,
                version,
                diagnostics,
            } = ev
            {
                assert_eq!(server, "mock-lsp");
                assert_eq!(d_uri, uri);
                assert_eq!(version, Some(2));
                assert_eq!(diagnostics.len(), 1);
                assert_eq!(diagnostics[0].message, "unused variable");
                saw_diag = true;
                break;
            }
        }
        assert!(saw_diag);
        assert_eq!(client.dynamic_registrations().len(), 1);
        assert!(!client.wire_log().is_empty());

        client.shutdown().await.unwrap();
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn apply_edit_progress_and_crash_detection() {
        let (client_io, server_io) = tokio::io::duplex(16 * 1024);
        let (client_r, client_w) = tokio::io::split(client_io);
        let (server_r, mut server_w) = tokio::io::split(server_io);

        let config = ServerConfig {
            name: "crash-mock".to_string(),
            command: "crash-mock".to_string(),
            args: vec![],
            env: HashMap::new(),
            languages: vec!["rust".to_string()],
            root_markers: vec![],
            initialization_options: None,
            settings: None,
        };

        let (client, mut events) = LspClient::from_io(
            &tokio::runtime::Handle::current(),
            config,
            PathBuf::from("/tmp"),
            client_r,
            client_w,
        );

        let server_task = tokio::spawn(async move {
            let mut reader = BufReader::new(server_r);
            let init_raw = read_message(&mut reader).await.unwrap().unwrap();
            let init_val: serde_json::Value = serde_json::from_str(&init_raw).unwrap();
            let init_id = init_val["id"].as_i64().unwrap();
            let init_resp = serde_json::json!({
                "jsonrpc": "2.0",
                "id": init_id,
                "result": { "capabilities": {} }
            });
            write_message(&mut server_w, &init_resp.to_string()).await.unwrap();
            let _ = read_message(&mut reader).await.unwrap().unwrap(); // initialized

            // Emit $/progress begin
            let prog = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "$/progress",
                "params": {
                    "token": "idx-1",
                    "value": { "kind": "begin", "title": "Indexing", "percentage": 25 }
                }
            });
            write_message(&mut server_w, &prog.to_string()).await.unwrap();

            // Send workspace/applyEdit and assert reply is {applied: false}
            let apply_req = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 77,
                "method": "workspace/applyEdit",
                "params": { "label": "Quick fix", "edit": {} }
            });
            write_message(&mut server_w, &apply_req.to_string()).await.unwrap();

            let reply_raw = read_message(&mut reader).await.unwrap().unwrap();
            let reply_val: serde_json::Value = serde_json::from_str(&reply_raw).unwrap();
            assert_eq!(reply_val["id"], 77);
            assert_eq!(reply_val["result"]["applied"], false);

            // Drop server stream abruptly to trigger ServerStatus::Crashed.
        });

        let mut saw_progress = false;
        let mut saw_apply_edit = false;
        let mut saw_crashed = false;
        while let Some(ev) = events.next().await {
            match ev {
                LspEvent::Progress {
                    token,
                    title,
                    percentage,
                    ..
                } => {
                    assert_eq!(token, "idx-1");
                    assert_eq!(title.as_deref(), Some("Indexing"));
                    assert_eq!(percentage, Some(25));
                    saw_progress = true;
                }
                LspEvent::ApplyEdit { label, .. } => {
                    assert_eq!(label.as_deref(), Some("Quick fix"));
                    saw_apply_edit = true;
                }
                LspEvent::Status {
                    status: ServerStatus::Crashed { .. },
                    ..
                } => {
                    saw_crashed = true;
                    break;
                }
                _ => {}
            }
        }
        server_task.await.unwrap();
        assert!(saw_progress);
        assert!(saw_apply_edit);
        assert!(saw_crashed);
        assert!(matches!(client.status(), ServerStatus::Crashed { .. }));
    }
}
