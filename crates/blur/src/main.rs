//! Blur — Native Rust + GPUI Development Environment.
//!
//! Launches the native GPUI application window with a lazy Tokio runtime
//! for LSP/DAP I/O, layered TOML configuration, native theme engine,
//! Ropey + Tree-sitter editor, Git integration, GPU terminal, developer
//! utilities, native Markdown/data viewers, and sandboxed Lua extensions.

mod app;
mod views;

use std::path::PathBuf;

use app::AppShell;
use blur_core::{paths, ConfigStore};
use blur_theme::{Theme, ThemeRegistry};
use gpui::{px, size, App, AppContext, Application, Bounds, WindowBounds, WindowOptions};
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    // Workspace root from CLI arg or current directory
    let workspace_root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .and_then(|p| p.canonicalize().ok())
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        });

    let config = ConfigStore::load(Some(&workspace_root));
    let theme_registry = ThemeRegistry::load(Some(&paths::themes_dir()));
    let mut active_theme = theme_registry.resolve(&config.settings().ui.theme);
    active_theme.ui_scale = config.settings().ui.ui_scale.clamp(0.75, 1.75);

    // Lightweight Tokio runtime for LSP / DAP stdio pipes
    let tokio_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("blur-async")
        .enable_all()
        .build()
        .expect("failed to build Tokio runtime");
    let tokio_handle = tokio_rt.handle().clone();

    Application::new().run(move |cx: &mut App| {
        cx.set_global(theme_registry);
        Theme::set_global(active_theme, cx);

        let bounds = Bounds::centered(None, size(px(1440.0), px(900.0)), cx);
        let opts = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };

        let root = workspace_root.clone();
        let handle = tokio_handle.clone();
        let cfg = config.clone();

        cx.open_window(opts, move |_window, cx| {
            cx.new(|cx| AppShell::new(root, handle, cfg, cx))
        })
        .expect("failed to open Blur main window");

        cx.activate(true);
    });
}
