//! Aperçu approximatif de formules LaTeX en Unicode (lettres grecques, opérateurs, exposants, indices, fractions simples).
//!
//! Ce n'est pas un moteur de composition : les constructions non reconnues sont conservées telles quelles.

const SYMBOLS: &[(&str, &str)] = &[
    ("alpha", "α"),
    ("beta", "β"),
    ("gamma", "γ"),
    ("delta", "δ"),
    ("epsilon", "ε"),
    ("varepsilon", "ε"),
    ("zeta", "ζ"),
    ("eta", "η"),
    ("theta", "θ"),
    ("iota", "ι"),
    ("kappa", "κ"),
    ("lambda", "λ"),
    ("mu", "μ"),
    ("nu", "ν"),
    ("xi", "ξ"),
    ("pi", "π"),
    ("rho", "ρ"),
    ("sigma", "σ"),
    ("tau", "τ"),
    ("upsilon", "υ"),
    ("phi", "φ"),
    ("varphi", "φ"),
    ("chi", "χ"),
    ("psi", "ψ"),
    ("omega", "ω"),
    ("Gamma", "Γ"),
    ("Delta", "Δ"),
    ("Theta", "Θ"),
    ("Lambda", "Λ"),
    ("Xi", "Ξ"),
    ("Pi", "Π"),
    ("Sigma", "Σ"),
    ("Phi", "Φ"),
    ("Psi", "Ψ"),
    ("Omega", "Ω"),
    ("sum", "∑"),
    ("prod", "∏"),
    ("int", "∫"),
    ("oint", "∮"),
    ("partial", "∂"),
    ("nabla", "∇"),
    ("infty", "∞"),
    ("pm", "±"),
    ("mp", "∓"),
    ("times", "×"),
    ("div", "÷"),
    ("cdot", "·"),
    ("ast", "∗"),
    ("leq", "≤"),
    ("le", "≤"),
    ("geq", "≥"),
    ("ge", "≥"),
    ("neq", "≠"),
    ("ne", "≠"),
    ("approx", "≈"),
    ("equiv", "≡"),
    ("sim", "∼"),
    ("propto", "∝"),
    ("in", "∈"),
    ("notin", "∉"),
    ("subset", "⊂"),
    ("subseteq", "⊆"),
    ("supset", "⊃"),
    ("cup", "∪"),
    ("cap", "∩"),
    ("emptyset", "∅"),
    ("forall", "∀"),
    ("exists", "∃"),
    ("neg", "¬"),
    ("land", "∧"),
    ("lor", "∨"),
    ("to", "→"),
    ("rightarrow", "→"),
    ("leftarrow", "←"),
    ("leftrightarrow", "↔"),
    ("Rightarrow", "⇒"),
    ("Leftarrow", "⇐"),
    ("Leftrightarrow", "⇔"),
    ("mapsto", "↦"),
    ("ldots", "…"),
    ("cdots", "⋯"),
    ("dots", "…"),
    ("degree", "°"),
    ("angle", "∠"),
    ("perp", "⊥"),
    ("parallel", "∥"),
    ("therefore", "∴"),
    ("hbar", "ℏ"),
    ("ell", "ℓ"),
    ("langle", "⟨"),
    ("rangle", "⟩"),
    ("lfloor", "⌊"),
    ("rfloor", "⌋"),
    ("lceil", "⌈"),
    ("rceil", "⌉"),
    ("quad", "  "),
    ("qquad", "    "),
    ("lbrace", "{"),
    ("rbrace", "}"),
    ("left", ""),
    ("right", ""),
];

fn sup_char(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        '+' => '⁺',
        '-' | '−' => '⁻',
        '=' => '⁼',
        '(' => '⁽',
        ')' => '⁾',
        'n' => 'ⁿ',
        'i' => 'ⁱ',
        'x' => 'ˣ',
        'y' => 'ʸ',
        _ => return None,
    })
}

fn sub_char(c: char) -> Option<char> {
    Some(match c {
        '0' => '₀',
        '1' => '₁',
        '2' => '₂',
        '3' => '₃',
        '4' => '₄',
        '5' => '₅',
        '6' => '₆',
        '7' => '₇',
        '8' => '₈',
        '9' => '₉',
        '+' => '₊',
        '-' | '−' => '₋',
        '=' => '₌',
        '(' => '₍',
        ')' => '₎',
        'a' => 'ₐ',
        'e' => 'ₑ',
        'i' => 'ᵢ',
        'j' => 'ⱼ',
        'k' => 'ₖ',
        'n' => 'ₙ',
        'm' => 'ₘ',
        'o' => 'ₒ',
        'x' => 'ₓ',
        _ => return None,
    })
}

/// Convertit `s` en exposant (ou indice) si chaque caractère a un équivalent Unicode ; sinon `^(…)` / `_(…)`.
fn script(s: &str, up: bool) -> String {
    let map = |c: char| if up { sup_char(c) } else { sub_char(c) };
    let conv: Option<String> = s.chars().map(map).collect();
    match conv {
        Some(c) => c,
        None if s.chars().count() == 1 => format!("{}{s}", if up { '^' } else { '_' }),
        None => format!("{}({s})", if up { '^' } else { '_' }),
    }
}

