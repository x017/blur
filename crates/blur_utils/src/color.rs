//! CSS-style colour parsing, formatting, and WCAG 2.1 contrast ratio calculation.

use anyhow::{anyhow, bail};

/// An 8-bit sRGBA colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// Construct a colour from RGBA channels.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Construct an opaque colour from RGB channels.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
}

/// Parse a CSS-ish colour string: hex (`#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, with or without `#`),
/// `rgb(...)`, `rgba(...)`, `hsl(...)`, or `hsla(...)`.
pub fn parse(input: &str) -> anyhow::Result<Color> {
    let s = input.trim();
    if s.is_empty() {
        bail!("empty colour string");
    }

    let lower = s.to_ascii_lowercase();
    if let Some(inner) = strip_fn(&lower, "rgba").or_else(|| strip_fn(&lower, "rgb")) {
        return parse_rgb_args(inner);
    }
    if let Some(inner) = strip_fn(&lower, "hsla").or_else(|| strip_fn(&lower, "hsl")) {
        return parse_hsl_args(inner);
    }

    let hex = s.strip_prefix('#').unwrap_or(s);
    parse_hex(hex)
}

fn strip_fn<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(name)?.trim_start();
    let inner = rest.strip_prefix('(')?.strip_suffix(')')?;
    Some(inner.trim())
}

fn parse_hex(hex: &str) -> anyhow::Result<Color> {
    let bytes = hex.as_bytes();
    let hex_byte = |hi: u8, lo: u8| -> anyhow::Result<u8> {
        let h = nibble(hi)?;
        let l = nibble(lo)?;
        Ok((h << 4) | l)
    };
    match bytes.len() {
        3 => Ok(Color::rgb(
            hex_byte(bytes[0], bytes[0])?,
            hex_byte(bytes[1], bytes[1])?,
            hex_byte(bytes[2], bytes[2])?,
        )),
        4 => Ok(Color::rgba(
            hex_byte(bytes[0], bytes[0])?,
            hex_byte(bytes[1], bytes[1])?,
            hex_byte(bytes[2], bytes[2])?,
            hex_byte(bytes[3], bytes[3])?,
        )),
        6 => Ok(Color::rgb(
            hex_byte(bytes[0], bytes[1])?,
            hex_byte(bytes[2], bytes[3])?,
            hex_byte(bytes[4], bytes[5])?,
        )),
        8 => Ok(Color::rgba(
            hex_byte(bytes[0], bytes[1])?,
            hex_byte(bytes[2], bytes[3])?,
            hex_byte(bytes[4], bytes[5])?,
            hex_byte(bytes[6], bytes[7])?,
        )),
        _ => bail!("invalid hex colour '{hex}' (expected 3, 4, 6, or 8 hex digits)"),
    }
}

fn nibble(b: u8) -> anyhow::Result<u8> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => bail!("invalid hex digit '{}'", b as char),
    }
}

fn split_css_args(inner: &str) -> Vec<&str> {
    if inner.contains(',') {
        inner.split(',').map(str::trim).filter(|p| !p.is_empty()).collect()
    } else {
        // CSS Color 4 syntax: "r g b / a" or "r g b"
        inner
            .split(|c: char| c.is_whitespace() || c == '/')
            .filter(|p| !p.is_empty())
            .collect()
    }
}

fn parse_channel(s: &str) -> anyhow::Result<u8> {
    if let Some(pct) = s.strip_suffix('%') {
        let v: f64 = pct.trim().parse().map_err(|_| anyhow!("invalid percentage '{s}'"))?;
        Ok((v.clamp(0.0, 100.0) * 2.55).round() as u8)
    } else {
        let v: f64 = s.parse().map_err(|_| anyhow!("invalid RGB channel '{s}'"))?;
        Ok(v.clamp(0.0, 255.0).round() as u8)
    }
}

fn parse_alpha(s: &str) -> anyhow::Result<u8> {
    if let Some(pct) = s.strip_suffix('%') {
        let v: f64 = pct.trim().parse().map_err(|_| anyhow!("invalid alpha '{s}'"))?;
        Ok((v.clamp(0.0, 100.0) * 2.55).round() as u8)
    } else {
        let v: f64 = s.parse().map_err(|_| anyhow!("invalid alpha '{s}'"))?;
        Ok((v.clamp(0.0, 1.0) * 255.0).round() as u8)
    }
}

fn parse_rgb_args(inner: &str) -> anyhow::Result<Color> {
    let parts = split_css_args(inner);
    if parts.len() != 3 && parts.len() != 4 {
        bail!("rgb()/rgba() expects 3 or 4 arguments, got {}", parts.len());
    }
    let r = parse_channel(parts[0])?;
    let g = parse_channel(parts[1])?;
    let b = parse_channel(parts[2])?;
    let a = if parts.len() == 4 {
        parse_alpha(parts[3])?
    } else {
        255
    };
    Ok(Color::rgba(r, g, b, a))
}

