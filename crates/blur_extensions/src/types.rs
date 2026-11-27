//! Public host-communication and UI-model types for `blur_extensions`.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::manifest::ExtensionManifest;

/// Notification severity level pushed by an extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotifyLevel {
    Info,
    Warning,
    Error,
}

/// Metadata for a command registered by an extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRegistration {
    pub extension_id: String,
    pub id: String,
    pub title: String,
    pub category: String,
}

/// Metadata for a panel registered by an extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelRegistration {
    pub extension_id: String,
    pub id: String,
    pub title: String,
}

/// A single rendered item inside an extension panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelItem {
    pub label: String,
    pub detail: Option<String>,
    pub command: Option<String>,
}

/// Rendered contents returned by `ExtensionHost::render_panel`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelContent {
    pub title: String,
    pub items: Vec<PanelItem>,
}

/// Theme definition registered by an extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeRegistration {
    pub extension_id: String,
    pub name: String,
    pub base: String,
    pub colors: HashMap<String, String>,
}

/// Requests pushed by Lua extensions for the host application to drain and apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostRequest {
    Notify {
        extension_id: String,
        message: String,
        level: NotifyLevel,
    },
    Log {
        extension_id: String,
        message: String,
    },
    InsertText {
        extension_id: String,
        text: String,
    },
    OpenFile {
        extension_id: String,
        path: PathBuf,
    },
    RegisterCommand(CommandRegistration),
    RegisterPanel(PanelRegistration),
    RegisterTheme(ThemeRegistration),
    Bind {
        extension_id: String,
        keys: String,
        command_id: String,
    },
}

/// Current editor/workspace snapshot provided by the host application.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorContext {
    pub workspace_root: Option<PathBuf>,
    pub active_path: Option<PathBuf>,
    pub active_text: Option<String>,
    pub selection: Option<String>,
}

/// Runtime status summary for a discovered extension.
#[derive(Debug, Clone)]
pub struct ExtensionInfo {
    pub manifest: ExtensionManifest,
    pub dir: PathBuf,
    pub enabled: bool,
    pub loaded: bool,
    pub last_error: Option<String>,
}
