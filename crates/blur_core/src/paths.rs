//! Standard filesystem paths for Blur configuration, data, themes, extensions, and keymaps.

use std::path::{Path, PathBuf};

/// Returns the Blur configuration directory (`~/.config/blur` on Linux).
///
/// Can be overridden via the `BLUR_CONFIG_DIR` environment variable.
pub fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("BLUR_CONFIG_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("blur")
}

/// Returns the Blur data directory (`~/.local/share/blur` on Linux).
///
/// Can be overridden via the `BLUR_DATA_DIR` environment variable.
pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("BLUR_DATA_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("blur")
}

/// Returns the user configuration file path (`<config_dir>/config.toml`).
pub fn user_config_file() -> PathBuf {
    config_dir().join("config.toml")
}

/// Returns the workspace configuration file path (`<root>/.blur/config.toml`).
pub fn workspace_config_file(root: impl AsRef<Path>) -> PathBuf {
    root.as_ref().join(".blur").join("config.toml")
}

/// Returns the user themes directory (`<config_dir>/themes`).
pub fn themes_dir() -> PathBuf {
    config_dir().join("themes")
}

/// Returns the user extensions directory (`<config_dir>/extensions`).
pub fn extensions_dir() -> PathBuf {
    config_dir().join("extensions")
}

/// Returns the user keymap file path (`<config_dir>/keymap.toml`).
pub fn keymap_file() -> PathBuf {
    config_dir().join("keymap.toml")
}
