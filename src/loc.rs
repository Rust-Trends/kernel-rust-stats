//! Line counter for Rust, C and C headers that follows tokei's classification rules.
//!
//! A small, dependency-free reimplementation (the `tokei` crate pulls in a large dependency tree
//! for file walking and parallelism that this tool does not need). Rules, mirroring tokei 15:
//!
//! * Lines are split on `\n` and trimmed.
//! * Outside strings and block comments, an empty line is **blank**.
//! * After scanning a line (tracking strings, block comments and `//` comments), it is a
//!   **comment** line if it starts with `//`, or starts with `/*` and ends with `*/`, or began
//!   inside a block comment, or starts with `/*` and ends inside that comment. A line that ends
//!   inside a string literal is **code**. Everything else is **code**.
//! * Blank lines inside a block comment count as comments; inside a string, as code.
//! * Rust block comments nest. Rust raw strings (`r"…"`, `r#"…"#`, `br"…"`, `cr"…"`) have no
//!   escapes. Char literals such as `'"'` do not open a string (tokei gets this wrong; the
//!   difference is negligible).
//! * Rust doc comments (`///`, `//!`), including code examples inside them, are comment lines
//!   (an empty `///` too). tokei hands doc blocks to a Markdown sub-parser and reports them as a
//!   separate child language, so tokei's *Rust code* figure is the same as ours; its Rust
//!   comment figure leaves doc comments out, ours includes them.

