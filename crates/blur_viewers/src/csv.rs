//! Delimiter-sniffed CSV/TSV parser backed by the `csv` crate.

/// A parsed CSV table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Heuristically detect the field delimiter (`,`, `\t`, `;`, or `|`) from the first few lines.
pub fn sniff_delimiter(src: &str) -> u8 {
    let sample_lines: Vec<&str> = src
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(10)
        .collect();
    if sample_lines.is_empty() {
        return b',';
    }

    let candidates = [b',', b'\t', b';', b'|'];
    let mut best_delim = b',';
    let mut best_score = 0_usize;

    for &delim in &candidates {
        let counts: Vec<usize> = sample_lines
            .iter()
            .map(|line| count_unquoted(line, delim as char))
            .collect();
        let first = counts[0];
        if first == 0 {
            continue;
        }
        let consistent = counts.iter().all(|&c| c == first);
        let score = first * 10 + if consistent { 100 } else { 0 };
        if score > best_score {
            best_score = score;
            best_delim = delim;
        }
    }

    best_delim
}

fn count_unquoted(line: &str, target: char) -> usize {
    let mut in_quotes = false;
    let mut count = 0;
    for ch in line.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if !in_quotes && ch == target {
            count += 1;
        }
    }
    count
}

/// Parse CSV/TSV text into a [`Table`].
///
/// Pass `delimiter: None` to auto-detect the delimiter via [`sniff_delimiter`].
pub fn parse(src: &str, delimiter: Option<u8>) -> anyhow::Result<Table> {
    let delim = delimiter.unwrap_or_else(|| sniff_delimiter(src));
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delim)
        .has_headers(true)
        .flexible(true)
        .from_reader(src.as_bytes());

    let headers: Vec<String> = reader.headers()?.iter().map(str::to_string).collect();
    let mut rows = Vec::new();
    for record in reader.records() {
        let rec = record?;
        rows.push(rec.iter().map(str::to_string).collect());
    }

    Ok(Table { headers, rows })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_and_parses_csv_and_tsv() {
        let csv_src = "name,age,city\n\"Alice, A.\",30,Tokyo\nBob,25,Osaka\n";
        assert_eq!(sniff_delimiter(csv_src), b',');
        let tbl = parse(csv_src, None).unwrap();
        assert_eq!(tbl.headers, vec!["name", "age", "city"]);
        assert_eq!(tbl.rows.len(), 2);
        assert_eq!(tbl.rows[0][0], "Alice, A.");

        let tsv_src = "k\tv\na\t1\nb\t2\n";
        assert_eq!(sniff_delimiter(tsv_src), b'\t');
        let tsv = parse(tsv_src, None).unwrap();
        assert_eq!(tsv.headers, vec!["k", "v"]);
        assert_eq!(tsv.rows, vec![vec!["a", "1"], vec!["b", "2"]]);

        let semi_src = "x;y;z\n1;2;3\n";
        assert_eq!(sniff_delimiter(semi_src), b';');
    }
}
