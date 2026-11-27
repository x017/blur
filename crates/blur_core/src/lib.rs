//! Core configuration, command registry, keymap model, event bus, service registry, and paths for Blur.
//!
//! This crate is GPUI-free pure Rust and provides the foundational types consumed by
//! both service crates and the UI layer.

pub mod commands;
pub mod events;
pub mod keymap;
pub mod paths;
pub mod services;
pub mod settings;

pub use commands::{CommandMeta, CommandRegistry};
pub use events::{EventBus, SubscriptionId};
pub use keymap::{display_keys, default_bindings, load_user_keymap, KeyBindingSpec, NavigationMode};
pub use services::{ServiceRegistry, ServiceStatus};
pub use settings::{
    deep_merge_toml, toml_to_json, ConfigStore, EditorSettings, ExtensionSettings, FileSettings,
    GitSettings, LspServerEntry, LspSettings, Settings, TerminalSettings, UiDensity, UiSettings,
};
