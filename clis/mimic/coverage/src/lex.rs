//! A TypeScript tokenizer just deep enough for the scans: identifiers, string values, template text, punctuation and the line
//! of each, with comments, regex literals and JSX text skipped. It never fails; a construct it misreads costs one row, not the run.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Ident,
    Str,
    Template,
    Punct,
    Regex,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: Kind,
    /// An identifier's name, a string's value, a template's raw text or the punctuation (`=>` is one token).
    pub text: String,
    pub line: u32,
    /// Column 0 marks a top-level statement in formatted code.
    pub col: u32,
    /// Inside a template's `${}`: it also follows the template token, so a reader of the template's text skips it.
    pub nested: bool,
}

impl Token {
    pub fn is(&self, text: &str) -> bool {
        self.text == text && self.kind != Kind::Str && self.kind != Kind::Template
    }
    pub fn ident(&self) -> Option<&str> {
        (self.kind == Kind::Ident).then_some(self.text.as_str())
    }
    pub fn string(&self) -> Option<&str> {
        (self.kind == Kind::Str).then_some(self.text.as_str())
    }
}

fn ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80
}

/// After these a `/` starts a regex literal, not a division.
fn regex_may_follow(prev: Option<&Token>) -> bool {
    match prev {
        None => true,
        // After `<` it is a JSX closing tag (`</T>`).
        Some(t) if t.kind == Kind::Punct => !matches!(t.text.as_str(), ")" | "]" | "}" | "<" | ">"),
        Some(t) if t.kind == Kind::Ident => matches!(t.text.as_str(), "return" | "typeof" | "case" | "in" | "of" | "delete" | "void" | "throw" | "new" | "do" | "else" | "yield" | "await"),
        Some(_) => false,
    }
}

pub fn lex(src: &str) -> Vec<Token> {
    let mut out = Vec::new();
    lex_into(src, 1, &mut out);
    out
}

fn lex_into(src: &str, first_line: u32, out: &mut Vec<Token>) {
    let b = src.as_bytes();
    let (n, mut i, mut line, mut line_start) = (b.len(), 0usize, first_line, 0usize);
    let push = |out: &mut Vec<Token>, kind, text: String, line, col: usize| out.push(Token { kind, text, line, col: col as u32, nested: false });
    while i < n {
        let c = b[i];
        match c {
            b'\n' => {
                line += 1;
                i += 1;
                line_start = i;
            }
            b' ' | b'\t' | b'\r' => i += 1,
            b'/' if b.get(i + 1) == Some(&b'/') => i = src[i..].find('\n').map_or(n, |k| i + k),
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let end = src[i + 2..].find("*/").map_or(n, |k| i + 2 + k + 2);
                for (k, ch) in b[i..end].iter().enumerate() {
                    if *ch == b'\n' {
                        line += 1;
                        line_start = i + k + 1;
                    }
                }
                i = end;
            }
            // A quote right after a word is JSX text ("Don't"), never the start of a string in code.
            b'"' | b'\'' if i > 0 && ident_byte(b[i - 1]) => i += 1,
            b'"' | b'\'' => {
                let (mut j, mut value) = (i + 1, String::new());
                while j < n && b[j] != c && b[j] != b'\n' {
                    if b[j] == b'\\' && j + 1 < n {
                        let next = src[j + 1..].chars().next().unwrap_or('\\');
                        value.push(next);
                        j += 1 + next.len_utf8();
                        continue;
                    }
                    let ch = src[j..].chars().next().unwrap_or(' ');
                    value.push(ch);
                    j += ch.len_utf8();
                }
                push(out, Kind::Str, value, line, i - line_start);
                i = (j + 1).min(n);
            }
            b'`' => {
                let at = out.len();
                push(out, Kind::Template, String::new(), line, i - line_start);
                let mut raw = String::new();
                i += 1;
                while i < n && b[i] != b'`' {
                    if b[i] == b'\\' {
                        raw.push_str(&src[i..(i + 2).min(n)]);
                        i += 2;
                        continue;
                    }
                    if b[i] == b'$' && b.get(i + 1) == Some(&b'{') {
                        let end = template_expression_end(b, i + 2);
                        raw.push_str(&src[i..(end + 1).min(n)]);
                        let before = out.len();
                        lex_into(&src[i + 2..end.min(n)], line, out);
                        out[before..].iter_mut().for_each(|t| t.nested = true);
                        for (k, ch) in b[i..end.min(n)].iter().enumerate() {
                            if *ch == b'\n' {
                                line += 1;
                                line_start = i + k + 1;
                            }
                        }
                        i = end + 1;
                        continue;
                    }
                    if b[i] == b'\n' {
                        line += 1;
                        line_start = i + 1;
                    }
                    let ch = src[i..].chars().next().unwrap_or(' ');
                    raw.push(ch);
                    i += ch.len_utf8();
                }
                i = (i + 1).min(n);
                out[at].text = raw;
            }
            _ if ident_byte(c) => {
                let mut j = i;
                while j < n && ident_byte(b[j]) {
                    j += 1;
                }
                push(out, Kind::Ident, src[i..j].to_string(), line, i - line_start);
                i = j;
            }
            b'/' if regex_may_follow(out.last()) => match regex_end(b, i) {
                Some(end) => {
                    push(out, Kind::Regex, src[i..end].to_string(), line, i - line_start);
                    i = end;
                }
                None => {
                    push(out, Kind::Punct, "/".into(), line, i - line_start);
                    i += 1;
                }
            },
            b'=' if b.get(i + 1) == Some(&b'>') => {
                push(out, Kind::Punct, "=>".into(), line, i - line_start);
                i += 2;
            }
            _ => {
                let ch = src[i..].chars().next().unwrap_or(' ');
                push(out, Kind::Punct, ch.to_string(), line, i - line_start);
                i += ch.len_utf8();
            }
        }
    }
}

