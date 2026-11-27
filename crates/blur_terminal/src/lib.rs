//! GPUI-free terminal emulator backend for Blur.
//!
//! Spawns a PTY via `portable-pty` and maintains a VT100/xterm screen grid via
//! `alacritty_terminal`. Callers receive coalesced update notifications and can
//! take lightweight viewport snapshots on each UI frame.

mod keys;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{
    point_to_viewport, viewport_to_point, Config as TermConfig, Term, TermMode, MIN_COLUMNS,
    MIN_SCREEN_LINES,
};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Processor, Rgb};
use anyhow::{Context, Result};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use tracing::{debug, trace, warn};

pub use keys::encode_key;

/// Options for spawning a new [`Terminal`].
#[derive(Debug, Clone)]
pub struct TerminalOptions {
    /// Shell executable path. If `None`, uses `$SHELL` or `/bin/sh`.
    pub shell: Option<String>,
    /// Arguments passed to the shell.
    pub args: Vec<String>,
    /// Initial working directory.
    pub cwd: Option<PathBuf>,
    /// Extra environment variables.
    pub env: Vec<(String, String)>,
    /// Initial column count.
    pub cols: u16,
    /// Initial row count.
    pub rows: u16,
    /// Scrollback history length in lines.
    pub scrollback: usize,
}

impl Default for TerminalOptions {
    fn default() -> Self {
        Self {
            shell: None,
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            cols: 80,
            rows: 24,
            scrollback: 10_000,
        }
    }
}

/// Serializable terminal profile definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalProfile {
    pub name: String,
    pub shell: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// A single cell color in a [`TerminalSnapshot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermColor {
    Default,
    DefaultBg,
    Named(u8),
    Indexed(u8),
    Rgb(u8, u8, u8),
    Cursor,
}

/// A single rendered cell in a [`TerminalSnapshot`].
///
/// Colors are kept raw (not swapped for `inverse`); consumers should swap
/// `fg`/`bg` when `inverse` is true.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermCell {
    pub ch: char,
    pub fg: TermColor,
    pub bg: TermColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub dim: bool,
    pub strikeout: bool,
    pub wide_spacer: bool,
}

/// Immutable snapshot of the visible terminal viewport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSnapshot {
    pub cols: usize,
    pub rows: usize,
    pub lines: Vec<Vec<TermCell>>,
    /// Cursor `(row, col)` in viewport coordinates, or `None` if hidden or scrolled out of view.
    pub cursor: Option<(usize, usize)>,
    pub title: Option<String>,
    pub display_offset: usize,
}

/// Subset of terminal mode flags relevant to input and UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TermModeFlags {
    pub app_cursor: bool,
    pub bracketed_paste: bool,
    pub alt_screen: bool,
}

#[derive(Debug, Clone, Copy)]
struct TermDims {
    cols: usize,
    rows: usize,
}

impl Dimensions for TermDims {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

struct ListenerState {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    title: Arc<Mutex<Option<String>>>,
    exit_code: Arc<Mutex<Option<i32>>>,
    alive: Arc<AtomicBool>,
    notify_pending: Arc<AtomicBool>,
    on_update: Arc<dyn Fn() + Send + Sync>,
}

impl ListenerState {
    fn notify(&self) {
        if !self.notify_pending.swap(true, Ordering::AcqRel) {
            (self.on_update)();
        }
    }
}

#[derive(Clone)]
struct TermEventListener {
    state: Arc<ListenerState>,
}

impl EventListener for TermEventListener {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(text) => {
                let mut writer = self.state.writer.lock();
                if let Err(err) = writer.write_all(text.as_bytes()).and_then(|_| writer.flush()) {
                    trace!("pty write-back failed: {err}");
                }
            }
            Event::Title(title) => {
                *self.state.title.lock() = Some(title);
                self.state.notify();
            }
            Event::ResetTitle => {
                *self.state.title.lock() = None;
                self.state.notify();
            }
            Event::Bell => {
                trace!("terminal bell");
                self.state.notify();
            }
            Event::ChildExit(code) => {
                *self.state.exit_code.lock() = Some(code);
                self.state.alive.store(false, Ordering::Release);
                self.state.notify();
            }
            Event::Exit => {
                self.state.alive.store(false, Ordering::Release);
                self.state.notify();
            }
            Event::Wakeup => {
                self.state.notify();
            }
            _ => {}
        }
    }
}

