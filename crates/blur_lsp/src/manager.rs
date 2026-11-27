//! Multi-server LSP manager with lazy start, root detection, and event fan-out.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::channel::mpsc::UnboundedReceiver;
use parking_lot::Mutex;
use tracing::{info, warn};

use crate::client::{EventHub, LspClient, LspEvent, ServerInfo, ServerStatus, WireFrame};
use crate::presets::{detect_root, find_executable, ServerConfig};

/// Manages language server configurations and lazily spawned [`LspClient`] instances.
///
/// Cheaply cloneable (`Arc` inside) and `Send + Sync`. Supports multiple servers
/// per workspace and multiple servers per language, deduplicated by
/// `(server_name, detected_root)`.
#[derive(Clone)]
pub struct LspManager {
    inner: Arc<ManagerInner>,
}

impl std::fmt::Debug for LspManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspManager")
            .field("configs", &self.inner.configs.lock().len())
            .field("clients", &self.inner.clients.lock().len())
            .finish()
    }
}

struct ManagerInner {
    handle: tokio::runtime::Handle,
    configs: Mutex<Vec<ServerConfig>>,
    /// Keyed by `(server_name, root_dir)`.
    clients: Mutex<HashMap<(String, PathBuf), LspClient>>,
    events: Arc<EventHub>,
}

impl LspManager {
    /// Create a new [`LspManager`] using `handle` for all background server I/O.
    ///
    /// Does not spawn any servers until [`Self::client_for`] or [`Self::clients_for`]
    /// is called.
    pub fn new(handle: tokio::runtime::Handle, configs: Vec<ServerConfig>) -> Self {
        Self {
            inner: Arc::new(ManagerInner {
                handle,
                configs: Mutex::new(configs),
                clients: Mutex::new(HashMap::new()),
                events: Arc::new(EventHub::new()),
            }),
        }
    }

    /// Replace the active server configuration list.
    ///
    /// Existing running clients continue running until stopped or restarted;
    /// future [`Self::client_for`] calls use the updated configurations.
    pub fn set_configs(&self, configs: Vec<ServerConfig>) {
        *self.inner.configs.lock() = configs;
    }

    /// Return a copy of the currently registered server configurations.
    pub fn configs(&self) -> Vec<ServerConfig> {
        self.inner.configs.lock().clone()
    }

    /// Subscribe to the shared [`LspEvent`] stream across all managed servers.
    ///
    /// Multiple subscribers are supported independently.
    pub fn events(&self) -> UnboundedReceiver<LspEvent> {
        self.inner.events.subscribe()
    }

    /// Return (lazily starting if needed) the primary [`LspClient`] for `language_id`
    /// and `root` (which may be either a workspace directory or a file path inside it).
    ///
    /// Returns `None` if no configured server matches `language_id` or none of the
    /// matching server executables can be found on `PATH`.
    pub fn client_for(&self, language_id: &str, root: &Path) -> Option<LspClient> {
        self.clients_for(language_id, root).into_iter().next()
    }

    /// Return (lazily starting if needed) all matching [`LspClient`]s for `language_id`
    /// and `root`, enabling multiple servers per language/workspace.
    pub fn clients_for(&self, language_id: &str, root: &Path) -> Vec<LspClient> {
        let matching_configs: Vec<ServerConfig> = {
            let configs = self.inner.configs.lock();
            configs
                .iter()
                .filter(|c| c.languages.iter().any(|l| l == language_id))
                .cloned()
                .collect()
        };

        let mut result = Vec::new();
        for config in matching_configs {
            let resolved_root = resolve_workspace_root(root, &config.root_markers);
            let key = (config.name.clone(), resolved_root.clone());

            // Reuse existing non-crashed/non-stopped client if present.
            {
                let clients = self.inner.clients.lock();
                if let Some(existing) = clients.get(&key) {
                    if !matches!(
                        existing.status(),
                        ServerStatus::Stopped | ServerStatus::Crashed { .. }
                    ) {
                        result.push(existing.clone());
                        continue;
                    }
                }
            }

            let Some(exe) = find_executable(&config.command) else {
                continue;
            };

            match LspClient::spawn_process(
                &self.inner.handle,
                config.clone(),
                exe,
                resolved_root.clone(),
                self.inner.events.clone(),
            ) {
                Ok(client) => {
                    info!(
                        server = %config.name,
                        root = %resolved_root.display(),
                        "lazily started LSP server"
                    );
                    self.inner.clients.lock().insert(key, client.clone());
                    result.push(client);
                }
                Err(e) => {
                    warn!(server = %config.name, "failed to start LSP server: {}", e);
                }
            }
        }
        result
    }