/// The index of the `}` closing a `${` whose body starts at `from`, skipping strings and nested templates in it.
fn template_expression_end(b: &[u8], from: usize) -> usize {
    let (mut depth, mut i) = (0usize, from);
    while i < b.len() {
        match b[i] {
            b'{' => depth += 1,
            b'}' if depth == 0 => return i,
            b'}' => depth -= 1,
            q @ (b'"' | b'\'' | b'`') => {
                i += 1;
                while i < b.len() && b[i] != q {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
            }
            _ => {}
        }
        i += 1;
    }
    b.len()
}

fn regex_end(b: &[u8], from: usize) -> Option<usize> {
    let (mut j, mut class) = (from + 1, false);
    while j < b.len() && b[j] != b'\n' {
        match b[j] {
            b'\\' => j += 1,
            b'[' => class = true,
            b']' => class = false,
            b'/' if !class => {
                j += 1;
                while j < b.len() && ident_byte(b[j]) {
                    j += 1;
                }
                return Some(j);
            }
            _ => {}
        }
        j += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(src: &str) -> Vec<(Kind, String)> {
        lex(src).into_iter().map(|t| (t.kind, t.text)).collect()
    }

    #[test]
    fn strings_comments_and_regexes_do_not_leak_tokens() {
        // Given code with a comment, a regex holding a quote and a JSX apostrophe
        let toks = texts("// a 'x'\nconst r = /it's/g; <T>Don't</T> call(\"a.b\") /* \"c\" */");
        // Then only the code's own tokens come out, with the string value unquoted
        assert!(toks.contains(&(Kind::Str, "a.b".into())), "{toks:?}");
        assert!(!toks.iter().any(|(k, t)| *k == Kind::Str && (t == "x" || t == "c")), "{toks:?}");
        assert!(toks.iter().any(|(k, _)| *k == Kind::Regex));
    }

    #[test]
    fn a_template_keeps_its_raw_text_and_lexes_its_expressions_in_balance() {
        // Given a template whose expression holds braces
        let toks = lex("f(`${a.b}:${ {c: 1}.c }/x`, y)");
        // Then the template token carries the raw text and the expression tokens follow it, braces balanced
        assert_eq!(toks[2].kind, Kind::Template);
        assert_eq!(toks[2].text, "${a.b}:${ {c: 1}.c }/x");
        let opens = toks.iter().filter(|t| t.is("{")).count();
        assert_eq!(opens, toks.iter().filter(|t| t.is("}")).count());
        assert!(toks.iter().any(|t| t.ident() == Some("y")));
    }

    #[test]
    fn lines_and_top_level_columns_are_recorded() {
        let toks = lex("export const A = 1\n  b\nfunction c() {}");
        assert_eq!((toks[0].line, toks[0].col), (1, 0));
        let b = toks.iter().find(|t| t.text == "b").unwrap();
        assert_eq!((b.line, b.col), (2, 2));
        let f = toks.iter().find(|t| t.text == "function").unwrap();
        assert_eq!((f.line, f.col), (3, 0));
    }
}
