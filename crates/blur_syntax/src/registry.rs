//! Built-in tree-sitter language definitions and registry.

use std::fmt;
use std::path::Path;
use std::sync::Arc;

use tree_sitter::Query;

use crate::highlight::{map_capture_name, HighlightKind};

/// Describes a programming/markup language, its file associations, comment tokens,
/// tree-sitter grammar, and precompiled highlight query.
pub struct Language {
    pub name: &'static str,
    pub lsp_id: &'static str,
    pub extensions: &'static [&'static str],
    pub filenames: &'static [&'static str],
    pub line_comment: Option<&'static str>,
    pub block_comment: Option<(&'static str, &'static str)>,
    pub(crate) ts_language: tree_sitter::Language,
    pub(crate) highlight_query: Option<Query>,
    pub(crate) capture_kinds: Vec<Option<HighlightKind>>,
}

impl fmt::Debug for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Language")
            .field("name", &self.name)
            .field("lsp_id", &self.lsp_id)
            .field("extensions", &self.extensions)
            .field("filenames", &self.filenames)
            .field("has_highlight_query", &self.highlight_query.is_some())
            .finish()
    }
}

impl Language {
    /// Returns `true` if this language's highlight query compiled successfully.
    pub fn has_highlight_query(&self) -> bool {
        self.highlight_query.is_some()
    }

    /// Returns the underlying `tree_sitter::Language`.
    pub fn ts_language(&self) -> &tree_sitter::Language {
        &self.ts_language
    }
}

struct LangSpec {
    name: &'static str,
    lsp_id: &'static str,
    extensions: &'static [&'static str],
    filenames: &'static [&'static str],
    line_comment: Option<&'static str>,
    block_comment: Option<(&'static str, &'static str)>,
    ts_language: tree_sitter::Language,
    /// Primary highlight query source (may be combined from base + extension queries).
    primary_query: String,
    /// Optional fallback query if the combined query fails to compile.
    fallback_query: Option<&'static str>,
}

fn build_language(spec: LangSpec) -> Arc<Language> {
    let compiled = match Query::new(&spec.ts_language, &spec.primary_query) {
        Ok(q) => Some(q),
        Err(err) => {
            if let Some(fb) = spec.fallback_query {
                match Query::new(&spec.ts_language, fb) {
                    Ok(q) => {
                        tracing::debug!(
                            language = spec.name,
                            "combined highlight query failed ({err}); using fallback query"
                        );
                        Some(q)
                    }
                    Err(fb_err) => {
                        tracing::warn!(
                            language = spec.name,
                            "failed to compile highlight query: {fb_err}"
                        );
                        None
                    }
                }
            } else {
                tracing::warn!(
                    language = spec.name,
                    "failed to compile highlight query: {err}"
                );
                None
            }
        }
    };

    let capture_kinds = compiled
        .as_ref()
        .map(|q| {
            q.capture_names()
                .iter()
                .map(|name| map_capture_name(name))
                .collect()
        })
        .unwrap_or_default();

    Arc::new(Language {
        name: spec.name,
        lsp_id: spec.lsp_id,
        extensions: spec.extensions,
        filenames: spec.filenames,
        line_comment: spec.line_comment,
        block_comment: spec.block_comment,
        ts_language: spec.ts_language,
        highlight_query: compiled,
        capture_kinds,
    })
}

/// Registry of supported languages with path and name lookup.
#[derive(Debug, Clone)]
pub struct LanguageRegistry {
    languages: Vec<Arc<Language>>,
}

