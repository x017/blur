//! Debounced filesystem watcher built on `notify` 7.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use notify::event::ModifyKind;
use notify::{recommended_watcher, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tracing::warn;

const DEBOUNCE_WINDOW: Duration = Duration::from_millis(100);

/// High-level filesystem event kind emitted by [`FileWatcher`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FsEventKind {
    Created,
    Modified,
    Removed,
    Renamed,
}

/// A debounced filesystem change event.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FsEvent {
    pub path: PathBuf,
    pub kind: FsEventKind,
}

/// Recursive filesystem watcher that coalesces events over a 100ms debounce window
/// and ignores noisy build/VCS directories (`.git/objects`, `target/`, `node_modules/`).
pub struct FileWatcher {
    watcher: Option<RecommendedWatcher>,
    thread: Option<JoinHandle<()>>,
}

impl FileWatcher {
    /// Starts watching `roots` recursively and delivers debounced batches to `on_events`.
    pub fn new(
        roots: &[PathBuf],
        on_events: impl Fn(Vec<FsEvent>) + Send + 'static,
    ) -> Result<Self> {
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher =
            recommended_watcher(move |res| {
                let _ = tx.send(res);
            })
            .context("failed to create filesystem watcher")?;

        for root in roots {
            if root.exists() {
                watcher
                    .watch(root, RecursiveMode::Recursive)
                    .with_context(|| format!("failed to watch path {}", root.display()))?;
            }
        }

        let handle = thread::Builder::new()
            .name("blur-file-watcher".to_string())
            .spawn(move || debounce_loop(rx, on_events))
            .context("failed to spawn file watcher debounce thread")?;

        Ok(Self {
            watcher: Some(watcher),
            thread: Some(handle),
        })
    }
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        // Dropping the notify watcher closes the mpsc sender, causing the debounce thread to exit.
        self.watcher.take();
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn debounce_loop(
    rx: mpsc::Receiver<notify::Result<Event>>,
    on_events: impl Fn(Vec<FsEvent>) + Send + 'static,
) {
    let mut pending: HashMap<PathBuf, FsEventKind> = HashMap::new();
    let mut deadline: Option<Instant> = None;

    loop {
        let timeout = match deadline {
            Some(dl) => dl.saturating_duration_since(Instant::now()),
            None => Duration::from_secs(3600),
        };

        match rx.recv_timeout(timeout) {
            Ok(Ok(event)) => {
                if let Some(kind) = map_event_kind(&event.kind) {
                    for path in event.paths {
                        if !is_noisy_path(&path) {
                            pending.insert(path, kind);
                            if deadline.is_none() {
                                deadline = Some(Instant::now() + DEBOUNCE_WINDOW);
                            }
                        }
                    }
                }
            }
            Ok(Err(err)) => {
                warn!("file watcher error: {err}");
            }
            Err(RecvTimeoutError::Timeout) => {
                if !pending.is_empty() {
                    let batch = drain_batch(&mut pending);
                    on_events(batch);
                }
                deadline = None;
            }
            Err(RecvTimeoutError::Disconnected) => {
                if !pending.is_empty() {
                    let batch = drain_batch(&mut pending);
                    on_events(batch);
                }
                break;
            }
        }
    }
}

fn drain_batch(pending: &mut HashMap<PathBuf, FsEventKind>) -> Vec<FsEvent> {
    let mut batch: Vec<FsEvent> = pending
        .drain()
        .map(|(path, kind)| FsEvent { path, kind })
        .collect();
    batch.sort_by(|a, b| a.path.cmp(&b.path));
    batch
}

fn map_event_kind(kind: &EventKind) -> Option<FsEventKind> {
    match kind {
        EventKind::Create(_) => Some(FsEventKind::Created),
        EventKind::Modify(ModifyKind::Name(_)) => Some(FsEventKind::Renamed),
        EventKind::Modify(_) => Some(FsEventKind::Modified),
        EventKind::Remove(_) => Some(FsEventKind::Removed),
        _ => None,
    }
}

/// Returns `true` if `path` falls inside `.git/objects`, `target/`, or `node_modules/`.
pub(crate) fn is_noisy_path(path: &Path) -> bool {
    let comps: Vec<&str> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(os) => os.to_str(),
            _ => None,
        })
        .collect();

    for (idx, &comp) in comps.iter().enumerate() {
        if comp == "target" || comp == "node_modules" {
            return true;
        }
        if comp == ".git" && comps.get(idx + 1).copied() == Some("objects") {
            return true;
        }
    }
    false
}