    /// Return runtime info for all known server instances.
    pub fn running(&self) -> Vec<ServerInfo> {
        self.inner
            .clients
            .lock()
            .values()
            .map(|c| c.info())
            .collect()
    }

    /// Restart all instances matching `name` (shuts down old instances and spawns fresh ones).
    pub fn restart(&self, name: &str) {
        let to_restart: Vec<(PathBuf, LspClient)> = {
            let mut clients = self.inner.clients.lock();
            let keys: Vec<(String, PathBuf)> = clients
                .keys()
                .filter(|(n, _)| n == name)
                .cloned()
                .collect();
            let mut removed = Vec::new();
            for k in keys {
                if let Some(c) = clients.remove(&k) {
                    removed.push((k.1, c));
                }
            }
            removed
        };

        let config = {
            let configs = self.inner.configs.lock();
            configs.iter().find(|c| c.name == name).cloned()
        };

        for (root, old_client) in to_restart {
            old_client.shutdown_nowait();
            if let Some(ref cfg) = config {
                if let Some(exe) = find_executable(&cfg.command) {
                    match LspClient::spawn_process(
                        &self.inner.handle,
                        cfg.clone(),
                        exe,
                        root.clone(),
                        self.inner.events.clone(),
                    ) {
                        Ok(new_client) => {
                            self.inner
                                .clients
                                .lock()
                                .insert((cfg.name.clone(), root), new_client);
                        }
                        Err(e) => {
                            warn!(server = %name, "failed to restart LSP server: {}", e);
                        }
                    }
                }
            }
        }
    }

    /// Gracefully stop all instances matching `name`.
    pub fn stop(&self, name: &str) {
        let to_stop: Vec<LspClient> = {
            let mut clients = self.inner.clients.lock();
            let keys: Vec<(String, PathBuf)> = clients
                .keys()
                .filter(|(n, _)| n == name)
                .cloned()
                .collect();
            keys.into_iter()
                .filter_map(|k| clients.remove(&k))
                .collect()
        };

        for client in to_stop {
            client.shutdown_nowait();
        }
    }

    /// Gracefully shut down all running language servers.
    pub fn shutdown_all(&self) {
        let all: Vec<LspClient> = {
            let mut clients = self.inner.clients.lock();
            clients.drain().map(|(_, c)| c).collect()
        };

        for client in all {
            client.shutdown_nowait();
        }
    }

    /// Return the last up to 200 wire frames for the first instance matching `name`.
    pub fn wire_log(&self, name: &str) -> Vec<WireFrame> {
        let clients = self.inner.clients.lock();
        let mut frames = Vec::new();
        for ((n, _), client) in clients.iter() {
            if n == name {
                frames.extend(client.wire_log());
            }
        }
        frames.sort_by_key(|f| f.at);
        if frames.len() > 200 {
            let drain_count = frames.len() - 200;
            frames.drain(..drain_count);
        }
        frames
    }
}

fn resolve_workspace_root(path: &Path, markers: &[String]) -> PathBuf {
    if let Some(detected) = detect_root(path, markers) {
        return detected;
    }
    if path.is_file() {
        if let Some(parent) = path.parent() {
            return parent.to_path_buf();
        }
    }
    path.to_path_buf()
}
