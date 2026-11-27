use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use blur_core::commands::{CommandMeta, CommandRegistry};
use blur_core::events::EventBus;
use blur_core::keymap::{
    default_bindings_for_platform, display_keys, load_user_keymap, KeyBindingSpec, NavigationMode,
};
use blur_core::services::ServiceRegistry;
use blur_core::settings::{ConfigStore, Settings, UiDensity};

struct TempDir(PathBuf);

impl TempDir {
    fn new(prefix: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "blur_core_test_{prefix}_{}_{}",
            std::process::id(),
            id
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn default_toml_roundtrips_to_default_settings() {
    let raw = Settings::default_toml();
    let parsed: Settings = toml::from_str(&raw).expect("default_toml must parse cleanly");
    assert_eq!(parsed, Settings::default());
}

#[test]
fn config_layering_deep_merge_and_unknown_keys() {
    let tmp = TempDir::new("layering");
    let user_cfg = tmp.path().join("user.toml");
    let ws_cfg = tmp.path().join("workspace.toml");

    std::fs::write(
        &user_cfg,
        r#"
[editor]
font_size = 15.0
tab_size = 2

[ui]
theme = "Nord"
density = "compact"

[lsp.servers.rust-analyzer]
command = "rust-analyzer"
languages = ["rust"]
root_markers = ["Cargo.toml"]
initialization_options = { check = { command = "clippy" } }

[custom_unknown]
foo = "bar"
"#,
    )
    .unwrap();

    std::fs::write(
        &ws_cfg,
        r#"
[editor]
tab_size = 8
format_on_save = true

[lsp.servers.rust-analyzer]
args = ["--log-file", "/tmp/ra.log"]
"#,
    )
    .unwrap();

    let store = ConfigStore::load_from_paths(user_cfg, Some(ws_cfg));
    assert!(store.errors().is_empty(), "errors: {:?}", store.errors());

    let s = store.settings();
    // From user layer (preserved through workspace merge):
    assert_eq!(s.editor.font_size, 15.0);
    assert_eq!(s.ui.theme, "Nord");
    assert_eq!(s.ui.density, UiDensity::Compact);
    // Overridden by workspace layer:
    assert_eq!(s.editor.tab_size, 8);
    assert!(s.editor.format_on_save);
    // Still default:
    assert_eq!(s.editor.font_family, "JetBrains Mono");

    // Deep-merged LSP server entry:
    let ra = s
        .lsp
        .servers
        .get("rust-analyzer")
        .expect("rust-analyzer entry");
    assert_eq!(ra.command, "rust-analyzer");
    assert_eq!(ra.args, vec!["--log-file", "/tmp/ra.log"]);
    assert_eq!(ra.languages, vec!["rust"]);
    assert!(ra.enabled);
    let init_json = ra.initialization_options_json().expect("init json");
    assert_eq!(init_json["check"]["command"], "clippy");

    // Unknown key preserved in effective TOML:
    assert!(store.effective_toml().contains("custom_unknown"));
}

#[test]
fn config_parse_errors_collected_without_panic() {
    let tmp = TempDir::new("errors");
    let user_cfg = tmp.path().join("user.toml");
    let ws_cfg = tmp.path().join("workspace.toml");

    std::fs::write(&user_cfg, "this is = [not valid toml").unwrap();
    std::fs::write(
        &ws_cfg,
        r#"
[editor]
tab_size = 2
"#,
    )
    .unwrap();

    let mut store = ConfigStore::load_from_paths(user_cfg.clone(), Some(ws_cfg));
    assert_eq!(store.errors().len(), 1);
    // Workspace layer still applied on top of defaults:
    assert_eq!(store.settings().editor.tab_size, 2);
    assert_eq!(store.settings().editor.font_size, 13.5);

    // Now test invalid type preserving previous valid settings on reload:
    std::fs::write(
        &user_cfg,
        r#"
[editor]
font_size = "not-a-float"
"#,
    )
    .unwrap();
    store.reload().unwrap();
    assert!(!store.errors().is_empty());
    // Previous valid settings preserved:
    assert_eq!(store.settings().editor.tab_size, 2);
}

#[test]
fn set_user_value_roundtrip_preserves_other_keys() {
    let tmp = TempDir::new("set_user");
    let user_cfg = tmp.path().join("nested").join("config.toml");

    let mut store = ConfigStore::load_from_paths(user_cfg.clone(), None);
    store
        .set_user_value("editor.font_size", toml::Value::Float(16.0))
        .unwrap();
    store
        .set_user_value("ui.theme", toml::Value::String("Tokyo Night".into()))
        .unwrap();
    store
        .set_user_value("editor.tab_size", toml::Value::Integer(2))
        .unwrap();

    assert!(store.errors().is_empty());
    assert_eq!(store.settings().editor.font_size, 16.0);
    assert_eq!(store.settings().editor.tab_size, 2);
    assert_eq!(store.settings().ui.theme, "Tokyo Night");

    // Reload fresh from disk to verify persistence:
    let reloaded = ConfigStore::load_from_paths(user_cfg, None);
    assert_eq!(reloaded.settings().editor.font_size, 16.0);
    assert_eq!(reloaded.settings().editor.tab_size, 2);
    assert_eq!(reloaded.settings().ui.theme, "Tokyo Night");
}

#[test]
fn command_registry_defaults_and_fuzzy_search() {
    let reg = CommandRegistry::with_defaults();
    let required = [
        "workspace.command_palette",
        "workspace.file_finder",
        "workspace.project_search",
        "workspace.toggle_sidebar",
        "workspace.toggle_panel",
        "workspace.toggle_terminal",
        "workspace.new_terminal",
        "workspace.open_folder",
        "workspace.open_settings",
        "workspace.theme_selector",
        "workspace.zen_mode",
        "workspace.view_editor",
        "workspace.view_git",
        "workspace.view_lsp",
        "workspace.view_utilities",
        "workspace.view_settings",
        "workspace.split_right",
        "workspace.split_down",
        "workspace.close_group",
        "workspace.focus_next_group",
        "workspace.quit",
        "editor.save",
        "editor.save_all",
        "editor.close_tab",
        "editor.next_tab",
        "editor.prev_tab",
        "editor.undo",
        "editor.redo",
        "editor.cut",
        "editor.copy",
        "editor.paste",
        "editor.select_all",
        "editor.find",
        "editor.replace",
        "editor.find_next",
        "editor.find_prev",
        "editor.goto_line",
        "editor.goto_symbol",
        "editor.goto_definition",
        "editor.find_references",
        "editor.rename_symbol",
        "editor.code_actions",
        "editor.format",
        "editor.hover",
        "editor.trigger_completion",
        "editor.toggle_comment",
        "editor.duplicate_line",
        "editor.move_line_up",
        "editor.move_line_down",
        "editor.delete_line",
        "editor.add_cursor_above",
        "editor.add_cursor_below",
        "editor.select_next_occurrence",
        "editor.fold",
        "editor.unfold",
        "editor.indent",
        "editor.outdent",
        "editor.markdown_preview",
        "git.stage_hunk",
        "git.commit",
        "git.fetch",
        "git.pull",
        "git.push",
        "git.graph",
        "git.stash",
        "git.blame",
        "task.run",
        "task.run_test",
    ];
    for id in required {
        assert!(reg.get(id).is_some(), "missing canonical command: {id}");
    }

    let results = reg.fuzzy_search("cmdpal");
    assert!(!results.is_empty());
    assert_eq!(results[0].0.id, "workspace.command_palette");
    assert!(!results[0].2.is_empty());

    let mut custom = reg;
    custom.register(CommandMeta::new("custom.hello", "Say Hello", "Custom"));
    assert!(custom.get("custom.hello").is_some());
}

#[test]
fn keymap_display_modes_and_user_file() {
    assert_eq!(display_keys("ctrl-shift-p", false), "Ctrl+Shift+P");
    assert_eq!(display_keys("ctrl-shift-p", true), "⌃⇧P");
    assert_eq!(display_keys("cmd-shift-p", true), "⌘⇧P");
    assert_eq!(display_keys("ctrl-k ctrl-s", false), "Ctrl+K Ctrl+S");
    assert_eq!(display_keys("ctrl-x ctrl-s", true), "⌃X ⌃S");
    assert_eq!(display_keys("ctrl--", false), "Ctrl+-");

    let std_linux = default_bindings_for_platform(NavigationMode::Standard, false);
    assert!(std_linux
        .iter()
        .any(|b| b.keys == "ctrl-shift-p" && b.command == "workspace.command_palette"));

    let std_mac = default_bindings_for_platform(NavigationMode::Standard, true);
    assert!(std_mac
        .iter()
        .any(|b| b.keys == "cmd-shift-p" && b.command == "workspace.command_palette"));

    let emacs = default_bindings_for_platform(NavigationMode::Emacs, false);
    assert!(emacs
        .iter()
        .any(|b| b.keys == "ctrl-x ctrl-s" && b.command == "editor.save"));
    assert!(emacs
        .iter()
        .any(|b| b.keys == "alt-x" && b.command == "workspace.command_palette"));

    let vim = default_bindings_for_platform(NavigationMode::Vim, false);
    assert!(vim.iter().any(|b| b.command == "vim.normal_mode"));

    let tmp = TempDir::new("keymap");
    let km_path = tmp.path().join("keymap.toml");
    std::fs::write(
        &km_path,
        r#"
[[binding]]
keys = "ctrl-alt-s"
command = "editor.save_all"

[[binding]]
keys = "g d"
command = "editor.goto_definition"
context = "Editor"
"#,
    )
    .unwrap();
    let user_km = load_user_keymap(&km_path).unwrap();
    assert_eq!(
        user_km,
        vec![
            KeyBindingSpec::new("ctrl-alt-s", "editor.save_all"),
            KeyBindingSpec::with_context("g d", "editor.goto_definition", "Editor"),
        ]
    );
}

#[test]
fn event_bus_and_service_registry() {
    let bus: EventBus<String> = EventBus::new();
    let rx = bus.subscribe();
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_clone = Arc::clone(&hits);
    let sub_id = bus.on(move |msg| {
        if msg == "ping" {
            hits_clone.fetch_add(1, Ordering::SeqCst);
        }
    });

    bus.publish("ping".to_string());
    assert_eq!(rx.recv().unwrap(), "ping");
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    bus.unsubscribe(sub_id);
    bus.publish("ping".to_string());
    assert_eq!(rx.recv().unwrap(), "ping");
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    let reg = ServiceRegistry::new();
    reg.register("lsp", "Language Server Protocol manager");
    assert!(!reg.is_running("lsp"));
    reg.mark_started("lsp");
    assert!(reg.is_running("lsp"));
    let list = reg.list();
    assert_eq!(list.len(), 1);
    assert!(list[0].started_at.is_some());
    reg.mark_stopped("lsp");
    assert!(!reg.is_running("lsp"));
    assert!(reg.get("lsp").unwrap().started_at.is_none());
}
