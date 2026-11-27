//! Document and data viewers for Blur (GPUI-free, pure Rust).
//!
//! Parses Markdown/GFM (with math & Mermaid blocks), KaTeX-subset math to Unicode,
//! Mermaid flowcharts (with deterministic layered layout) & sequence diagrams,
//! JSON/YAML/TOML into uniform [`data::DataNode`] trees, and delimiter-sniffed CSV tables.

pub mod csv;
pub mod data;
pub mod markdown;
pub mod math;
pub mod mermaid;