fn parse_hsl_args(inner: &str) -> anyhow::Result<Color> {
    let parts = split_css_args(inner);
    if parts.len() != 3 && parts.len() != 4 {
        bail!("hsl()/hsla() expects 3 or 4 arguments, got {}", parts.len());
    }
    let h_str = parts[0].strip_suffix("deg").unwrap_or(parts[0]).trim();
    let h: f64 = h_str.parse().map_err(|_| anyhow!("invalid hue '{}'", parts[0]))?;
    let s_str = parts[1].strip_suffix('%').unwrap_or(parts[1]).trim();
    let s: f64 = s_str.parse().map_err(|_| anyhow!("invalid saturation '{}'", parts[1]))?;
    let l_str = parts[2].strip_suffix('%').unwrap_or(parts[2]).trim();
    let l: f64 = l_str.parse().map_err(|_| anyhow!("invalid lightness '{}'", parts[2]))?;
    let a = if parts.len() == 4 {
        parse_alpha(parts[3])?
    } else {
        255
    };

    let (r, g, b) = hsl_to_rgb(h, (s / 100.0).clamp(0.0, 1.0), (l / 100.0).clamp(0.0, 1.0));
    Ok(Color::rgba(r, g, b, a))
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    let h = ((h % 360.0) + 360.0) % 360.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    (
        ((r1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((g1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((b1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let rf = r as f64 / 255.0;
    let gf = g as f64 / 255.0;
    let bf = b as f64 / 255.0;
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d.abs() < f64::EPSILON {
        return (0.0, 0.0, l * 100.0);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let mut h = if (max - rf).abs() < f64::EPSILON {
        60.0 * (((gf - bf) / d) % 6.0)
    } else if (max - gf).abs() < f64::EPSILON {
        60.0 * ((bf - rf) / d + 2.0)
    } else {
        60.0 * ((rf - gf) / d + 4.0)
    };
    if h < 0.0 {
        h += 360.0;
    }
    (h, s * 100.0, l * 100.0)
}

/// Format a [`Color`] as `#rrggbb` (or `#rrggbbaa` when `a < 255`).
pub fn to_hex(c: Color) -> String {
    if c.a == 255 {
        format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a)
    }
}

/// Format a [`Color`] as `rgb(r, g, b)` or `rgba(r, g, b, a)`.
pub fn to_rgb_string(c: Color) -> String {
    if c.a == 255 {
        format!("rgb({}, {}, {})", c.r, c.g, c.b)
    } else {
        let a = (c.a as f64 / 255.0 * 100.0).round() / 100.0;
        format!("rgba({}, {}, {}, {a})", c.r, c.g, c.b)
    }
}

/// Format a [`Color`] as `hsl(h, s%, l%)` or `hsla(h, s%, l%, a)`.
pub fn to_hsl_string(c: Color) -> String {
    let (h, s, l) = rgb_to_hsl(c.r, c.g, c.b);
    let h = h.round() as u16 % 360;
    let s = s.round() as u8;
    let l = l.round() as u8;
    if c.a == 255 {
        format!("hsl({h}, {s}%, {l}%)")
    } else {
        let a = (c.a as f64 / 255.0 * 100.0).round() / 100.0;
        format!("hsla({h}, {s}%, {l}%, {a})")
    }
}

fn relative_luminance(c: Color) -> f64 {
    let linearize = |v: u8| {
        let s = v as f64 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linearize(c.r) + 0.7152 * linearize(c.g) + 0.0722 * linearize(c.b)
}

/// Compute the WCAG 2.1 contrast ratio between two colours (range `1.0..=21.0`).
pub fn contrast_ratio(a: Color, b: Color) -> f64 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (lighter, darker) = if la >= lb { (la, lb) } else { (lb, la) };
    (lighter + 0.05) / (darker + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_rgb_and_hsl() {
        assert_eq!(parse("#f0a").unwrap(), Color::rgb(255, 0, 170));
        assert_eq!(parse("#f0a8").unwrap(), Color::rgba(255, 0, 170, 136));
        assert_eq!(parse("#112233").unwrap(), Color::rgb(0x11, 0x22, 0x33));
        assert_eq!(parse("11223380").unwrap(), Color::rgba(0x11, 0x22, 0x33, 0x80));
        assert_eq!(parse("rgb(10, 20, 30)").unwrap(), Color::rgb(10, 20, 30));
        assert_eq!(parse("rgba(10, 20, 30, 0.5)").unwrap(), Color::rgba(10, 20, 30, 128));
        assert_eq!(parse("hsl(0, 100%, 50%)").unwrap(), Color::rgb(255, 0, 0));
        assert_eq!(parse("hsla(120, 100%, 50%, 0.5)").unwrap(), Color::rgba(0, 255, 0, 128));
    }

    #[test]
    fn formats_and_computes_wcag_contrast() {
        let black = Color::rgb(0, 0, 0);
        let white = Color::rgb(255, 255, 255);
        assert_eq!(to_hex(white), "#ffffff");
        assert_eq!(to_rgb_string(black), "rgb(0, 0, 0)");
        assert_eq!(to_hsl_string(Color::rgb(255, 0, 0)), "hsl(0, 100%, 50%)");
        assert!((contrast_ratio(black, white) - 21.0).abs() < 1e-6);
        assert!((contrast_ratio(white, white) - 1.0).abs() < 1e-6);
    }
}
