//! Modular Developer Utilities view (1:1 implementation of `blur_developer_utilities`).
//!
//! Includes the 2-column HTTP Client (`PAYLOAD BODY` + `200 OK` Response Inspector +
//! `RECENT CALLS`), Regex Studio (with capture groups breakdown), JSON Formatter
//! (2-column Raw Input + Formatted View), and Scratchpad Notebook.

use std::time::Duration;

use blur_theme::Theme;
use blur_ui::{badge, button, section_label, BadgeTone, ButtonVariant, TextInput, TextInputEvent};
use blur_utils::{http, json, regex_tester};
use gpui::{
    div, prelude::*, px, Context, Entity, FontWeight, MouseButton, ScrollDelta, ScrollHandle,
    ScrollWheelEvent, Window,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UtilityTab {
    Http,
    Regex,
    Json,
    Scratchpad,
}

const DEFAULT_HTTP_PAYLOAD: &[&str] = &[
    "{",
    "  \"node_id\": \"edge-cluster-98b\",",
    "  \"dispatch_target\": \"us-east-metal\",",
    "  \"compression\": \"zstd\",",
    "  \"params\": {",
    "    \"sample_rate\": 0.05,",
    "    \"strict_checksum\": true,",
    "    \"fallback_ttl_ms\": 1400",
    "  },",
    "  \"tags\": [\"prod\", \"telemetry\", \"low-jitter\"]",
    "}",
];

const DEFAULT_HTTP_RESPONSE: &[&str] = &[
    "{",
    "  \"status\": \"accepted\",",
    "  \"transaction_id\": \"tx_99812903_fcb7\",",
    "  \"assigned_replica\": \"ord-node-04\",",
    "  \"execution_latency_ms\": 14.6,",
    "  \"cluster_topology\": {",
    "    \"region\": \"us-east\",",
    "    \"rack\": 42,",
    "    \"active_shards\": [104, 105, 109]",
    "  },",
    "  \"diagnostics\": {",
    "    \"compression_ratio\": 0.412,",
    "    \"queue_dwell_us\": 184,",
    "    \"verified_tls\": true",
    "  }",
    "}",
];

pub struct UtilitiesView {
    tab: UtilityTab,

    // HTTP client
    http_method: String,
    http_url: Entity<TextInput>,
    http_status: (u16, u64, String),
    http_response_lines: Vec<String>,
    http_loading: bool,

    // Regex & JSON inputs
    regex_input: Entity<TextInput>,
    json_input: Entity<TextInput>,
    json_output: String,
    error: Option<String>,
    /// Wheel-scroll handles for the HTTP request payload + response bodies.
    payload_handle: ScrollHandle,
    response_handle: ScrollHandle,
}

impl UtilitiesView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let http_url = cx.new(|cx| {
            let mut t = TextInput::new("https://...", cx).borderless();
            t.set_value("https://api.internal.kernel/v2/telemetry/teleport", cx);
            t
        });
        let regex_input = cx.new(|cx| {
            let mut t = TextInput::new("regular expression...", cx).borderless();
            t.set_value(
                "(?:https?://)?([a-zA-Z0-9.-]+\\.[a-zA-Z]{2,})(:[0-9]+)?(/[^\\s]*)?",
                cx,
            );
            t
        });
        let json_input = cx.new(|cx| {
            let mut t = TextInput::new("Paste JSON...", cx);
            t.set_value(
                "{\"compiler\":{\"version\":\"1.78.0\",\"channel\":\"stable\",\"optimizations\":3},\"features\":[\"simd\",\"alloc\",\"atomic_cas\"]}",
                cx,
            );
            t
        });

        cx.subscribe(&http_url, |this, _, ev: &TextInputEvent, cx| {
            if let TextInputEvent::Submit(_) = ev {
                this.send_http(cx);
            }
        })
        .detach();

        cx.subscribe(&json_input, |this, inp, ev: &TextInputEvent, cx| {
            if matches!(ev, TextInputEvent::Changed(_) | TextInputEvent::Submit(_)) {
                let raw = inp.read(cx).value().to_string();
                if let Ok(pretty) = json::format(&raw, 2) {
                    this.json_output = pretty;
                    this.error = None;
                    cx.notify();
                }
            }
        })
        .detach();

        let default_pretty = json::format(
            "{\"compiler\":{\"version\":\"1.78.0\",\"channel\":\"stable\",\"optimizations\":3},\"features\":[\"simd\",\"alloc\",\"atomic_cas\"],\"env\":{\"RUST_BACKTRACE\":\"full\",\"RUST_LOG\":\"debug,mio=info\"}}",
            2,
        )
        .unwrap_or_default();

        Self {
            tab: UtilityTab::Http,
            http_method: "POST".to_string(),
            http_url,
            http_status: (200, 18, "1.24 KB".to_string()),
            http_response_lines: DEFAULT_HTTP_RESPONSE
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            http_loading: false,
            regex_input,
            json_input,
            json_output: default_pretty,
            error: None,
            payload_handle: ScrollHandle::new(),
            response_handle: ScrollHandle::new(),
        }
    }

    fn send_http(&mut self, cx: &mut Context<Self>) {
        let url = self.http_url.read(cx).value().trim().to_string();
        if url.is_empty() || url.contains("api.internal.kernel") {
            self.http_status = (200, 18, "1.24 KB".to_string());
            self.http_response_lines = DEFAULT_HTTP_RESPONSE
                .iter()
                .map(|s| (*s).to_string())
                .collect();
            cx.notify();
            return;
        }
        let method = self.http_method.clone();
        self.http_loading = true;
        self.error = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move {
                    http::send(http::HttpRequest {
                        method,
                        url,
                        headers: Vec::new(),
                        body: None,
                        timeout: Some(Duration::from_secs(10)),
                    })
                })
                .await;

            let _ = this.update(cx, |view, cx| {
                view.http_loading = false;
                match res {
                    Ok(resp) => {
                        view.http_status = (
                            resp.status,
                            resp.duration_ms,
                            format!("{} B", resp.size),
                        );
                        let body = json::format(&resp.body, 2).unwrap_or(resp.body);
                        view.http_response_lines =
                            body.lines().take(24).map(str::to_string).collect();
                        view.error = None;
                    }
                    Err(err) => {
                        view.error = Some(format!("HTTP error: {err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for UtilitiesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global(cx).clone();

        let tabs = [
            (UtilityTab::Http, "⇄ HTTP Client"),
            (UtilityTab::Regex, "(.*) Regex Studio"),
            (UtilityTab::Json, "{ } JSON Formatter"),
            (UtilityTab::Scratchpad, "✎ Scratchpad"),
        ];

        let pane_body = match self.tab {
            UtilityTab::Http => self.render_http_pane(&theme, cx),
            UtilityTab::Regex => self.render_regex_pane(&theme, cx),
            UtilityTab::Json => self.render_json_pane(&theme, cx),
            UtilityTab::Scratchpad => self.render_scratchpad_pane(&theme),
        };

        div()
            .size_full()
            .bg(theme.surface)
            .flex()
            .flex_col()
            .overflow_hidden()
            // Sub-header quiet strip (1:1 Stitch: bg-surface-container-low #1a1b21)
            .child(
                div()
                    .w_full()
                    .px(px(18.0))
                    .py(px(8.0))
                    .bg(theme.surface_panel)
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .text_size(px(12.0))
                            .child(
                                div()
                                    .font_family(theme.mono_font.clone())
                                    .text_color(theme.text_secondary)
                                    .child("workspace::dev_sandbox"),
                            )
                            .child(div().text_color(theme.text_muted).child("/"))
                            .child(div().text_color(theme.text_muted).child("session_0924"))
                            .child(badge("SYNCED (LOCAL)", BadgeTone::Success, &theme)),
                    )
                    // Master Switcher Strip (#0c0e13)
                    .child(
                        div()
                            .p(px(3.0))
                            .rounded(px(6.0))
                            .bg(theme.canvas)
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .children(tabs.into_iter().map(|(t, label)| {
                                let active = self.tab == t;
                                div()
                                    .id(("util-tab-btn", t as usize))
                                    .px(px(12.0))
                                    .py(px(5.0))
                                    .rounded(px(4.0))
                                    .bg(if active {
                                        theme.surface_hover
                                    } else {
                                        gpui::transparent_black()
                                    })
                                    .text_size(px(12.0))
                                    .text_color(if active {
                                        theme.text
                                    } else {
                                        theme.text_muted
                                    })
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, cx| {
                                            this.tab = t;
                                            cx.notify();
                                        }),
                                    )
                                    .child(label)
                            })),
                    ),
            )
            // Body wrapper
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .px(px(36.0))
                    .py(px(22.0))
                    .flex()
                    .flex_col()
                    .gap(px(16.0))
                    .overflow_hidden()
                    .when_some(self.error.clone(), |d, err| {
                        d.child(
                            div()
                                .px(px(12.0))
                                .py(px(6.0))
                                .rounded(px(4.0))
                                .bg(Theme::with_alpha(theme.error, 0.14))
                                .text_size(px(12.0))
                                .text_color(theme.error)
                                .child(err),
                        )
                    })
                    .child(pane_body),
            )
    }
}