struct TerminalInner {
    term: Mutex<Term<TermEventListener>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    title: Arc<Mutex<Option<String>>>,
    exit_code: Arc<Mutex<Option<i32>>>,
    alive: Arc<AtomicBool>,
    notify_pending: Arc<AtomicBool>,
    pid: Option<u32>,
}

/// Handle to a running PTY-backed terminal emulator.
///
/// Cheap to clone (`Arc` inside) and `Send + Sync`.
#[derive(Clone)]
pub struct Terminal {
    inner: Arc<TerminalInner>,
}

impl std::fmt::Debug for Terminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Terminal")
            .field("pid", &self.inner.pid)
            .field("alive", &self.is_alive())
            .finish()
    }
}

impl Terminal {
    /// Spawn a new terminal with the given options and coalesced update callback.
    pub fn spawn(
        opts: TerminalOptions,
        on_update: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self> {
        let cols = (opts.cols as usize).max(MIN_COLUMNS);
        let rows = (opts.rows as usize).max(MIN_SCREEN_LINES);

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: rows as u16,
                cols: cols as u16,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("failed to open PTY")?;

        let shell = opts
            .shell
            .filter(|s| !s.is_empty())
            .unwrap_or_else(default_shell);

        let mut cmd = CommandBuilder::new(&shell);
        for arg in &opts.args {
            cmd.arg(arg);
        }
        if let Some(cwd) = &opts.cwd {
            cmd.cwd(cwd);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        for (k, v) in &opts.env {
            cmd.env(k, v);
        }

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .with_context(|| format!("failed to spawn shell `{shell}`"))?;
        // Drop slave immediately so master receives EOF when child exits.
        drop(pair.slave);

        let pid = child.process_id();
        let killer = child.clone_killer();
        let mut reader = pair
            .master
            .try_clone_reader()
            .context("failed to clone PTY reader")?;
        let writer = pair
            .master
            .take_writer()
            .context("failed to take PTY writer")?;

        let writer = Arc::new(Mutex::new(writer));
        let title = Arc::new(Mutex::new(None));
        let exit_code = Arc::new(Mutex::new(None));
        let alive = Arc::new(AtomicBool::new(true));
        let notify_pending = Arc::new(AtomicBool::new(false));
        let on_update: Arc<dyn Fn() + Send + Sync> = Arc::new(on_update);

        let listener_state = Arc::new(ListenerState {
            writer: Arc::clone(&writer),
            title: Arc::clone(&title),
            exit_code: Arc::clone(&exit_code),
            alive: Arc::clone(&alive),
            notify_pending: Arc::clone(&notify_pending),
            on_update: Arc::clone(&on_update),
        });

        let listener = TermEventListener {
            state: Arc::clone(&listener_state),
        };

        let term_config = TermConfig {
            scrolling_history: opts.scrollback,
            ..TermConfig::default()
        };
        let dims = TermDims { cols, rows };
        let term = Term::new(term_config, &dims, listener);

        let inner = Arc::new(TerminalInner {
            term: Mutex::new(term),
            master: Mutex::new(pair.master),
            writer,
            killer: Mutex::new(killer),
            title,
            exit_code,
            alive,
            notify_pending,
            pid,
        });

        let reader_inner = Arc::clone(&inner);
        let reader_listener = Arc::clone(&listener_state);
        std::thread::Builder::new()
            .name("blur-pty-reader".to_string())
            .spawn(move || {
                let mut processor: Processor = Processor::new();
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => {
                            debug!("PTY reader reached EOF");
                            break;
                        }
                        Ok(n) => {
                            {
                                let mut term = reader_inner.term.lock();
                                processor.advance(&mut *term, &buf[..n]);
                            }
                            reader_listener.notify();
                        }
                        Err(err) => {
                            debug!("PTY reader stopped: {err}");
                            break;
                        }
                    }
                }

                // Reap child and capture exit code.
                let code = match child.wait() {
                    Ok(status) => Some(status.exit_code() as i32),
                    Err(err) => {
                        warn!("failed to wait on PTY child: {err}");
                        None
                    }
                };
                if let Some(c) = code {
                    *reader_inner.exit_code.lock() = Some(c);
                }
                reader_inner.alive.store(false, Ordering::Release);
                reader_listener.notify();
            })
            .context("failed to spawn PTY reader thread")?;

        Ok(Self { inner })
    }

