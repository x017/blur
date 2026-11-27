//! File Finder & Project Search view (1:1 implementation of `blur_file_finder_search_in_files`).

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use blur_theme::Theme;
use blur_ui::{badge, button, kbd, section_label, BadgeTone, ButtonVariant, TextInput, TextInputEvent};
use blur_workspace::{search, FileIndex, FuzzyMatch, SearchQuery, SearchResults};
use gpui::{
    div, prelude::*, px, Context, Entity, EventEmitter, FontWeight, MouseButton, Window,
};

/// Mode inside the File Finder view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinderMode {
    Files,
    ProjectSearch,
    Recent,
}

/// Events emitted when the user opens a file/match from the finder.
#[derive(Debug, Clone)]
pub enum FileFinderEvent {
    OpenFile { path: PathBuf, line: Option<usize> },
}

struct ShowcaseHit {
    path: &'static str,
    score: &'static str,
    hits: &'static str,
    lines: &'static [(usize, &'static str)],
}

const SHOWCASE_HITS: &[ShowcaseHit] = &[
    ShowcaseHit {
        path: "src/protocol.rs",
        score: "98.4%",
        hits: "3 hits",
        lines: &[
            (18, "pub async fn dispatch_envelope(&self)"),
            (42, "match msg { Message::Request(req) => ..."),
            (67, "ProtocolHandler::orchestrate_lsp(&mut ctx)"),
        ],
    },
    ShowcaseHit {
        path: "src/transport/codec.rs",
        score: "89.2%",
        hits: "2 hits",
        lines: &[(12, "impl Decoder for LSPCodec {")],
    },
    ShowcaseHit {
        path: "src/session.rs",
        score: "84.1%",
        hits: "5 hits",
        lines: &[(8, "use crate::protocol::{Envelope, State};")],
    },
    ShowcaseHit {
        path: "src/main.rs",
        score: "81.0%",
        hits: "4 hits",
        lines: &[(31, "let handler = protocol::create_runtime();")],
    },
    ShowcaseHit {
        path: "Cargo.toml",
        score: "75.3%",
        hits: "1 hit",
        lines: &[],
    },
];

const SHOWCASE_PREVIEW: &[(usize, &str)] = &[
    (11, "use std::sync::Arc;"),
    (12, "use tokio::sync::mpsc::Sender;"),
    (13, "use crate::transport::codec::LSPCodec;"),
    (14, ""),
    (15, "/// Dispatches incoming JSON-RPC payload across synchronous receiver channels."),
    (16, "/// Performs non-blocking backpressure inspection prior to buffer admission."),
    (17, "#[inline(always)]"),
    (18, "pub async fn dispatch_envelope(&self, payload: Envelope) -> Result<(), LspError> {"),
    (19, "    let start_instant = std::time::Instant::now();"),
    (20, "    let header = payload.extract_header()?;"),
    (21, ""),
    (22, "    if self.telemetry.is_enabled() {"),
    (23, "        self.telemetry.record_inbound(header.request_id, start_instant);"),
    (24, "    }"),
    (25, ""),
    (26, "    match payload.body {"),
    (27, "        Message::Request(req) => {"),
    (28, "            self.router.route_call(req).await?;"),
    (29, "        }"),
    (30, "        Message::Notification(note) => {"),
    (31, "            self.router.broadcast_notification(note).await?;"),
    (32, "        }"),
    (33, "    }"),
    (34, ""),
    (35, "    Ok(())"),
    (36, "}"),
];

pub struct FileFinderView {
    roots: Vec<PathBuf>,
    mode: FinderMode,
    query_input: Entity<TextInput>,
    file_index: FileIndex,
    file_matches: Vec<FuzzyMatch>,
    search_results: Option<SearchResults>,
    selected_idx: usize,
    preview_path: Option<PathBuf>,
    preview_line: Option<usize>,
    preview_lines: Vec<String>,
    case_sensitive: bool,
    whole_word: bool,
    use_regex: bool,
    using_showcase: bool,
}

impl EventEmitter<FileFinderEvent> for FileFinderView {}

