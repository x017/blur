//! KaTeX-subset LaTeX math to Unicode converter.

/// Convert a KaTeX/LaTeX math expression into a readable Unicode string.
///
/// Supports Greek letters, common operators/relations, `\frac{a}{b}` (and `\frac a b`),
/// `\sqrt{x}`, `\sqrt[n]{x}`, superscripts (`^`), subscripts (`_`), and text wrappers.
pub fn to_unicode(tex: &str) -> String {
    let chars: Vec<char> = tex.chars().collect();
    let mut pos = 0;
    render_seq(&chars, &mut pos, false)
}

fn render_seq(chars: &[char], pos: &mut usize, stop_at_brace: bool) -> String {
    let mut out = String::new();
    while *pos < chars.len() {
        let c = chars[*pos];
        if stop_at_brace && c == '}' {
            *pos += 1;
            break;
        }
        match c {
            '{' => {
                *pos += 1;
                out.push_str(&render_seq(chars, pos, true));
            }
            '}' => {
                *pos += 1;
            }
            '^' => {
                *pos += 1;
                let arg = read_arg(chars, pos);
                out.push_str(&to_superscript(&arg));
            }
            '_' => {
                *pos += 1;
                let arg = read_arg(chars, pos);
                out.push_str(&to_subscript(&arg));
            }
            '\\' => {
                *pos += 1;
                if *pos >= chars.len() {
                    break;
                }
                let next = chars[*pos];
                if next.is_ascii_alphabetic() {
                    let start = *pos;
                    while *pos < chars.len() && chars[*pos].is_ascii_alphabetic() {
                        *pos += 1;
                    }
                    let cmd: String = chars[start..*pos].iter().collect();
                    handle_command(&cmd, chars, pos, &mut out);
                } else {
                    *pos += 1;
                    match next {
                        ',' | ';' | ':' | ' ' => out.push(' '),
                        '!' => {}
                        '\\' => out.push('\n'),
                        '{' | '}' | '%' | '#' | '&' | '_' | '$' => out.push(next),
                        _ => out.push(next),
                    }
                }
            }
            _ => {
                out.push(c);
                *pos += 1;
            }
        }
    }
    out
}

fn skip_ws(chars: &[char], pos: &mut usize) {
    while *pos < chars.len() && chars[*pos].is_whitespace() {
        *pos += 1;
    }
}

fn read_arg(chars: &[char], pos: &mut usize) -> String {
    skip_ws(chars, pos);
    if *pos >= chars.len() {
        return String::new();
    }
    if chars[*pos] == '{' {
        *pos += 1;
        render_seq(chars, pos, true)
    } else if chars[*pos] == '\\' {
        *pos += 1;
        if *pos < chars.len() && chars[*pos].is_ascii_alphabetic() {
            let start = *pos;
            while *pos < chars.len() && chars[*pos].is_ascii_alphabetic() {
                *pos += 1;
            }
            let cmd: String = chars[start..*pos].iter().collect();
            let mut tmp = String::new();
            handle_command(&cmd, chars, pos, &mut tmp);
            tmp
        } else if *pos < chars.len() {
            let ch = chars[*pos];
            *pos += 1;
            ch.to_string()
        } else {
            String::new()
        }
    } else {
        let ch = chars[*pos];
        *pos += 1;
        ch.to_string()
    }
}

fn read_optional_bracket_arg(chars: &[char], pos: &mut usize) -> Option<String> {
    skip_ws(chars, pos);
    if *pos < chars.len() && chars[*pos] == '[' {
        *pos += 1;
        let start = *pos;
        while *pos < chars.len() && chars[*pos] != ']' {
            *pos += 1;
        }
        let inner: String = chars[start..*pos].iter().collect();
        if *pos < chars.len() && chars[*pos] == ']' {
            *pos += 1;
        }
        Some(to_unicode(&inner))
    } else {
        None
    }
}

