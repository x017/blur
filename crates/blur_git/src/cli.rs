//! Non-interactive `git` CLI wrappers for network and high-level operations.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use tracing::debug;

/// Run `git -C <root> <args...>` non-interactively and return combined output.
pub fn run(root: &Path, args: &[&str]) -> Result<String> {
    debug!(?root, ?args, "running git CLI");
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("failed to spawn git {:?}", args))?;

    format_output(args, output)
}

/// Run `git -C <root> <args...>` with `stdin_data` piped to stdin.
pub fn run_with_stdin(root: &Path, args: &[&str], stdin_data: &[u8]) -> Result<String> {
    debug!(?root, ?args, "running git CLI with stdin");
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn git {:?}", args))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(stdin_data)
            .context("failed to write stdin to git")?;
    }

    let output = child
        .wait_with_output()
        .with_context(|| format!("failed to wait on git {:?}", args))?;

    format_output(args, output)
}

fn format_output(args: &[&str], output: std::process::Output) -> Result<String> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        let mut combined = String::new();
        combined.push_str(&stdout);
        if !stderr.is_empty() {
            if !combined.is_empty() && !combined.ends_with('\n') {
                combined.push('\n');
            }
            combined.push_str(&stderr);
        }
        Ok(combined)
    } else {
        let detail = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            format!("exit status {}", output.status)
        };
        bail!("git {} failed: {}", args.join(" "), detail);
    }
}

/// Fetch from a specific remote or all configured remotes.
pub fn fetch(root: &Path, remote: Option<&str>) -> Result<String> {
    match remote {
        Some(r) => run(root, &["fetch", r]),
        None => run(root, &["fetch", "--all"]),
    }
}

/// Pull changes into the current branch.
pub fn pull(root: &Path) -> Result<String> {
    run(root, &["pull"])
}

/// Push the current branch, optionally setting upstream (`-u origin HEAD`).
pub fn push(root: &Path, set_upstream: bool) -> Result<String> {
    if set_upstream {
        run(root, &["push", "-u", "origin", "HEAD"])
    } else {
        run(root, &["push"])
    }
}

/// Merge `branch` into the current `HEAD`.
pub fn merge(root: &Path, branch: &str) -> Result<String> {
    run(root, &["merge", branch])
}

/// Rebase the current branch onto `onto`.
pub fn rebase(root: &Path, onto: &str) -> Result<String> {
    run(root, &["rebase", onto])
}

/// Cherry-pick commit `oid`.
pub fn cherry_pick(root: &Path, oid: &str) -> Result<String> {
    run(root, &["cherry-pick", oid])
}

/// Revert commit `oid` without opening an editor.
pub fn revert(root: &Path, oid: &str) -> Result<String> {
    run(root, &["revert", "--no-edit", oid])
}

/// Add a linked worktree at `path` for `branch`.
pub fn worktree_add(root: &Path, path: &Path, branch: &str) -> Result<String> {
    let path_str = path.to_string_lossy();
    run(root, &["worktree", "add", &path_str, branch])
}

/// Remove a linked worktree at `path`.
pub fn worktree_remove(root: &Path, path: &Path) -> Result<String> {
    let path_str = path.to_string_lossy();
    run(root, &["worktree", "remove", &path_str])
}

/// Add a remote with `name` and `url`.
pub fn remote_add(root: &Path, name: &str, url: &str) -> Result<String> {
    run(root, &["remote", "add", name, url])
}

/// Remove a remote by `name`.
pub fn remote_remove(root: &Path, name: &str) -> Result<String> {
    run(root, &["remote", "remove", name])
}

/// Abort an in-progress merge.
pub fn abort_merge(root: &Path) -> Result<String> {
    run(root, &["merge", "--abort"])
}

/// Abort an in-progress rebase.
pub fn abort_rebase(root: &Path) -> Result<String> {
    run(root, &["rebase", "--abort"])
}

/// Continue an in-progress rebase.
pub fn continue_rebase(root: &Path) -> Result<String> {
    run(root, &["rebase", "--continue"])
}
