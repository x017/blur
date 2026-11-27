# Blur Architecture (GPUI + Rust)

## Architectural Separation

Blur enforces a strict boundary between **pure-Rust service crates** (GPUI-free, independently testable) and **native GPUI presentation crates**:

```
crates/blur (AppShell + Mode Views)
  ├── crates/blur_editor (EditorView: Ropey + Tree-sitter + LSP + Vim/Emacs)
  │     ├── crates/blur_buffer
  │     ├── crates/blur_syntax
  │     ├── crates/blur_lsp
  │     ├── crates/blur_core
  │     └── crates/blur_theme
  ├── crates/blur_ui (Stateless components + TextInput)
  │     └── crates/blur_theme
  └── Pure-Rust Services:
        blur_core · blur_workspace · blur_lsp · blur_dap
        blur_git · blur_terminal · blur_extensions · blur_utils · blur_viewers
```

## Execution & Concurrency Model

1. **GPUI Main Thread**: Owns all UI state, layout, focus, keyboard dispatch, and GPU rendering via Blade/Vulkan/Metal.
2. **Tokio Runtime (`blur-async`)**: A lightweight 2-thread Tokio runtime handles non-blocking stdio JSON-RPC framing for LSP and DAP child processes. Public async methods on `LspClient` and `DapClient` await only `futures::channel::oneshot::Receiver`, making them seamlessly awaitable from GPUI's native `cx.spawn(...)` tasks.
3. **Background Threads**: Terminal PTY readers (`blur_terminal`), file watcher debounce (`blur_workspace`), task runners, and blocking HTTP requests (`blur_utils`) run off the UI thread and notify GPUI cleanly.

## Design Language

The native GPUI component system (`blur_theme` + `blur_ui`) implements the **Quiet Precision** (`Furusato Slate`, default) and **Obsidian Precision** specifications from `stitch_blur_development_environment/`:

- Micro-tonal surface tiers (`canvas` -> `surface` -> `surface_panel` -> `surface_elevated`) separated by 1px hairline borders.
- Rectilinear geometry (`0px` panes/tabs, `2px` badges, `3px` `<kbd>` keycaps, `4px` buttons/inputs, `6px` modals).
- Six built-in themes plus custom TOML themes loaded from `~/.config/blur/themes/`.

## Extension Sandbox

Each Lua extension runs in its own isolated `mlua::Lua` state with `io`, `os.execute`, `package.loadlib`, and `debug` removed, a 64 MiB memory ceiling, an instruction-count hook against infinite loops, explicit per-permission checks (`extension.toml` ∩ user-granted permissions), and workspace-root confinement for filesystem APIs.
