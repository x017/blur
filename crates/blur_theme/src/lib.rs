//! Native theme engine for Blur.
//!
//! Provides a complete color, typography, syntax, and spacing token system
//! inspired by the Quiet Precision ("Furusato Slate") and Obsidian Precision
//! design languages, with 6 built-in themes and TOML custom theme loading.

mod presets;

use std::path::Path;

use anyhow::{Context, Result};
use gpui::{rgba, App, Global, Hsla};
use serde::{Deserialize, Serialize};

pub use presets::builtin_themes;

/// Complete theme definition for Blur.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeDef {
    pub name: String,
    pub description: String,
    pub is_light: bool,
    pub preview_swatches: [String; 4],
    pub colors: ColorPalette,
    pub syntax: SyntaxPalette,
    pub terminal: TerminalPalette,
}

/// Core UI surface, border, text, accent, and semantic colors (hex strings in TOML).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorPalette {
    pub canvas: String,
    pub surface: String,
    pub surface_panel: String,
    pub surface_elevated: String,
    pub surface_hover: String,
    pub surface_active: String,
    pub border: String,
    pub border_strong: String,
    pub text: String,
    pub text_secondary: String,
    pub text_muted: String,
    pub accent: String,
    pub on_accent: String,
    pub accent_secondary: String,
    pub success: String,
    pub warning: String,
    pub error: String,
    pub selection: String,
    pub active_line: String,
    pub diff_add_bg: String,
    pub diff_del_bg: String,
    pub scrim: String,
}

/// Syntax highlighting palette matching `blur_syntax::HighlightKind`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntaxPalette {
    pub keyword: String,
    pub function: String,
    pub r#type: String,
    pub string: String,
    pub number: String,
    pub comment: String,
    pub constant: String,
    pub variable: String,
    pub variable_builtin: String,
    pub property: String,
    pub operator: String,
    pub punctuation: String,
    pub attribute: String,
    pub tag: String,
    pub r#macro: String,
    pub namespace: String,
    pub label: String,
    pub escape: String,
    pub constructor: String,
    pub heading: String,
    pub link: String,
}

/// ANSI 16-color terminal palette + foreground/background/cursor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalPalette {
    pub background: String,
    pub foreground: String,
    pub cursor: String,
    pub ansi: [String; 16],
}

/// Resolved GPUI `Hsla` theme ready for zero-allocation rendering.
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub description: String,
    pub is_light: bool,
    pub preview_swatches: [Hsla; 4],

    // Surfaces
    pub canvas: Hsla,
    pub surface: Hsla,
    pub surface_panel: Hsla,
    pub surface_elevated: Hsla,
    pub surface_hover: Hsla,
    pub surface_active: Hsla,

    // Borders
    pub border: Hsla,
    pub border_strong: Hsla,

    // Text
    pub text: Hsla,
    pub text_secondary: Hsla,
    pub text_muted: Hsla,

    // Accents & semantics
    pub accent: Hsla,
    pub on_accent: Hsla,
    pub accent_secondary: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub error: Hsla,

    // Editor & diff
    pub selection: Hsla,
    pub active_line: Hsla,
    pub diff_add_bg: Hsla,
    pub diff_del_bg: Hsla,
    pub scrim: Hsla,

    // Syntax
    pub syn_keyword: Hsla,
    pub syn_function: Hsla,
    pub syn_type: Hsla,
    pub syn_string: Hsla,
    pub syn_number: Hsla,
    pub syn_comment: Hsla,
    pub syn_constant: Hsla,
    pub syn_variable: Hsla,
    pub syn_variable_builtin: Hsla,
    pub syn_property: Hsla,
    pub syn_operator: Hsla,
    pub syn_punctuation: Hsla,
    pub syn_attribute: Hsla,
    pub syn_tag: Hsla,
    pub syn_macro: Hsla,
    pub syn_namespace: Hsla,
    pub syn_label: Hsla,
    pub syn_escape: Hsla,
    pub syn_constructor: Hsla,
    pub syn_heading: Hsla,
    pub syn_link: Hsla,

    // Terminal
    pub term_bg: Hsla,
    pub term_fg: Hsla,
    pub term_cursor: Hsla,
    pub term_ansi: [Hsla; 16],

    // Graph branch colors (derived from theme accents)
    pub graph_lanes: [Hsla; 6],

    // Typography
    pub ui_font: String,
    pub mono_font: String,
    pub editor_font_size: f32,
    pub editor_line_height: f32,
    pub ui_scale: f32,
}

