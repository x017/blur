//! Timestamp parsing and formatting utilities.

use anyhow::{anyhow, bail};
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, SecondsFormat, Utc};

/// Parsed timestamp rendered in multiple common representations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimestampInfo {
    /// Unix timestamp in whole seconds.
    pub unix_s: i64,
    /// Unix timestamp in milliseconds.
    pub unix_ms: i64,
    /// RFC 3339 timestamp in UTC.
    pub rfc3339_utc: String,
    /// Local timezone formatted timestamp (`YYYY-MM-DD HH:MM:SS ±HH:MM`).
    pub local: String,
    /// Human-readable relative description (e.g. `"5 minutes ago"`, `"in 2 days"`).
    pub relative: String,
}

/// Parse a timestamp from Unix seconds, Unix milliseconds, RFC 3339,
/// `"YYYY-MM-DD HH:MM:SS"`, `"YYYY-MM-DD"`, or `"now"`.
pub fn parse_any(input: &str) -> anyhow::Result<TimestampInfo> {
    parse_any_relative_to(input, Utc::now())
}

fn parse_any_relative_to(input: &str, now: DateTime<Utc>) -> anyhow::Result<TimestampInfo> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        bail!("empty timestamp input");
    }

    let dt: DateTime<Utc> = if trimmed.eq_ignore_ascii_case("now") {
        now
    } else if let Ok(num) = trimmed.parse::<i64>() {
        // Heuristic: >= 100 billion (12+ digits, i.e. past year 5138 in seconds) -> milliseconds.
        if num.abs() >= 100_000_000_000 {
            DateTime::<Utc>::from_timestamp_millis(num)
                .ok_or_else(|| anyhow!("unix millisecond timestamp out of range: {num}"))?
        } else {
            DateTime::<Utc>::from_timestamp(num, 0)
                .ok_or_else(|| anyhow!("unix second timestamp out of range: {num}"))?
        }
    } else if let Ok(parsed) = DateTime::parse_from_rfc3339(trimmed) {
        parsed.with_timezone(&Utc)
    } else if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S%.f"))
        .or_else(|_| NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S"))
        .or_else(|_| NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S%.f"))
    {
        naive.and_utc()
    } else if let Ok(date) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        date.and_hms_opt(0, 0, 0)
            .ok_or_else(|| anyhow!("invalid date"))?
            .and_utc()
    } else {
        bail!("unrecognized timestamp format: '{trimmed}'");
    };

    let local_dt = dt.with_timezone(&Local);
    Ok(TimestampInfo {
        unix_s: dt.timestamp(),
        unix_ms: dt.timestamp_millis(),
        rfc3339_utc: dt.to_rfc3339_opts(SecondsFormat::AutoSi, true),
        local: local_dt.format("%Y-%m-%d %H:%M:%S %:z").to_string(),
        relative: format_relative(dt, now),
    })
}

fn format_relative(target: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let diff = now.signed_duration_since(target).num_seconds();
    let abs = diff.unsigned_abs();
    if abs < 5 {
        return "just now".to_string();
    }
    let (amount, unit) = if abs < 60 {
        (abs, "second")
    } else if abs < 3_600 {
        (abs / 60, "minute")
    } else if abs < 86_400 {
        (abs / 3_600, "hour")
    } else if abs < 86_400 * 30 {
        (abs / 86_400, "day")
    } else if abs < 86_400 * 365 {
        (abs / (86_400 * 30), "month")
    } else {
        (abs / (86_400 * 365), "year")
    };
    let plural = if amount == 1 { "" } else { "s" };
    if diff > 0 {
        format!("{amount} {unit}{plural} ago")
    } else {
        format!("in {amount} {unit}{plural}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unix_rfc3339_and_datetime() {
        let now = DateTime::parse_from_rfc3339("2025-01-01T00:10:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let from_s = parse_any_relative_to("1735689600", now).unwrap();
        assert_eq!(from_s.rfc3339_utc, "2025-01-01T00:00:00Z");
        assert_eq!(from_s.relative, "10 minutes ago");

        let from_ms = parse_any_relative_to("1735689600500", now).unwrap();
        assert_eq!(from_ms.unix_ms, 1_735_689_600_500);
        assert_eq!(from_ms.unix_s, 1_735_689_600);

        let from_rfc = parse_any_relative_to("2025-01-01T02:10:00Z", now).unwrap();
        assert_eq!(from_rfc.relative, "in 2 hours");

        let from_plain = parse_any_relative_to("2025-01-01 00:00:00", now).unwrap();
        assert_eq!(from_plain.unix_s, 1_735_689_600);
    }
}