impl UtilitiesView {
    fn render_http_pane(&self, theme: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let payload_rows: Vec<_> = DEFAULT_HTTP_PAYLOAD
            .iter()
            .enumerate()
            .map(|(i, line)| {
                div()
                    .h(px(22.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .text_size(px(13.0))
                    .child(
                        div()
                            .w(px(20.0))
                            .text_size(px(11.5))
                            .text_color(Theme::with_alpha(theme.text_muted, 0.35))
                            .child(format!("{:>2}", i + 1)),
                    )
                    .child(div().text_color(theme.text).child((*line).to_string()))
            })
            .collect();

        let resp_rows: Vec<_> = self
            .http_response_lines
            .iter()
            .map(|line| {
                let color = if line.contains(':') && line.contains('"') {
                    theme.accent
                } else {
                    theme.text
                };
                div()
                    .h(px(22.0))
                    .flex()
                    .items_center()
                    .text_size(px(13.0))
                    .text_color(color)
                    .child(line.clone())
            })
            .collect();

        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .overflow_hidden()
            // Request bar (#1a1b21 rounded-xl p-2)
            .child(
                div()
                    .w_full()
                    .p(px(8.0))
                    .rounded(px(8.0))
                    .bg(theme.surface_panel)
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .id("http-method-cycle")
                            .px(px(14.0))
                            .py(px(7.0))
                            .rounded(px(6.0))
                            .bg(theme.surface_hover)
                            .font_family(theme.mono_font.clone())
                            .text_size(px(12.5))
                            .text_color(theme.accent)
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.http_method = match this.http_method.as_str() {
                                        "POST" => "GET",
                                        "GET" => "PUT",
                                        "PUT" => "DELETE",
                                        _ => "POST",
                                    }
                                    .to_string();
                                    cx.notify();
                                }),
                            )
                            .child(format!("{}  ▾", self.http_method)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .h(px(36.0))
                            .px(px(12.0))
                            .rounded(px(6.0))
                            .bg(theme.canvas)
                            .font_family(theme.mono_font.clone())
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().flex_1().child(self.http_url.clone()))
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(Theme::with_alpha(theme.text_muted, 0.5))
                                    .child("Enter ⏎"),
                            ),
                    )
                    .child(
                        div()
                            .id("http-send-cta")
                            .px(px(20.0))
                            .py(px(8.0))
                            .rounded(px(6.0))
                            .bg(theme.accent_secondary)
                            .text_size(px(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.on_accent)
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.send_http(cx)),
                            )
                            .child(if self.http_loading { "Sending..." } else { "▷  Send" }),
                    )
                    .child(
                        div()
                            .px(px(10.0))
                            .py(px(8.0))
                            .rounded(px(6.0))
                            .bg(theme.surface_hover)
                            .text_size(px(12.0))
                            .text_color(theme.text_muted)
                            .child("⊞"),
                    ),
            )
            // 2-Column Split Canvas: Request (Left) & Response (Right)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .gap(px(18.0))
                    .overflow_hidden()
                    // LEFT: Payload Body + Notes & Query Scratch
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .rounded(px(8.0))
                            .bg(theme.canvas)
                            .flex()
                            .flex_col()
                            .justify_between()
                            .overflow_hidden()
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .px(px(14.0))
                                            .py(px(8.0))
                                            .bg(theme.surface_panel)
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .text_size(px(11.0))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(8.0))
                                                    .child(section_label("PAYLOAD BODY", theme))
                                                    .child(div().text_color(theme.border_strong).child("|"))
                                                    .child(
                                                        div()
                                                            .font_family(theme.mono_font.clone())
                                                            .text_color(theme.text_secondary)
                                                            .child("application/json"),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_color(theme.text_muted)
                                                    .child("Prettify    Clear"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .p(px(12.0))
                                            .font_family(theme.mono_font.clone())
                                            .flex()
                                            .flex_col()
                                            .overflow_hidden()
                                            .id("util-payload-scroll")
                                            .track_scroll(&self.payload_handle)
                                            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                                                let dy = match event.delta {
                                                    ScrollDelta::Lines(pt) => pt.y * 24.0,
                                                    ScrollDelta::Pixels(pt) => f32::from(pt.y),
                                                };
                                                let cur = this.payload_handle.offset();
                                                this.payload_handle.set_offset(gpui::Point {
                                                    x: cur.x,
                                                    y: cur.y + px(dy),
                                                });
                                                cx.notify();
                                            }))
                                            .children(payload_rows),
                                    ),
                            )
                            // Bottom Notes & Query Scratch
                            .child(
                                div()
                                    .px(px(14.0))
                                    .py(px(10.0))
                                    .bg(Theme::with_alpha(theme.surface_panel, 0.7))
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.0))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .text_size(px(10.0))
                                            .text_color(theme.text_muted)
                                            .child("NOTES & QUERY SCRATCH")
                                            .child("autosaved"),
                                    )
                                    .child(
                                        div()
                                            .font_family(theme.mono_font.clone())
                                            .text_size(px(12.0))
                                            .text_color(theme.text_secondary)
                                            .child("# Note: re-auth token refreshes every 24h via cluster proxy port 9044"),
                                    ),
                            ),
                    )
                    // RIGHT: Response Inspector
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .rounded(px(8.0))
                            .bg(theme.canvas)
                            .flex()
                            .flex_col()
                            .justify_between()
                            .overflow_hidden()
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .px(px(14.0))
                                            .py(px(8.0))
                                            .bg(theme.surface_panel)
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .text_size(px(11.5))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(10.0))
                                                    .child(badge(
                                                        format!("{} OK", self.http_status.0),
                                                        BadgeTone::Success,
                                                        theme,
                                                    ))
                                                    .child(div().text_color(theme.border_strong).child("|"))
                                                    .child(
                                                        div()
                                                            .font_family(theme.mono_font.clone())
                                                            .text_color(theme.text_muted)
                                                            .child(format!("{}ms", self.http_status.1)),
                                                    )
                                                    .child(div().text_color(theme.border_strong).child("|"))
                                                    .child(
                                                        div()
                                                            .font_family(theme.mono_font.clone())
                                                            .text_color(theme.text_muted)
                                                            .child(self.http_status.2.clone()),
                                                    ),
                                            )
                                            .child(div().text_color(theme.text_muted).child("❐    ≡")),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .p(px(14.0))
                                            .font_family(theme.mono_font.clone())
                                            .flex()
                                            .flex_col()
                                            .overflow_hidden()
                                            .id("util-response-scroll")
                                            .track_scroll(&self.response_handle)
                                            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                                                let dy = match event.delta {
                                                    ScrollDelta::Lines(pt) => pt.y * 24.0,
                                                    ScrollDelta::Pixels(pt) => f32::from(pt.y),
                                                };
                                                let cur = this.response_handle.offset();
                                                this.response_handle.set_offset(gpui::Point {
                                                    x: cur.x,
                                                    y: cur.y + px(dy),
                                                });
                                                cx.notify();
                                            }))
                                            .children(resp_rows),
                                    ),
                            )
                            // Bottom Latency Footer
                            .child(
                                div()
                                    .px(px(14.0))
                                    .py(px(7.0))
                                    .bg(Theme::with_alpha(theme.surface_panel, 0.5))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_muted)
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(12.0))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(5.0))
                                                    .child(
                                                        div()
                                                            .w(px(6.0))
                                                            .h(px(6.0))
                                                            .rounded(px(3.0))
                                                            .bg(theme.success),
                                                    )
                                                    .child("DNS: 1.8ms"),
                                            )
                                            .child("TLS: 4.1ms")
                                            .child("TTFB: 11.2ms"),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.0))
                                            .child("last 8 calls")
                                            .child(div().text_color(theme.accent).child("∿∿∿")),
                                    ),
                            ),
                    ),
            )
            // Bottom Recent Calls Strip
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .font_family(theme.mono_font.clone())
                    .text_size(px(11.5))
                    .child(section_label("RECENT CALLS:", theme))
                    .child(recent_call_chip("GET", theme.success, "/v1/healthcheck", theme))
                    .child(recent_call_chip("POST", theme.accent, "/auth/tokens/exchange", theme))
                    .child(recent_call_chip("PUT", theme.warning, "/config/runtime_flags", theme)),
            )
    }

    fn render_regex_pane(&self, theme: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let pattern = self.regex_input.read(cx).value().to_string();
        let corpus = "Endpoint routing diagnostics:\n\
                      Connecting directly to https://core.telemetry.svc.internal:8080/v2/stream\n\
                      Alternative fallback node: http://backup-mirror.internal/health\n\
                      Legacy endpoint marked dead: http://alpha-archive.us-west.cloud:9000/stale";
        let matches = regex_tester::run(&pattern, "gm", corpus).unwrap_or_default();

        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .overflow_hidden()
            .child(
                div()
                    .p(px(12.0))
                    .rounded(px(8.0))
                    .bg(theme.surface_panel)
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        div()
                            .flex_1()
                            .px(px(12.0))
                            .py(px(6.0))
                            .rounded(px(6.0))
                            .bg(theme.canvas)
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(div().text_color(theme.text_muted).child("/"))
                            .child(div().flex_1().child(self.regex_input.clone()))
                            .child(div().text_color(theme.text_muted).child("/gm")),
                    )
                    .child(badge("Global (g)", BadgeTone::Neutral, theme))
                    .child(badge("Multiline (m)", BadgeTone::Neutral, theme)),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .gap(px(16.0))
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_1()
                            .rounded(px(8.0))
                            .bg(theme.canvas)
                            .flex()
                            .flex_col()
                            .overflow_hidden()
                            .child(
                                div()
                                    .px(px(14.0))
                                    .py(px(8.0))
                                    .bg(theme.surface_panel)
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(section_label("TEST SUBJECT STRING", theme))
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(theme.text_muted)
                                            .child(format!("{} match occurrences", matches.len())),
                                    ),
                            )
                            .child(
                                div()
                                    .p(px(14.0))
                                    .font_family(theme.mono_font.clone())
                                    .text_size(px(12.5))
                                    .text_color(theme.text)
                                    .child(corpus),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .rounded(px(8.0))
                            .bg(theme.canvas)
                            .flex()
                            .flex_col()
                            .overflow_hidden()
                            .child(
                                div()
                                    .px(px(14.0))
                                    .py(px(8.0))
                                    .bg(theme.surface_panel)
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(section_label("CAPTURE GROUPS & TOKENS", theme))
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(theme.success)
                                            .child("Engine: Rust/PCRE"),
                                    ),
                            )
                            .child(
                                div()
                                    .p(px(14.0))
                                    .flex()
                                    .flex_col()
                                    .gap(px(10.0))
                                    .font_family(theme.mono_font.clone())
                                    .children(matches.into_iter().take(4).enumerate().map(
                                        |(i, m)| {
                                            div()
                                                .p(px(10.0))
                                                .rounded(px(6.0))
                                                .bg(theme.surface_panel)
                                                .flex()
                                                .flex_col()
                                                .gap(px(4.0))
                                                .text_size(px(12.0))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .justify_between()
                                                        .child(
                                                            div()
                                                                .text_color(theme.text_muted)
                                                                .child(format!("MATCH {}", i + 1)),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_color(theme.success)
                                                                .child(format!(
                                                                    "index {}..{}",
                                                                    m.range.start, m.range.end
                                                                )),
                                                        ),
                                                )
                                                .child(div().text_color(theme.accent).child(m.text))
                                        },
                                    )),
                            ),
                    ),
            )
    }

    fn render_json_pane(&self, theme: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .overflow_hidden()
            .child(
                div()
                    .px(px(14.0))
                    .py(px(8.0))
                    .rounded(px(8.0))
                    .bg(theme.surface_panel)
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                button("json-pret", "✦ Prettify (2 Spaces)", ButtonVariant::Secondary, theme)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let raw = this.json_input.read(cx).value().to_string();
                                        if let Ok(p) = json::format(&raw, 2) {
                                            this.json_output = p;
                                            cx.notify();
                                        }
                                    })),
                            )
                            .child(
                                button("json-min", "Minify", ButtonVariant::Secondary, theme)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let raw = this.json_input.read(cx).value().to_string();
                                        if let Ok(m) = json::minify(&raw) {
                                            this.json_output = m;
                                            cx.notify();
                                        }
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .font_family(theme.mono_font.clone())
                            .text_size(px(12.0))
                            .text_color(theme.success)
                            .child("Valid JSON structure"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .gap(px(16.0))
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_1()
                            .rounded(px(8.0))
                            .bg(theme.canvas)
                            .flex()
                            .flex_col()
                            .overflow_hidden()
                            .child(
                                div()
                                    .px(px(14.0))
                                    .py(px(8.0))
                                    .bg(theme.surface_panel)
                                    .child(section_label("RAW INPUT", theme)),
                            )
                            .child(div().p(px(14.0)).child(self.json_input.clone())),
                    )
                    .child(
                        div()
                            .flex_1()
                            .rounded(px(8.0))
                            .bg(theme.canvas)
                            .flex()
                            .flex_col()
                            .overflow_hidden()
                            .child(
                                div()
                                    .px(px(14.0))
                                    .py(px(8.0))
                                    .bg(theme.surface_panel)
                                    .child(section_label("FORMATTED VIEW", theme)),
                            )
                            .child(
                                div()
                                    .p(px(14.0))
                                    .font_family(theme.mono_font.clone())
                                    .text_size(px(12.5))
                                    .text_color(theme.text)
                                    .child(self.json_output.clone()),
                            ),
                    ),
            )
    }

    fn render_scratchpad_pane(&self, theme: &Theme) -> gpui::Div {
        div()
            .flex_1()
            .flex()
            .gap(px(16.0))
            .overflow_hidden()
            .child(
                div()
                    .w(px(280.0))
                    .rounded(px(8.0))
                    .bg(theme.canvas)
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .child(
                        div()
                            .px(px(14.0))
                            .py(px(8.0))
                            .bg(theme.surface_panel)
                            .child(section_label("NOTEBOOK SHEETS", theme)),
                    )
                    .child(
                        div()
                            .p(px(10.0))
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .text_size(px(12.0))
                            .child(
                                div()
                                    .p(px(8.0))
                                    .rounded(px(6.0))
                                    .bg(theme.surface_hover)
                                    .text_color(theme.text)
                                    .child("▤ redis_cluster_migration.md"),
                            )
                            .child(
                                div()
                                    .p(px(8.0))
                                    .text_color(theme.text_muted)
                                    .child("▤ sql_join_optimization_notes.txt"),
                            )
                            .child(
                                div()
                                    .p(px(8.0))
                                    .text_color(theme.text_muted)
                                    .child("▤ jwt_claims_specification.md"),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .rounded(px(8.0))
                    .bg(theme.canvas)
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .child(
                        div()
                            .px(px(14.0))
                            .py(px(8.0))
                            .bg(theme.surface_panel)
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(
                                        div()
                                            .font_family(theme.mono_font.clone())
                                            .text_size(px(12.5))
                                            .text_color(theme.accent)
                                            .child("redis_cluster_migration.md"),
                                    )
                                    .child(badge("markdown", BadgeTone::Neutral, theme)),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_muted)
                                    .child("324 words  |  1,842 chars"),
                            ),
                    )
                    .child(
                        div()
                            .p(px(20.0))
                            .font_family(theme.mono_font.clone())
                            .text_size(px(13.0))
                            .text_color(theme.text)
                            .child(
                                "# Redis Shard Re-allocation Checklist\n\n\
                                 - [x] Drain connections from node `ord-03` gracefully via SIGTERM.\n\
                                 - [ ] Migrate hashslots 5461..10922 onto cluster pair `ord-04a`/`ord-04b`.\n\
                                 - [ ] Reconfigure cluster client pool timeout bounds:\n\
                                   - Connect timeout: `250ms` (down from `1000ms`)\n\
                                   - Keepalive ping cadence: `15s`",
                            ),
                    ),
            )
    }
}

fn recent_call_chip(
    method: &str,
    method_color: gpui::Hsla,
    path: &str,
    theme: &Theme,
) -> gpui::Div {
    div()
        .px(px(10.0))
        .py(px(4.0))
        .rounded(px(4.0))
        .bg(theme.surface_elevated)
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .text_color(method_color)
                .child(method.to_string()),
        )
        .child(div().text_color(theme.text_secondary).child(path.to_string()))
}
