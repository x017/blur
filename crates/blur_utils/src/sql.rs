//! SQL query execution via external CLIs (`sqlite3`, `psql`, `mysql`).

use std::io::ErrorKind;
use std::process::Command;
use std::time::Instant;

use anyhow::{anyhow, bail};
use serde_json::Value;

/// Supported SQL database engines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlKind {
    Sqlite,
    Postgres,
    MySql,
}

/// Connection descriptor for [`run`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlConnection {
    pub kind: SqlKind,
    /// Database path (SQLite), connection URI/dbname (Postgres), or database name/flags (MySQL).
    pub target: String,
}

/// Tabular result of a SQL query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub elapsed_ms: u64,
}

/// Execute `query` against `conn` using the appropriate external CLI.
///
/// Returns a clear error if the required CLI binary is not installed on `PATH`.
pub fn run(conn: &SqlConnection, query: &str) -> anyhow::Result<SqlResult> {
    let start = Instant::now();
    let (bin, args): (&str, Vec<String>) = match conn.kind {
        SqlKind::Sqlite => (
            "sqlite3",
            vec!["-json".into(), conn.target.clone(), query.into()],
        ),
        SqlKind::Postgres => (
            "psql",
            vec![
                "--csv".into(),
                "-v".into(),
                "ON_ERROR_STOP=1".into(),
                conn.target.clone(),
                "-c".into(),
                query.into(),
            ],
        ),
        SqlKind::MySql => (
            "mysql",
            vec!["--batch".into(), "-e".into(), query.into(), conn.target.clone()],
        ),
    };

    let output = Command::new(bin).args(&args).output().map_err(|err| {
        if err.kind() == ErrorKind::NotFound {
            anyhow!("required SQL CLI '{bin}' not found on PATH")
        } else {
            anyhow!("failed to launch '{bin}': {err}")
        }
    })?;

    let elapsed_ms = start.elapsed().as_millis() as u64;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{bin} exited with {}: {}", output.status, stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let (columns, rows) = match conn.kind {
        SqlKind::Sqlite => parse_sqlite_json(&stdout)?,
        SqlKind::Postgres => parse_psql_csv(&stdout),
        SqlKind::MySql => parse_mysql_tsv(&stdout),
    };

    Ok(SqlResult {
        columns,
        rows,
        elapsed_ms,
    })
}

/// Parse `sqlite3 -json` stdout into `(columns, rows)`.
pub fn parse_sqlite_json(output: &str) -> anyhow::Result<(Vec<String>, Vec<Vec<String>>)> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let val: Value = serde_json::from_str(trimmed)?;
    let arr = val
        .as_array()
        .ok_or_else(|| anyhow!("expected JSON array from sqlite3 -json"))?;
    if arr.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut columns = Vec::new();
    for item in arr {
        if let Some(obj) = item.as_object() {
            for k in obj.keys() {
                if !columns.contains(k) {
                    columns.push(k.clone());
                }
            }
        }
    }
    let mut rows = Vec::with_capacity(arr.len());
    for item in arr {
        let mut row = Vec::with_capacity(columns.len());
        for col in &columns {
            let cell = match item.get(col) {
                None | Some(Value::Null) => "NULL".to_string(),
                Some(Value::String(s)) => s.clone(),
                Some(other) => other.to_string(),
            };
            row.push(cell);
        }
        rows.push(row);
    }
    Ok((columns, rows))
}

/// Parse `psql --csv` stdout into `(columns, rows)`.
pub fn parse_psql_csv(output: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let mut records = parse_simple_csv(output);
    if records.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let columns = records.remove(0);
    (columns, records)
}

/// Parse `mysql --batch` TSV stdout into `(columns, rows)`.
pub fn parse_mysql_tsv(output: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let mut lines = output.lines().filter(|l| !l.is_empty());
    let Some(header) = lines.next() else {
        return (Vec::new(), Vec::new());
    };
    let columns: Vec<String> = header.split('\t').map(str::to_string).collect();
    let rows: Vec<Vec<String>> = lines
        .map(|l| l.split('\t').map(str::to_string).collect())
        .collect();
    (columns, rows)
}

fn parse_simple_csv(input: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
        } else {
            match c {
                '"' => in_quotes = true,
                ',' => {
                    row.push(std::mem::take(&mut field));
                }
                '\r' => {}
                '\n' => {
                    row.push(std::mem::take(&mut field));
                    if !(row.len() == 1 && row[0].is_empty()) {
                        rows.push(std::mem::take(&mut row));
                    } else {
                        row.clear();
                    }
                }
                _ => field.push(c),
            }
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sqlite_psql_and_mysql_outputs() {
        let (cols, rows) =
            parse_sqlite_json(r#"[{"id":1,"name":"alice"},{"id":2,"name":null}]"#).unwrap();
        assert_eq!(cols, vec!["id", "name"]);
        assert_eq!(rows, vec![vec!["1", "alice"], vec!["2", "NULL"]]);

        let (cols, rows) = parse_psql_csv("id,name\n1,\"alice, bob\"\n2,carol\n");
        assert_eq!(cols, vec!["id", "name"]);
        assert_eq!(rows, vec![vec!["1", "alice, bob"], vec!["2", "carol"]]);

        let (cols, rows) = parse_mysql_tsv("id\tname\n1\talice\n2\tbob\n");
        assert_eq!(cols, vec!["id", "name"]);
        assert_eq!(rows, vec![vec!["1", "alice"], vec!["2", "bob"]]);
    }
}
