//! Protocols & LSP Inspector view (1:1 implementation of `blur_protocols_lsp`).

use std::path::Path;

use blur_lsp::{builtin_server_presets, find_executable, LspManager, ServerConfig, ServerInfo};
use blur_theme::Theme;
use blur_ui::{badge, button, section_label, BadgeTone, ButtonVariant};
use gpui::{
    div, prelude::*, px, Context, FontWeight, MouseButton, ScrollDelta, ScrollHandle,
    ScrollWheelEvent, Window,
};

struct ShowcaseDaemon {
    name: &'static str,
    version: &'static str,
    path: &'static str,
    status: &'static str,
    langs: &'static str,
}

const SHOWCASE_DAEMONS: &[ShowcaseDaemon] = &[
    ShowcaseDaemon {
        name: "rust-analyzer",
        version: "v0.3.1942",
        path: "/opt/homebrew/bin/rust-analyzer",
        status: "● stdio: 34ms ping",
        langs: "rs, toml",
    },
    ShowcaseDaemon {
        name: "gopls",
        version: "v0.15.2",
        path: "~/go/bin/gopls",
        status: "stdio: alive",
        langs: "go, gomod",
    },
    ShowcaseDaemon {
        name: "pyright-langserver",
        version: "v1.1.350",
        path: "~/.nvm/versions/node/bin/pyright",
        status: "node ipc: idle",
        langs: "py, pyi",
    },
    ShowcaseDaemon {
        name: "clangd",
        version: "18.1.3",
        path: "/usr/bin/clangd",
        status: "stdio: idle",
        langs: "c, cpp, h",
    },
];

pub struct LspView {
    manager: LspManager,
    presets: Vec<ServerConfig>,
    selected_name: String,
    _dap_configs: Vec<blur_dap::LaunchConfig>,
    /// Wheel-scroll handle for the registered-daemons list.
    daemon_handle: ScrollHandle,
}

impl LspView {
    pub fn new(manager: LspManager, workspace_root: &Path, _cx: &mut Context<Self>) -> Self {
        let presets = builtin_server_presets();
        let dap_configs = blur_dap::load_launch_configs(workspace_root).unwrap_or_default();
        Self {
            manager,
            presets,
            selected_name: "rust-analyzer".to_string(),
            _dap_configs: dap_configs,
            daemon_handle: ScrollHandle::new(),
        }
    }
}

