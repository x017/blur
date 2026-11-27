# Blur — Service Crate Contracts (shared by all agents)

Workspace: `/home/parsa/Projects/blur-real` (Cargo workspace, crates in `crates/`).
Architecture rule: **service crates are GPUI-free** pure Rust. The GPUI layer (`blur_theme`, `blur_ui`,
`blur_editor`, `blur`) consumes them. No business logic in UI; no UI in services.

## Common rules for every service crate
- `anyhow::Result` for fallible public APIs; `tracing` for logs. No `println!`.
- No network/background threads unless the API is explicitly asked for (lazy start).
- Public types `Debug + Clone` where cheap; data types that cross threads must be `Send + Sync`.
- Async crates (lsp/dap) run their I/O on **tokio tasks** spawned on a `tokio::runtime::Handle` passed in by
  the caller. Public request methods must return futures that only await `futures::channel::oneshot`
  receivers, so they can be awaited from **any executor** (the GPUI executor is not tokio).
  Event streams use `futures::channel::mpsc::UnboundedReceiver<Event>`.
- Unit tests for the core logic (`cargo test -p <crate> --offline`).
- Build OFFLINE ONLY: always pass `--offline`. Dependencies already declared in your crate's Cargo.toml
  are fetched. Any crate already present in `Cargo.lock` may be added (with `--offline`). Do NOT edit the
  root `Cargo.toml` or crates you do not own.
- Keep code idiomatic, documented (`//!` crate docs + doc comments on public items), and warning-free
  (`cargo clippy` not required, but `cargo build` must have zero warnings in your crate).