struct Parser<'a> {
    chars: Vec<char>,
    pos: usize,
    _src: &'a str,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Self { chars: src.chars().collect(), pos: 0, _src: src }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    /// Lit un argument : groupe `{…}` (converti récursivement) ou un seul caractère / une commande.
    fn arg(&mut self) -> String {
        while self.peek() == Some(' ') {
            self.pos += 1;
        }
        match self.peek() {
            Some('{') => {
                self.pos += 1;
                let start = self.pos;
                let mut depth = 1;
                while let Some(c) = self.peek() {
                    match c {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    self.pos += 1;
                }
                let inner: String = self.chars[start..self.pos].iter().collect();
                self.pos = (self.pos + 1).min(self.chars.len());
                to_unicode(&inner)
            }
            Some('\\') => self.command(),
            Some(c) => {
                self.pos += 1;
                c.to_string()
            }
            None => String::new(),
        }
    }

    fn command(&mut self) -> String {
        self.pos += 1; // '\'
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.pos += 1;
        }
        if self.pos == start {
            // commande d'un seul caractère : \, \; \{ \} \\ …
            let c = self.peek();
            self.pos += usize::from(c.is_some());
            return match c {
                Some(',' | ';' | ':' | '!') => " ".into(),
                Some('\\') => "\n".into(),
                Some(c) => c.to_string(),
                None => String::new(),
            };
        }
        let name: String = self.chars[start..self.pos].iter().collect();
        match name.as_str() {
            "frac" | "dfrac" | "tfrac" => {
                let (n, d) = (self.arg(), self.arg());
                let wrap = |s: String| if s.chars().count() > 1 && !s.chars().all(|c| c.is_alphanumeric()) { format!("({s})") } else { s };
                format!("{}⁄{}", wrap(n), wrap(d))
            }
            "sqrt" => {
                let r = self.arg();
                if r.chars().count() > 1 { format!("√({r})") } else { format!("√{r}") }
            }
            "text" | "mathrm" | "mathbf" | "mathit" | "operatorname" | "mathbb" | "mathcal" => {
                let a = self.arg();
                if name == "mathbb" { a.chars().map(blackboard).collect() } else { a }
            }
            "overline" | "bar" => {
                let a = self.arg();
                a.chars().flat_map(|c| [c, '\u{0304}']).collect()
            }
            "vec" => {
                let a = self.arg();
                a.chars().flat_map(|c| [c, '\u{20d7}']).collect()
            }
            "hat" => {
                let a = self.arg();
                a.chars().flat_map(|c| [c, '\u{0302}']).collect()
            }
            "sin" | "cos" | "tan" | "log" | "ln" | "exp" | "lim" | "max" | "min" | "det" | "dim" | "ker" | "arg" | "sup" | "inf" => name,
            other => SYMBOLS.iter().find(|(k, _)| *k == other).map_or_else(|| format!("\\{other}"), |(_, v)| (*v).to_owned()),
        }
    }
}

fn blackboard(c: char) -> char {
    match c {
        'R' => 'ℝ',
        'N' => 'ℕ',
        'Z' => 'ℤ',
        'Q' => 'ℚ',
        'C' => 'ℂ',
        'P' => 'ℙ',
        _ => c,
    }
}

/// Convertit une formule LaTeX en texte Unicode (approximation lisible).
pub fn to_unicode(src: &str) -> String {
    let mut p = Parser::new(src);
    let mut out = String::new();
    while let Some(c) = p.peek() {
        match c {
            '\\' => out.push_str(&p.command()),
            '^' => {
                p.pos += 1;
                let a = p.arg();
                out.push_str(&script(&a, true));
            }
            '_' => {
                p.pos += 1;
                let a = p.arg();
                out.push_str(&script(&a, false));
            }
            '{' => out.push_str(&p.arg()),
            '}' | '$' => p.pos += 1,
            '~' => {
                out.push(' ');
                p.pos += 1;
            }
            '-' => {
                out.push('−');
                p.pos += 1;
            }
            c => {
                out.push(c);
                p.pos += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greek_letters_and_operators() {
        assert_eq!(to_unicode(r"\alpha + \beta \leq \pi"), "α + β ≤ π");
        assert_eq!(to_unicode(r"x \neq y \Rightarrow z"), "x ≠ y ⇒ z");
    }

    #[test]
    fn superscripts_and_subscripts() {
        assert_eq!(to_unicode("x^2 + y^{10}"), "x² + y¹⁰");
        assert_eq!(to_unicode("a_1 + a_{n+1}"), "a₁ + aₙ₊₁");
        assert_eq!(to_unicode("e^{i\\pi}"), "e^(iπ)");
    }

    #[test]
    fn fractions_roots_and_sums() {
        assert_eq!(to_unicode(r"\frac{a}{b}"), "a⁄b");
        assert_eq!(to_unicode(r"\frac{a+b}{c}"), "(a+b)⁄c");
        assert_eq!(to_unicode(r"\sqrt{x}"), "√x");
        assert_eq!(to_unicode(r"\sqrt{x+1}"), "√(x+1)");
        assert_eq!(to_unicode(r"\sum_{i=1}^{n} i"), "∑ᵢ₌₁ⁿ i");
        assert_eq!(to_unicode(r"\int_0^1 f"), "∫₀¹ f");
    }

    #[test]
    fn unknown_commands_are_kept_and_blackboard_works() {
        assert_eq!(to_unicode(r"\foo{x}"), r"\foox");
        assert_eq!(to_unicode(r"x \in \mathbb{R}"), "x ∈ ℝ");
        assert_eq!(to_unicode("$a-b$"), "a−b");
    }
}
