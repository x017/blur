//! Sandboxed Lua 5.4 extension runtime for Blur (GPUI-free, pure Rust).
//!
//! Each extension runs in its own isolated `mlua::Lua` VM with a restricted standard library,
//! memory limit, instruction-count hook, and explicit capability-based permissions.

pub mod host;
pub mod install;
pub mod manifest;
pub mod types;

pub use host::ExtensionHost;
pub use manifest::{DiscoveredExtension, ExtensionManifest, Permission};
pub use types::{
    CommandRegistration, EditorContext, ExtensionInfo, HostRequest, NotifyLevel, PanelContent,
    PanelItem, PanelRegistration, ThemeRegistration,
};
