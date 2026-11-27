//! Line-based text diffing via `similar`.

use similar::{ChangeTag, TextDiff};

/// Kind of a line diff operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Equal,
    Insert,
    Delete,
}

/// A single line in a line-by-line diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffOp {
    pub kind: DiffKind,
    /// 1-based line number in the old text, if applicable.
    pub old_line: Option<usize>,
    /// 1-based line number in the new text, if applicable.
    pub new_line: Option<usize>,
    /// Line text (without trailing newline).
    pub text: String,
}

/// Compute a line-by-line diff between `a` (old) and `b` (new).
pub fn line_diff(a: &str, b: &str) -> Vec<DiffOp> {
    let diff = TextDiff::from_lines(a, b);
    diff.iter_all_changes()
        .map(|change| {
            let kind = match change.tag() {
                ChangeTag::Equal => DiffKind::Equal,
                ChangeTag::Insert => DiffKind::Insert,
                ChangeTag::Delete => DiffKind::Delete,
            };
            let text = change
                .value()
                .trim_end_matches('\n')
                .trim_end_matches('\r')
                .to_string();
            DiffOp {
                kind,
                old_line: change.old_index().map(|i| i + 1),
                new_line: change.new_index().map(|i| i + 1),
                text,
            }
        })
        .collect()
}

/// Render a unified diff with `ctx` context lines around changes.
pub fn unified(a: &str, b: &str, ctx: usize) -> String {
    TextDiff::from_lines(a, b)
        .unified_diff()
        .context_radius(ctx)
        .header("a", "b")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_line_diff_and_unified() {
        let a = "alpha\nbeta\ngamma\n";
        let b = "alpha\nBETA\ngamma\ndelta\n";
        let ops = line_diff(a, b);
        assert!(ops.iter().any(|op| op.kind == DiffKind::Delete && op.text == "beta"));
        assert!(ops.iter().any(|op| op.kind == DiffKind::Insert && op.text == "BETA"));
        assert!(ops.iter().any(|op| op.kind == DiffKind::Insert && op.text == "delta"));

        let u = unified(a, b, 1);
        assert!(u.contains("-beta"));
        assert!(u.contains("+BETA"));
        assert!(u.contains("+delta"));
    }
}
