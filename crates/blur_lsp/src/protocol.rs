//! JSON-RPC 2.0 message helpers, LSP 3.17 client capabilities, symbol flattening,
//! and UTF-16 incremental document sync helpers.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use lsp_types::{
    ClientCapabilities, CodeActionClientCapabilities, CodeActionKind,
    CodeActionKindLiteralSupport, CodeActionLiteralSupport, CompletionClientCapabilities,
    CompletionItemCapability, DocumentFormattingClientCapabilities,
    DocumentSymbolClientCapabilities, DocumentSymbolResponse, GeneralClientCapabilities,
    GotoCapability, HoverClientCapabilities, MarkupKind, Position, PositionEncodingKind, Range,
    ReferenceClientCapabilities, RenameClientCapabilities, ServerCapabilities, SymbolKind,
    TextDocumentClientCapabilities, TextDocumentContentChangeEvent, TextDocumentSyncCapability,
    TextDocumentSyncClientCapabilities, TextDocumentSyncKind, Uri, WindowClientCapabilities,
    WorkspaceClientCapabilities, WorkspaceEditClientCapabilities,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

/// Flattened document symbol entry produced from either hierarchical `DocumentSymbol`
/// or flat `SymbolInformation` responses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSymbolItem {
    pub name: String,
    pub detail: Option<String>,
    pub kind: SymbolKind,
    pub range: Range,
    pub selection_range: Range,
    pub container_name: Option<String>,
    pub depth: usize,
}

/// Flatten an LSP `DocumentSymbolResponse` into a pre-order list of [`DocumentSymbolItem`].
pub fn flatten_document_symbols(resp: DocumentSymbolResponse) -> Vec<DocumentSymbolItem> {
    let mut out = Vec::new();
    match resp {
        DocumentSymbolResponse::Flat(items) => {
            for item in items {
                out.push(DocumentSymbolItem {
                    name: item.name,
                    detail: None,
                    kind: item.kind,
                    range: item.location.range,
                    selection_range: item.location.range,
                    container_name: item.container_name,
                    depth: 0,
                });
            }
        }
        DocumentSymbolResponse::Nested(items) => {
            fn visit(
                sym: lsp_types::DocumentSymbol,
                parent: Option<&str>,
                depth: usize,
                out: &mut Vec<DocumentSymbolItem>,
            ) {
                let name = sym.name;
                out.push(DocumentSymbolItem {
                    name: name.clone(),
                    detail: sym.detail,
                    kind: sym.kind,
                    range: sym.range,
                    selection_range: sym.selection_range,
                    container_name: parent.map(|s| s.to_string()),
                    depth,
                });
                if let Some(children) = sym.children {
                    for child in children {
                        visit(child, Some(&name), depth + 1, out);
                    }
                }
            }
            for item in items {
                visit(item, None, 0, &mut out);
            }
        }
    }
    out
}

/// Convert a [`url::Url`] into an [`lsp_types::Uri`].
pub fn url_to_lsp_uri(url: &Url) -> Result<Uri> {
    url.as_str()
        .parse::<Uri>()
        .map_err(|e| anyhow!("invalid LSP URI {:?}: {}", url.as_str(), e))
}

/// Convert an [`lsp_types::Uri`] into a [`url::Url`].
pub fn lsp_uri_to_url(uri: &Uri) -> Result<Url> {
    Url::parse(uri.as_str()).map_err(|e| anyhow!("invalid URL {:?}: {}", uri.as_str(), e))
}

/// Convert a local filesystem path into a `file://` [`Url`].
pub fn path_to_url(path: &Path) -> Result<Url> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Url::from_file_path(&abs).map_err(|_| anyhow!("failed to convert path {:?} to file URL", abs))
}