    /// Write raw bytes to the PTY input.
    pub fn write(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut writer = self.inner.writer.lock();
        if let Err(err) = writer.write_all(bytes).and_then(|_| writer.flush()) {
            debug!("failed to write to PTY: {err}");
        }
    }

    /// Paste text into the PTY, wrapping in bracketed paste markers when active.
    pub fn paste(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.mode().bracketed_paste {
            let mut payload = Vec::with_capacity(text.len() + 12);
            payload.extend_from_slice(b"\x1b[200~");
            payload.extend_from_slice(text.as_bytes());
            payload.extend_from_slice(b"\x1b[201~");
            self.write(&payload);
        } else {
            self.write(text.as_bytes());
        }
    }

    /// Resize both the PTY and the terminal grid.
    pub fn resize(&self, cols: u16, rows: u16) {
        let cols = (cols as usize).max(MIN_COLUMNS);
        let rows = (rows as usize).max(MIN_SCREEN_LINES);

        {
            let master = self.inner.master.lock();
            if let Err(err) = master.resize(PtySize {
                rows: rows as u16,
                cols: cols as u16,
                pixel_width: 0,
                pixel_height: 0,
            }) {
                debug!("PTY resize failed: {err}");
            }
        }

        let dims = TermDims { cols, rows };
        self.inner.term.lock().resize(dims);
    }

    /// Take a snapshot of the visible terminal viewport.
    ///
    /// Clears the pending update notification flag so subsequent PTY output will
    /// trigger `on_update` again.
    pub fn snapshot(&self) -> TerminalSnapshot {
        self.inner.notify_pending.store(false, Ordering::Release);

        let term = self.inner.term.lock();
        let grid = term.grid();
        let cols = grid.columns();
        let rows = grid.screen_lines();
        let display_offset = grid.display_offset();

        let mut lines = Vec::with_capacity(rows);
        for row_idx in 0..rows {
            let line_idx = Line(row_idx as i32 - display_offset as i32);
            let grid_row = &grid[line_idx];
            let mut cells = Vec::with_capacity(cols);
            for col_idx in 0..cols {
                let cell = &grid_row[Column(col_idx)];
                let flags = cell.flags;
                cells.push(TermCell {
                    ch: cell.c,
                    fg: convert_color(cell.fg),
                    bg: convert_color(cell.bg),
                    bold: flags.contains(Flags::BOLD),
                    italic: flags.contains(Flags::ITALIC),
                    underline: flags.intersects(Flags::ALL_UNDERLINES),
                    inverse: flags.contains(Flags::INVERSE),
                    dim: flags.contains(Flags::DIM),
                    strikeout: flags.contains(Flags::STRIKEOUT),
                    wide_spacer: flags
                        .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER),
                });
            }
            lines.push(cells);
        }

        let cursor = if term.mode().contains(TermMode::SHOW_CURSOR) {
            point_to_viewport(display_offset, grid.cursor.point).and_then(|vp| {
                if vp.line < rows {
                    Some((vp.line, vp.column.0.min(cols.saturating_sub(1))))
                } else {
                    None
                }
            })
        } else {
            None
        };

        drop(term);
        let title = self.inner.title.lock().clone();