impl Render for LspView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global(cx).clone();
        let running: Vec<ServerInfo> = self.manager.running();

        // Build left daemon cards (1:1 Stitch 4 registered daemons)
        let daemon_cards: Vec<_> = SHOWCASE_DAEMONS
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let active = d.name == self.selected_name;
                let name = d.name.to_string();
                let real_exe = self
                    .presets
                    .iter()
                    .find(|p| p.name == d.name)
                    .and_then(|p| find_executable(&p.command))
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| d.path.to_string());

                div()
                    .id(("lsp-sc-daemon", i))
                    .relative()
                    .w_full()
                    .p(px(14.0))
                    .rounded(px(4.0))
                    .bg(if active {
                        theme.surface_hover
                    } else {
                        theme.surface_panel
                    })
                    .border_l_2()
                    .border_color(if active {
                        theme.accent
                    } else {
                        gpui::transparent_black()
                    })
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.selected_name = name.clone();
                            cx.notify();
                        }),
                    )
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.0))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.0))
                                            .child(
                                                div()
                                                    .text_size(px(16.0))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(theme.text)
                                                    .child(d.name),
                                            )
                                            .child(badge(
                                                d.version,
                                                if active {
                                                    BadgeTone::Accent
                                                } else {
                                                    BadgeTone::Neutral
                                                },
                                                &theme,
                                            )),
                                    )
                                    .child(
                                        div()
                                            .font_family(theme.mono_font.clone())
                                            .text_size(px(11.0))
                                            .text_color(theme.text_muted)
                                            .child(real_exe),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .text_color(if active {
                                        theme.accent
                                    } else {
                                        theme.success
                                    })
                                    .child(if active { "✦" } else { "●" }),
                            ),
                    )
                    .child(
                        div()
                            .font_family(theme.mono_font.clone())
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(12.0))
                            .child(
                                div()
                                    .text_color(if active {
                                        theme.success
                                    } else {
                                        theme.text_muted
                                    })
                                    .child(d.status),
                            )
                            .child(div().text_color(theme.text_muted).child(d.langs)),
                    )
            })
            .collect();

        let selected_info = running
            .iter()
            .find(|r| r.name == self.selected_name)
            .cloned();
        let pid_str = selected_info
            .as_ref()
            .and_then(|i| i.pid)
            .map(|p| format!("{p} · 246 MB"))
            .unwrap_or_else(|| "48912 · 246 MB".to_string());
        let selected_name_for_restart = self.selected_name.clone();

        let capabilities = [
            ("textDocument/hover", true),
            ("textDocument/completion (Snippets)", true),
            ("textDocument/inlayHint", true),
            ("textDocument/publishDiagnostics", true),
            ("workspace/symbol", true),
            ("textDocument/codeAction", true),
            ("semanticTokens (Disabled)", false),
        ];

        div()
            .size_full()
            .bg(theme.surface)
            .px(px(36.0))
            .py(px(20.0))
            .flex()
            .flex_col()
            .gap(px(18.0))
            .overflow_hidden()
            // Top Architectural Philosophy Banner (1:1 Stitch)
            .child(
                div()
                    .w_full()
                    .p(px(22.0))
                    .rounded(px(8.0))
                    .bg(theme.canvas)
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(section_label("SUBSYSTEM ARCHITECTURE", &theme))
                                    .child(div().text_color(Theme::with_alpha(theme.text_muted, 0.4)).child("/"))
                                    .child(div().text_size(px(11.0)).text_color(theme.accent).child("LSP 3.17 Client")),
                            )
                            .child(
                                div()
                                    .text_size(px(26.0))
                                    .font_weight(FontWeight::NORMAL)
                                    .text_color(theme.text)
                                    .child("Language Daemons"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .text_color(theme.text_secondary)
                                    .child("Blur implements the protocol once. Language servers provide language-specific intelligence over raw stdio pipes. Zero telemetry, zero marketplace layers, purely determinative execution."),
                            ),
                    )
                    .child(
                        div()
                            .px(px(14.0))
                            .py(px(8.0))
                            .rounded(px(4.0))
                            .bg(theme.surface_panel)
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(div().w(px(8.0)).h(px(8.0)).rounded(px(4.0)).bg(theme.success))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(theme.text_secondary)
                                    .child("4 Daemons Active"),
                            )
                            .child(div().text_color(theme.text_muted).child("·"))
                            .child(
                                div()
                                    .font_family(theme.mono_font.clone())
                                    .text_size(px(12.0))
                                    .text_color(theme.text_muted)
                                    .child("stdio | pid tree"),
                            ),
                    ),
            )
            // Main 4/8 Split
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .flex()
                    .gap(px(22.0))
                    .overflow_hidden()
                    // Left Column (4 cols)
                    .child(
                        div()
                            .w(px(330.0))
                            .h_full()
                            .flex()
                            .flex_col()
                            .gap(px(8.0))
                            .overflow_hidden()
                            .id("lsp-daemons-scroll")
                            .track_scroll(&self.daemon_handle)
                            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                                let dy = match event.delta {
                                    ScrollDelta::Lines(pt) => pt.y * 24.0,
                                    ScrollDelta::Pixels(pt) => f32::from(pt.y),
                                };
                                let cur = this.daemon_handle.offset();
                                this.daemon_handle.set_offset(gpui::Point {
                                    x: cur.x,
                                    y: cur.y + px(dy),
                                });
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .px(px(4.0))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(section_label("REGISTERED DAEMONS", &theme))
                                    .child(
                                        div()
                                            .font_family(theme.mono_font.clone())
                                            .text_size(px(11.0))
                                            .text_color(theme.text_muted)
                                            .child("4 / 4 OK"),
                                    ),
                            )
                            .children(daemon_cards)
                            .child(
                                div()
                                    .mt(px(4.0))
                                    .w_full()
                                    .py(px(9.0))
                                    .rounded(px(4.0))
                                    .bg(theme.canvas)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(12.0))
                                    .text_color(theme.text_muted)
                                    .child("+ Declare custom daemon command..."),
                            ),
                    )
                    // Right Column (8 cols: 4 cards matching Stitch 1:1)
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .flex()
                            .flex_col()
                            .gap(px(12.0))
                            .overflow_hidden()
                            // Card 1: Active Server Summary + 4-col Diagnostics + Negotiated Capabilities
                            .child(
                                div()
                                    .p(px(18.0))
                                    .rounded(px(8.0))
                                    .bg(theme.surface_panel)
                                    .flex()
                                    .flex_col()
                                    .gap(px(12.0))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(2.0))
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .gap(px(10.0))
                                                            .child(
                                                                div()
                                                                    .text_size(px(19.0))
                                                                    .font_weight(FontWeight::MEDIUM)
                                                                    .text_color(theme.text)
                                                                    .child(self.selected_name.clone()),
                                                            )
                                                            .child(badge(
                                                                "rustc 1.78.0-nightly",
                                                                BadgeTone::Warning,
                                                                &theme,
                                                            )),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(11.5))
                                                            .text_color(theme.text_muted)
                                                            .child("Autonomous background daemon spawned by Blur Unix pipe runner."),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(8.0))
                                                    .child(
                                                        button(
                                                            "lsp-restart-btn",
                                                            "↻ Restart Daemon",
                                                            ButtonVariant::Secondary,
                                                            &theme,
                                                        )
                                                        .on_click(cx.listener(move |this, _, _, cx| {
                                                            this.manager.restart(&selected_name_for_restart);
                                                            cx.notify();
                                                        })),
                                                    )
                                                    .child(button(
                                                        "lsp-logs-btn",
                                                        ">_ Stdio Pipe Logs",
                                                        ButtonVariant::Secondary,
                                                        &theme,
                                                    )),
                                            ),
                                    )
                                    // 4-col Recessed Connection Diagnostics Bar (#0c0e13)
                                    .child(
                                        div()
                                            .p(px(12.0))
                                            .rounded(px(4.0))
                                            .bg(theme.canvas)
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(metric_cell("TRANSPORT", "● Stdio Duplex", theme.text, &theme))
                                            .child(metric_cell("PID & MEMORY", &pid_str, theme.text, &theme))
                                            .child(metric_cell(
                                                "HANDSHAKE ROUNDTRIP",
                                                "1.2ms (Zero latency)",
                                                theme.success,
                                                &theme,
                                            ))
                                            .child(metric_cell(
                                                "CLIENT SEMANTIC MODEL",
                                                "UTF-16 Positions",
                                                theme.text,
                                                &theme,
                                            )),
                                    )
                                    // Negotiated Client Capabilities chips
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(6.0))
                                            .child(section_label("NEGOTIATED CLIENT CAPABILITIES", &theme))
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_wrap()
                                                    .gap(px(6.0))
                                                    .children(capabilities.into_iter().map(|(cap, enabled)| {
                                                        div()
                                                            .px(px(10.0))
                                                            .py(px(4.0))
                                                            .rounded(px(4.0))
                                                            .bg(theme.surface_elevated)
                                                            .flex()
                                                            .items_center()
                                                            .gap(px(6.0))
                                                            .text_size(px(11.5))
                                                            .text_color(if enabled {
                                                                theme.text
                                                            } else {
                                                                theme.text_muted
                                                            })
                                                            .child(
                                                                div()
                                                                    .w(px(5.0))
                                                                    .h(px(5.0))
                                                                    .rounded(px(2.5))
                                                                    .bg(if enabled {
                                                                        theme.accent
                                                                    } else {
                                                                        theme.text_muted
                                                                    }),
                                                            )
                                                            .child(cap)
                                                    })),
                                            ),
                                    ),
                            )
                            // Card 2 & Card 3 side-by-side so both Protocol Wire JSON and Daemon TOML Config fit in viewport
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .gap(px(12.0))
                                    .overflow_hidden()
                                    // Card 2: Transparent Protocol Wire (JSON-RPC 2.0)
                                    .child(
                                        div()
                                            .flex_1()
                                            .p(px(16.0))
                                            .rounded(px(8.0))
                                            .bg(theme.surface_panel)
                                            .flex()
                                            .flex_col()
                                            .gap(px(8.0))
                                            .overflow_hidden()
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .child(section_label(
                                                        "⇄ TRANSPARENT PROTOCOL WIRE (JSON-RPC 2.0)",
                                                        &theme,
                                                    ))
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .gap(px(6.0))
                                                            .text_size(px(11.0))
                                                            .child(
                                                                div()
                                                                    .text_color(theme.text_muted)
                                                                    .child("Latest Frame: textDocument/hover"),
                                                            )
                                                            .child(
                                                                div()
                                                                    .font_family(theme.mono_font.clone())
                                                                    .text_color(theme.warning)
                                                                    .child("#142"),
                                                            ),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .p(px(12.0))
                                                    .rounded(px(4.0))
                                                    .bg(theme.canvas)
                                                    .font_family(theme.mono_font.clone())
                                                    .text_size(px(11.5))
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(2.0))
                                                    .overflow_hidden()
                                                    .child(div().text_color(theme.text_muted).child("// Client request -> Server response"))
                                                    .child(div().text_color(theme.text_muted).child("{"))
                                                    .child(div().pl(px(12.0)).text_color(theme.accent).child("\"jsonrpc\": \"2.0\","))
                                                    .child(div().pl(px(12.0)).text_color(theme.warning).child("\"id\": 142,"))
                                                    .child(div().pl(px(12.0)).text_color(theme.accent).child("\"result\": {"))
                                                    .child(div().pl(px(24.0)).text_color(theme.success).child("\"contents\": { \"kind\": \"markdown\","))
                                                    .child(div().pl(px(36.0)).text_color(theme.success).child("\"value\": \"rust\\npub fn parse_frame(&self) -> Result\" },"))
                                                    .child(div().pl(px(24.0)).text_color(theme.accent).child("\"range\": { \"start\": { \"line\": 42, \"character\": 7 } }"))
                                                    .child(div().pl(px(12.0)).text_color(theme.text_muted).child("}"))
                                                    .child(div().text_color(theme.text_muted).child("}")),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_size(px(11.0))
                                                    .child(div().text_color(theme.text_muted).child("Buffer size: 418 bytes"))
                                                    .child(
                                                        div()
                                                            .text_color(theme.success)
                                                            .child("Parsed & applied to text buffer instantly"),
                                                    ),
                                            ),
                                    )
                                    // Card 3: Daemon Initialization Configuration (TOML)
                                    .child(
                                        div()
                                            .w(px(380.0))
                                            .p(px(16.0))
                                            .rounded(px(8.0))
                                            .bg(theme.surface_panel)
                                            .flex()
                                            .flex_col()
                                            .gap(px(8.0))
                                            .overflow_hidden()
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .child(
                                                        div()
                                                            .text_size(px(14.0))
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(theme.text)
                                                            .child("Daemon Initialization Config"),
                                                    )
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .gap(px(6.0))
                                                            .child(badge("Synced", BadgeTone::Success, &theme))
                                                            .child(button(
                                                                "lsp-apply-toml",
                                                                "Apply & Reload",
                                                                ButtonVariant::Primary,
                                                                &theme,
                                                            )),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .p(px(12.0))
                                                    .rounded(px(4.0))
                                                    .bg(theme.canvas)
                                                    .font_family(theme.mono_font.clone())
                                                    .text_size(px(11.5))
                                                    .text_color(theme.text_secondary)
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(3.0))
                                                    .overflow_hidden()
                                                    .child("[servers.rust-analyzer]")
                                                    .child("command = \"rust-analyzer\"")
                                                    .child("args = [\"--log-file\", \"/tmp/ra.log\"]")
                                                    .child("filetypes = [\"rust\", \"toml\"]")
                                                    .child("")
                                                    .child("[servers.rust-analyzer.initialization_options]")
                                                    .child("checkOnSave = { command = \"clippy\" }")
                                                    .child("inlayHints = { typeHints = true }")
                                                    .child("procMacro = { enable = true }"),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_size(px(11.0))
                                                    .text_color(theme.text_muted)
                                                    .child("Direct stdin/stdout IPC")
                                                    .child(div().text_color(theme.success).child("p95: 1.8ms")),
                                            ),
                                    ),
                            ),
                    ),
            )
    }
}

fn metric_cell(
    label: &str,
    value: &str,
    value_color: gpui::Hsla,
    theme: &Theme,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            div()
                .text_size(px(10.0))
                .text_color(theme.text_muted)
                .child(label.to_string()),
        )
        .child(
            div()
                .font_family(theme.mono_font.clone())
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(value_color)
                .child(value.to_string()),
        )
}
