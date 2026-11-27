//! Extension installation and removal helpers (`from_dir`, `from_git`, `remove`).

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context};

use crate::manifest::{validate_extension_id, ExtensionManifest};

/// Install an extension by copying a local directory containing `extension.toml`
/// into `extensions_dir/<manifest.id>`.
pub fn from_dir(src: &Path, extensions_dir: &Path) -> anyhow::Result<ExtensionManifest> {
    let manifest_path = src.join("extension.toml");
    let manifest = ExtensionManifest::load(&manifest_path)?;
    let main_file = src.join(&manifest.main);
    if !main_file.is_file() {
        bail!(
            "extension entry point '{}' not found in {}",
            manifest.main,
            src.display()
        );
    }

    fs::create_dir_all(extensions_dir).with_context(|| {
        format!(
            "failed to create extensions directory {}",
            extensions_dir.display()
        )
    })?;

    let dest = extensions_dir.join(&manifest.id);
    if dest.exists() {
        fs::remove_dir_all(&dest)
            .with_context(|| format!("failed to replace existing extension at {}", dest.display()))?;
    }
    copy_dir_recursive(src, &dest)?;

    // Validate installed manifest after copy.
    ExtensionManifest::load(&dest.join("extension.toml"))
}

/// Install an extension by cloning a git repository (`git clone --depth 1`)
/// into `extensions_dir/<manifest.id>`.
pub fn from_git(url: &str, extensions_dir: &Path) -> anyhow::Result<ExtensionManifest> {
    fs::create_dir_all(extensions_dir).with_context(|| {
        format!(
            "failed to create extensions directory {}",
            extensions_dir.display()
        )
    })?;

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_dir = extensions_dir.join(format!(".tmp-git-install-{nonce}"));

    let output = Command::new("git")
        .args(["clone", "--depth", "1", url])
        .arg(&tmp_dir)
        .output()
        .map_err(|e| anyhow!("failed to execute 'git clone': {e}"))?;

    if !output.status.success() {
        let _ = fs::remove_dir_all(&tmp_dir);
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git clone failed: {}", stderr.trim());
    }

    let manifest = match ExtensionManifest::load(&tmp_dir.join("extension.toml")) {
        Ok(m) => m,
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp_dir);
            return Err(e);
        }
    };

    if !tmp_dir.join(&manifest.main).is_file() {
        let _ = fs::remove_dir_all(&tmp_dir);
        bail!("cloned extension is missing entry point '{}'", manifest.main);
    }

    let dest = extensions_dir.join(&manifest.id);
    if dest.exists() {
        fs::remove_dir_all(&dest)
            .with_context(|| format!("failed to replace existing extension at {}", dest.display()))?;
    }
    if let Err(e) = fs::rename(&tmp_dir, &dest) {
        let _ = fs::remove_dir_all(&tmp_dir);
        return Err(anyhow!(
            "failed to move cloned extension into {}: {e}",
            dest.display()
        ));
    }

    ExtensionManifest::load(&dest.join("extension.toml"))
}

/// Remove an installed extension by `id` from `extensions_dir`.
pub fn remove(id: &str, extensions_dir: &Path) -> anyhow::Result<()> {
    validate_extension_id(id)?;
    let target = extensions_dir.join(id);
    if !target.exists() {
        bail!("extension '{id}' is not installed in {}", extensions_dir.display());
    }
    fs::remove_dir_all(&target)
        .with_context(|| format!("failed to remove extension at {}", target.display()))?;
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if file_type.is_dir() {
            // Skip .git metadata when copying from local directories
            if entry.file_name() == ".git" {
                continue;
            }
            copy_dir_recursive(&from, &to)?;
        } else if file_type.is_file() {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_from_dir_and_remove() {
        let base = std::env::temp_dir().join(format!(
            "blur-ext-install-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        let src = base.join("src-ext");
        let target_dir = base.join("installed");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("extension.toml"),
            "id = \"sample-ext\"\nname = \"Sample\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        fs::write(src.join("init.lua"), "blur.log('hi')").unwrap();

        let manifest = from_dir(&src, &target_dir).unwrap();
        assert_eq!(manifest.id, "sample-ext");
        assert!(target_dir.join("sample-ext").join("init.lua").is_file());

        remove("sample-ext", &target_dir).unwrap();
        assert!(!target_dir.join("sample-ext").exists());

        let _ = fs::remove_dir_all(&base);
    }
}