use crate::model::Loc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    C,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Str {
    Normal,
    Raw(usize),
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

fn trim(mut s: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = s {
        if first.is_ascii_whitespace() {
            s = rest;
        } else {
            break;
        }
    }
    while let [rest @ .., last] = s {
        if last.is_ascii_whitespace() {
            s = rest;
        } else {
            break;
        }
    }
    s
}

pub fn count(lang: Lang, src: &[u8]) -> Loc {
    let mut loc = Loc { files: 1, ..Loc::default() };
    let nested = lang == Lang::Rust;
    let mut depth = 0u32;
    let mut string: Option<Str> = None;

    let mut lines: Vec<&[u8]> = src.split(|&b| b == b'\n').collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }

    for raw in lines {
        let line = trim(raw);
        let plain = depth == 0 && string.is_none();


        if plain && line.is_empty() {
            loc.blanks += 1;
            continue;
        }
        let started_in_comment = depth > 0;

        let b = line;
        let n = b.len();
        let mut i = 0;
        while i < n {
            if let Some(s) = string {
                match s {
                    Str::Normal => {
                        if b[i] == b'\\' {
                            i += 2;
                            continue;
                        }
                        if b[i] == b'"' {
                            string = None;
                        }
                    }
                    Str::Raw(h) => {
                        if b[i] == b'"' && b.len() >= i + 1 + h && b[i + 1..i + 1 + h].iter().all(|&c| c == b'#') {
                            string = None;
                            i += 1 + h;
                            continue;
                        }
                    }
                }
                i += 1;
                continue;
            }
            if depth > 0 {
                if b[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else if nested && b[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else {
                    i += 1;
                }
                continue;
            }
            match b[i] {
                b'/' if b[i..].starts_with(b"//") => break,
                b'/' if b[i..].starts_with(b"/*") => {
                    depth = 1;
                    i += 2;
                    continue;
                }
                b'"' => {
                    string = Some(Str::Normal);
                }
                b'r' if lang == Lang::Rust => {
                    let prefix_ok = i == 0
                        || !is_ident(b[i - 1])
                        || ((b[i - 1] == b'b' || b[i - 1] == b'c') && (i < 2 || !is_ident(b[i - 2])));
                    if prefix_ok {
                        let mut j = i + 1;
                        while j < n && b[j] == b'#' {
                            j += 1;
                        }
                        if j < n && b[j] == b'"' {
                            string = Some(Str::Raw(j - i - 1));
                            i = j + 1;
                            continue;
                        }
                    }
                }
                b'\'' => {
                    // Char literal? Skip it so a quote inside does not open a string.
                    if i + 1 < n && b[i + 1] == b'\\' {
                        if let Some(k) = b[(i + 3).min(n)..].iter().take(10).position(|&c| c == b'\'') {
                            i = i + 3 + k + 1;
                            continue;
                        }
                    } else if i + 2 < n && b[i + 2] == b'\'' {
                        i += 3;
                        continue;
                    } else if i + 1 < n && b[i + 1] >= 0xC0 {
                        let len = match b[i + 1] {
                            0xC0..=0xDF => 2,
                            0xE0..=0xEF => 3,
                            _ => 4,
                        };
                        if i + 1 + len < n && b[i + 1 + len] == b'\'' {
                            i += 2 + len;
                            continue;
                        }
                    }
                }
                _ => {}
            }
            i += 1;
        }

        let is_comment = if string.is_some() {
            false
        } else if line.starts_with(b"//") || (line.starts_with(b"/*") && line.ends_with(b"*/")) {
            true
        } else if started_in_comment {
            true
        } else {
            depth > 0 && line.starts_with(b"/*")
        };
        if is_comment {
            loc.comments += 1;
        } else {
            loc.code += 1;
        }
    }
    loc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(lang: Lang, s: &str) -> (u64, u64, u64) {
        let l = count(lang, s.as_bytes());
        (l.code, l.comments, l.blanks)
    }

    #[test]
    fn basics() {
        assert_eq!(c(Lang::C, ""), (0, 0, 0));
        assert_eq!(c(Lang::C, "int x;\n\n// c\n"), (1, 1, 1));
        assert_eq!(c(Lang::C, "int x; // trailing\n"), (1, 0, 0));
        assert_eq!(c(Lang::C, "/* a */\n/*\n\n * b\n */\nint y;\n"), (1, 5, 0));
        // tokei quirk kept: starts with /* and ends with */ is a comment line.
        assert_eq!(c(Lang::C, "/* a */ int x; /* b */\n/* a */ int x;\n"), (1, 1, 0));
        // A line that began in a comment is a comment even if code follows.
        assert_eq!(c(Lang::C, "/* a\n */ int x;\n"), (0, 2, 0));
        // Strings hide comment markers and can span lines.
        assert_eq!(c(Lang::C, "char *s = \"/* no\";\nint z;\n"), (2, 0, 0));
        // The closing line starts with `//`, so tokei (and this counter) call it a comment.
        assert_eq!(c(Lang::C, "s = \"a\\\n\n// in string\";\n"), (2, 1, 0));
        assert_eq!(c(Lang::C, "c = '\"'; /* x\n*/\n"), (1, 1, 0));
        assert_eq!(c(Lang::C, "c = '\\''; /* x\n*/\n"), (1, 1, 0));
        // C does not nest.
        assert_eq!(c(Lang::C, "/* /* */\nint a;\n"), (1, 1, 0));
    }

    #[test]
    fn rust_specifics() {
        // Nested block comments.
        assert_eq!(c(Lang::Rust, "/* /* */\nstill comment */\nlet a = 1;\n"), (1, 2, 0));
        // Raw strings.
        assert_eq!(c(Lang::Rust, "let s = r#\"\" /* \"#;\nlet t = 1;\n"), (2, 0, 0));
        assert_eq!(c(Lang::Rust, "let s = br\"a\\\";\nlet t = 1;\n"), (2, 0, 0));
        // Lifetimes are not char literals.
        assert_eq!(c(Lang::Rust, "fn f<'a>(x: &'a str) {} // c\n/* x */\n"), (1, 1, 0));
        assert_eq!(c(Lang::Rust, "let q = '\"'; let s = \"x\";\n"), (1, 0, 0));
        // Raw identifiers are not raw strings.
        assert_eq!(c(Lang::Rust, "let r#type = \"a\";\n"), (1, 0, 0));
    }

    #[test]
    fn rust_doc_comments() {
        let src = "\
//! Crate docs.
//!
/// Adds.
///
/// ```rust
/// let y = 2;
/// ```
fn add() {}
";
        assert_eq!(c(Lang::Rust, src), (1, 7, 0));
    }

    #[test]
    fn fixture_file() {
        let src = include_str!("../tests/fixtures/sample.rs");
        let l = count(Lang::Rust, src.as_bytes());
        // 80 lines: 14 blank, 5 `//` comment lines, the rest code.
        assert_eq!(l.code + l.comments + l.blanks, src.lines().count() as u64);
        assert_eq!((l.code, l.comments, l.blanks), (61, 5, 14));
    }
}