impl Default for LanguageRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageRegistry {
    /// Creates a registry populated with all 16 built-in languages.
    pub fn new() -> Self {
        let js_query = tree_sitter_javascript::HIGHLIGHT_QUERY;
        let jsx_query = tree_sitter_javascript::JSX_HIGHLIGHT_QUERY;
        let ts_query = tree_sitter_typescript::HIGHLIGHTS_QUERY;
        let c_query = tree_sitter_c::HIGHLIGHT_QUERY;
        let cpp_query = tree_sitter_cpp::HIGHLIGHT_QUERY;

        let specs = vec![
            LangSpec {
                name: "rust",
                lsp_id: "rust",
                extensions: &["rs"],
                filenames: &[],
                line_comment: Some("//"),
                block_comment: Some(("/*", "*/")),
                ts_language: tree_sitter_rust::LANGUAGE.into(),
                primary_query: tree_sitter_rust::HIGHLIGHTS_QUERY.to_string(),
                fallback_query: None,
            },
            LangSpec {
                name: "python",
                lsp_id: "python",
                extensions: &["py", "pyi", "pyw"],
                filenames: &["SConstruct", "SConscript"],
                line_comment: Some("#"),
                block_comment: None,
                ts_language: tree_sitter_python::LANGUAGE.into(),
                primary_query: tree_sitter_python::HIGHLIGHTS_QUERY.to_string(),
                fallback_query: None,
            },
            LangSpec {
                name: "go",
                lsp_id: "go",
                extensions: &["go"],
                filenames: &[],
                line_comment: Some("//"),
                block_comment: Some(("/*", "*/")),
                ts_language: tree_sitter_go::LANGUAGE.into(),
                primary_query: tree_sitter_go::HIGHLIGHTS_QUERY.to_string(),
                fallback_query: None,
            },
            LangSpec {
                name: "javascript",
                lsp_id: "javascript",
                extensions: &["js", "mjs", "cjs", "jsx"],
                filenames: &[],
                line_comment: Some("//"),
                block_comment: Some(("/*", "*/")),
                ts_language: tree_sitter_javascript::LANGUAGE.into(),
                primary_query: format!("{js_query}\n{jsx_query}"),
                fallback_query: Some(js_query),
            },
            LangSpec {
                name: "typescript",
                lsp_id: "typescript",
                extensions: &["ts", "mts", "cts"],
                filenames: &[],
                line_comment: Some("//"),
                block_comment: Some(("/*", "*/")),
                ts_language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                primary_query: format!("{js_query}\n{ts_query}"),
                fallback_query: Some(ts_query),
            },
            LangSpec {
                name: "tsx",
                lsp_id: "typescriptreact",
                extensions: &["tsx"],
                filenames: &[],
                line_comment: Some("//"),
                block_comment: Some(("/*", "*/")),
                ts_language: tree_sitter_typescript::LANGUAGE_TSX.into(),
                primary_query: format!("{js_query}\n{jsx_query}\n{ts_query}"),
                fallback_query: Some(ts_query),
            },
            LangSpec {
                name: "c",
                lsp_id: "c",
                extensions: &["c", "h"],
                filenames: &[],
                line_comment: Some("//"),
                block_comment: Some(("/*", "*/")),
                ts_language: tree_sitter_c::LANGUAGE.into(),
                primary_query: c_query.to_string(),
                fallback_query: None,
            },
            LangSpec {
                name: "cpp",
                lsp_id: "cpp",
                extensions: &["cpp", "cc", "cxx", "hpp", "hh", "hxx", "c++", "h++"],
                filenames: &[],
                line_comment: Some("//"),
                block_comment: Some(("/*", "*/")),
                ts_language: tree_sitter_cpp::LANGUAGE.into(),
                primary_query: format!("{c_query}\n{cpp_query}"),
                fallback_query: Some(cpp_query),
            },
            LangSpec {
                name: "json",
                lsp_id: "json",
                extensions: &["json", "jsonc"],
                filenames: &[],
                line_comment: None,
                block_comment: None,
                ts_language: tree_sitter_json::LANGUAGE.into(),
                primary_query: tree_sitter_json::HIGHLIGHTS_QUERY.to_string(),
                fallback_query: None,
            },
            LangSpec {
                name: "bash",
                lsp_id: "shellscript",
                extensions: &["sh", "bash", "zsh"],
                filenames: &[".bashrc", ".zshrc", ".bash_profile", ".profile"],
                line_comment: Some("#"),
                block_comment: None,
                ts_language: tree_sitter_bash::LANGUAGE.into(),
                primary_query: tree_sitter_bash::HIGHLIGHT_QUERY.to_string(),
                fallback_query: None,
            },
            LangSpec {
                name: "lua",
                lsp_id: "lua",
                extensions: &["lua"],
                filenames: &[],
                line_comment: Some("--"),
                block_comment: Some(("--[[", "]]")),
                ts_language: tree_sitter_lua::LANGUAGE.into(),
                primary_query: tree_sitter_lua::HIGHLIGHTS_QUERY.to_string(),
                fallback_query: None,
            },
            LangSpec {
                name: "toml",
                lsp_id: "toml",
                extensions: &["toml"],
                filenames: &["Cargo.lock"],
                line_comment: Some("#"),
                block_comment: None,
                ts_language: tree_sitter_toml_ng::LANGUAGE.into(),
                primary_query: tree_sitter_toml_ng::HIGHLIGHTS_QUERY.to_string(),
                fallback_query: None,
            },
            LangSpec {
                name: "markdown",
                lsp_id: "markdown",
                extensions: &["md", "markdown", "mdx"],
                filenames: &[],
                line_comment: None,
                block_comment: Some(("<!--", "-->")),
                ts_language: tree_sitter_md::LANGUAGE.into(),
                primary_query: tree_sitter_md::HIGHLIGHT_QUERY_BLOCK.to_string(),
                fallback_query: None,
            },
            LangSpec {
                name: "html",
                lsp_id: "html",
                extensions: &["html", "htm", "xhtml"],
                filenames: &[],
                line_comment: None,
                block_comment: Some(("<!--", "-->")),
                ts_language: tree_sitter_html::LANGUAGE.into(),
                primary_query: tree_sitter_html::HIGHLIGHTS_QUERY.to_string(),
                fallback_query: None,
            },
            LangSpec {
                name: "css",
                lsp_id: "css",
                extensions: &["css"],
                filenames: &[],
                line_comment: None,
                block_comment: Some(("/*", "*/")),
                ts_language: tree_sitter_css::LANGUAGE.into(),
                primary_query: tree_sitter_css::HIGHLIGHTS_QUERY.to_string(),
                fallback_query: None,
            },
            LangSpec {
                name: "yaml",
                lsp_id: "yaml",
                extensions: &["yaml", "yml"],
                filenames: &[],
                line_comment: Some("#"),
                block_comment: None,
                ts_language: tree_sitter_yaml::LANGUAGE.into(),
                primary_query: tree_sitter_yaml::HIGHLIGHTS_QUERY.to_string(),
                fallback_query: None,
            },
        ];

        Self {
            languages: specs.into_iter().map(build_language).collect(),
        }
    }

    /// Detects the language for a file path by exact filename first, then case-insensitive extension.
    pub fn for_path(&self, p: &Path) -> Option<Arc<Language>> {
        if let Some(fname) = p.file_name().and_then(|f| f.to_str()) {
            for lang in &self.languages {
                if lang.filenames.iter().any(|&f| f.eq_ignore_ascii_case(fname)) {
                    return Some(Arc::clone(lang));
                }
            }
        }
        if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            for lang in &self.languages {
                if lang.extensions.iter().any(|&e| e.eq_ignore_ascii_case(ext)) {
                    return Some(Arc::clone(lang));
                }
            }
        }
        None
    }

    /// Looks up a language by its canonical name or LSP language ID (case-insensitive).
    pub fn by_name(&self, name: &str) -> Option<Arc<Language>> {
        self.languages
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(name) || l.lsp_id.eq_ignore_ascii_case(name))
            .cloned()
    }

    /// Returns all registered languages.
    pub fn all(&self) -> Vec<Arc<Language>> {
        self.languages.clone()
    }
}
