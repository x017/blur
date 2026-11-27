//! Developer utilities for Blur (GPUI-free, pure Rust).
//!
//! Provides blocking HTTP requests, JSON formatting/minification/validation/querying,
//! regex testing, base64/URL/hex encoding, UUID v4/v7 generation, timestamp parsing,
//! CSS colour parsing & WCAG contrast, line diffs, and CLI-backed SQL execution.

pub mod color;
pub mod diff;
pub mod encoding;
pub mod http;
pub mod ids;
pub mod json;
pub mod regex_tester;
pub mod sql;
pub mod time;
