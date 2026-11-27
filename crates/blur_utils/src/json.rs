//! JSON formatting, minification, validation, and simple JSONPath-like querying.

use std::fmt;

use anyhow::{anyhow, bail};
use serde::Serialize;
use serde_json::ser::PrettyFormatter;
use serde_json::Value;

/// A JSON syntax error with 1-based line and column numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    /// 1-based line number where the error occurred.
    pub line: usize,
    /// 1-based column number where the error occurred.
    pub col: usize,
    /// Human-readable error description.
    pub message: String,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}, col {}: {}", self.line, self.col, self.message)
    }
}

impl std::error::Error for JsonError {}

/// Validate JSON text, returning a [`JsonError`] with line/column on failure.
pub fn validate(s: &str) -> Result<(), JsonError> {
    match serde_json::from_str::<Value>(s) {
        Ok(_) => Ok(()),
        Err(err) => Err(JsonError {
            line: err.line(),
            col: err.column(),
            message: err.to_string(),
        }),
    }
}

/// Pretty-print JSON with the specified number of spaces per indentation level.
pub fn format(s: &str, indent: usize) -> anyhow::Result<String> {
    let value: Value = serde_json::from_str(s)?;
    let indent_bytes = vec![b' '; indent];
    let formatter = PrettyFormatter::with_indent(&indent_bytes);
    let mut buf = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
    value.serialize(&mut ser)?;
    Ok(String::from_utf8(buf)?)
}

/// Minify JSON into its most compact representation.
pub fn minify(s: &str) -> anyhow::Result<String> {
    let value: Value = serde_json::from_str(s)?;
    Ok(serde_json::to_string(&value)?)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PathSegment {
    Field(String),
    Index(usize),
    Wildcard,
}

fn parse_path(path: &str) -> anyhow::Result<Vec<PathSegment>> {
    let mut s = path.trim();
    if let Some(rest) = s.strip_prefix('$') {
        s = rest;
    }
    let mut segments = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'.' => {
                i += 1;
                if i >= bytes.len() {
                    bail!("trailing '.' in JSON path");
                }
                if bytes[i] == b'*' {
                    segments.push(PathSegment::Wildcard);
                    i += 1;
                } else {
                    let start = i;
                    while i < bytes.len() && bytes[i] != b'.' && bytes[i] != b'[' {
                        i += 1;
                    }
                    if start == i {
                        bail!("empty field name in JSON path at byte {i}");
                    }
                    segments.push(PathSegment::Field(s[start..i].to_string()));
                }
            }
            b'[' => {
                i += 1;
                if i >= bytes.len() {
                    bail!("unclosed '[' in JSON path");
                }
                if bytes[i] == b'*' {
                    i += 1;
                    if i >= bytes.len() || bytes[i] != b']' {
                        bail!("expected ']' after '*' in JSON path");
                    }
                    i += 1;
                    segments.push(PathSegment::Wildcard);
                } else if bytes[i] == b'\'' || bytes[i] == b'"' {
                    let quote = bytes[i];
                    i += 1;
                    let start = i;
                    while i < bytes.len() && bytes[i] != quote {
                        i += 1;
                    }
                    if i >= bytes.len() {
                        bail!("unclosed string in JSON path");
                    }
                    let key = s[start..i].to_string();
                    i += 1; // skip closing quote
                    if i >= bytes.len() || bytes[i] != b']' {
                        bail!("expected ']' after quoted key in JSON path");
                    }
                    i += 1;
                    segments.push(PathSegment::Field(key));
                } else {
                    let start = i;
                    while i < bytes.len() && bytes[i] != b']' {
                        i += 1;
                    }
                    if i >= bytes.len() {
                        bail!("unclosed '[' in JSON path");
                    }
                    let idx_str = s[start..i].trim();
                    let idx: usize = idx_str
                        .parse()
                        .map_err(|_| anyhow!("invalid array index '{idx_str}' in JSON path"))?;
                    i += 1;
                    segments.push(PathSegment::Index(idx));
                }
            }
            _ => {
                // Allow leading bare identifier without leading dot (e.g. "a.b[0]")
                if segments.is_empty() {
                    let start = i;
                    while i < bytes.len() && bytes[i] != b'.' && bytes[i] != b'[' {
                        i += 1;
                    }
                    segments.push(PathSegment::Field(s[start..i].to_string()));
                } else {
                    bail!("unexpected character '{}' in JSON path", bytes[i] as char);
                }
            }
        }
    }

    Ok(segments)
}

/// Evaluate a simple JSONPath-like expression (e.g. `$.a.b[0].c` or `$.items[*].name`)
/// against a JSON string.
///
/// Returns `Value::Array` of matches when wildcards (`*`) are present, or the single
/// matched [`serde_json::Value`] otherwise.
pub fn query(s: &str, path: &str) -> anyhow::Result<Value> {
    let root: Value = serde_json::from_str(s)?;
    let segments = parse_path(path)?;
    let has_wildcard = segments.iter().any(|seg| matches!(seg, PathSegment::Wildcard));

    let mut current: Vec<&Value> = vec![&root];
    for seg in &segments {
        let mut next = Vec::new();
        for val in current {
            match seg {
                PathSegment::Field(name) => {
                    if let Some(child) = val.get(name.as_str()) {
                        next.push(child);
                    }
                }
                PathSegment::Index(idx) => {
                    if let Some(child) = val.get(*idx) {
                        next.push(child);
                    }
                }
                PathSegment::Wildcard => match val {
                    Value::Array(arr) => next.extend(arr.iter()),
                    Value::Object(map) => next.extend(map.values()),
                    _ => {}
                },
            }
        }
        current = next;
    }

    if has_wildcard {
        Ok(Value::Array(current.into_iter().cloned().collect()))
    } else if let Some(first) = current.into_iter().next() {
        Ok(first.clone())
    } else {
        bail!("JSON path '{path}' matched no value")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_and_minify() {
        let raw = r#"{"a":1,"b":[2,3]}"#;
        let pretty = format(raw, 2).unwrap();
        assert!(pretty.contains("  \"a\": 1"));
        assert_eq!(minify(&pretty).unwrap(), raw);
    }

    #[test]
    fn validate_reports_line_and_col() {
        assert!(validate(r#"{"ok": true}"#).is_ok());
        let err = validate("{\n  \"a\": \n}").unwrap_err();
        assert_eq!(err.line, 3);
        assert!(err.col >= 1);
    }

    #[test]
    fn query_paths() {
        let doc = r#"{"a":{"b":[{"c":42},{"c":99}]},"special key":"ok"}"#;
        assert_eq!(query(doc, "$.a.b[0].c").unwrap(), Value::from(42));
        assert_eq!(query(doc, "$.a.b[1].c").unwrap(), Value::from(99));
        assert_eq!(
            query(doc, "$.a.b[*].c").unwrap(),
            serde_json::json!([42, 99])
        );
        assert_eq!(
            query(doc, "$['special key']").unwrap(),
            Value::from("ok")
        );
        assert!(query(doc, "$.missing").is_err());
    }
}