/// Convert a `file://` [`Url`] back into a local [`PathBuf`].
pub fn url_to_path(url: &Url) -> Option<PathBuf> {
    url.to_file_path().ok()
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Parsed incoming JSON-RPC 2.0 message.
#[derive(Debug, Clone)]
pub enum IncomingMessage {
    Response {
        id: i64,
        result: Option<Value>,
        error: Option<JsonRpcError>,
    },
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
}

/// Parse a raw JSON-RPC 2.0 object into an [`IncomingMessage`].
pub fn parse_incoming(raw: &str) -> Result<IncomingMessage> {
    let val: Value = serde_json::from_str(raw)?;
    let obj = val
        .as_object()
        .ok_or_else(|| anyhow!("JSON-RPC message must be an object"))?;

    let has_method = obj.get("method").and_then(|v| v.as_str()).is_some();
    let has_id = obj.contains_key("id") && !obj.get("id").unwrap().is_null();

    if has_method && has_id {
        let id = obj.get("id").cloned().unwrap_or(Value::Null);
        let method = obj["method"].as_str().unwrap().to_string();
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        Ok(IncomingMessage::Request { id, method, params })
    } else if has_method {
        let method = obj["method"].as_str().unwrap().to_string();
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        Ok(IncomingMessage::Notification { method, params })
    } else if obj.contains_key("id") {
        let id = obj
            .get("id")
            .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .ok_or_else(|| anyhow!("JSON-RPC response missing integer id"))?;
        let error = match obj.get("error") {
            Some(e) if !e.is_null() => Some(serde_json::from_value::<JsonRpcError>(e.clone())?),
            _ => None,
        };
        let result = obj.get("result").cloned();
        Ok(IncomingMessage::Response { id, result, error })
    } else {
        Err(anyhow!("unrecognized JSON-RPC message: {}", raw))
    }
}

/// Serialize a JSON-RPC 2.0 request.
pub fn serialize_request(id: i64, method: &str, params: Value) -> Result<String> {
    let msg = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    Ok(serde_json::to_string(&msg)?)
}

/// Serialize a JSON-RPC 2.0 notification.
pub fn serialize_notification(method: &str, params: Value) -> Result<String> {
    let msg = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    });
    Ok(serde_json::to_string(&msg)?)
}

/// Serialize a JSON-RPC 2.0 success response (for server -> client requests).
pub fn serialize_response_ok(id: Value, result: Value) -> Result<String> {
    let msg = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    });
    Ok(serde_json::to_string(&msg)?)
}

/// Serialize a JSON-RPC 2.0 error response (for server -> client requests).
pub fn serialize_response_err(id: Value, code: i64, message: &str) -> Result<String> {
    let msg = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
        }
    });
    Ok(serde_json::to_string(&msg)?)
}