        TerminalSnapshot {
            cols,
            rows,
            lines,
            cursor,
            title,
            display_offset,
        }
    }

    /// Scroll the viewport by `delta_lines` (positive scrolls up into history,
    /// negative scrolls down toward the live bottom).
    pub fn scroll(&self, delta_lines: i32) {
        if delta_lines != 0 {
            self.inner.term.lock().scroll_display(Scroll::Delta(delta_lines));
        }
    }

    /// Scroll the viewport back to the bottom (live output).
    pub fn scroll_to_bottom(&self) {
        self.inner.term.lock().scroll_display(Scroll::Bottom);
    }

    /// Returns `true` if the child process has not exited yet.
    pub fn is_alive(&self) -> bool {
        self.inner.alive.load(Ordering::Acquire)
    }

    /// Returns the child's exit code once it has terminated.
    pub fn exit_code(&self) -> Option<i32> {
        *self.inner.exit_code.lock()
    }

    /// Returns the child process ID, if available.
    pub fn pid(&self) -> Option<u32> {
        self.inner.pid
    }

    /// Best-effort current working directory of the child process.
    ///
    /// On Linux this reads `/proc/<pid>/cwd`; on other platforms returns `None`.
    pub fn cwd(&self) -> Option<PathBuf> {
        #[cfg(target_os = "linux")]
        {
            let pid = self.inner.pid?;
            std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }

    /// Returns current terminal mode flags relevant to input handling.
    pub fn mode(&self) -> TermModeFlags {
        let term = self.inner.term.lock();
        let mode = term.mode();
        TermModeFlags {
            app_cursor: mode.contains(TermMode::APP_CURSOR),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
            alt_screen: mode.contains(TermMode::ALT_SCREEN),
        }
    }

    /// Terminate the child process.
    pub fn kill(&self) {
        if let Err(err) = self.inner.killer.lock().kill() {
            debug!("failed to kill PTY child: {err}");
        }
        self.inner.alive.store(false, Ordering::Release);
    }

    /// Extract text between two viewport `(row, col)` coordinates (inclusive).
    pub fn selection_text(&self, start: (usize, usize), end: (usize, usize)) -> String {
        let term = self.inner.term.lock();
        let grid = term.grid();
        let rows = grid.screen_lines();
        let cols = grid.columns();
        if rows == 0 || cols == 0 {
            return String::new();
        }

        let (a, b) = if start <= end { (start, end) } else { (end, start) };
        let start_vp = Point::new(a.0.min(rows - 1), Column(a.1.min(cols - 1)));
        let end_vp = Point::new(b.0.min(rows - 1), Column(b.1.min(cols - 1)));

        let display_offset = grid.display_offset();
        let p_start = viewport_to_point(display_offset, start_vp);
        let p_end = viewport_to_point(display_offset, end_vp);

        term.bounds_to_string(p_start, p_end)
    }
}

fn default_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".to_string())
}

fn convert_color(color: Color) -> TermColor {
    match color {
        Color::Spec(Rgb { r, g, b }) => TermColor::Rgb(r, g, b),
        Color::Indexed(idx) => TermColor::Indexed(idx),
        Color::Named(named) => match named {
            NamedColor::Black => TermColor::Named(0),
            NamedColor::Red => TermColor::Named(1),
            NamedColor::Green => TermColor::Named(2),
            NamedColor::Yellow => TermColor::Named(3),
            NamedColor::Blue => TermColor::Named(4),
            NamedColor::Magenta => TermColor::Named(5),
            NamedColor::Cyan => TermColor::Named(6),
            NamedColor::White => TermColor::Named(7),
            NamedColor::BrightBlack => TermColor::Named(8),
            NamedColor::BrightRed => TermColor::Named(9),
            NamedColor::BrightGreen => TermColor::Named(10),
            NamedColor::BrightYellow => TermColor::Named(11),
            NamedColor::BrightBlue => TermColor::Named(12),
            NamedColor::BrightMagenta => TermColor::Named(13),
            NamedColor::BrightCyan => TermColor::Named(14),
            NamedColor::BrightWhite => TermColor::Named(15),
            NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::DimForeground => {
                TermColor::Default
            }
            NamedColor::Background => TermColor::DefaultBg,
            NamedColor::Cursor => TermColor::Cursor,
            NamedColor::DimBlack => TermColor::Named(0),
            NamedColor::DimRed => TermColor::Named(1),
            NamedColor::DimGreen => TermColor::Named(2),
            NamedColor::DimYellow => TermColor::Named(3),
            NamedColor::DimBlue => TermColor::Named(4),
            NamedColor::DimMagenta => TermColor::Named(5),
            NamedColor::DimCyan => TermColor::Named(6),
            NamedColor::DimWhite => TermColor::Named(7),
        },
    }
}