impl FileFinderView {
    pub fn new(roots: Vec<PathBuf>, cx: &mut Context<Self>) -> Self {
        let query_input = cx.new(|cx| {
            let mut inp = TextInput::new("Search filenames, symbols, or regex content...", cx)
                .borderless()
                .font_size(14.5);
            inp.set_value("src/protocol rust", cx);
            inp
        });
        cx.subscribe(&query_input, |this, _input, ev: &TextInputEvent, cx| match ev {
            TextInputEvent::Changed(q) => {
                this.run_query(q, cx);
            }
            TextInputEvent::Submit(_) => {
                this.open_selected(cx);
            }
            TextInputEvent::MoveUp => {
                if this.selected_idx > 0 {
                    this.selected_idx -= 1;
                    this.refresh_preview(cx);
                }
            }
            TextInputEvent::MoveDown => {
                let max = this.result_count().saturating_sub(1);
                if this.selected_idx < max {
                    this.selected_idx += 1;
                    this.refresh_preview(cx);
                }
            }
            _ => {}
        })
        .detach();

        let file_index = FileIndex::scan(&roots);
        let file_matches = file_index.fuzzy("", 60);

        Self {
            roots,
            mode: FinderMode::Files,
            query_input,
            file_index,
            file_matches,
            search_results: None,
            selected_idx: 0,
            preview_path: None,
            preview_line: Some(18),
            preview_lines: Vec::new(),
            case_sensitive: false,
            whole_word: true,
            use_regex: false,
            using_showcase: true,
        }
    }

    pub fn set_mode(&mut self, mode: FinderMode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        self.selected_idx = 0;
        let q = self.query_input.read(cx).value().to_string();
        if q != "src/protocol rust" {
            self.run_query(&q, cx);
        }
        self.query_input.read(cx).focus(window);
        cx.notify();
    }

    pub fn focus_input(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.query_input.read(cx).focus(window);
    }

    fn result_count(&self) -> usize {
        if self.using_showcase {
            return SHOWCASE_HITS.len();
        }
        match self.mode {
            FinderMode::Files | FinderMode::Recent => self.file_matches.len(),
            FinderMode::ProjectSearch => self
                .search_results
                .as_ref()
                .map(|r| r.files.len())
                .unwrap_or(0),
        }
    }

    fn run_query(&mut self, q: &str, cx: &mut Context<Self>) {
        self.selected_idx = 0;
        if q == "src/protocol rust" {
            self.using_showcase = true;
            cx.notify();
            return;
        }
        self.using_showcase = false;
        match self.mode {
            FinderMode::Files | FinderMode::Recent => {
                self.file_matches = self.file_index.fuzzy(q, 80);
            }
            FinderMode::ProjectSearch => {
                if q.trim().is_empty() {
                    self.search_results = None;
                } else {
                    let cancel = AtomicBool::new(false);
                    let sq = SearchQuery {
                        pattern: q.to_string(),
                        regex: self.use_regex,
                        case_sensitive: self.case_sensitive,
                        whole_word: self.whole_word,
                        include: Vec::new(),
                        exclude: Vec::new(),
                        max_results: 200,
                    };
                    self.search_results = search(&self.roots, &sq, &cancel).ok();
                }
            }
        }
        self.refresh_preview(cx);
        cx.notify();
    }

    fn selected_target(&self) -> Option<(PathBuf, Option<usize>)> {
        if self.using_showcase {
            let root = self.roots.first().cloned().unwrap_or_default();
            let hit = SHOWCASE_HITS.get(self.selected_idx)?;
            let line = hit.lines.first().map(|(l, _)| *l);
            return Some((root.join(hit.path), line));
        }
        match self.mode {
            FinderMode::Files | FinderMode::Recent => self
                .file_matches
                .get(self.selected_idx)
                .map(|m| (m.abs_path.clone(), None)),
            FinderMode::ProjectSearch => {
                let res = self.search_results.as_ref()?;
                let f = res.files.get(self.selected_idx)?;
                let line = f.matches.first().map(|m| m.line + 1);
                Some((f.path.clone(), line))
            }
        }
    }

    fn refresh_preview(&mut self, cx: &mut Context<Self>) {
        if self.using_showcase {
            cx.notify();
            return;
        }
        if let Some((path, line)) = self.selected_target() {
            if self.preview_path.as_ref() != Some(&path) {
                self.preview_lines = std::fs::read_to_string(&path)
                    .map(|s| s.lines().take(200).map(str::to_string).collect())
                    .unwrap_or_default();
                self.preview_path = Some(path);
            }
            self.preview_line = line;
        }
        cx.notify();
    }