/// Build Blur's LSP 3.17 [`ClientCapabilities`].
pub fn build_client_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        general: Some(GeneralClientCapabilities {
            position_encodings: Some(vec![PositionEncodingKind::UTF16]),
            ..Default::default()
        }),
        workspace: Some(WorkspaceClientCapabilities {
            apply_edit: Some(true),
            workspace_edit: Some(WorkspaceEditClientCapabilities {
                document_changes: Some(true),
                ..Default::default()
            }),
            configuration: Some(true),
            did_change_configuration: Some(lsp_types::DynamicRegistrationClientCapabilities {
                dynamic_registration: Some(true),
            }),
            did_change_watched_files: Some(lsp_types::DidChangeWatchedFilesClientCapabilities {
                dynamic_registration: Some(true),
                relative_pattern_support: Some(true),
            }),
            workspace_folders: Some(true),
            ..Default::default()
        }),
        window: Some(WindowClientCapabilities {
            work_done_progress: Some(true),
            show_message: Some(lsp_types::ShowMessageRequestClientCapabilities::default()),
            ..Default::default()
        }),
        text_document: Some(TextDocumentClientCapabilities {
            synchronization: Some(TextDocumentSyncClientCapabilities {
                dynamic_registration: Some(true),
                will_save: Some(false),
                will_save_wait_until: Some(false),
                did_save: Some(true),
            }),
            completion: Some(CompletionClientCapabilities {
                dynamic_registration: Some(true),
                completion_item: Some(CompletionItemCapability {
                    snippet_support: Some(true),
                    commit_characters_support: Some(true),
                    documentation_format: Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]),
                    deprecated_support: Some(true),
                    preselect_support: Some(true),
                    resolve_support: Some(lsp_types::CompletionItemCapabilityResolveSupport {
                        properties: vec![
                            "documentation".to_string(),
                            "detail".to_string(),
                            "additionalTextEdits".to_string(),
                        ],
                    }),
                    ..Default::default()
                }),
                context_support: Some(true),
                ..Default::default()
            }),
            hover: Some(HoverClientCapabilities {
                dynamic_registration: Some(true),
                content_format: Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]),
            }),
            definition: Some(GotoCapability {
                dynamic_registration: Some(true),
                link_support: Some(true),
            }),
            references: Some(ReferenceClientCapabilities {
                dynamic_registration: Some(true),
            }),
            rename: Some(RenameClientCapabilities {
                dynamic_registration: Some(true),
                prepare_support: Some(true),
                ..Default::default()
            }),
            code_action: Some(CodeActionClientCapabilities {
                dynamic_registration: Some(true),
                code_action_literal_support: Some(CodeActionLiteralSupport {
                    code_action_kind: CodeActionKindLiteralSupport {
                        value_set: vec![
                            CodeActionKind::QUICKFIX.as_str().to_string(),
                            CodeActionKind::REFACTOR.as_str().to_string(),
                            CodeActionKind::REFACTOR_EXTRACT.as_str().to_string(),
                            CodeActionKind::REFACTOR_INLINE.as_str().to_string(),
                            CodeActionKind::REFACTOR_REWRITE.as_str().to_string(),
                            CodeActionKind::SOURCE.as_str().to_string(),
                            CodeActionKind::SOURCE_ORGANIZE_IMPORTS.as_str().to_string(),
                        ],
                    },
                }),
                data_support: Some(true),
                resolve_support: Some(lsp_types::CodeActionCapabilityResolveSupport {
                    properties: vec!["edit".to_string()],
                }),
                ..Default::default()
            }),
            formatting: Some(DocumentFormattingClientCapabilities {
                dynamic_registration: Some(true),
            }),
            document_symbol: Some(DocumentSymbolClientCapabilities {
                dynamic_registration: Some(true),
                hierarchical_document_symbol_support: Some(true),
                ..Default::default()
            }),
            publish_diagnostics: Some(lsp_types::PublishDiagnosticsClientCapabilities {
                related_information: Some(true),
                version_support: Some(true),
                code_description_support: Some(true),
                data_support: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Extract the negotiated [`TextDocumentSyncKind`] from [`ServerCapabilities`].
pub fn negotiated_sync_kind(caps: Option<&ServerCapabilities>) -> TextDocumentSyncKind {
    let Some(caps) = caps else {
        return TextDocumentSyncKind::INCREMENTAL;
    };
    match &caps.text_document_sync {
        Some(TextDocumentSyncCapability::Kind(kind)) => *kind,
        Some(TextDocumentSyncCapability::Options(opts)) => {
            opts.change.unwrap_or(TextDocumentSyncKind::NONE)
        }
        None => TextDocumentSyncKind::NONE,
    }
}

/// Resolve a `workspace/configuration` section against a JSON settings object.
pub fn resolve_config_section(settings: Option<&Value>, section: Option<&str>) -> Value {
    let Some(root) = settings else {
        return Value::Null;
    };
    let Some(section) = section else {
        return root.clone();
    };
    if section.is_empty() {
        return root.clone();
    }
    let mut current = root;
    for part in section.split('.') {
        match current.get(part) {
            Some(next) => current = next,
            None => return Value::Null,
        }
    }
    current.clone()
}

/// Apply LSP content changes (full or UTF-16 incremental) to an in-memory document string.
pub fn apply_content_changes(text: &mut String, changes: &[TextDocumentContentChangeEvent]) {
    for change in changes {
        match change.range {
            None => {
                text.clear();
                text.push_str(&change.text);
            }
            Some(range) => {
                let start = utf16_position_to_byte_offset(text, range.start);
                let end = utf16_position_to_byte_offset(text, range.end);
                if start <= end && end <= text.len() {
                    text.replace_range(start..end, &change.text);
                }
            }
        }
    }
}

/// Convert an LSP UTF-16 [`Position`] into a UTF-8 byte offset in `text` (clamped).
pub fn utf16_position_to_byte_offset(text: &str, pos: Position) -> usize {
    let target_line = pos.line as usize;
    let target_utf16 = pos.character as usize;

    let mut current_line = 0usize;
    let mut line_start_byte = 0usize;

    if target_line > 0 {
        for (idx, b) in text.bytes().enumerate() {
            if b == b'\n' {
                current_line += 1;
                if current_line == target_line {
                    line_start_byte = idx + 1;
                    break;
                }
            }
        }
        if current_line < target_line {
            return text.len();
        }
    }

    let rest = &text[line_start_byte..];
    let mut utf16_col = 0usize;
    for (rel_byte, ch) in rest.char_indices() {
        if ch == '\n' || ch == '\r' {
            return line_start_byte + rel_byte;
        }
        if utf16_col >= target_utf16 {
            return line_start_byte + rel_byte;
        }
        utf16_col += ch.len_utf16();
    }
    text.len()
}
