# Blur

**Blur** is a native, lightweight, cross-platform development environment built entirely with **Rust** and **GPUI** — with zero WebView, HTML, CSS, or JavaScript frontend, zero built-in AI dependencies, and zero mandatory cloud services or marketplaces.

## Core Principles

- **Native GPU UI**: Rendered directly with GPUI (Wayland/X11/macOS/Windows).
- **Lightweight & Lazy**: Expensive services (language servers, PTYs, debug adapters, file watchers) start only when needed.
- **Universal**: Generic LSP 3.17 and DAP clients support any external language server or debug adapter.
- **Modular**: 15 focused workspace crates separating pure-Rust services from GPUI presentation.
- **Offline-first & Local**: Layered TOML configuration (`~/.config/blur/config.toml` + `.blur/config.toml`), local workspace persistence, and local/Git-installed Lua extensions.

## Workspace Crates

| Crate | Layer | Purpose |
| --- | --- | --- |
| `crates/blur` | Application | Binary entry point, `AppShell`, mode views (Editor, File Finder, Git, Protocols & LSP, Utilities, Settings), Command Palette, Theme Switcher, and integrated Terminal drawer |
| `crates/blur_editor` | UI | Native GPUI code editor (`EditorView`) with incremental Tree-sitter highlights, gutter diff/diagnostics, hover/completion popups, and Standard/Vim/Emacs navigation |
| `crates/blur_ui` | UI | Reusable GPUI component library (buttons, badges, `<kbd>` caps, toggles, list rows, toasts, interactive `TextInput`) |
| `crates/blur_theme` | UI | Theme engine with 6 built-in themes (`Furusato Slate`, `Obsidian Precision`, `Warm Amber Soft`, `Tokyo Dusk`, `Rose Pine Moon`, `E-Ink Paper Light`) + TOML custom theme loading |
| `crates/blur_core` | Core | Layered TOML `ConfigStore`, `CommandRegistry`, `KeyBindingSpec` defaults, `EventBus`, `ServiceRegistry`, paths |
| `crates/blur_buffer` | Editor Core | Ropey text buffer, multi-cursor `Selections`, coalescing undo/redo, `EditDelta` (Tree-sitter + LSP UTF-16), movement & editing ops |
| `crates/blur_syntax` | Editor Core | Incremental Tree-sitter parsing (16 built-in grammars), visible-range syntax highlighting, fold ranges, symbol outline |
| `crates/blur_lsp` | Protocols | Generic LSP 3.17 stdio JSON-RPC client, lazy per-root `LspManager`, capability negotiation, incremental/full doc sync, wire log & latency tracking |
| `crates/blur_dap` | Protocols | Generic Debug Adapter Protocol client + `.blur/launch.toml` loader |
| `crates/blur_git` | Development | First-class Git via `git2` + Git CLI: status, commit log, visual DAG lane layout, hunk/line partial staging, blame, stash, 2-way/diff3 conflict resolver, heatmap |
| `crates/blur_terminal` | Development | Cross-platform PTY (`portable-pty`) + VT100/ANSI grid (`alacritty_terminal`) with xterm key encoding |
| `crates/blur_workspace` | Workspace | Directory listing, `nucleo-matcher` fuzzy file index, parallel project search & replace, debounced file watcher, local workspace persistence & snapshots, task detection & runner |
| `crates/blur_extensions` | Extensibility | Sandboxed Lua 5.4 runtime (`mlua`), explicit permission gates, memory/instruction limits, `blur.*` API, local/Git installer + sample `extensions/hello-blur` |
| `crates/blur_utils` | Utilities | HTTP client, JSON formatter/validator/query, Regex tester, Base64/URL/Hex, UUID v4/v7, Timestamp converter, Color/contrast tools, Diff, SQL CLI runner |
| `crates/blur_viewers` | Viewers | Native GFM Markdown parser, KaTeX-subset Unicode math, Mermaid flowchart/sequence parser & layered layout, JSON/YAML/TOML tree, CSV table |

## Building & Running

```bash
# Run all unit tests across the workspace
cargo test --workspace

# Build and launch Blur on the current directory (or pass a workspace path)
cargo run -p blur -- /path/to/project
```

## Keyboard Shortcuts

| Shortcut | Action |
| --- | --- |
| `Ctrl+K` / `Ctrl+Shift+P` | Command Palette |
| `Ctrl+P` | Quick File Open |
| `Ctrl+Shift+F` | Search in Project |
| `Ctrl+B` | Toggle File Explorer Sidebar |
| `` Ctrl+` `` | Toggle Integrated GPU Terminal |
| `Ctrl+S` | Save Active File |
| `Ctrl+F` | Find in Active Buffer |
| `Ctrl+Space` | Trigger LSP Autocomplete |
| `Ctrl+Shift+I` / `K` (Vim) | LSP Hover |
| `F12` / `gd` (Vim) | Go to Definition |
| `Ctrl+,` | System Settings & Theme Studio |
| `Ctrl+Q` | Quit |
