//! Regular expression tester with flag and capture group support.

use std::ops::Range;

use anyhow::bail;
use regex::RegexBuilder;

/// A single match returned by [`run`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegexMatch {
    /// Byte range of the full match in the input text.
    pub range: Range<usize>,
    /// Matched substring.
    pub text: String,
    /// Capture groups (1-indexed groups; index 0 in this vec corresponds to group 1).
    /// Each entry is `Some((optional_group_name, byte_range, matched_text))` or `None` if unmatched.
    pub groups: Vec<Option<(Option<String>, Range<usize>, String)>>,
}

/// Execute `pattern` with the given `flags` (`i`, `m`, `s`, `x`) against `text`.
pub fn run(pattern: &str, flags: &str, text: &str) -> anyhow::Result<Vec<RegexMatch>> {
    let mut builder = RegexBuilder::new(pattern);
    for ch in flags.chars() {
        match ch {
            'i' => {
                builder.case_insensitive(true);
            }
            'm' => {
                builder.multi_line(true);
            }
            's' => {
                builder.dot_matches_new_line(true);
            }
            'x' => {
                builder.ignore_whitespace(true);
            }
            'g' => {
                // `captures_iter` is always global; accept 'g' gracefully.
            }
            other => bail!("unsupported regex flag: '{other}' (supported: i, m, s, x)"),
        }
    }

    let re = builder.build()?;
    let names: Vec<Option<String>> = re
        .capture_names()
        .skip(1)
        .map(|n| n.map(str::to_string))
        .collect();

    let mut matches = Vec::new();
    for caps in re.captures_iter(text) {
        let Some(full) = caps.get(0) else { continue };
        let mut groups = Vec::with_capacity(names.len());
        for (idx, name) in names.iter().enumerate() {
            let group = caps
                .get(idx + 1)
                .map(|m| (name.clone(), m.range(), m.as_str().to_string()));
            groups.push(group);
        }
        matches.push(RegexMatch {
            range: full.range(),
            text: full.as_str().to_string(),
            groups,
        });
    }

    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_and_numbered_groups_and_flags() {
        let matches = run(r"(?P<word>[a-z]+)-(\d+)", "i", "Foo-12 bar-34").unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].text, "Foo-12");
        assert_eq!(matches[0].range, 0..6);
        assert_eq!(
            matches[0].groups[0],
            Some((Some("word".to_string()), 0..3, "Foo".to_string()))
        );
        assert_eq!(
            matches[0].groups[1],
            Some((None, 4..6, "12".to_string()))
        );
    }

    #[test]
    fn multiline_and_dotall_flags() {
        let m = run(r"^line\d$", "m", "line1\nline2").unwrap();
        assert_eq!(m.len(), 2);
        let s = run(r"a.*b", "s", "a\nb").unwrap();
        assert_eq!(s.len(), 1);
        assert!(run(r"a", "z", "a").is_err());
    }
}
