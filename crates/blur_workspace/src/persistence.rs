//! Workspace state persistence, named snapshots, and recent workspaces list.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::warn;

const MAX_RECENT_WORKSPACES: usize = 20;

/// Persisted cursor/scroll state of an open editor file.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct OpenFile {
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
    pub scroll_line: usize,
}

/// Persisted UI layout state for a workspace.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct LayoutState {
    pub sidebar_visible: bool,
    pub sidebar_width: f32,
    pub panel_visible: bool,
    pub panel_height: f32,
    pub active_view: String,
    pub editor_groups: Vec<Vec<PathBuf>>,
}

impl Default for LayoutState {
    fn default() -> Self {
        Self {
            sidebar_visible: true,
            sidebar_width: 260.0,
            panel_visible: false,
            panel_height: 220.0,
            active_view: "editor".to_string(),
            editor_groups: Vec::new(),
        }
    }
}

/// Persisted state of a Blur workspace.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct WorkspaceState {
    pub roots: Vec<PathBuf>,
    pub open_files: Vec<OpenFile>,
    pub active_file: Option<PathBuf>,
    pub layout: LayoutState,
    pub profile: Option<String>,
}

/// Computes a stable 64-bit FNV-1a hash (independent of Rust std's `DefaultHasher`).
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    let mut hash = OFFSET_BASIS;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

fn base_data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("BLUR_DATA_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("blur")
}

fn workspace_hash(root: &Path) -> String {
    let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let key = canonical.to_string_lossy();
    format!("{:016x}", fnv1a_64(key.as_bytes()))
}

fn workspace_file_path(root: &Path) -> PathBuf {
    base_data_dir()
        .join("workspaces")
        .join(format!("{}.json", workspace_hash(root)))
}

fn snapshots_dir(root: &Path) -> PathBuf {
    base_data_dir()
        .join("snapshots")
        .join(workspace_hash(root))
}

fn sanitize_snapshot_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn recents_file_path() -> PathBuf {
    base_data_dir().join("recent_workspaces.json")
}

/// Loads the persisted [`WorkspaceState`] for `root`, if present and valid.
pub fn load(root: &Path) -> Option<WorkspaceState> {
    let path = workspace_file_path(root);
    let raw = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str(&raw) {
        Ok(state) => Some(state),
        Err(err) => {
            warn!("failed to parse workspace state {}: {err}", path.display());
            None
        }
    }
}

/// Saves `state` for `root` as JSON under `<data_dir>/workspaces/<hash>.json`.
pub fn save(root: &Path, state: &WorkspaceState) -> Result<()> {
    let path = workspace_file_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("failed to create workspaces directory {}", parent.display())
        })?;
    }
    let json = serde_json::to_string_pretty(state)
        .context("failed to serialize workspace state")?;
    std::fs::write(&path, json)
        .with_context(|| format!("failed to write workspace state {}", path.display()))?;
    Ok(())
}

/// Saves a named snapshot of `state` for `root`.
pub fn snapshot(root: &Path, name: &str, state: &WorkspaceState) -> Result<()> {
    let dir = snapshots_dir(root);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create snapshots directory {}", dir.display()))?;
    let safe_name = sanitize_snapshot_name(name);
    if safe_name.is_empty() {
        anyhow::bail!("snapshot name cannot be empty");
    }
    let path = dir.join(format!("{safe_name}.json"));
    let json = serde_json::to_string_pretty(state)
        .context("failed to serialize workspace snapshot")?;
    std::fs::write(&path, json)
        .with_context(|| format!("failed to write snapshot {}", path.display()))?;
    Ok(())
}

/// Lists saved snapshot names for `root`, sorted lexicographically.
pub fn list_snapshots(root: &Path) -> Vec<String> {
    let dir = snapshots_dir(root);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                names.push(stem.to_string());
            }
        }
    }
    names.sort();
    names
}

/// Loads a named snapshot for `root`.
pub fn load_snapshot(root: &Path, name: &str) -> Option<WorkspaceState> {
    let safe_name = sanitize_snapshot_name(name);
    let path = snapshots_dir(root).join(format!("{safe_name}.json"));
    let raw = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Returns the list of recently opened workspace roots (most recent first, up to 20).
pub fn recent_workspaces() -> Vec<PathBuf> {
    let path = recents_file_path();
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Pushes `root` to the front of the recent workspaces list (deduped, capped at 20).
pub fn push_recent(root: &Path) {
    let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut recents = recent_workspaces();
    recents.retain(|p| p != &canonical);
    recents.insert(0, canonical);
    recents.truncate(MAX_RECENT_WORKSPACES);

    let path = recents_file_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(&recents) {
        let _ = std::fs::write(&path, json);
    }
}
