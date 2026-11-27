//! Task definitions, project task auto-detection, process runner, and test result parsing.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tracing::warn;

/// Category of a workspace task.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TaskGroup {
    Build,
    Test,
    Run,
    #[default]
    Other,
}

/// Definition of a runnable workspace task.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TaskDef {
    pub label: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub group: TaskGroup,
    #[serde(default)]
    pub problem_matcher: Option<String>,
}

#[derive(Deserialize)]
struct TasksFile {
    #[serde(default)]
    task: Vec<TaskDef>,
}

/// Loads user-defined tasks from `<root>/.blur/tasks.toml` (`[[task]]`).
pub fn load(root: &Path) -> Vec<TaskDef> {
    let path = root.join(".blur").join("tasks.toml");
    if !path.exists() {
        return Vec::new();
    }
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    match toml::from_str::<TasksFile>(&raw) {
        Ok(file) => file.task,
        Err(err) => {
            warn!("failed to parse tasks file {}: {err}", path.display());
            Vec::new()
        }
    }
}

/// Auto-detects standard tasks from project manifest files in `root`.
///
/// Supports Cargo (`Cargo.toml`), Go (`go.mod`), npm/pnpm/yarn (`package.json`),
/// Python/pytest (`pyproject.toml`/`pytest.ini`/`setup.py`), and `Makefile` targets.
pub fn detect(root: &Path) -> Vec<TaskDef> {
    let mut tasks = Vec::new();

    // Cargo (Rust)
    if root.join("Cargo.toml").exists() {
        tasks.push(make_task("cargo build", "cargo", &["build"], root, TaskGroup::Build));
        tasks.push(make_task("cargo test", "cargo", &["test"], root, TaskGroup::Test));
        tasks.push(make_task("cargo run", "cargo", &["run"], root, TaskGroup::Run));
        tasks.push(make_task("cargo clippy", "cargo", &["clippy"], root, TaskGroup::Build));
    }

    // Go
    if root.join("go.mod").exists() {
        tasks.push(make_task("go build", "go", &["build", "./..."], root, TaskGroup::Build));
        tasks.push(make_task("go test", "go", &["test", "./..."], root, TaskGroup::Test));
        tasks.push(make_task("go run", "go", &["run", "."], root, TaskGroup::Run));
    }

    // Node / npm / pnpm / yarn
    let pkg_json = root.join("package.json");
    if pkg_json.exists() {
        if let Ok(raw) = std::fs::read_to_string(&pkg_json) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(scripts) = val.get("scripts").and_then(|s| s.as_object()) {
                    let pm = if root.join("pnpm-lock.yaml").exists() {
                        "pnpm"
                    } else if root.join("yarn.lock").exists() {
                        "yarn"
                    } else {
                        "npm"
                    };
                    let mut names: Vec<&String> = scripts.keys().collect();
                    names.sort();
                    for name in names {
                        let group = infer_group(name);
                        tasks.push(TaskDef {
                            label: format!("{pm}: {name}"),
                            command: pm.to_string(),
                            args: vec!["run".to_string(), name.clone()],
                            cwd: Some(root.to_path_buf()),
                            env: BTreeMap::new(),
                            group,
                            problem_matcher: None,
                        });
                    }
                }
            }
        }
    }

    // Python / pytest
    if root.join("pyproject.toml").exists()
        || root.join("pytest.ini").exists()
        || root.join("setup.py").exists()
    {
        tasks.push(make_task("pytest", "pytest", &[], root, TaskGroup::Test));
    }

    // Makefile targets
    for name in ["Makefile", "makefile", "GNUmakefile"] {
        let makefile = root.join(name);
        if makefile.exists() {
            if let Ok(raw) = std::fs::read_to_string(&makefile) {
                for target in parse_makefile_targets(&raw) {
                    let group = infer_group(&target);
                    tasks.push(TaskDef {
                        label: format!("make {target}"),
                        command: "make".to_string(),
                        args: vec![target],
                        cwd: Some(root.to_path_buf()),
                        env: BTreeMap::new(),
                        group,
                        problem_matcher: None,
                    });
                }
            }
            break;
        }
    }

    tasks
}

