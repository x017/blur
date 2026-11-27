//! Extension manifest (`extension.toml`) and permission definitions.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};

/// Explicit capabilities an extension can request and be granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    #[serde(rename = "commands")]
    Commands,
    #[serde(rename = "keymap")]
    Keymap,
    #[serde(rename = "ui")]
    Ui,
    #[serde(rename = "workspace.read")]
    WorkspaceRead,
    #[serde(rename = "fs.read")]
    FsRead,
    #[serde(rename = "fs.write")]
    FsWrite,
    #[serde(rename = "process")]
    Process,
    #[serde(rename = "network")]
    Network,
    #[serde(rename = "themes")]
    Themes,
}

impl Permission {
    /// Canonical string name used in `extension.toml` and error messages.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Commands => "commands",
            Self::Keymap => "keymap",
            Self::Ui => "ui",
            Self::WorkspaceRead => "workspace.read",
            Self::FsRead => "fs.read",
            Self::FsWrite => "fs.write",
            Self::Process => "process",
            Self::Network => "network",
            Self::Themes => "themes",
        }
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

fn default_main() -> String {
    "init.lua".to_string()
}

/// Deserialized `extension.toml` manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default = "default_main")]
    pub main: String,
    #[serde(default)]
    pub permissions: Vec<Permission>,
}

impl ExtensionManifest {
    /// Load and validate an `extension.toml` file.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read manifest at {}", path.display()))?;
        let manifest: Self = toml::from_str(&raw)
            .with_context(|| format!("failed to parse manifest at {}", path.display()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validate required fields and ensure `id` and `main` contain no path traversal.
    pub fn validate(&self) -> anyhow::Result<()> {
        validate_extension_id(&self.id)?;
        if self.name.trim().is_empty() {
            bail!("extension manifest 'name' cannot be empty");
        }
        if self.version.trim().is_empty() {
            bail!("extension manifest 'version' cannot be empty");
        }
        if self.main.trim().is_empty() {
            bail!("extension manifest 'main' cannot be empty");
        }
        let main_path = Path::new(&self.main);
        if main_path.is_absolute()
            || main_path
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_)))
        {
            bail!("extension manifest 'main' must be a relative path inside the extension directory");
        }
        Ok(())
    }
}

/// Validate that an extension ID is safe for use as a directory name.
pub(crate) fn validate_extension_id(id: &str) -> anyhow::Result<()> {
    if id.is_empty() {
        bail!("extension id cannot be empty");
    }
    if id == "." || id == ".." {
        bail!("invalid extension id '{id}'");
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        bail!("extension id '{id}' contains disallowed characters");
    }
    Ok(())
}

/// An extension discovered on disk.
#[derive(Debug, Clone)]
pub struct DiscoveredExtension {
    pub manifest: ExtensionManifest,
    pub dir: PathBuf,
    pub enabled: bool,
}