fn handle_command(cmd: &str, chars: &[char], pos: &mut usize, out: &mut String) {
    match cmd {
        "frac" | "dfrac" | "tfrac" => {
            let num = read_arg(chars, pos);
            let den = read_arg(chars, pos);
            let wrap = |s: &str| {
                if s.chars().count() > 1 && s.contains(' ') {
                    format!("({s})")
                } else {
                    s.to_string()
                }
            };
            out.push_str(&format!("{}⁄{}", wrap(num.trim()), wrap(den.trim())));
        }
        "sqrt" => {
            if let Some(root) = read_optional_bracket_arg(chars, pos) {
                out.push_str(&to_superscript(&root));
            }
            let inner = read_arg(chars, pos);
            if inner.chars().count() <= 1 {
                out.push('√');
                out.push_str(&inner);
            } else {
                out.push_str(&format!("√({inner})"));
            }
        }
        "text" | "textrm" | "mathrm" | "mathbf" | "mathit" | "mathsf" | "mathtt" | "mathcal"
        | "mathbb" | "operatorname" => {
            let inner = read_arg(chars, pos);
            out.push_str(&inner);
        }
        "left" | "right" | "big" | "Big" | "bigg" | "Bigg" => {
            // Skip sizing prefix; the following delimiter is emitted normally.
        }
        "quad" => out.push_str("  "),
        "qquad" => out.push_str("    "),
        _ => {
            if let Some(sym) = lookup_symbol(cmd) {
                out.push_str(sym);
            } else {
                // Unknown command: preserve readable name
                out.push('\\');
                out.push_str(cmd);
            }
        }
    }
}

fn lookup_symbol(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        // Lowercase Greek
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" => "ϵ",
        "varepsilon" => "ε",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" => "θ",
        "vartheta" => "ϑ",
        "iota" => "ι",
        "kappa" => "κ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "pi" => "π",
        "varpi" => "ϖ",
        "rho" => "ρ",
        "varrho" => "ϱ",
        "sigma" => "σ",
        "varsigma" => "ς",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" => "ϕ",
        "varphi" => "φ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        // Uppercase Greek
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Upsilon" => "Υ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        // Operators & relations
        "times" => "×",
        "div" => "÷",
        "pm" => "±",
        "mp" => "∓",
        "cdot" => "·",
        "circ" => "∘",
        "bullet" => "•",
        "star" => "⋆",
        "oplus" => "⊕",
        "otimes" => "⊗",
        "leq" | "le" => "≤",
        "geq" | "ge" => "≥",
        "neq" | "ne" => "≠",
        "approx" => "≈",
        "equiv" => "≡",
        "sim" => "∼",
        "simeq" => "≃",
        "cong" => "≅",
        "propto" => "∝",
        "ll" => "≪",
        "gg" => "≫",
        "in" => "∈",
        "notin" => "∉",
        "ni" => "∋",
        "subset" => "⊂",
        "supset" => "⊃",
        "subseteq" => "⊆",
        "supseteq" => "⊇",
        "cup" => "∪",
        "cap" => "∩",
        "emptyset" | "varnothing" => "∅",
        "forall" => "∀",
        "exists" => "∃",
        "neg" | "lnot" => "¬",
        "land" | "wedge" => "∧",
        "lor" | "vee" => "∨",
        "to" | "rightarrow" => "→",
        "leftarrow" => "←",
        "leftrightarrow" => "↔",
        "Rightarrow" | "implies" => "⇒",
        "Leftarrow" => "⇐",
        "Leftrightarrow" | "iff" => "⇔",
        "mapsto" => "↦",
        "infty" => "∞",
        "partial" => "∂",
        "nabla" => "∇",
        "sum" => "∑",
        "prod" => "∏",
        "int" => "∫",
        "iint" => "∬",
        "iiint" => "∭",
        "oint" => "∮",
        "ldots" | "dots" => "…",
        "cdots" => "⋯",
        "vdots" => "⋮",
        "ddots" => "⋱",
        "prime" => "′",
        "degree" => "°",
        "angle" => "∠",
        "perp" => "⊥",
        "parallel" => "∥",
        "triangle" => "△",
        "hbar" => "ℏ",
        "ell" => "ℓ",
        "Re" => "ℜ",
        "Im" => "ℑ",
        "aleph" => "ℵ",
        "langle" => "⟨",
        "rangle" => "⟩",
        "lfloor" => "⌊",
        "rfloor" => "⌋",
        "lceil" => "⌈",
        "rceil" => "⌉",
        "sin" => "sin",
        "cos" => "cos",
        "tan" => "tan",
        "log" => "log",
        "ln" => "ln",
        "exp" => "exp",
        "lim" => "lim",
        "max" => "max",
        "min" => "min",
        "det" => "det",
        _ => return None,
    })
}

