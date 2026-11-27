//! Base64, URL percent-encoding, and hexadecimal encoding/decoding helpers.

use anyhow::{anyhow, bail};
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;

/// Base64 alphabet variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base64Variant {
    /// Standard alphabet (`+`, `/`) with `=` padding.
    Standard,
    /// URL-safe alphabet (`-`, `_`) with `=` padding.
    UrlSafe,
}

/// Encode bytes as Base64 using the selected alphabet.
pub fn base64_encode(input: &[u8], variant: Base64Variant) -> String {
    match variant {
        Base64Variant::Standard => STANDARD.encode(input),
        Base64Variant::UrlSafe => URL_SAFE.encode(input),
    }
}

/// Decode a Base64 string, tolerating missing `=` padding and surrounding whitespace.
pub fn base64_decode(input: &str, variant: Base64Variant) -> anyhow::Result<Vec<u8>> {
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    let res = match variant {
        Base64Variant::Standard => STANDARD
            .decode(&cleaned)
            .or_else(|_| STANDARD_NO_PAD.decode(cleaned.trim_end_matches('='))),
        Base64Variant::UrlSafe => URL_SAFE
            .decode(&cleaned)
            .or_else(|_| URL_SAFE_NO_PAD.decode(cleaned.trim_end_matches('='))),
    };
    res.map_err(|e| anyhow!("invalid base64: {e}"))
}

/// Percent-encode a string per RFC 3986 (leaves `A-Z`, `a-z`, `0-9`, `-`, `_`, `.`, `~` unescaped).
pub fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            b => {
                out.push('%');
                out.push(HEX_UPPER[(b >> 4) as usize] as char);
                out.push(HEX_UPPER[(b & 0x0f) as usize] as char);
            }
        }
    }
    out
}

/// Percent-decode a URL-encoded string into UTF-8.
pub fn url_decode(input: &str) -> anyhow::Result<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                bail!("truncated percent-encoding at byte {i}");
            }
            let hi = hex_nibble(bytes[i + 1])?;
            let lo = hex_nibble(bytes[i + 2])?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|e| anyhow!("invalid UTF-8 after URL decode: {e}"))
}

/// Encode bytes as lowercase hexadecimal.
pub fn hex_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    for b in input {
        out.push(HEX_LOWER[(b >> 4) as usize] as char);
        out.push(HEX_LOWER[(b & 0x0f) as usize] as char);
    }
    out
}

/// Decode a hexadecimal string into bytes (ignores whitespace and optional `0x` prefix).
pub fn hex_decode(input: &str) -> anyhow::Result<Vec<u8>> {
    let trimmed = input.trim();
    let stripped = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    let cleaned: Vec<u8> = stripped
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if cleaned.len() % 2 != 0 {
        bail!("hex string has odd length ({})", cleaned.len());
    }
    let mut out = Vec::with_capacity(cleaned.len() / 2);
    for pair in cleaned.chunks_exact(2) {
        let hi = hex_nibble(pair[0])?;
        let lo = hex_nibble(pair[1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

/// Submodule alias for hexadecimal helpers (`encoding::hex::encode` / `decode`).
pub mod hex {
    pub use super::{hex_decode as decode, hex_encode as encode};
}

const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";
const HEX_LOWER: &[u8; 16] = b"0123456789abcdef";

fn hex_nibble(b: u8) -> anyhow::Result<u8> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => bail!("invalid hex digit '{}'", b as char),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_roundtrip_and_nopad() {
        let data = b"hello blur?>>";
        let std_enc = base64_encode(data, Base64Variant::Standard);
        assert_eq!(base64_decode(&std_enc, Base64Variant::Standard).unwrap(), data);
        let no_pad = std_enc.trim_end_matches('=');
        assert_eq!(base64_decode(no_pad, Base64Variant::Standard).unwrap(), data);

        let url_enc = base64_encode(data, Base64Variant::UrlSafe);
        assert!(!url_enc.contains('+'));
        assert_eq!(base64_decode(&url_enc, Base64Variant::UrlSafe).unwrap(), data);
    }

    #[test]
    fn url_roundtrip() {
        let s = "hello world & café=100%";
        let enc = url_encode(s);
        assert_eq!(enc, "hello%20world%20%26%20caf%C3%A9%3D100%25");
        assert_eq!(url_decode(&enc).unwrap(), s);
    }

    #[test]
    fn hex_roundtrip() {
        assert_eq!(hex::encode(b"\xde\xad\xbe\xef"), "deadbeef");
        assert_eq!(hex::decode("0xDE AD be ef").unwrap(), b"\xde\xad\xbe\xef");
    }
}
