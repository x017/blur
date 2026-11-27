//! Typed Debug Adapter Protocol (DAP) structs, adapter configuration, and `.blur/launch.toml` loader.

use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Configuration describing how to spawn a debug adapter process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugAdapterConfig {
    /// Adapter identifier (e.g. `"codelldb"`, `"delve"`, `"debugpy"`).
    pub name: String,
    /// Executable name or path.
    pub command: String,
    /// Command-line arguments passed to the adapter executable.
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment variables passed to the adapter process.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Languages supported by this adapter.
    #[serde(default)]
    pub languages: Vec<String>,
}

/// Whether a [`LaunchConfig`] launches a new target or attaches to an existing process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LaunchRequestKind {
    Launch,
    Attach,
}

/// A launch/attach configuration entry loaded from `.blur/launch.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchConfig {
    pub name: String,
    pub adapter: String,
    pub request: LaunchRequestKind,
    #[serde(default = "default_empty_object")]
    pub args: Value,
}

fn default_empty_object() -> Value {
    Value::Object(Default::default())
}

#[derive(Debug, Deserialize)]
struct LaunchFile {
    #[serde(default, alias = "launch")]
    configuration: Vec<LaunchConfig>,
    #[serde(default)]
    configurations: Vec<LaunchConfig>,
}

/// Load launch configurations from `<workspace_root>/.blur/launch.toml`.
///
/// Supports `[[configuration]]`, `[[launch]]`, or `configurations = [...]`.
/// Returns an empty `Vec` if `.blur/launch.toml` does not exist.
pub fn load_launch_configs(workspace_root: &Path) -> Result<Vec<LaunchConfig>> {
    let path = workspace_root.join(".blur").join("launch.toml");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    parse_launch_toml(&content)
}

/// Parse launch configurations directly from a TOML string.
pub fn parse_launch_toml(toml_str: &str) -> Result<Vec<LaunchConfig>> {
    let file: LaunchFile = toml::from_str(toml_str).context("failed to parse launch.toml")?;
    let mut configs = file.configuration;
    configs.extend(file.configurations);
    Ok(configs)
}

/// Direction of a captured DAP wire frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireDirection {
    In,
    Out,
}

/// Captured DAP wire frame for debugging / protocol inspection.
#[derive(Debug, Clone)]
pub struct WireFrame {
    pub direction: WireDirection,
    pub kind: Option<String>,
    pub seq: Option<i64>,
    pub json: String,
    pub at: SystemTime,
}

/// DAP server capabilities returned from `initialize`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Capabilities {
    pub supports_configuration_done_request: bool,
    pub supports_function_breakpoints: bool,
    pub supports_conditional_breakpoints: bool,
    pub supports_hit_conditional_breakpoints: bool,
    pub supports_evaluate_for_hovers: bool,
    pub supports_step_back: bool,
    pub supports_set_variable: bool,
    pub supports_restart_frame: bool,
    pub supports_terminate_request: bool,
    pub supports_log_points: bool,
    pub supports_completions_request: bool,
    pub supports_modules_request: bool,
    pub supports_exception_info_request: bool,
    pub supports_value_formatting_options: bool,
}

/// DAP `Source` descriptor.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Source {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_reference: Option<i64>,
}

/// Breakpoint specification passed to `setBreakpoints`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourceBreakpoint {
    pub line: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_message: Option<String>,
}

/// Function breakpoint specification passed to `setFunctionBreakpoints`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FunctionBreakpoint {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
}

/// Breakpoint state returned by the adapter.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Breakpoint {
    pub id: Option<i64>,
    pub verified: bool,
    pub message: Option<String>,
    pub source: Option<Source>,
    pub line: Option<u64>,
    pub column: Option<u64>,
    pub end_line: Option<u64>,
    pub end_column: Option<u64>,
}

/// Thread descriptor returned by `threads`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: i64,
    pub name: String,
}

/// Stack frame returned by `stackTrace`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StackFrame {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub source: Option<Source>,
    pub line: u64,
    pub column: u64,
    #[serde(default)]
    pub end_line: Option<u64>,
    #[serde(default)]
    pub end_column: Option<u64>,
    #[serde(default)]
    pub module_id: Option<Value>,
}

/// Variable scope returned by `scopes`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    pub name: String,
    #[serde(default)]
    pub presentation_hint: Option<String>,
    pub variables_reference: i64,
    #[serde(default)]
    pub named_variables: Option<u64>,
    #[serde(default)]
    pub indexed_variables: Option<u64>,
    #[serde(default)]
    pub expensive: bool,
}

/// Variable returned by `variables`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Variable {
    pub name: String,
    pub value: String,
    #[serde(rename = "type", default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub variables_reference: i64,
    #[serde(default)]
    pub named_variables: Option<u64>,
    #[serde(default)]
    pub indexed_variables: Option<u64>,
    #[serde(default)]
    pub evaluate_name: Option<String>,
}

/// Result of an `evaluate` request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateResponse {
    pub result: String,
    #[serde(rename = "type", default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub variables_reference: i64,
}

/// Events emitted by a debug adapter.
#[derive(Debug, Clone)]
pub enum DapEvent {
    Initialized,
    Stopped {
        reason: String,
        description: Option<String>,
        thread_id: Option<i64>,
        all_threads_stopped: bool,
        text: Option<String>,
    },
    Continued {
        thread_id: i64,
        all_threads_continued: bool,
    },
    Exited(i64),
    Terminated {
        restart: Option<Value>,
    },
    Output {
        category: Option<String>,
        output: String,
    },
    Breakpoint {
        reason: String,
        breakpoint: Breakpoint,
    },
    Thread {
        reason: String,
        thread_id: i64,
    },
    Raw(Value),
}

// --- Internal wire envelope types ---

#[derive(Debug, Serialize)]
pub(crate) struct WireRequest<'a> {
    pub seq: i64,
    #[serde(rename = "type")]
    pub msg_type: &'static str,
    pub command: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WireResponse<'a> {
    pub seq: i64,
    #[serde(rename = "type")]
    pub msg_type: &'static str,
    pub request_seq: i64,
    pub success: bool,
    pub command: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct IncomingEnvelope {
    #[serde(default)]
    pub seq: i64,
    #[serde(rename = "type")]
    pub msg_type: String,
    // Response fields
    #[serde(default)]
    pub request_seq: Option<i64>,
    #[serde(default)]
    pub success: Option<bool>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub body: Option<Value>,
    // Event fields
    #[serde(default)]
    pub event: Option<String>,
}