fn make_task(label: &str, cmd: &str, args: &[&str], root: &Path, group: TaskGroup) -> TaskDef {
    TaskDef {
        label: label.to_string(),
        command: cmd.to_string(),
        args: args.iter().map(|s| s.to_string()).collect(),
        cwd: Some(root.to_path_buf()),
        env: BTreeMap::new(),
        group,
        problem_matcher: None,
    }
}

fn infer_group(name: &str) -> TaskGroup {
    let lower = name.to_ascii_lowercase();
    if lower.contains("test") || lower.contains("check") || lower.contains("spec") {
        TaskGroup::Test
    } else if lower.contains("build") || lower.contains("compile") || lower.contains("bundle") {
        TaskGroup::Build
    } else if lower.contains("start") || lower.contains("dev") || lower.contains("run") || lower.contains("serve") {
        TaskGroup::Run
    } else {
        TaskGroup::Other
    }
}

fn parse_makefile_targets(content: &str) -> Vec<String> {
    let mut targets = Vec::new();
    for line in content.lines() {
        if line.starts_with('\t') || line.starts_with(' ') || line.starts_with('#') {
            continue;
        }
        let Some((lhs, rhs)) = line.split_once(':') else {
            continue;
        };
        // Skip variable assignments like `FOO := bar` or `FOO ::= bar`.
        if rhs.starts_with('=') || lhs.contains('=') {
            continue;
        }
        let target = lhs.trim();
        if target.is_empty()
            || target.starts_with('.')
            || target.contains(' ')
            || target.contains('$')
            || target.contains('%')
        {
            continue;
        }
        if !targets.iter().any(|t| t == target) {
            targets.push(target.to_string());
        }
    }
    targets
}

/// A single output line emitted by a running task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskOutput {
    pub task_id: u64,
    pub line: String,
    pub is_stderr: bool,
}

/// Completion notification emitted when a task exits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskFinished {
    pub task_id: u64,
    pub code: Option<i32>,
}

/// Events delivered over a [`TaskHandle`]'s channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskEvent {
    Output(TaskOutput),
    Finished(TaskFinished),
}

/// Execution state of a spawned task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskStatus {
    Running,
    Exited(Option<i32>),
    Killed,
}

/// Handle to a running or completed task process.
pub struct TaskHandle {
    pub id: u64,
    rx: mpsc::Receiver<TaskEvent>,
    kill_flag: Arc<AtomicBool>,
    status: Arc<Mutex<TaskStatus>>,
}

impl TaskHandle {
    /// Returns a reference to the channel receiving [`TaskEvent`]s.
    pub fn receiver(&self) -> &mpsc::Receiver<TaskEvent> {
        &self.rx
    }

    /// Requests termination of the underlying task process.
    pub fn kill(&self) {
        self.kill_flag.store(true, Ordering::SeqCst);
    }

    /// Returns the current [`TaskStatus`].
    pub fn status(&self) -> TaskStatus {
        *self.status.lock()
    }
}

/// Spawns workspace tasks with piped stdout/stderr streaming and kill support.
#[derive(Debug, Default)]
pub struct TaskRunner {
    next_id: AtomicU64,
}

impl TaskRunner {
    /// Creates a new `TaskRunner`.
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
        }
    }

    /// Spawns `def` and returns a [`TaskHandle`] streaming its output and exit code.
    pub fn spawn(&self, def: &TaskDef) -> Result<TaskHandle> {
        let task_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut cmd = Command::new(&def.command);
        cmd.args(&def.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(cwd) = &def.cwd {
            cmd.current_dir(cwd);
        }
        for (k, v) in &def.env {
            cmd.env(k, v);
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to spawn task '{}' ({})", def.label, def.command))?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let (tx, rx) = mpsc::channel::<TaskEvent>();
        let kill_flag = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(TaskStatus::Running));

        let out_handle = stdout.map(|pipe| {
            let tx = tx.clone();
            thread::spawn(move || {
                let reader = BufReader::new(pipe);
                for line in reader.lines().map_while(Result::ok) {
                    let _ = tx.send(TaskEvent::Output(TaskOutput {
                        task_id,
                        line,
                        is_stderr: false,
                    }));
                }
            })
        });

        let err_handle = stderr.map(|pipe| {
            let tx = tx.clone();
            thread::spawn(move || {
                let reader = BufReader::new(pipe);
                for line in reader.lines().map_while(Result::ok) {
                    let _ = tx.send(TaskEvent::Output(TaskOutput {
                        task_id,
                        line,
                        is_stderr: true,
                    }));
                }
            })
        });

        let waiter_kill = Arc::clone(&kill_flag);
        let waiter_status = Arc::clone(&status);
        thread::spawn(move || {
            let mut killed = false;
            let exit_code = loop {
                if waiter_kill.load(Ordering::SeqCst) {
                    let _ = child.kill();
                    killed = true;
                    break child.wait().ok().and_then(|s| s.code());
                }
                match child.try_wait() {
                    Ok(Some(exit)) => break exit.code(),
                    Ok(None) => thread::sleep(Duration::from_millis(15)),
                    Err(_) => break None,
                }
            };

            if let Some(h) = out_handle {
                let _ = h.join();
            }
            if let Some(h) = err_handle {
                let _ = h.join();
            }

            *waiter_status.lock() = if killed {
                TaskStatus::Killed
            } else {
                TaskStatus::Exited(exit_code)
            };
            let _ = tx.send(TaskEvent::Finished(TaskFinished {
                task_id,
                code: exit_code,
            }));
        });

        Ok(TaskHandle {
            id: task_id,
            rx,
            kill_flag,
            status,
        })
    }
}