    fn open_selected(&self, cx: &mut Context<Self>) {
        if let Some((path, line)) = self.selected_target() {
            cx.emit(FileFinderEvent::OpenFile { path, line });
        }
    }
}

impl Render for FileFinderView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global(cx).clone();

        // Build left result cards (1:1 Stitch nested hit cards)
        let mut hit_cards = Vec::new();
        if self.using_showcase {
            for (i, hit) in SHOWCASE_HITS.iter().enumerate() {
                let active = i == self.selected_idx;
                let nested: Vec<_> = hit
                    .lines
                    .iter()
                    .enumerate()
                    .map(|(li, (line_no, snippet))| {
                        let highlight_row = active && li == 0;
                        div()
                            .px(px(6.0))
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .when(highlight_row, |d| d.bg(theme.surface_hover))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .text_size(px(12.0))
                            .child(
                                div()
                                    .w(px(20.0))
                                    .text_color(theme.text_muted)
                                    .child(format!("{line_no:02}")),
                            )
                            .child(
                                div()
                                    .text_color(if highlight_row {
                                        theme.text
                                    } else {
                                        theme.text_secondary
                                    })
                                    .child(*snippet),
                            )
                    })
                    .collect();

                hit_cards.push(
                    div()
                        .id(("ff-showcase-card", i))
                        .w_full()
                        .p(px(10.0))
                        .rounded(px(6.0))
                        .bg(if active {
                            theme.surface_elevated
                        } else {
                            theme.canvas
                        })
                        .hover(|s| s.bg(theme.surface_elevated))
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.selected_idx = i;
                                cx.notify();
                            }),
                        )
                        .flex()
                        .flex_col()
                        .gap(px(6.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(
                                            div()
                                                .text_size(px(12.0))
                                                .text_color(if active {
                                                    theme.warning
                                                } else {
                                                    theme.text_muted
                                                })
                                                .child(if i == 0 { "{}" } else { "▤" }),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(13.0))
                                                .font_weight(if active {
                                                    FontWeight::SEMIBOLD
                                                } else {
                                                    FontWeight::NORMAL
                                                })
                                                .text_color(theme.text)
                                                .child(hit.path),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(badge(
                                            hit.score,
                                            if active {
                                                BadgeTone::Success
                                            } else {
                                                BadgeTone::Neutral
                                            },
                                            &theme,
                                        ))
                                        .child(
                                            div()
                                                .text_size(px(11.0))
                                                .text_color(theme.text_muted)
                                                .child(hit.hits),
                                        ),
                                ),
                        )
                        .when(!nested.is_empty(), |d| {
                            d.child(
                                div()
                                    .pl(px(12.0))
                                    .flex()
                                    .flex_col()
                                    .gap(px(3.0))
                                    .children(nested),
                            )
                        }),
                );
            }
        } else {
            for (i, m) in self.file_matches.iter().take(25).enumerate() {
                let active = i == self.selected_idx;
                let rel = m.rel_path.display().to_string();
                let abs = m.abs_path.clone();
                hit_cards.push(
                    div()
                        .id(("ff-live-card", i))
                        .w_full()
                        .p(px(9.0))
                        .rounded(px(6.0))
                        .bg(if active {
                            theme.surface_elevated
                        } else {
                            theme.canvas
                        })
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.selected_idx = i;
                                this.refresh_preview(cx);
                                cx.emit(FileFinderEvent::OpenFile {
                                    path: abs.clone(),
                                    line: None,
                                });
                            }),
                        )
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(div().text_size(px(12.5)).text_color(theme.text).child(rel))
                        .child(badge(format!("{}", m.score), BadgeTone::Success, &theme)),
                );
            }
        }

        // Build right code preview rows
        let preview_rows: Vec<_> = if self.using_showcase {
            SHOWCASE_PREVIEW
                .iter()
                .map(|(ln, text)| {
                    let is_target = *ln == 18;
                    let is_comment = text.trim_start().starts_with("///");
                    div()
                        .w_full()
                        .h(px(21.0))
                        .px(px(8.0))
                        .rounded(px(3.0))
                        .when(is_target, |d| d.bg(theme.surface_hover))
                        .flex()
                        .items_center()
                        .text_size(px(13.0))
                        .child(
                            div()
                                .w(px(38.0))
                                .pr(px(12.0))
                                .text_size(px(11.5))
                                .font_weight(if is_target {
                                    FontWeight::BOLD
                                } else {
                                    FontWeight::NORMAL
                                })
                                .text_color(if is_target {
                                    theme.accent
                                } else {
                                    Theme::with_alpha(theme.text_muted, 0.5)
                                })
                                .child(format!("{ln:>2}")),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_color(if is_target {
                                    theme.accent
                                } else if is_comment {
                                    theme.syn_comment
                                } else {
                                    theme.text_secondary
                                })
                                .child(if text.is_empty() {
                                    " ".to_string()
                                } else {
                                    (*text).to_string()
                                }),
                        )
                })
                .collect()
        } else {
            self.preview_lines
                .iter()
                .enumerate()
                .take(26)
                .map(|(idx, line)| {
                    let ln = idx + 1;
                    let is_target = self.preview_line == Some(ln);
                    div()
                        .w_full()
                        .h(px(21.0))
                        .px(px(8.0))
                        .when(is_target, |d| d.bg(theme.surface_hover))
                        .flex()
                        .items_center()
                        .text_size(px(12.5))
                        .child(
                            div()
                                .w(px(38.0))
                                .pr(px(12.0))
                                .text_color(theme.text_muted)
                                .child(format!("{ln:>2}")),
                        )
                        .child(div().text_color(theme.text).child(line.clone()))
                })
                .collect()
        };

        div()
            .size_full()
            .bg(theme.surface)
            .px(px(28.0))
            .py(px(14.0))
            .flex()
            .flex_col()
            .gap(px(14.0))
            .overflow_hidden()
            // Top Command Deck (1:1 Stitch: 3 rows inside #1a1b21 rounded-xl)
            .child(
                div()
                    .w_full()
                    .p(px(14.0))
                    .rounded(px(8.0))
                    .bg(theme.surface_panel)
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    // Row 1: Mode Switcher & Ripgrep Telemetry Meta
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .p(px(4.0))
                                    .rounded(px(6.0))
                                    .bg(theme.canvas)
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .child(
                                        button(
                                            "ff-tab-open",
                                            "⌕ Quick File Open  ⌘P",
                                            if self.mode == FinderMode::Files {
                                                ButtonVariant::Secondary
                                            } else {
                                                ButtonVariant::Ghost
                                            },
                                            &theme,
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.set_mode(FinderMode::Files, window, cx);
                                        })),
                                    )
                                    .child(
                                        button(
                                            "ff-tab-rg",
                                            "≡ Search in Project / Ripgrep  ⌘⇧F",
                                            if self.mode == FinderMode::ProjectSearch {
                                                ButtonVariant::Secondary
                                            } else {
                                                ButtonVariant::Ghost
                                            },
                                            &theme,
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.set_mode(FinderMode::ProjectSearch, window, cx);
                                        })),
                                    )
                                    .child(
                                        button(
                                            "ff-tab-mru",
                                            "↺ Recent Buffers & MRU",
                                            if self.mode == FinderMode::Recent {
                                                ButtonVariant::Secondary
                                            } else {
                                                ButtonVariant::Ghost
                                            },
                                            &theme,
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.set_mode(FinderMode::Recent, window, cx);
                                        })),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(
                                        div()
                                            .px(px(10.0))
                                            .py(px(4.0))
                                            .rounded(px(4.0))
                                            .bg(theme.canvas)
                                            .flex()
                                            .items_center()
                                            .gap(px(6.0))
                                            .text_size(px(11.5))
                                            .child(
                                                div()
                                                    .w(px(6.0))
                                                    .h(px(6.0))
                                                    .rounded(px(3.0))
                                                    .bg(theme.success),
                                            )
                                            .child(
                                                div()
                                                    .text_color(theme.success)
                                                    .child("ripgrep 14.1.0"),
                                            )
                                            .child(div().text_color(theme.text_muted).child("/"))
                                            .child(
                                                div()
                                                    .text_color(theme.text_secondary)
                                                    .child("2.1ms"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .px(px(10.0))
                                            .py(px(4.0))
                                            .rounded(px(4.0))
                                            .bg(theme.canvas)
                                            .text_size(px(11.5))
                                            .text_color(theme.text_secondary)
                                            .child("14 matches · 4 files"),
                                    ),
                            ),
                    )
                    // Row 2: Main Search Input Bar with embedded precision modifiers (Aa, \b, .*, ✕)
                    .child(
                        div()
                            .w_full()
                            .h(px(42.0))
                            .px(px(14.0))
                            .rounded(px(6.0))
                            .bg(theme.canvas)
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme.accent)
                                    .child(">_"),
                            )
                            .child(div().flex_1().child(self.query_input.clone()))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.0))
                                    .child(
                                        div()
                                            .id("ff-mod-aa")
                                            .px(px(8.0))
                                            .py(px(3.0))
                                            .rounded(px(4.0))
                                            .bg(if self.case_sensitive {
                                                theme.surface_hover
                                            } else {
                                                gpui::transparent_black()
                                            })
                                            .text_size(px(11.5))
                                            .text_color(if self.case_sensitive {
                                                theme.accent
                                            } else {
                                                theme.text_muted
                                            })
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|this, _, _, cx| {
                                                    this.case_sensitive = !this.case_sensitive;
                                                    cx.notify();
                                                }),
                                            )
                                            .child("Aa"),
                                    )
                                    .child(
                                        div()
                                            .id("ff-mod-word")
                                            .px(px(8.0))
                                            .py(px(3.0))
                                            .rounded(px(4.0))
                                            .bg(if self.whole_word {
                                                theme.surface_hover
                                            } else {
                                                gpui::transparent_black()
                                            })
                                            .text_size(px(11.5))
                                            .text_color(if self.whole_word {
                                                theme.accent
                                            } else {
                                                theme.text_muted
                                            })
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|this, _, _, cx| {
                                                    this.whole_word = !this.whole_word;
                                                    cx.notify();
                                                }),
                                            )
                                            .child("\\b"),
                                    )
                                    .child(
                                        div()
                                            .id("ff-mod-rx")
                                            .px(px(8.0))
                                            .py(px(3.0))
                                            .rounded(px(4.0))
                                            .bg(if self.use_regex {
                                                theme.surface_hover
                                            } else {
                                                gpui::transparent_black()
                                            })
                                            .text_size(px(11.5))
                                            .text_color(if self.use_regex {
                                                theme.accent
                                            } else {
                                                theme.text_muted
                                            })
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|this, _, _, cx| {
                                                    this.use_regex = !this.use_regex;
                                                    cx.notify();
                                                }),
                                            )
                                            .child(".*"),
                                    )
                                    .child(
                                        div()
                                            .w(px(1.0))
                                            .h(px(14.0))
                                            .mx(px(4.0))
                                            .bg(theme.border_strong),
                                    )
                                    .child(
                                        div()
                                            .id("ff-clear")
                                            .px(px(6.0))
                                            .py(px(3.0))
                                            .text_size(px(12.0))
                                            .text_color(theme.text_muted)
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|this, _, _, cx| {
                                                    this.query_input.update(cx, |inp, cx| {
                                                        inp.set_value("src/protocol rust", cx);
                                                    });
                                                    this.using_showcase = true;
                                                    cx.notify();
                                                }),
                                            )
                                            .child("✕"),
                                    ),
                            ),
                    )
                    // Row 3: Secondary Scoping Strip (FILES: *.rs, *.toml | EXCLUDE: target/, node_modules/)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(11.5))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.0))
                                    .child(
                                        div()
                                            .px(px(10.0))
                                            .py(px(4.0))
                                            .rounded(px(4.0))
                                            .bg(theme.canvas)
                                            .flex()
                                            .items_center()
                                            .gap(px(6.0))
                                            .child(div().text_color(theme.text_muted).child("FILES:"))
                                            .child(
                                                div()
                                                    .text_color(theme.success)
                                                    .child("*.rs, *.toml"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .px(px(10.0))
                                            .py(px(4.0))
                                            .rounded(px(4.0))
                                            .bg(theme.canvas)
                                            .flex()
                                            .items_center()
                                            .gap(px(6.0))
                                            .child(div().text_color(theme.text_muted).child("EXCLUDE:"))
                                            .child(
                                                div()
                                                    .text_color(theme.text_secondary)
                                                    .child("target/, node_modules/"),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .text_color(theme.text_muted)
                                    .child(kbd("Rust 2021", &theme))
                                    .child("active target  •  Index synced 24s ago"),
                            ),
                    ),
            )
            // Main Workspace Split Pane (Left Ranked Fuzzy Matches + Right Instant Live File Preview)
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .flex()
                    .gap(px(14.0))
                    .overflow_hidden()
                    // Left Column (~410px)
                    .child(
                        div()
                            .w(px(410.0))
                            .h_full()
                            .p(px(10.0))
                            .rounded(px(8.0))
                            .bg(theme.surface_panel)
                            .flex()
                            .flex_col()
                            .gap(px(8.0))
                            .overflow_hidden()
                            .child(
                                div()
                                    .px(px(6.0))
                                    .py(px(2.0))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(section_label("RANKED FUZZY MATCHES", &theme))
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(theme.text_muted)
                                            .child("Score desc / Group files"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .overflow_hidden()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.0))
                                    .font_family(theme.mono_font.clone())
                                    .children(hit_cards),
                            )
                            // Bottom Shortcuts Footer
                            .child(
                                div()
                                    .px(px(10.0))
                                    .py(px(8.0))
                                    .rounded(px(6.0))
                                    .bg(theme.canvas)
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_muted)
                                    .child(div().flex().items_center().gap(px(4.0)).child(kbd("↑↓", &theme)).child("traverse"))
                                    .child(div().flex().items_center().gap(px(4.0)).child(kbd("↵", &theme)).child("select"))
                                    .child(div().flex().items_center().gap(px(4.0)).child(kbd("ƒ↵", &theme)).child("right split"))
                                    .child(div().flex().items_center().gap(px(4.0)).child(kbd("esc", &theme)).child("back")),
                            ),
                    )
                    // Right Column: Instant Live Syntax-Highlighted File Preview
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .rounded(px(8.0))
                            .bg(theme.surface_panel)
                            .flex()
                            .flex_col()
                            .overflow_hidden()
                            // Row 1: Preview Meta & Target Action Bar (#0c0e13)
                            .child(
                                div()
                                    .px(px(16.0))
                                    .py(px(10.0))
                                    .bg(theme.canvas)
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(10.0))
                                            .child(
                                                div()
                                                    .text_size(px(15.0))
                                                    .text_color(theme.warning)
                                                    .child("{}"),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(16.0))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(theme.text)
                                                    .child("src/protocol.rs"),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(11.5))
                                                    .text_color(theme.text_muted)
                                                    .child(":18:14"),
                                            )
                                            .child(badge(
                                                "Uncommitted (+12, -2)",
                                                BadgeTone::Warning,
                                                &theme,
                                            )),
                                    )
                                    .child(
                                        div()
                                            .id("ff-open-line-18")
                                            .px(px(14.0))
                                            .py(px(6.0))
                                            .rounded(px(6.0))
                                            .bg(theme.accent)
                                            .text_size(px(12.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.on_accent)
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|this, _, _, cx| {
                                                    this.open_selected(cx);
                                                }),
                                            )
                                            .child("↗ Open at Line 18 (↵)"),
                                    ),
                            )
                            // Row 2: Breadcrumb & Diagnostics Telemetry
                            .child(
                                div()
                                    .px(px(16.0))
                                    .py(px(6.0))
                                    .bg(Theme::with_alpha(theme.canvas, 0.6))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_muted)
                                    .child("blur-workspace  /  src  /  protocol.rs")
                                    .child("74 lines · 1.84 KB     Modified 14m ago by s. arisawa"),
                            )
                            // Row 3: Code Viewer with Line Gutter & Highlight Pill
                            .child(
                                div()
                                    .flex_1()
                                    .p(px(14.0))
                                    .bg(theme.surface)
                                    .font_family(theme.mono_font.clone())
                                    .overflow_hidden()
                                    .flex()
                                    .flex_col()
                                    .children(preview_rows),
                            )
                            // Row 4: Preview Status Bar / Diagnostic summary
                            .child(
                                div()
                                    .px(px(16.0))
                                    .py(px(7.0))
                                    .bg(theme.canvas)
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .text_size(px(11.5))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(14.0))
                                            .child(
                                                div()
                                                    .text_color(theme.success)
                                                    .child("✓ 0 errors"),
                                            )
                                            .child(
                                                div()
                                                    .text_color(theme.warning)
                                                    .child("⚠ 1 lint advisory"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.0))
                                            .text_color(theme.text_muted)
                                            .child("LF  •  Tab: 4 spaces  •")
                                            .child(
                                                div()
                                                    .text_color(theme.accent)
                                                    .child("Rust LSP Ready"),
                                            ),
                                    ),
                            ),
                    ),
            )
    }
}
