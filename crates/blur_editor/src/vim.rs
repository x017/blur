//! Modal Vim state machine for `EditorView`.

/// Active Vim mode when `NavigationMode::Vim` is enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VimMode {
    #[default]
    Normal,
    Insert,
    Visual,
}

impl VimMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Visual => "VISUAL",
        }
    }
}

/// Tracks pending multi-key Vim sequences (`dd`, `yy`, `gg`).
#[derive(Debug, Default)]
pub struct VimState {
    pub mode: VimMode,
    pub pending: Option<char>,
}

impl VimState {
    pub fn reset_pending(&mut self) {
        self.pending = None;
    }
}