fn to_superscript(s: &str) -> String {
    let mapped: Option<String> = s
        .chars()
        .map(|c| match c {
            '0' => Some('⁰'),
            '1' => Some('¹'),
            '2' => Some('²'),
            '3' => Some('³'),
            '4' => Some('⁴'),
            '5' => Some('⁵'),
            '6' => Some('⁶'),
            '7' => Some('⁷'),
            '8' => Some('⁸'),
            '9' => Some('⁹'),
            '+' => Some('⁺'),
            '-' => Some('⁻'),
            '=' => Some('⁼'),
            '(' => Some('⁽'),
            ')' => Some('⁾'),
            'n' => Some('ⁿ'),
            'i' => Some('ⁱ'),
            'a' => Some('ᵃ'),
            'b' => Some('ᵇ'),
            'c' => Some('ᶜ'),
            'd' => Some('ᵈ'),
            'e' => Some('ᵉ'),
            'f' => Some('ᶠ'),
            'g' => Some('ᵍ'),
            'h' => Some('ʰ'),
            'j' => Some('ʲ'),
            'k' => Some('ᵏ'),
            'l' => Some('ˡ'),
            'm' => Some('ᵐ'),
            'o' => Some('ᵒ'),
            'p' => Some('ᵖ'),
            'r' => Some('ʳ'),
            's' => Some('ˢ'),
            't' => Some('ᵗ'),
            'u' => Some('ᵘ'),
            'v' => Some('ᵛ'),
            'w' => Some('ʷ'),
            'x' => Some('ˣ'),
            'y' => Some('ʸ'),
            'z' => Some('ᶻ'),
            ' ' => Some(' '),
            _ => None,
        })
        .collect();
    match mapped {
        Some(m) => m,
        None => format!("^({s})"),
    }
}

fn to_subscript(s: &str) -> String {
    let mapped: Option<String> = s
        .chars()
        .map(|c| match c {
            '0' => Some('₀'),
            '1' => Some('₁'),
            '2' => Some('₂'),
            '3' => Some('₃'),
            '4' => Some('₄'),
            '5' => Some('₅'),
            '6' => Some('₆'),
            '7' => Some('₇'),
            '8' => Some('₈'),
            '9' => Some('₉'),
            '+' => Some('₊'),
            '-' => Some('₋'),
            '=' => Some('₌'),
            '(' => Some('₍'),
            ')' => Some('₎'),
            'a' => Some('ₐ'),
            'e' => Some('ₑ'),
            'h' => Some('ₕ'),
            'i' => Some('ᵢ'),
            'j' => Some('ⱼ'),
            'k' => Some('ₖ'),
            'l' => Some('ₗ'),
            'm' => Some('ₘ'),
            'n' => Some('ₙ'),
            'o' => Some('ₒ'),
            'p' => Some('ₚ'),
            'r' => Some('ᵣ'),
            's' => Some('ₛ'),
            't' => Some('ₜ'),
            'u' => Some('ᵤ'),
            'v' => Some('ᵥ'),
            'x' => Some('ₓ'),
            ' ' => Some(' '),
            _ => None,
        })
        .collect();
    match mapped {
        Some(m) => m,
        None => format!("_({s})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_greek_scripts_frac_and_sqrt() {
        assert_eq!(to_unicode(r"E = mc^2"), "E = mc²");
        assert_eq!(to_unicode(r"\alpha + \beta \leq \infty"), "α + β ≤ ∞");
        assert_eq!(to_unicode(r"\frac{a}{b}"), "a⁄b");
        assert_eq!(to_unicode(r"\frac a b"), "a⁄b");
        assert_eq!(to_unicode(r"\sqrt{x+1}"), "√(x+1)");
        assert_eq!(to_unicode(r"\sqrt[3]{8}"), "³√8");
        assert_eq!(to_unicode(r"\sum_{i=1}^{n} x_i"), "∑ᵢ₌₁ⁿ xᵢ");
    }
}
