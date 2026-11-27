//! Registry tracking named lazily started background services.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::SystemTime;

use parking_lot::Mutex;

/// Snapshot of a registered service's lifecycle status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceStatus {
    pub name: String,
    pub description: String,
    pub running: bool,
    pub started_at: Option<SystemTime>,
}

/// Thread-safe registry tracking named services and whether they are running.
#[derive(Clone, Debug, Default)]
pub struct ServiceRegistry {
    services: Arc<Mutex<BTreeMap<String, ServiceStatus>>>,
}

impl ServiceRegistry {
    /// Creates an empty service registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a named service (initially stopped).
    pub fn register(&self, name: impl Into<String>, description: impl Into<String>) {
        let name = name.into();
        let description = description.into();
        self.services.lock().entry(name.clone()).or_insert(ServiceStatus {
            name,
            description,
            running: false,
            started_at: None,
        });
    }

    /// Marks a service as started and records its start timestamp.
    pub fn mark_started(&self, name: &str) {
        let mut guard = self.services.lock();
        let entry = guard
            .entry(name.to_string())
            .or_insert_with(|| ServiceStatus {
                name: name.to_string(),
                description: String::new(),
                running: false,
                started_at: None,
            });
        entry.running = true;
        entry.started_at = Some(SystemTime::now());
    }

    /// Marks a service as stopped.
    pub fn mark_stopped(&self, name: &str) {
        if let Some(entry) = self.services.lock().get_mut(name) {
            entry.running = false;
            entry.started_at = None;
        }
    }

    /// Returns whether the named service is currently marked running.
    pub fn is_running(&self, name: &str) -> bool {
        self.services
            .lock()
            .get(name)
            .map(|s| s.running)
            .unwrap_or(false)
    }

    /// Returns the status of a single service by name.
    pub fn get(&self, name: &str) -> Option<ServiceStatus> {
        self.services.lock().get(name).cloned()
    }

    /// Lists all registered services sorted by name.
    pub fn list(&self) -> Vec<ServiceStatus> {
        self.services.lock().values().cloned().collect()
    }
}
