//! Keystroke encoding to xterm/VT escape sequences.

/// Encode a key event (using GPUI-style key names) into bytes to write to the PTY.
///
/// Supported key names (case-insensitive):
/// `"enter"`, `"return"`, `"backspace"`, `"tab"`, `"escape"`, `"esc"`, `"space"`,
/// `"up"`, `"down"`, `"left"`, `"right"`, `"home"`, `"end"`, `"pageup"`, `"pagedown"`,
/// `"delete"`, `"insert"`, `"f1"`..`"f12"`, or any single Unicode character.
pub fn encode_key(
    key: &str,
    ctrl: bool,
    alt: bool,
    shift: bool,
    app_cursor: bool,
) -> Option<Vec<u8>> {
    if key.is_empty() {
        return None;
    }

    let mod_param: u8 = 1 + (shift as u8) + ((alt as u8) << 1) + ((ctrl as u8) << 2);
    let has_mod = shift || alt || ctrl;

    let lower = key.to_ascii_lowercase();
    match lower.as_str() {
        "enter" | "return" => {
            if alt {
                return Some(vec![0x1b, b'\r']);
            }
            return Some(vec![b'\r']);
        }
        "backspace" => {
            if ctrl && alt {
                return Some(vec![0x1b, 0x08]);
            } else if ctrl {
                return Some(vec![0x08]);
            } else if alt {
                return Some(vec![0x1b, 0x7f]);
            }
            return Some(vec![0x7f]);
        }
        "tab" => {
            if shift {
                return Some(b"\x1b[Z".to_vec());
            } else if alt {
                return Some(vec![0x1b, b'\t']);
            }
            return Some(vec![b'\t']);
        }
        "escape" | "esc" => {
            if alt {
                return Some(vec![0x1b, 0x1b]);
            }
            return Some(vec![0x1b]);
        }
        "up" => return Some(encode_cursor_key(b'A', mod_param, has_mod, app_cursor)),
        "down" => return Some(encode_cursor_key(b'B', mod_param, has_mod, app_cursor)),
        "right" => return Some(encode_cursor_key(b'C', mod_param, has_mod, app_cursor)),
        "left" => return Some(encode_cursor_key(b'D', mod_param, has_mod, app_cursor)),
        "home" => return Some(encode_cursor_key(b'H', mod_param, has_mod, app_cursor)),
        "end" => return Some(encode_cursor_key(b'F', mod_param, has_mod, app_cursor)),
        "insert" => return Some(encode_tilde_key(2, mod_param, has_mod)),
        "delete" => return Some(encode_tilde_key(3, mod_param, has_mod)),
        "pageup" => return Some(encode_tilde_key(5, mod_param, has_mod)),
        "pagedown" => return Some(encode_tilde_key(6, mod_param, has_mod)),
        "f1" => return Some(encode_ss3_key(b'P', mod_param, has_mod)),
        "f2" => return Some(encode_ss3_key(b'Q', mod_param, has_mod)),
        "f3" => return Some(encode_ss3_key(b'R', mod_param, has_mod)),
        "f4" => return Some(encode_ss3_key(b'S', mod_param, has_mod)),
        "f5" => return Some(encode_tilde_key(15, mod_param, has_mod)),
        "f6" => return Some(encode_tilde_key(17, mod_param, has_mod)),
        "f7" => return Some(encode_tilde_key(18, mod_param, has_mod)),
        "f8" => return Some(encode_tilde_key(19, mod_param, has_mod)),
        "f9" => return Some(encode_tilde_key(20, mod_param, has_mod)),
        "f10" => return Some(encode_tilde_key(21, mod_param, has_mod)),
        "f11" => return Some(encode_tilde_key(23, mod_param, has_mod)),
        "f12" => return Some(encode_tilde_key(24, mod_param, has_mod)),
        "space" => return encode_char(' ', ctrl, alt, shift),
        _ => {}
    }

    let mut chars = key.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        return encode_char(ch, ctrl, alt, shift);
    }

    None
}

fn encode_cursor_key(code: u8, mod_param: u8, has_mod: bool, app_cursor: bool) -> Vec<u8> {
    if has_mod {
        format!("\x1b[1;{}{}", mod_param, code as char).into_bytes()
    } else if app_cursor {
        vec![0x1b, b'O', code]
    } else {
        vec![0x1b, b'[', code]
    }
}

fn encode_ss3_key(code: u8, mod_param: u8, has_mod: bool) -> Vec<u8> {
    if has_mod {
        format!("\x1b[1;{}{}", mod_param, code as char).into_bytes()
    } else {
        vec![0x1b, b'O', code]
    }
}

fn encode_tilde_key(num: u8, mod_param: u8, has_mod: bool) -> Vec<u8> {
    if has_mod {
        format!("\x1b[{};{}~", num, mod_param).into_bytes()
    } else {
        format!("\x1b[{}~", num).into_bytes()
    }
}

fn encode_char(ch: char, ctrl: bool, alt: bool, shift: bool) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(6);
    if alt {
        out.push(0x1b);
    }

    if ctrl {
        let ctrl_byte = match ch {
            'a'..='z' => Some((ch as u8) - b'a' + 1),
            'A'..='Z' => Some((ch as u8) - b'A' + 1),
            '@' | ' ' | '2' => Some(0x00),
            '[' | '3' => Some(0x1b),
            '\\' | '4' => Some(0x1c),
            ']' | '5' => Some(0x1d),
            '^' | '6' => Some(0x1e),
            '_' | '/' | '7' => Some(0x1f),
            '?' | '8' => Some(0x7f),
            _ => None,
        };
        if let Some(b) = ctrl_byte {
            out.push(b);
            return Some(out);
        }
        return None;
    }

    let final_ch = if shift && ch.is_ascii_lowercase() {
        ch.to_ascii_uppercase()
    } else {
        ch
    };
    let mut buf = [0u8; 4];
    let s = final_ch.encode_utf8(&mut buf);
    out.extend_from_slice(s.as_bytes());
    Some(out)
}