/// Discover available shells from `/etc/shells` and `$SHELL`.
pub fn available_shells() -> Vec<String> {
    let mut shells = Vec::new();
    if let Ok(contents) = std::fs::read_to_string("/etc/shells") {
        for line in contents.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if !shells.iter().any(|s| s == trimmed) {
                shells.push(trimmed.to_string());
            }
        }
    }
    if let Ok(env_shell) = std::env::var("SHELL") {
        if !env_shell.is_empty() && !shells.iter().any(|s| s == &env_shell) {
            shells.push(env_shell);
        }
    }
    if shells.is_empty() {
        shells.push("/bin/sh".to_string());
    }
    shells
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::{Duration, Instant};

    #[test]
    fn encode_key_table() {
        // Basic keys
        assert_eq!(encode_key("enter", false, false, false, false), Some(b"\r".to_vec()));
        assert_eq!(encode_key("backspace", false, false, false, false), Some(vec![0x7f]));
        assert_eq!(encode_key("tab", false, false, false, false), Some(b"\t".to_vec()));
        assert_eq!(encode_key("tab", false, false, true, false), Some(b"\x1b[Z".to_vec()));
        assert_eq!(encode_key("escape", false, false, false, false), Some(vec![0x1b]));

        // Ctrl + letter
        assert_eq!(encode_key("c", true, false, false, false), Some(vec![0x03]));
        assert_eq!(encode_key("d", true, false, false, false), Some(vec![0x04]));
        assert_eq!(encode_key("a", true, false, false, false), Some(vec![0x01]));
        assert_eq!(encode_key("z", true, false, false, false), Some(vec![0x1a]));

        // Alt prefix
        assert_eq!(encode_key("x", false, true, false, false), Some(b"\x1bx".to_vec()));
        assert_eq!(encode_key("c", true, true, false, false), Some(vec![0x1b, 0x03]));

        // Arrows normal vs app_cursor vs modifiers
        assert_eq!(encode_key("up", false, false, false, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(encode_key("up", false, false, false, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(encode_key("down", false, false, true, false), Some(b"\x1b[1;2B".to_vec()));
        assert_eq!(encode_key("left", true, false, false, false), Some(b"\x1b[1;5D".to_vec()));
        assert_eq!(encode_key("right", false, true, false, true), Some(b"\x1b[1;3C".to_vec()));
        assert_eq!(encode_key("home", true, false, true, false), Some(b"\x1b[1;6H".to_vec()));
        assert_eq!(encode_key("end", false, false, false, true), Some(b"\x1bOF".to_vec()));

        // Tilde keys
        assert_eq!(encode_key("delete", false, false, false, false), Some(b"\x1b[3~".to_vec()));
        assert_eq!(encode_key("pageup", false, false, true, false), Some(b"\x1b[5;2~".to_vec()));

        // Function keys
        assert_eq!(encode_key("f1", false, false, false, false), Some(b"\x1bOP".to_vec()));
        assert_eq!(encode_key("f1", false, false, true, false), Some(b"\x1b[1;2P".to_vec()));
        assert_eq!(encode_key("f5", false, false, false, false), Some(b"\x1b[15~".to_vec()));
        assert_eq!(encode_key("f12", true, false, false, false), Some(b"\x1b[24;5~".to_vec()));
    }

    #[test]
    fn spawn_and_read_output() {
        let updates = Arc::new(AtomicUsize::new(0));
        let updates_cb = Arc::clone(&updates);

        let term = Terminal::spawn(
            TerminalOptions {
                shell: Some("/bin/sh".to_string()),
                args: vec!["-c".to_string(), "printf 'hello world'".to_string()],
                cols: 80,
                rows: 24,
                ..Default::default()
            },
            move || {
                updates_cb.fetch_add(1, Ordering::Relaxed);
            },
        )
        .expect("spawn terminal");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut found = false;
        while Instant::now() < deadline {
            let snap = term.snapshot();
            let first_line: String = snap.lines[0].iter().map(|c| c.ch).collect();
            if first_line.contains("hello world") {
                found = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        assert!(found, "expected 'hello world' in terminal snapshot");
        assert!(updates.load(Ordering::Relaxed) >= 1);

        // Wait for process to exit
        let exit_deadline = Instant::now() + Duration::from_secs(5);
        while term.is_alive() && Instant::now() < exit_deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!term.is_alive());
        assert_eq!(term.exit_code(), Some(0));

        // Test selection_text
        let sel = term.selection_text((0, 0), (0, 10));
        assert_eq!(sel, "hello world");
    }

    #[test]
    fn resize_updates_snapshot_dimensions() {
        let term = Terminal::spawn(
            TerminalOptions {
                shell: Some("/bin/sh".to_string()),
                args: vec!["-c".to_string(), "sleep 0.2".to_string()],
                cols: 80,
                rows: 24,
                ..Default::default()
            },
            || {},
        )
        .expect("spawn terminal");

        let snap = term.snapshot();
        assert_eq!(snap.cols, 80);
        assert_eq!(snap.rows, 24);

        term.resize(120, 40);
        let snap2 = term.snapshot();
        assert_eq!(snap2.cols, 120);
        assert_eq!(snap2.rows, 40);
        assert_eq!(snap2.lines.len(), 40);
        assert_eq!(snap2.lines[0].len(), 120);

        term.kill();
    }

    #[test]
    fn available_shells_non_empty() {
        let shells = available_shells();
        assert!(!shells.is_empty());
    }
}
