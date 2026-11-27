use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::Duration;

use blur_workspace::persistence::{self, LayoutState, OpenFile, WorkspaceState};
use blur_workspace::tasks::{self, tests as task_tests, TaskDef, TaskEvent, TaskGroup, TaskRunner, TaskStatus};
use blur_workspace::{
    list_dir, replace_in_file, search, FileIndex, FileWatcher, ListOptions, Project, SearchQuery,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new(prefix: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "blur_ws_test_{prefix}_{}_{}",
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
fn project_and_list_dir_ordering() {
    let tmp = TempDir::new("listdir");
    std::fs::create_dir_all(tmp.path().join("zeta")).unwrap();
    std::fs::create_dir_all(tmp.path().join("Alpha")).unwrap();
    std::fs::write(tmp.path().join("beta.txt"), "hello").unwrap();
    std::fs::write(tmp.path().join("alpha.rs"), "fn main() {}").unwrap();
    std::fs::write(tmp.path().join(".hidden"), "secret").unwrap();

    let mut proj = Project::default();
    proj.add_root(tmp.path().to_path_buf());
    proj.add_root(tmp.path().to_path_buf()); // dedup
    assert_eq!(proj.roots.len(), 1);

    let entries = list_dir(tmp.path(), &ListOptions::default()).unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["Alpha", "zeta", "alpha.rs", "beta.txt"]);

    let hidden_entries = list_dir(
        tmp.path(),
        &ListOptions {
            show_hidden: true,
            respect_gitignore: true,
        },
    )
    .unwrap();
    assert!(hidden_entries.iter().any(|e| e.name == ".hidden"));

    assert!(proj.remove_root(tmp.path()));
    assert!(proj.roots.is_empty());
}

#[test]
fn file_index_and_fuzzy_ranking() {
    let tmp = TempDir::new("fuzzy");
    std::fs::create_dir_all(tmp.path().join("src/nested/deep")).unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    std::fs::write(tmp.path().join(".gitignore"), "ignored.log\n").unwrap();
    std::fs::write(tmp.path().join("ignored.log"), "skip me").unwrap();
    std::fs::write(tmp.path().join(".git/HEAD"), "ref: refs/heads/main").unwrap();
    std::fs::write(tmp.path().join("src/main.rs"), "fn main() {}").unwrap();
    std::fs::write(tmp.path().join("src/nested/deep/main_helper.rs"), "").unwrap();
    std::fs::write(tmp.path().join("Cargo.toml"), "[package]").unwrap();

    let index = FileIndex::scan(&[tmp.path().to_path_buf()]);
    let rels: Vec<String> = index
        .files()
        .iter()
        .map(|f| f.rel_path.to_string_lossy().into_owned())
        .collect();
    assert!(!rels.iter().any(|p| p.contains("ignored.log")));
    assert!(!rels.iter().any(|p| p.contains(".git")));

    let matches = index.fuzzy("main.rs", 10);
    assert!(!matches.is_empty());
    assert_eq!(matches[0].rel_path, PathBuf::from("src/main.rs"));
    assert!(!matches[0].indices.is_empty());
}

#[test]
fn search_regex_case_whole_word_globs_binary_and_replace() {
    let tmp = TempDir::new("search");
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::create_dir_all(tmp.path().join("docs")).unwrap();
    std::fs::write(
        tmp.path().join("src/lib.rs"),
        "fn foo() {}\nfn foobar() {}\nfn FOO() {}\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("docs/readme.md"), "foo in docs\n").unwrap();
    // Binary file containing NUL byte
    std::fs::write(tmp.path().join("src/bin.dat"), b"foo\0bar\0baz").unwrap();

    let cancel = AtomicBool::new(false);

    // Case-insensitive whole word, include only *.rs
    let q = SearchQuery {
        pattern: "foo".into(),
        regex: false,
        case_sensitive: false,
        whole_word: true,
        include: vec!["*.rs".into()],
        exclude: vec![],
        max_results: 100,
    };
    let res = search(&[tmp.path().to_path_buf()], &q, &cancel).unwrap();
    assert_eq!(res.files.len(), 1);
    assert_eq!(res.total_matches, 2); // `foo` and `FOO`, not `foobar`
    assert_eq!(res.files[0].matches[0].line, 0);
    assert_eq!(res.files[0].matches[1].line, 2);

    // Case-sensitive regex across all files excluding docs
    let q_re = SearchQuery {
        pattern: r"fn\s+foo\w*".into(),
        regex: true,
        case_sensitive: true,
        whole_word: false,
        include: vec![],
        exclude: vec!["docs/**".into()],
        max_results: 100,
    };
    let res_re = search(&[tmp.path().to_path_buf()], &q_re, &cancel).unwrap();
    assert_eq!(res_re.total_matches, 2); // `fn foo` and `fn foobar`

    // Replace in file
    let q_rep = SearchQuery {
        pattern: "foo".into(),
        regex: false,
        case_sensitive: true,
        whole_word: true,
        ..Default::default()
    };
    let replaced = replace_in_file(&tmp.path().join("src/lib.rs"), &q_rep, "baz").unwrap();
    assert_eq!(replaced, 1);
    let updated = std::fs::read_to_string(tmp.path().join("src/lib.rs")).unwrap();
    assert!(updated.contains("fn baz() {}"));
    assert!(updated.contains("fn foobar() {}"));
}

#[test]
fn file_watcher_emits_debounced_events() {
    let tmp = TempDir::new("watch");
    let (tx, rx) = mpsc::channel();
    let _watcher = FileWatcher::new(&[tmp.path().to_path_buf()], move |events| {
        let _ = tx.send(events);
    })
    .unwrap();

    // Give notify a moment to arm watches, then create a file.
    std::thread::sleep(Duration::from_millis(50));
    let target_file = tmp.path().join("hello.txt");
    std::fs::write(&target_file, "world").unwrap();

    let batch = rx
        .recv_timeout(Duration::from_secs(3))
        .expect("expected watcher event batch");
    assert!(
        batch.iter().any(|e| e.path.ends_with("hello.txt")),
        "unexpected batch: {batch:?}"
    );
}

#[test]
fn persistence_roundtrip_snapshots_and_recents() {
    static ENV_LOCK: Mutex<()> = Mutex::new(());
    let _guard = ENV_LOCK.lock().unwrap();

    let data_tmp = TempDir::new("data_dir");
    let ws_tmp = TempDir::new("workspace_root");
    std::env::set_var("BLUR_DATA_DIR", data_tmp.path());

    let state = WorkspaceState {
        roots: vec![ws_tmp.path().to_path_buf()],
        open_files: vec![OpenFile {
            path: ws_tmp.path().join("src/main.rs"),
            line: 42,
            col: 8,
            scroll_line: 20,
        }],
        active_file: Some(ws_tmp.path().join("src/main.rs")),
        layout: LayoutState {
            sidebar_visible: true,
            sidebar_width: 280.0,
            panel_visible: true,
            panel_height: 200.0,
            active_view: "editor".into(),
            editor_groups: vec![vec![ws_tmp.path().join("src/main.rs")]],
        },
        profile: Some("rust".into()),
    };

    persistence::save(ws_tmp.path(), &state).unwrap();
    let loaded = persistence::load(ws_tmp.path()).expect("saved workspace state");
    assert_eq!(loaded, state);

    persistence::snapshot(ws_tmp.path(), "before-refactor", &state).unwrap();
    assert_eq!(
        persistence::list_snapshots(ws_tmp.path()),
        vec!["before-refactor".to_string()]
    );
    assert_eq!(
        persistence::load_snapshot(ws_tmp.path(), "before-refactor"),
        Some(state)
    );

    persistence::push_recent(ws_tmp.path());
    persistence::push_recent(ws_tmp.path()); // dedup
    let recents = persistence::recent_workspaces();
    assert_eq!(recents.len(), 1);

    std::env::remove_var("BLUR_DATA_DIR");
}

#[test]
fn task_loading_detection_runner_and_cargo_test_parser() {
    let tmp = TempDir::new("tasks");
    std::fs::create_dir_all(tmp.path().join(".blur")).unwrap();
    std::fs::write(
        tmp.path().join(".blur/tasks.toml"),
        r#"
[[task]]
label = "custom check"
command = "echo"
args = ["ok"]
group = "test"
"#,
    )
    .unwrap();
    std::fs::write(tmp.path().join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
    std::fs::write(tmp.path().join("go.mod"), "module example.com/demo\n").unwrap();
    std::fs::write(
        tmp.path().join("package.json"),
        r#"{"scripts": {"build": "vite build", "test": "vitest"}}"#,
    )
    .unwrap();
    std::fs::write(tmp.path().join("pyproject.toml"), "[project]\nname = \"demo\"\n").unwrap();
    std::fs::write(
        tmp.path().join("Makefile"),
        ".PHONY: all clean\nall:\n\techo all\nclean:\n\trm -rf dist\n",
    )
    .unwrap();

    let loaded = tasks::load(tmp.path());
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].label, "custom check");
    assert_eq!(loaded[0].group, TaskGroup::Test);

    let detected = tasks::detect(tmp.path());
    let labels: Vec<&str> = detected.iter().map(|t| t.label.as_str()).collect();
    assert!(labels.contains(&"cargo build"));
    assert!(labels.contains(&"cargo test"));
    assert!(labels.contains(&"go test"));
    assert!(labels.contains(&"npm: build"));
    assert!(labels.contains(&"npm: test"));
    assert!(labels.contains(&"pytest"));
    assert!(labels.contains(&"make all"));
    assert!(labels.contains(&"make clean"));

    // Run a real shell command via TaskRunner
    let runner = TaskRunner::new();
    let handle = runner
        .spawn(&TaskDef {
            label: "echo".into(),
            command: "sh".into(),
            args: vec!["-c".into(), "echo hello_out; echo hello_err >&2".into()],
            cwd: Some(tmp.path().to_path_buf()),
            env: BTreeMap::new(),
            group: TaskGroup::Run,
            problem_matcher: None,
        })
        .unwrap();

    let mut saw_out = false;
    let mut saw_err = false;
    let mut exit_code = None;
    for ev in handle.receiver().iter() {
        match ev {
            TaskEvent::Output(o) if !o.is_stderr && o.line == "hello_out" => saw_out = true,
            TaskEvent::Output(o) if o.is_stderr && o.line == "hello_err" => saw_err = true,
            TaskEvent::Finished(f) => {
                exit_code = f.code;
                break;
            }
            _ => {}
        }
    }
    assert!(saw_out);
    assert!(saw_err);
    assert_eq!(exit_code, Some(0));
    assert_eq!(handle.status(), TaskStatus::Exited(Some(0)));

    // Kill support on a long-running task
    let sleeper = runner
        .spawn(&TaskDef {
            label: "sleep".into(),
            command: "sh".into(),
            args: vec!["-c".into(), "sleep 10".into()],
            cwd: Some(tmp.path().to_path_buf()),
            env: BTreeMap::new(),
            group: TaskGroup::Run,
            problem_matcher: None,
        })
        .unwrap();
    sleeper.kill();
    let mut finished = false;
    for ev in sleeper.receiver().iter() {
        if matches!(ev, TaskEvent::Finished(_)) {
            finished = true;
            break;
        }
    }
    assert!(finished);
    assert_eq!(sleeper.status(), TaskStatus::Killed);

    // Cargo test output parser
    let sample = r#"
running 4 tests
test alpha::passing ... ok
test beta::failing ... FAILED
test gamma::skipped ... ignored
test delta::timed ... ok <0.042s>
test result: FAILED. 2 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out
"#;
    let parsed = task_tests::parse_cargo_test(sample);
    assert_eq!(parsed.len(), 4);
    assert_eq!(parsed[0].name, "alpha::passing");
    assert_eq!(parsed[0].status, task_tests::TestStatus::Passed);
    assert_eq!(parsed[1].status, task_tests::TestStatus::Failed);
    assert_eq!(parsed[2].status, task_tests::TestStatus::Ignored);
    assert_eq!(parsed[3].status, task_tests::TestStatus::Passed);
    assert!(parsed[3].duration.is_some());
}