impl Global for Theme {}

impl Theme {
    /// Compile a `ThemeDef` into a resolved GPUI `Theme`.
    pub fn from_def(def: &ThemeDef) -> Self {
        let c = &def.colors;
        let s = &def.syntax;
        let t = &def.terminal;
        let accent = parse_hex(&c.accent);
        let accent_secondary = parse_hex(&c.accent_secondary);
        let success = parse_hex(&c.success);
        let warning = parse_hex(&c.warning);
        let error = parse_hex(&c.error);
        let syn_function = parse_hex(&s.function);

        Self {
            name: def.name.clone(),
            description: def.description.clone(),
            is_light: def.is_light,
            preview_swatches: [
                parse_hex(&def.preview_swatches[0]),
                parse_hex(&def.preview_swatches[1]),
                parse_hex(&def.preview_swatches[2]),
                parse_hex(&def.preview_swatches[3]),
            ],
            canvas: parse_hex(&c.canvas),
            surface: parse_hex(&c.surface),
            surface_panel: parse_hex(&c.surface_panel),
            surface_elevated: parse_hex(&c.surface_elevated),
            surface_hover: parse_hex(&c.surface_hover),
            surface_active: parse_hex(&c.surface_active),
            border: parse_hex(&c.border),
            border_strong: parse_hex(&c.border_strong),
            text: parse_hex(&c.text),
            text_secondary: parse_hex(&c.text_secondary),
            text_muted: parse_hex(&c.text_muted),
            accent,
            on_accent: parse_hex(&c.on_accent),
            accent_secondary,
            success,
            warning,
            error,
            selection: parse_hex(&c.selection),
            active_line: parse_hex(&c.active_line),
            diff_add_bg: parse_hex(&c.diff_add_bg),
            diff_del_bg: parse_hex(&c.diff_del_bg),
            scrim: parse_hex(&c.scrim),
            syn_keyword: parse_hex(&s.keyword),
            syn_function,
            syn_type: parse_hex(&s.r#type),
            syn_string: parse_hex(&s.string),
            syn_number: parse_hex(&s.number),
            syn_comment: parse_hex(&s.comment),
            syn_constant: parse_hex(&s.constant),
            syn_variable: parse_hex(&s.variable),
            syn_variable_builtin: parse_hex(&s.variable_builtin),
            syn_property: parse_hex(&s.property),
            syn_operator: parse_hex(&s.operator),
            syn_punctuation: parse_hex(&s.punctuation),
            syn_attribute: parse_hex(&s.attribute),
            syn_tag: parse_hex(&s.tag),
            syn_macro: parse_hex(&s.r#macro),
            syn_namespace: parse_hex(&s.namespace),
            syn_label: parse_hex(&s.label),
            syn_escape: parse_hex(&s.escape),
            syn_constructor: parse_hex(&s.constructor),
            syn_heading: parse_hex(&s.heading),
            syn_link: parse_hex(&s.link),
            term_bg: parse_hex(&t.background),
            term_fg: parse_hex(&t.foreground),
            term_cursor: parse_hex(&t.cursor),
            term_ansi: std::array::from_fn(|i| parse_hex(&t.ansi[i])),
            graph_lanes: [
                accent,
                accent_secondary,
                success,
                warning,
                error,
                syn_function,
            ],
            ui_font: "Inter".to_string(),
            mono_font: "JetBrains Mono".to_string(),
            editor_font_size: 13.5,
            editor_line_height: 1.65,
            ui_scale: 1.0,
        }
    }

    /// Scale a logical pixel value by the active `ui_scale`.
    pub fn scaled(&self, v: f32) -> gpui::Pixels {
        gpui::px(v * self.ui_scale)
    }

    /// Return the default Blur theme ("Furusato Slate").
    pub fn default_theme() -> Self {
        let builtins = builtin_themes();
        Self::from_def(&builtins[0])
    }

    /// Get the currently active global theme from GPUI `App`.
    pub fn global(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    /// Install a theme as the active GPUI global.
    pub fn set_global(theme: Self, cx: &mut App) {
        cx.set_global(theme);
    }

    /// Adjust alpha of an `Hsla` color cleanly.
    pub fn with_alpha(mut color: Hsla, alpha: f32) -> Hsla {
        color.a = alpha.clamp(0.0, 1.0);
        color
    }
}

/// Manages built-in and user-installed TOML themes.
#[derive(Debug, Clone)]
pub struct ThemeRegistry {
    themes: Vec<ThemeDef>,
}

impl Global for ThemeRegistry {}

impl ThemeRegistry {
    /// Load all built-in themes and any `.toml` themes from `custom_dir`.
    pub fn load(custom_dir: Option<&Path>) -> Self {
        let mut themes = builtin_themes();
        if let Some(dir) = custom_dir {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                        match load_theme_file(&path) {
                            Ok(def) => {
                                if let Some(slot) = themes
                                    .iter_mut()
                                    .find(|t| t.name.eq_ignore_ascii_case(&def.name))
                                {
                                    *slot = def;
                                } else {
                                    themes.push(def);
                                }
                            }
                            Err(err) => {
                                tracing::warn!(
                                    path = %path.display(),
                                    %err,
                                    "failed to load custom theme"
                                );
                            }
                        }
                    }
                }
            }
        }
        Self { themes }
    }