/// Test discovery and output parsing helpers.
pub mod tests {
    use std::path::Path;
    use std::process::Command;
    use std::time::Duration;

    /// Outcome of a single parsed test case.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum TestStatus {
        Passed,
        Failed,
        Ignored,
    }

    /// Parsed result of a single test case.
    #[derive(Clone, Debug, PartialEq)]
    pub struct TestResult {
        pub name: String,
        pub status: TestStatus,
        pub duration: Option<Duration>,
    }

    /// Parses standard `cargo test` output into structured [`TestResult`] entries.
    pub fn parse_cargo_test(output: &str) -> Vec<TestResult> {
        let mut results = Vec::new();
        for raw_line in output.lines() {
            let line = raw_line.trim();
            let Some(rest) = line.strip_prefix("test ") else {
                continue;
            };
            let Some((name_part, outcome_part)) = rest.split_once(" ... ") else {
                continue;
            };
            let name = name_part.trim();
            if name.is_empty() || name == "result:" {
                continue;
            }
            let outcome = outcome_part.trim();
            let (status, remainder) = if let Some(rem) = outcome.strip_prefix("ok") {
                (TestStatus::Passed, rem)
            } else if let Some(rem) = outcome.strip_prefix("FAILED") {
                (TestStatus::Failed, rem)
            } else if let Some(rem) = outcome.strip_prefix("ignored") {
                (TestStatus::Ignored, rem)
            } else {
                continue;
            };

            let duration = parse_optional_duration(remainder);
            results.push(TestResult {
                name: name.to_string(),
                status,
                duration,
            });
        }
        results
    }

    fn parse_optional_duration(s: &str) -> Option<Duration> {
        // Matches optional `<0.12s>` or `(0.12s)` or `(12ms)` suffixes from `--report-time`.
        let trimmed = s.trim();
        let inner = trimmed
            .strip_prefix('<')
            .and_then(|v| v.strip_suffix('>'))
            .or_else(|| trimmed.strip_prefix('(').and_then(|v| v.strip_suffix(')')))?;
        if let Some(ms) = inner.strip_suffix("ms") {
            let val: f64 = ms.trim().parse().ok()?;
            Some(Duration::from_secs_f64(val / 1000.0))
        } else if let Some(secs) = inner.strip_suffix('s') {
            let val: f64 = secs.trim().parse().ok()?;
            Some(Duration::from_secs_f64(val))
        } else {
            None
        }
    }

    /// Discovers Rust test names in `root` by running `cargo test -- --list`.
    ///
    /// Returns an empty vector if `cargo` fails or is unavailable.
    pub fn discover_rust_tests(root: &Path) -> Vec<String> {
        let Ok(output) = Command::new("cargo")
            .args(["test", "--", "--list"])
            .current_dir(root)
            .output()
        else {
            return Vec::new();
        };
        if !output.status.success() {
            return Vec::new();
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout
            .lines()
            .filter_map(|line| line.trim().strip_suffix(": test"))
            .map(|name| name.to_string())
            .collect()
    }
}
