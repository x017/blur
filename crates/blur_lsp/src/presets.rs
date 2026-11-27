//! Built-in language server presets, PATH executable lookup, and workspace root detection.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Configuration describing how to launch and configure a language server.
///
/// Serializable so configurations can be loaded directly from TOML or JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Human-readable unique server name (e.g. `"rust-analyzer"`).
    pub name: String,
    /// Executable name (looked up on `PATH`) or absolute/relative path.
    pub command: String,
    /// Command-line arguments passed to the server executable.
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment variables injected into the server process.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// LSP language IDs handled by this server (e.g. `["rust"]`).
    #[serde(default)]
    pub languages: Vec<String>,
    /// Filenames or directory names used to locate a workspace root by walking up from a file.
    #[serde(default)]
    pub root_markers: Vec<String>,
    /// Sent as `InitializeParams.initialization_options`.
    #[serde(default)]
    pub initialization_options: Option<serde_json::Value>,
    /// Used to answer `workspace/configuration` requests from the server.
    #[serde(default)]
    pub settings: Option<serde_json::Value>,
}

/// Example built-in language server presets.
///
/// These are convenience presets only; [`crate::LspManager`] never hardcodes
/// any server and only starts a preset if its executable is found on `PATH`.
pub fn builtin_server_presets() -> Vec<ServerConfig> {
    vec![
        preset(
            "rust-analyzer",
            "rust-analyzer",
            &[],
            &["rust"],
            &["Cargo.toml", "rust-project.json"],
        ),
        preset(
            "gopls",
            "gopls",
            &[],
            &["go", "gomod", "gowork", "gotmpl"],
            &["go.work", "go.mod"],
        ),
        preset(
            "basedpyright",
            "basedpyright-langserver",
            &["--stdio"],
            &["python"],
            &[
                "pyproject.toml",
                "pyrightconfig.json",
                "setup.py",
                "setup.cfg",
                "requirements.txt",
            ],
        ),
        preset(
            "pyright",
            "pyright-langserver",
            &["--stdio"],
            &["python"],
            &[
                "pyproject.toml",
                "pyrightconfig.json",
                "setup.py",
                "setup.cfg",
                "requirements.txt",
            ],
        ),
        preset(
            "typescript-language-server",
            "typescript-language-server",
            &["--stdio"],
            &[
                "typescript",
                "typescriptreact",
                "javascript",
                "javascriptreact",
            ],
            &["tsconfig.json", "jsconfig.json", "package.json"],
        ),
        preset(
            "clangd",
            "clangd",
            &[],
            &["c", "cpp", "objc", "objcpp"],
            &[
                "compile_commands.json",
                "compile_flags.txt",
                ".clangd",
                "CMakeLists.txt",
            ],
        ),
        preset(
            "lua-language-server",
            "lua-language-server",
            &[],
            &["lua"],
            &[".luarc.json", ".luarc.jsonc", ".stylua.toml", ".git"],
        ),
        preset("zls", "zls", &[], &["zig"], &["build.zig", "build.zig.zon"]),
        preset(
            "bash-language-server",
            "bash-language-server",
            &["start"],
            &["bash", "sh", "shellscript"],
            &[".git"],
        ),
        preset(
            "yaml-language-server",
            "yaml-language-server",
            &["--stdio"],
            &["yaml"],
            &[".git"],
        ),
        preset(
            "taplo",
            "taplo",
            &["lsp", "stdio"],
            &["toml"],
            &["taplo.toml", ".taplo.toml", "Cargo.toml", ".git"],
        ),
        preset(
            "marksman",
            "marksman",
            &["server"],
            &["markdown"],
            &[".marksman.toml", ".git"],
        ),
    ]
}

fn preset(
    name: &str,
    command: &str,
    args: &[&str],
    languages: &[&str],
    root_markers: &[&str],
) -> ServerConfig {
    ServerConfig {
        name: name.to_string(),
        command: command.to_string(),
        args: args.iter().map(|s| s.to_string()).collect(),
        env: HashMap::new(),
        languages: languages.iter().map(|s| s.to_string()).collect(),
        root_markers: root_markers.iter().map(|s| s.to_string()).collect(),
        initialization_options: None,
        settings: None,
    }
}

/// Locate an executable by path or on `PATH`.
pub fn find_executable(command: &str) -> Option<PathBuf> {
    if command.is_empty() {
        return None;
    }
    let candidate = Path::new(command);
    if candidate.components().count() > 1 || candidate.is_absolute() {
        return is_executable_file(candidate).then(|| candidate.to_path_buf());
    }

    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let full = dir.join(command);
        if is_executable_file(&full) {
            return Some(full);
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Walk upward from `start` (a file or directory) looking for any entry in `root_markers`.
///
/// Returns the nearest ancestor directory containing at least one marker, or `None`
/// if no marker is found.
pub fn detect_root(start: &Path, root_markers: &[String]) -> Option<PathBuf> {
    if root_markers.is_empty() {
        return None;
    }
    let dir = if start.is_file() {
        start.parent()?
    } else {
        start
    };

    for ancestor in dir.ancestors() {
        for marker in root_markers {
            if ancestor.join(marker).exists() {
                return Some(ancestor.to_path_buf());
            }
        }
    }
    None
}