    pub fn list(&self) -> &[ThemeDef] {
        &self.themes
    }

    pub fn find(&self, name: &str) -> Option<&ThemeDef> {
        self.themes
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
    }

    pub fn register(&mut self, def: ThemeDef) {
        if let Some(slot) = self
            .themes
            .iter_mut()
            .find(|t| t.name.eq_ignore_ascii_case(&def.name))
        {
            *slot = def;
        } else {
            self.themes.push(def);
        }
    }

    pub fn resolve(&self, name: &str) -> Theme {
        self.find(name)
            .map(Theme::from_def)
            .unwrap_or_else(Theme::default_theme)
    }
}

/// Load a `ThemeDef` from a TOML file on disk.
pub fn load_theme_file(path: &Path) -> Result<ThemeDef> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading theme file {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("parsing theme TOML {}", path.display()))
}

/// Parse `#RRGGBB` or `#RRGGBBAA` into GPUI `Hsla`. Falls back to magenta on invalid input.
pub fn parse_hex(hex: &str) -> Hsla {
    let s = hex.trim().trim_start_matches('#');
    let val = match s.len() {
        6 => u32::from_str_radix(s, 16)
            .map(|rgb| (rgb << 8) | 0xFF)
            .unwrap_or(0xFF00FFFF),
        8 => u32::from_str_radix(s, 16).unwrap_or(0xFF00FFFF),
        _ => 0xFF00FFFF,
    };
    rgba(val).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_builtin_themes_resolve_and_roundtrip_toml() {
        let builtins = builtin_themes();
        assert_eq!(builtins.len(), 6);
        for def in &builtins {
            let encoded = toml::to_string_pretty(def).expect("serialize theme");
            let decoded: ThemeDef = toml::from_str(&encoded).expect("deserialize theme");
            assert_eq!(decoded.name, def.name);
            let resolved = Theme::from_def(&decoded);
            assert_eq!(resolved.name, def.name);
        }
    }

    #[test]
    fn hex_parsing_handles_alpha() {
        let opaque = parse_hex("#14161b");
        assert!((opaque.a - 1.0).abs() < 0.01);
        let semi = parse_hex("#6c99bb80");
        assert!((semi.a - 0.5).abs() < 0.02);
    }
}