## blur_buffer  (Ropey text buffer, selections, editing ops)
```rust
pub use ropey::{Rope, RopeSlice};
pub struct Buffer { /* rope, path, version, dirty, line_ending, history */ }
impl Buffer {
    pub fn new() -> Self;
    pub fn from_text(text: &str) -> Self;
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self>;      // detects CRLF/LF, normalizes to \n internally
    pub fn save(&mut self) -> anyhow::Result<()>;                         // writes with original line ending
    pub fn save_as(&mut self, path: impl AsRef<Path>) -> anyhow::Result<()>;
    pub fn rope(&self) -> &Rope;
    pub fn text(&self) -> String;
    pub fn path(&self) -> Option<&Path>;
    pub fn set_path(&mut self, path: PathBuf);
    pub fn is_dirty(&self) -> bool;
    pub fn version(&self) -> i32;                                         // monotonically increases on every edit (LSP version)
    pub fn len_chars(&self) -> usize; pub fn len_lines(&self) -> usize;
    pub fn line_text(&self, line: usize) -> String;                       // without trailing newline
    pub fn line_len_chars(&self, line: usize) -> usize;                   // without trailing newline
    pub fn edit(&mut self, edits: Vec<(Range<usize>, String)>) -> Vec<EditDelta>; // char ranges in ORIGINAL coords, non-overlapping, any order
    pub fn undo(&mut self) -> Option<UndoResult>;                         // UndoResult { deltas: Vec<EditDelta>, selections: Selections }
    pub fn redo(&mut self) -> Option<UndoResult>;
    pub fn set_selections_for_undo(&mut self, before: &Selections);       // records selection state with the next transaction
    pub fn break_undo_group(&mut self);                                   // undo grouping: consecutive edits within 500ms coalesce unless broken
    // coordinate conversion
    pub fn char_to_point(&self, char_idx: usize) -> Point;                // Point { line, col } (col in chars)
    pub fn point_to_char(&self, p: Point) -> usize;                       // clamps
    pub fn char_to_lsp(&self, char_idx: usize) -> LspPosition;            // LspPosition { line: u32, character: u32 } UTF-16
    pub fn lsp_to_char(&self, p: LspPosition) -> usize;
    pub fn char_to_byte(&self, c: usize) -> usize; pub fn byte_to_char(&self, b: usize) -> usize;
    pub fn find_all(&self, query: &str, opts: SearchOptions) -> Vec<Range<usize>>;   // SearchOptions { case_sensitive, whole_word, regex }
    pub fn matching_bracket(&self, char_idx: usize) -> Option<usize>;     // ()[]{} scan, skips nothing fancy
    pub fn word_range_at(&self, char_idx: usize) -> Option<Range<usize>>;
}
/// Everything tree-sitter and LSP incremental sync need for one applied edit.
pub struct EditDelta {
    pub start_byte: usize, pub old_end_byte: usize, pub new_end_byte: usize,
    pub start_point: (usize, usize), pub old_end_point: (usize, usize), pub new_end_point: (usize, usize), // (row, byte col)
    pub lsp_range: (LspPosition, LspPosition),   // range in the document BEFORE this delta (UTF-16)
    pub new_text: String,
}
// Deltas are returned in application order: each delta's coordinates are valid against the document
// state right after the previous delta was applied (so they can be fed sequentially to tree-sitter / LSP).

pub struct Selection { pub anchor: usize, pub head: usize, pub goal_col: Option<usize> } // char indices
impl Selection { pub fn cursor(c: usize) -> Self; pub fn range(&self) -> Range<usize>; pub fn is_empty(&self) -> bool; }
pub struct Selections { /* Vec<Selection>, primary index; always sorted & merged */ }
impl Selections {
    pub fn single(sel: Selection) -> Self; pub fn all(&self) -> &[Selection]; pub fn primary(&self) -> &Selection;
    pub fn add(&mut self, sel: Selection); pub fn set(&mut self, v: Vec<Selection>, primary: usize);
    pub fn collapse_to_primary(&mut self); pub fn len(&self) -> usize;
}
pub mod movement { // pure functions on (&Buffer, Selection) -> Selection, `extend` keeps anchor
    left, right, up, down (goal column), word_left, word_right, line_start (smart home: first non-ws then col 0),
    line_end, doc_start, doc_end, page_up(n_lines), page_down(n_lines)
}
pub mod ops { // apply to all selections, return deltas; each op records undo with selections
    pub fn insert(buf: &mut Buffer, sels: &mut Selections, text: &str) -> Vec<EditDelta>;  // auto-closes brackets/quotes when `auto_pair` arg true? -> keep separate fn insert_with_autopair
    backspace, delete_forward, delete_word_left, delete_word_right,
    newline (keeps indentation of current line; extra indent after `{ ( [ :`),
    indent / outdent (tab_size, insert_spaces), toggle_line_comment(token), duplicate_lines, move_lines_up/down,
    delete_lines, select_line, select_word, select_all, add_cursor_above/below, select_next_occurrence,
    cut_text/copy_text helpers returning String, paste(text) (multi-cursor aware: if lines == cursors, distribute)
}
```

## blur_syntax  (tree-sitter, incremental parse, highlights)
```rust
pub use tree_sitter::{InputEdit, Point as TsPoint};
pub struct Language { pub name: &'static str, pub lsp_id: &'static str, pub extensions: &'static [&'static str],
    pub filenames: &'static [&'static str], pub line_comment: Option<&'static str>,
    pub block_comment: Option<(&'static str, &'static str)>, /* ts language + compiled highlight Query */ }
pub struct LanguageRegistry; // Default with built-ins: rust python go javascript typescript tsx c cpp json bash lua toml markdown html css yaml
impl LanguageRegistry { pub fn new() -> Self; pub fn for_path(&self, p: &Path) -> Option<Arc<Language>>;
    pub fn by_name(&self, name: &str) -> Option<Arc<Language>>; pub fn all(&self) -> Vec<Arc<Language>>; }
pub struct Syntax { /* parser, tree, language */ }
impl Syntax {
    pub fn new(lang: Arc<Language>, rope: &Rope) -> Self;          // full parse
    pub fn edit(&mut self, edit: &InputEdit);                        // call per EditDelta, then:
    pub fn reparse(&mut self, rope: &Rope);                         // incremental reparse using old tree
    pub fn highlights(&self, rope: &Rope, byte_range: Range<usize>) -> Vec<HighlightSpan>; // sorted, non-overlapping, only visible range
    pub fn fold_ranges(&self) -> Vec<Range<usize>>;                 // multi-line node row ranges (start_row..end_row)
    pub fn outline(&self, rope: &Rope) -> Vec<OutlineItem>;         // OutlineItem { name, kind: String, line, depth }
    pub fn language(&self) -> &Arc<Language>;
}
pub struct HighlightSpan { pub range: Range<usize> /* bytes */, pub kind: HighlightKind }
pub enum HighlightKind { Keyword, Function, Type, String, Number, Comment, Constant, Variable, VariableBuiltin,
    Property, Operator, Punctuation, Attribute, Tag, Macro, Namespace, Label, Escape, Constructor, Embedded, Heading, Link, Emphasis, Strong }
```

## blur_lsp  (generic LSP 3.17 client)
```rust
pub struct ServerConfig { pub name: String, pub command: String, pub args: Vec<String>, pub env: HashMap<String,String>,
    pub languages: Vec<String> /* lsp language ids */, pub root_markers: Vec<String>,
    pub initialization_options: Option<serde_json::Value>, pub settings: Option<serde_json::Value> } // Serialize/Deserialize
pub fn builtin_server_presets() -> Vec<ServerConfig>; // rust-analyzer, gopls, basedpyright, typescript-language-server, clangd, lua-language-server... only used if executable found on PATH
pub struct LspManager;  // Clone (Arc inside)
impl LspManager {
    pub fn new(handle: tokio::runtime::Handle, configs: Vec<ServerConfig>) -> Self;
    pub fn set_configs(&self, configs: Vec<ServerConfig>);
    pub fn events(&self) -> futures::channel::mpsc::UnboundedReceiver<LspEvent>;  // multiple subscribers allowed
    pub fn client_for(&self, language_id: &str, root: &Path) -> Option<LspClient>;  // lazily starts (dedup per (server, root)), None if no config / exe missing
    pub fn running(&self) -> Vec<ServerInfo>;      // ServerInfo { name, root, pid, status: ServerStatus, started_at, capabilities: Option<ServerCapabilities>, languages, avg_latency_ms }
    pub fn restart(&self, name: &str); pub fn stop(&self, name: &str); pub fn shutdown_all(&self);
    pub fn wire_log(&self, name: &str) -> Vec<WireFrame>;  // last 200 JSON-RPC frames: WireFrame { direction: In|Out, method: Option<String>, id: Option<i64>, json: String, at }
}
pub struct LspClient; // Clone
impl LspClient {   // all async fns runtime-agnostic (await oneshot)
    pub fn name(&self) -> &str; pub fn capabilities(&self) -> Option<ServerCapabilities>;
    pub fn did_open(&self, uri: Url, language_id: &str, version: i32, text: String);
    pub fn did_change(&self, uri: Url, version: i32, changes: Vec<TextDocumentContentChangeEvent>); // incremental if server supports, caller may pass full text change
    pub fn did_save(&self, uri: Url, text: Option<String>); pub fn did_close(&self, uri: Url);
    pub async fn completion(&self, uri: Url, pos: Position) -> anyhow::Result<Vec<CompletionItem>>;
    pub async fn hover(&self, uri: Url, pos: Position) -> anyhow::Result<Option<Hover>>;
    pub async fn definition(&self, uri: Url, pos: Position) -> anyhow::Result<Vec<Location>>;
    pub async fn references(&self, uri: Url, pos: Position) -> anyhow::Result<Vec<Location>>;
    pub async fn rename(&self, uri: Url, pos: Position, new_name: String) -> anyhow::Result<Option<WorkspaceEdit>>;
    pub async fn code_actions(&self, uri: Url, range: Range, diagnostics: Vec<Diagnostic>) -> anyhow::Result<Vec<CodeActionOrCommand>>;
    pub async fn formatting(&self, uri: Url, tab_size: u32, insert_spaces: bool) -> anyhow::Result<Vec<TextEdit>>;
    pub async fn document_symbols(&self, uri: Url) -> anyhow::Result<Vec<DocumentSymbol or SymbolInformation flattened>>;
    pub async fn request_raw(&self, method: &str, params: Value) -> anyhow::Result<Value>;
}
pub enum LspEvent { Diagnostics { server: String, uri: Url, version: Option<i32>, diagnostics: Vec<Diagnostic> },
    Status { server: String, status: ServerStatus }, LogMessage { server, level, message }, ShowMessage { server, level, message },
    Progress { server, token, title, message, percentage }, }
```
Must handle: Content-Length framing, request ids/oneshot table, server->client requests (`workspace/configuration`,
`client/registerCapability` dynamic registration, `window/workDoneProgress/create`, `workspace/applyEdit` -> reply applied:false
for now but surface event), capability negotiation (send rich ClientCapabilities incl. utf-16 position encoding, incremental sync),
graceful shutdown (`shutdown` + `exit`, then kill after timeout), stderr captured to log.

## blur_dap  (generic DAP client)
Same transport style (Content-Length framing, seq/request_seq). `DebugAdapterConfig { name, command, args, env, languages }`,
`DapClient::spawn(handle, config) -> Result<DapClient>`, async: initialize, launch(Value), attach(Value), set_breakpoints(source_path,
Vec<SourceBreakpoint{line, condition, hit_condition, log_message}>), set_function_breakpoints, configuration_done, threads, stack_trace(thread),
scopes(frame), variables(ref), evaluate(expr, frame, context), continue_(thread), next, step_in, step_out, pause, disconnect(terminate).
Events stream: `DapEvent::{Initialized, Stopped{reason, thread_id, ..}, Continued, Exited(code), Terminated, Output{category, output}, Breakpoint, Thread, Raw(Value)}`.
Launch configs: `LaunchConfig { name, adapter, request: launch|attach, args: Value }` loaded from `.blur/launch.toml`. Own the protocol
types you need (serde structs) — no external DAP crate.

## blur_git  (git2 + git CLI)
```rust
pub struct GitRepo; impl GitRepo {
    pub fn discover(path: &Path) -> anyhow::Result<Self>; pub fn root(&self) -> &Path;
    pub fn head(&self) -> HeadInfo;  // { branch: Option<String>, oid: Option<String>, detached: bool, upstream: Option<String>, ahead: usize, behind: usize }
    pub fn status(&self) -> anyhow::Result<Vec<FileStatus>>;  // { path, index: Option<ChangeKind>, worktree: Option<ChangeKind>, conflicted: bool }  ChangeKind {Added, Modified, Deleted, Renamed, Untracked, Typechange}
    pub fn log(&self, opts: LogOptions) -> anyhow::Result<Vec<CommitInfo>>;  // LogOptions { limit, all_branches, path: Option<PathBuf>, author/message filter: Option<String> }
       // CommitInfo { id, short_id, summary, body, author_name, author_email, time: i64 (unix), parents: Vec<String>, refs: Vec<RefLabel{name, kind: LocalBranch|RemoteBranch|Tag|Head}>, insertions: Option<usize>, deletions: Option<usize> }
    pub fn commit_stats(&self, id: &str) -> anyhow::Result<(usize, usize, Vec<FileStat>)>;
    pub fn diff_file(&self, path: &Path, staged: bool) -> anyhow::Result<FileDiff>;   // working tree vs index, or index vs HEAD
    pub fn diff_commit(&self, id: &str) -> anyhow::Result<Vec<FileDiff>>;
    pub fn diff_commits(&self, a: &str, b: &str) -> anyhow::Result<Vec<FileDiff>>;
       // FileDiff { path, old_path, status, hunks: Vec<Hunk>, binary, insertions, deletions }
       // Hunk { header, old_start, old_lines, new_start, new_lines, lines: Vec<DiffLine{kind: Context|Addition|Deletion, old_lineno, new_lineno, content}> }
    pub fn line_changes(&self, path: &Path, current_text: &str) -> Vec<LineChange>;  // gutter markers vs HEAD/index: {line, kind: Added|Modified|Deleted}
    pub fn stage_path(&self, p) / unstage_path / stage_all / unstage_all / discard_path;
    pub fn stage_hunk(&self, path, hunk_index) / unstage_hunk / discard_hunk;
    pub fn stage_lines(&self, path, hunk_index, line_indices: &[usize]);   // partial staging via patch apply to index
    pub fn commit(&self, message: &str, amend: bool) -> anyhow::Result<String>;  // uses git CLI if hooks/gpg configured; else git2
    pub fn commit_template(&self) -> Option<String>;  // commit.template
    pub fn branches(&self) -> anyhow::Result<Vec<BranchInfo>>;  // { name, is_head, is_remote, upstream, last_commit_summary, last_commit_time }
    pub fn create_branch(&self, name, from: Option<&str>, checkout: bool); pub fn checkout(&self, name_or_oid); pub fn delete_branch(&self, name);
    pub fn stashes(&self) -> Vec<StashEntry{index, message, oid}>; stash_save(message, include_untracked), stash_apply(i), stash_pop(i), stash_drop(i);
    pub fn blame(&self, path) -> anyhow::Result<Vec<BlameLine>>;  // per line: { line, commit_short, author, time, summary }
    pub fn file_history(&self, path, limit) -> Vec<CommitInfo>;
    pub fn remotes(&self) -> Vec<RemoteInfo{name, url}>; worktrees() -> Vec<WorktreeInfo{name, path, branch}>;
    pub fn conflicts(&self) -> Vec<PathBuf>;
    pub fn heatmap(&self, days: u32) -> Vec<(chrono::NaiveDate, usize)>;  // commits per day
    pub fn file_churn(&self, limit_commits: usize) -> Vec<(PathBuf, usize)>;
}
pub mod cli { // shells out to `git` in repo root, returns combined output or error with stderr
    fetch(root, remote: Option<&str>), pull(root), push(root, set_upstream: bool), merge(root, branch), rebase(root, onto),
    cherry_pick(root, oid), revert(root, oid), worktree_add/remove, remote_add/remove, abort_merge, abort_rebase, continue_rebase }
pub mod conflict { pub fn parse(text: &str) -> Vec<ConflictRegion>; // { start_line, end_line, ours: String, base: Option<String> (diff3), theirs: String, ours_label, theirs_label }
    pub fn resolve(text: &str, region_index: usize, resolution: Resolution /* Ours|Theirs|Both|Base|Custom(String) */) -> String; }
pub mod graph { pub fn layout(commits: &[CommitInfo]) -> Vec<GraphRow>;  // GraphRow { node_lane: usize, color: usize, lanes_before: Vec<Option<usize color>>, edges: Vec<GraphEdge{from_lane, to_lane, color}>, lane_count }
}
```

## blur_terminal  (portable-pty + alacritty_terminal grid)
```rust
pub struct TerminalOptions { pub shell: Option<String>, pub args: Vec<String>, pub cwd: Option<PathBuf>, pub env: Vec<(String,String)>, pub cols: u16, pub rows: u16, pub scrollback: usize }
pub struct Terminal;  // Send + Sync handle (Arc inside)
impl Terminal {
    pub fn spawn(opts: TerminalOptions, on_update: impl Fn() + Send + Sync + 'static) -> anyhow::Result<Self>; // reader thread feeds vte parser into alacritty_terminal::Term; on_update called (coalesced) after output
    pub fn write(&self, bytes: &[u8]); pub fn paste(&self, text: &str) /* bracketed paste aware */;
    pub fn resize(&self, cols: u16, rows: u16);
    pub fn snapshot(&self) -> TerminalSnapshot;  // { cols, rows, lines: Vec<Vec<TermCell>>, cursor: Option<(usize,usize)>, title: Option<String>, display_offset }
       // TermCell { ch: char, fg: TermColor, bg: TermColor, bold, italic, underline, inverse, dim, strikeout, wide_spacer: bool }
       // TermColor { Default, DefaultBg, Named(u8 /*0..16*/), Indexed(u8), Rgb(u8,u8,u8), Cursor }  — inverse already applied? NO: keep raw + `inverse` flag
    pub fn scroll(&self, delta_lines: i32); pub fn scroll_to_bottom(&self);
    pub fn is_alive(&self) -> bool; pub fn exit_code(&self) -> Option<i32>; pub fn pid(&self) -> Option<u32>;
    pub fn cwd(&self) -> Option<PathBuf>;  // linux: /proc/<pid>/cwd ; macOS: best effort None
    pub fn mode(&self) -> TermModeFlags { app_cursor: bool, bracketed_paste: bool, alt_screen: bool }
    pub fn kill(&self);
    pub fn selection_text(&self, start: (usize,usize), end: (usize,usize)) -> String;
}
pub fn encode_key(key: &str, ctrl: bool, alt: bool, shift: bool, app_cursor: bool) -> Option<Vec<u8>>; // key names as gpui Keystroke.key: "enter","backspace","tab","escape","up","down","left","right","home","end","pageup","pagedown","delete","insert","f1".."f12", or single chars
pub fn available_shells() -> Vec<String>;  // /etc/shells + $SHELL
pub struct TerminalProfile { pub name: String, pub shell: String, pub args: Vec<String>, pub env: Vec<(String,String)> } // Serialize/Deserialize
```

## blur_workspace  (project, files, search, watch, persistence, tasks)
```rust
pub struct Project { pub roots: Vec<PathBuf> }  // add_root/remove_root
pub fn list_dir(path: &Path, opts: &ListOptions) -> anyhow::Result<Vec<DirEntry>>; // ListOptions { show_hidden, respect_gitignore }; DirEntry { name, path, is_dir, is_symlink, size }; dirs first, case-insensitive
pub struct FileIndex; impl FileIndex { pub fn scan(roots: &[PathBuf]) -> Self; pub fn files(&self) -> &[IndexedFile{root_idx, rel_path, abs_path}];
    pub fn fuzzy(&self, query: &str, limit: usize) -> Vec<FuzzyMatch>; } // FuzzyMatch { abs_path, rel_path, score: u32, indices: Vec<usize> } (nucleo-matcher, path-aware)
pub struct SearchQuery { pub pattern: String, pub regex: bool, pub case_sensitive: bool, pub whole_word: bool, pub include: Vec<String>, pub exclude: Vec<String>, pub max_results: usize }
pub fn search(roots: &[PathBuf], q: &SearchQuery, cancel: &AtomicBool) -> anyhow::Result<SearchResults>; // parallel via ignore::WalkParallel; SearchResults { files: Vec<FileMatches{path, matches: Vec<LineMatch{line: usize (0-based), text: String (trimmed to 300 chars), ranges: Vec<Range<usize>> byte ranges in text}>}>, total_matches, truncated, elapsed_ms }
pub fn replace_in_file(path, q: &SearchQuery, replacement: &str) -> anyhow::Result<usize>;
pub struct FileWatcher; impl FileWatcher { pub fn new(roots: &[PathBuf], on_events: impl Fn(Vec<FsEvent>) + Send + 'static) -> anyhow::Result<Self>; } // debounced 100ms; FsEvent { path, kind: Created|Modified|Removed|Renamed }; ignores .git/objects, target/, node_modules/
pub mod persistence {
    pub struct WorkspaceState { pub roots, pub open_files: Vec<OpenFile{path, line, col, scroll_line}>, pub active_file: Option<PathBuf>, pub layout: LayoutState, pub profile: Option<String> }
    pub struct LayoutState { pub sidebar_visible: bool, pub sidebar_width: f32, pub panel_visible: bool, pub panel_height: f32, pub active_view: String, pub editor_groups: Vec<Vec<PathBuf>> }
    pub fn load(root: &Path) -> Option<WorkspaceState>; pub fn save(root: &Path, s: &WorkspaceState) -> anyhow::Result<()>; // data_dir()/blur/workspaces/<stable-hash>.json
    pub fn snapshot(root, name, s) / list_snapshots(root) / load_snapshot(root, name);
    pub fn recent_workspaces() -> Vec<PathBuf>; pub fn push_recent(root);
}
pub mod tasks {
    pub struct TaskDef { pub label, pub command, pub args, pub cwd: Option<PathBuf>, pub env, pub group: TaskGroup /*Build|Test|Run|Other*/, pub problem_matcher: Option<String> }
    pub fn load(root: &Path) -> Vec<TaskDef>;      // .blur/tasks.toml  [[task]]
    pub fn detect(root: &Path) -> Vec<TaskDef>;    // Cargo.toml -> cargo build/test/run/clippy; go.mod; package.json scripts; pyproject/pytest; Makefile targets
    pub struct TaskRunner; // spawn(def) -> TaskHandle { id, output lines via std::sync::mpsc or callback, kill(), status() }
    pub mod tests { pub fn parse_cargo_test(output: &str) -> Vec<TestResult{name, status: Passed|Failed|Ignored, duration}>; pub fn discover_rust_tests(root) -> Vec<String> (via `cargo test -- --list` lazily) }
}
```

## blur_core  (config, commands, keymap model, events, services)
```rust
pub mod paths { config_dir() (~/.config/blur), data_dir(), user_config_file(), workspace_config_file(root) (.blur/config.toml), themes_dir(), extensions_dir(), keymap_file() }
pub mod settings {
    #[derive(Serialize, Deserialize, Clone, Debug)] #[serde(default)]
    pub struct Settings { pub editor: EditorSettings, pub ui: UiSettings, pub terminal: TerminalSettings, pub git: GitSettings,
        pub lsp: LspSettings /* servers: BTreeMap<String, LspServerEntry{command, args, languages, root_markers, initialization_options: Option<toml::Value>, enabled}> */,
        pub files: FileSettings /* exclude globs, watch, show_hidden */, pub extensions: ExtensionSettings }
    EditorSettings { font_family: "JetBrains Mono", font_size: 13.5, line_height: 1.65, tab_size: 4, insert_spaces: true,
        trim_trailing_whitespace: true, format_on_save: false, relative_line_numbers: false, cursor_blink: true, soft_wrap: false,
        navigation_mode: NavigationMode /* Standard|Vim|Emacs|Helix */, auto_pairs: true, show_whitespace: false, max_column: Option<u32>, inlay_hints: bool }
    UiSettings { theme: "Furusato Slate", ui_font_family: "Geist", ui_font_size: 13.0, density: Compact|Comfortable, show_minimap: false, zen_mode: false }
    pub struct ConfigStore { /* defaults <- user <- workspace layered */ }
    impl ConfigStore { pub fn load(workspace_root: Option<&Path>) -> Self; pub fn settings(&self) -> &Settings; pub fn reload(&mut self) -> anyhow::Result<()>;
        pub fn user_toml(&self) -> String; pub fn effective_toml(&self) -> String; pub fn set_user_value(&mut self, dotted_key: &str, value: toml::Value) -> anyhow::Result<()> /* writes file */;
        pub fn errors(&self) -> &[String]; }
    // deep-merge toml tables; unknown keys preserved & ignored; parse errors reported, never panic
}
pub mod commands { pub struct CommandMeta { pub id: &'static str or String, pub title: String, pub category: String, pub description: Option<String> }
    pub struct CommandRegistry; register(meta), get(id), all(), fuzzy_search(query) -> Vec<(CommandMeta, score, indices)> }
pub mod keymap { pub enum NavigationMode; pub struct KeyBindingSpec { pub keys: String /* gpui syntax "ctrl-s", "g d" chords */, pub command: String, pub context: Option<String> }
    pub fn default_bindings(mode: NavigationMode) -> Vec<KeyBindingSpec>;  // Standard: ctrl-based (secondary-), Emacs: ctrl-x ctrl-s etc
    pub fn load_user_keymap(path) -> anyhow::Result<Vec<KeyBindingSpec>>; // keymap.toml [[binding]]
    pub fn display_keys(keys: &str, mac: bool) -> String;  // "ctrl-shift-p" -> "Ctrl+Shift+P" or "⌃⇧P"
}
pub mod events { pub struct EventBus<E: Clone + Send> — subscribe() -> std::sync::mpsc::Receiver<E> / callbacks; publish(e) }
pub mod services { pub struct ServiceRegistry — tracks named lazily started services: register(name, description), mark_started/stopped, list() -> Vec<ServiceStatus{name, running, started_at}> }
```

## blur_extensions  (Lua via mlua)
- `extension.toml` manifest: `id, name, version, description, author, main = "init.lua", permissions = ["commands","keymap","ui","workspace.read","fs.read","fs.write","process","network","themes"]`.
- `ExtensionHost::new(extensions_dirs: Vec<PathBuf>, granted: HashMap<id, Vec<Permission>>)`; `discover() -> Vec<ExtensionManifest + dir + enabled>`;
  `load_all()`, `load(id)`, `unload(id)`; Lua is sandboxed: no `os.execute`, `io`, `package.loadlib`, `debug`; capabilities exposed only if permission granted.
- Lua API table `blur`: `blur.register_command{ id, title, category, run = function(ctx) end }`, `blur.bind(keys, command_id)`,
  `blur.notify(msg, level)`, `blur.log(msg)`, `blur.register_panel{ id, title, render = function() return { lines | items } end }`,
  `blur.register_theme{ name, base, ... }`, `blur.workspace.root()`, `blur.editor.active_text()/selection()/insert(text)`,
  `blur.fs.read(path)/write(path, s)` (perm), `blur.process.run(cmd, args)` (perm), `blur.on(event, fn)` for events
  (`file_opened`, `file_saved`, `workspace_opened`).
- Host interaction uses a request queue: Lua calls push `HostRequest` (Notify, InsertText, OpenFile, RegisterCommand, RegisterPanel,
  RegisterTheme, Bind, ...) that the app drains with `host.drain_requests()`. The app provides context via `host.set_context(EditorContext{ active_path, active_text, selection })`.
  `host.run_command(id) -> Result<()>`, `host.render_panel(id) -> Result<PanelContent>`, `host.emit(event, payload)`.
- `install::from_dir(src, extensions_dir)`, `install::from_git(url, extensions_dir)` (git CLI clone --depth 1), `install::remove(id)`.

## blur_utils  (developer utilities; pure functions)
`http::send(HttpRequest{method, url, headers, body, timeout}) -> Result<HttpResponse{status, status_text, headers, body: String, size, duration_ms}>` (ureq, blocking; caller runs it on a background thread),
`json::{format(s, indent), minify, validate -> Result<(), JsonError{line, col, message}>, query(path like $.a.b[0])}`, `regex_tester::run(pattern, flags, text) -> Result<Vec<RegexMatch{range, text, groups: Vec<Option<(name, range, text)>>}>>`,
`encoding::{base64_encode/decode (standard | url_safe), url_encode/decode, hex}`, `ids::{uuid_v4, uuid_v7}`, `time::{parse_any(input) -> TimestampInfo{unix_s, unix_ms, rfc3339_utc, local, relative}}`,
`color::{parse(any css-ish hex/rgb()/hsl()) -> Color, to_hex, to_rgb_string, to_hsl_string, contrast_ratio}`, `diff::{line_diff(a, b) -> Vec<DiffOp{kind, old_line, new_line, text}>, unified(a, b, ctx)}`,
`sql::{SqlConnection{kind: Sqlite|Postgres|MySql, target}, run(conn, query) -> Result<SqlResult{columns, rows, elapsed_ms}>}` via external CLIs (`sqlite3 -json`, `psql --csv`, `mysql --batch`).

## blur_viewers  (markdown/data parsing for native rendering)
`markdown::parse(src) -> Vec<Block>` (GFM: headings, paragraphs, lists w/ task items, code blocks with lang, block quotes, tables w/ alignment,
rules, footnotes optional, `Block::Math(String)` for `$$`, `Block::Mermaid(String)` for ```mermaid); `Inline { text, bold, italic, code, strike, link: Option<String>, math: bool }`.
`math::to_unicode(tex) -> String` (KaTeX-compatible subset: greek, operators, ^/_ to super/subscripts, \frac a b -> a⁄b, \sqrt, \sum…).
`mermaid::parse_flowchart(src) -> Result<Flowchart{direction, nodes: Vec<Node{id, label, shape}>, edges: Vec<Edge{from, to, label, style}>}>`,
`mermaid::layout(&Flowchart) -> FlowLayout{ nodes: Vec<(id, x, y, w, h)>, edges: Vec<Vec<(x,y)>> , width, height }` (layered Sugiyama-lite),
also `sequenceDiagram` parse -> participants/messages.
`data::{parse_json/parse_yaml/parse_toml(src) -> Result<DataNode>}` (DataNode { key, kind: Object|Array|String|Number|Bool|Null, value: Option<String>, children }),
`csv::parse(src, delimiter) -> Result<Table{headers, rows}>`.
