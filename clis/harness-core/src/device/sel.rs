//! Maestro's selector: `{id, text, css, point}`, or a bare string meaning `{text=...}`. A pattern keeps to one regex grammar
//! that the Java UiSelector, JavaScript and Rust read alike, and matches Maestro's way: case-insensitive, a full match or
//! literal equality, a newline in the value read as a space.
use regex_syntax::ast::{self, Ast, ClassPerlKind, ClassSet, ClassSetItem, GroupKind, LiteralKind};
use serde_json::{json, Map, Value};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Coord {
    Px(f64),
    Pct(f64),
}

/// A screen point, `'x,y'` in tap units or `'x%,y%'` of the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: Coord,
    pub y: Coord,
}

fn num(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        v.to_string()
    }
}

impl Point {
    pub fn parse(s: &str) -> Result<Point, String> {
        let bad = || format!("point '{s}' must be 'x,y' in tap units or 'x%,y%' of the screen");
        let (x, y) = s.split_once(',').ok_or_else(bad)?;
        let coord = |t: &str| -> Option<Coord> {
            let t = t.trim();
            match t.strip_suffix('%') {
                Some(p) => p.trim().parse::<f64>().ok().filter(|v| (0.0..=100.0).contains(v)).map(Coord::Pct),
                None => t.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0).map(Coord::Px),
            }
        };
        match (coord(x).ok_or_else(bad)?, coord(y).ok_or_else(bad)?) {
            (x @ Coord::Px(_), y @ Coord::Px(_)) | (x @ Coord::Pct(_), y @ Coord::Pct(_)) => Ok(Point { x, y }),
            _ => Err(bad()),
        }
    }

    pub fn is_pct(&self) -> bool {
        matches!(self.x, Coord::Pct(_))
    }

    /// The point on a `width` x `height` screen (tap units, or screenshot pixels for a screenshot's size).
    pub fn resolve(&self, width: f64, height: f64) -> (f64, f64) {
        let one = |c: Coord, size: f64| match c {
            Coord::Px(v) => v,
            Coord::Pct(p) => p * size / 100.0,
        };
        (one(self.x, width), one(self.y, height))
    }

    pub fn show(&self) -> String {
        let one = |c: Coord| match c {
            Coord::Px(v) => num(v),
            Coord::Pct(p) => format!("{}%", num(p)),
        };
        format!("{},{}", one(self.x), one(self.y))
    }
}

/// The escapes the three engines read alike: the syntax characters, and `/`.
const SYNTAX: &str = "^$\\.*+?()[]{}|/";

const GRAMMAR: &str = "literals, ., [...] with ranges and ^, \\d \\w \\s \\D \\W \\S, escapes of ^ $ \\ . * + ? ( ) [ ] { } | /, quantifiers, (...) and (?:...), |, ^ and $";

/// A regex in the shared grammar, with each engine's form built once: Java for the UiSelector, JavaScript for the page, Rust
/// for the dump fallback and the harness itself.
#[derive(Clone, Debug)]
pub struct Pattern {
    src: String,
    java: String,
    js: String,
    rust: regex::Regex,
}

impl PartialEq for Pattern {
    fn eq(&self, other: &Self) -> bool {
        self.src == other.src
    }
}

/// One rewrite of the source at a span, per engine (None keeps the source text there).
struct Edit {
    start: usize,
    end: usize,
    java: Option<&'static str>,
    js: Option<&'static str>,
    rust: Option<&'static str>,
}

fn refuse(src: &str, what: &str) -> String {
    format!("pattern '{src}': {what} is outside the regex grammar every platform reads alike ({GRAMMAR})")
}

fn span_text<'a>(src: &'a str, span: &ast::Span) -> &'a str {
    &src[span.start.offset..span.end.offset]
}

fn literal(src: &str, l: &ast::Literal, in_class: bool) -> Result<(), String> {
    match l.kind {
        LiteralKind::Verbatim if !in_class && matches!(l.c, '{' | '}' | ']') => Err(refuse(src, &format!("an unescaped '{}' (write \\{})", l.c, l.c))),
        LiteralKind::Verbatim if in_class && matches!(l.c, '[' | ']') => Err(refuse(src, &format!("an unescaped '{}' inside [...] (write \\{})", l.c, l.c))),
        LiteralKind::Verbatim => Ok(()),
        LiteralKind::Meta | LiteralKind::Superfluous if SYNTAX.contains(l.c) => Ok(()),
        _ => Err(refuse(src, &format!("the escape '{}'", span_text(src, &l.span)))),
    }
}

fn perl(p: &ast::ClassPerl, in_class: bool, edits: &mut Vec<Edit>) {
    let rust = match (p.kind.clone(), p.negated, in_class) {
        (ClassPerlKind::Digit, false, false) => "[[:digit:]]",
        (ClassPerlKind::Digit, true, false) => "[[:^digit:]]",
        (ClassPerlKind::Word, false, false) => "[[:word:]]",
        (ClassPerlKind::Word, true, false) => "[[:^word:]]",
        (ClassPerlKind::Space, false, false) => "[[:space:]]",
        (ClassPerlKind::Space, true, false) => "[[:^space:]]",
        (ClassPerlKind::Digit, false, true) => "[:digit:]",
        (ClassPerlKind::Digit, true, true) => "[:^digit:]",
        (ClassPerlKind::Word, false, true) => "[:word:]",
        (ClassPerlKind::Word, true, true) => "[:^word:]",
        (ClassPerlKind::Space, false, true) => "[:space:]",
        (ClassPerlKind::Space, true, true) => "[:^space:]",
    };
    // JavaScript's \s also takes Unicode spaces; Java's and the ASCII class above do not
    let js = match (p.kind.clone(), p.negated, in_class) {
        (ClassPerlKind::Space, false, false) => Some("[\\t\\n\\v\\f\\r ]"),
        (ClassPerlKind::Space, true, false) => Some("[^\\t\\n\\v\\f\\r ]"),
        (ClassPerlKind::Space, false, true) => Some("\\t\\n\\v\\f\\r "),
        _ => None,
    };
    edits.push(Edit { start: p.span.start.offset, end: p.span.end.offset, java: None, js, rust: Some(rust) });
}

/// A literal space also matches a newline, so a value's newline reads as a space.
fn space(l: &ast::Literal, in_class: bool, edits: &mut Vec<Edit>) {
    if l.c == ' ' && l.kind == LiteralKind::Verbatim {
        let with = if in_class { " \\n" } else { "[ \\n]" };
        edits.push(Edit { start: l.span.start.offset, end: l.span.end.offset, java: Some(with), js: Some(with), rust: Some(with) });
    }
}

fn class_item(src: &str, item: &ClassSetItem, edits: &mut Vec<Edit>) -> Result<(), String> {
    match item {
        ClassSetItem::Empty(_) => Ok(()),
        ClassSetItem::Literal(l) => {
            literal(src, l, true)?;
            space(l, true, edits);
            Ok(())
        }
        ClassSetItem::Range(r) => literal(src, &r.start, true).and_then(|()| literal(src, &r.end, true)),
        ClassSetItem::Ascii(_) => Err(refuse(src, "a POSIX class such as [:alpha:]")),
        ClassSetItem::Unicode(_) => Err(refuse(src, "a Unicode class \\p{...}")),
        ClassSetItem::Perl(p) => {
            perl(p, true, edits);
            Ok(())
        }
        ClassSetItem::Bracketed(_) => Err(refuse(src, "a class inside a class")),
        ClassSetItem::Union(u) => u.items.iter().try_for_each(|i| class_item(src, i, edits)),
    }
}

fn walk(src: &str, node: &Ast, edits: &mut Vec<Edit>) -> Result<(), String> {
    match node {
        Ast::Empty(_) | Ast::Dot(_) => Ok(()),
        Ast::Flags(_) => Err(refuse(src, "an inline flag such as (?i)")),
        Ast::Literal(l) => {
            literal(src, l, false)?;
            space(l, false, edits);
            Ok(())
        }
        Ast::Assertion(a) => match a.kind {
            ast::AssertionKind::StartLine | ast::AssertionKind::EndLine => Ok(()),
            _ => Err(refuse(src, &format!("the assertion '{}'", span_text(src, &a.span)))),
        },
        Ast::ClassUnicode(_) => Err(refuse(src, "a Unicode class \\p{...}")),
        Ast::ClassPerl(p) => {
            perl(p, false, edits);
            Ok(())
        }
        Ast::ClassBracketed(c) => match &c.kind {
            ClassSet::Item(item) => class_item(src, item, edits),
            ClassSet::BinaryOp(_) => Err(refuse(src, "a class operation (&&, --, ~~)")),
        },
        Ast::Repetition(r) => {
            if matches!(*r.ast, Ast::Assertion(_) | Ast::Empty(_)) {
                return Err(refuse(src, "a quantifier on an anchor"));
            }
            if span_text(src, &r.op.span).starts_with("{,") {
                return Err(refuse(src, "{,n} (write {0,n})"));
            }
            walk(src, &r.ast, edits)
        }
        Ast::Group(g) => match &g.kind {
            GroupKind::CaptureIndex(_) => walk(src, &g.ast, edits),
            GroupKind::NonCapturing(flags) if flags.items.is_empty() => walk(src, &g.ast, edits),
            GroupKind::NonCapturing(_) => Err(refuse(src, "an inline flag such as (?i:...)")),
            GroupKind::CaptureName { .. } => Err(refuse(src, "a named group")),
        },
        Ast::Alternation(a) => a.asts.iter().try_for_each(|x| walk(src, x, edits)),
        Ast::Concat(c) => c.asts.iter().try_for_each(|x| walk(src, x, edits)),
    }
}

fn rewrite(src: &str, edits: &[Edit], pick: impl Fn(&Edit) -> Option<&'static str>) -> String {
    let mut out = String::with_capacity(src.len() + 16);
    let mut at = 0;
    for e in edits {
        if let Some(with) = pick(e) {
            out.push_str(&src[at..e.start]);
            out.push_str(with);
            at = e.end;
        }
    }
    out.push_str(&src[at..]);
    out
}

/// The source as plain text: equality with it is the literal alternative, a space again also matching a newline.
fn literal_form(src: &str) -> String {
    let mut out = String::with_capacity(src.len() * 2);
    for c in src.chars() {
        match c {
            ' ' => out.push_str("[ \\n]"),
            c if "^$\\.*+?()[]{}|".contains(c) => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out
}

impl Pattern {
    pub fn parse(src: &str) -> Result<Pattern, String> {
        let tree = ast::parse::Parser::new().parse(src).map_err(|e| format!("pattern '{src}': {}", e.kind()))?;
        let mut edits = Vec::new();
        walk(src, &tree, &mut edits)?;
        edits.sort_by_key(|e| e.start);
        let lit = literal_form(src);
        let java = format!("(?ismu)(?:{})|(?:{lit})", rewrite(src, &edits, |e| e.java));
        let js = format!("(?<![\\s\\S])(?:(?:{})|(?:{lit}))(?![\\s\\S])", rewrite(src, &edits, |e| e.js));
        let rust_src = format!("\\A(?:(?:{})|(?:{lit}))\\z", rewrite(src, &edits, |e| e.rust));
        let rust = regex::RegexBuilder::new(&rust_src).case_insensitive(true).dot_matches_new_line(true).multi_line(true).build().map_err(|e| format!("pattern '{src}': {e}"))?;
        Ok(Pattern { src: src.to_string(), java, js, rust })
    }

    /// A literal text as a pattern that matches exactly it (its regex characters escaped).
    pub fn escape(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for c in text.chars() {
            if "^$\\.*+?()[]{}|".contains(c) {
                out.push('\\');
            }
            out.push(c);
        }
        out
    }

    pub fn source(&self) -> &str {
        &self.src
    }

    /// For the UiSelector's textMatches and descriptionMatches, which match the whole value.
    pub fn java(&self) -> &str {
        &self.java
    }

    /// For resourceIdMatches: the whole resource id, or the part after its last '/', as Maestro's idMatches.
    pub fn java_id(&self) -> String {
        format!("(?ismu)(?:.*/(?=[^/]*\\z))?(?:{})", &self.java["(?ismu)".len()..])
    }

    /// The Rust reading of `java_id`, for the dump fallback.
    pub fn is_id_match(&self, id: &str) -> bool {
        !id.is_empty() && (self.is_match(id) || id.rsplit_once('/').is_some_and(|(_, last)| self.is_match(last)))
    }

    /// The RegExp source and flags for the page: `new RegExp(source, flags).test(value)` is the match.
    pub fn js(&self) -> (&str, &'static str) {
        (&self.js, "imsu")
    }

    pub fn is_match(&self, value: &str) -> bool {
        self.rust.is_match(value)
    }
}

/// Maestro's selector. Every given field must hold.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sel {
    pub id: Option<Pattern>,
    pub text: Option<Pattern>,
    pub css: Option<String>,
    pub point: Option<Point>,
}

const OLD_PREFIXES: [(&str, &str); 5] = [("id:", "id"), ("text:", "text"), ("css:", "css"), ("xy:", "point"), ("label:", "text")];

impl Sel {
    /// A Lua table or CLI flags turned into JSON: `{id, text, css, point}`, or a string meaning `{text=<string>}`.
    pub fn from_json(v: &Value) -> Result<Sel, String> {
        match v {
            Value::String(s) => {
                for (prefix, field) in OLD_PREFIXES {
                    if let Some(rest) = s.strip_prefix(prefix) {
                        let new = if field == "text" { format!(".*{}.*", Pattern::escape(rest)) } else { rest.to_string() };
                        return Err(format!("old selector form '{s}': use {{{field}='{new}'}}"));
                    }
                }
                Ok(Sel { text: Some(Pattern::parse(s)?), ..Sel::default() })
            }
            Value::Object(m) => {
                let mut sel = Sel::default();
                for (k, v) in m {
                    let s = v.as_str().ok_or_else(|| format!("selector field {k} must be a string, got {v}"))?;
                    match k.as_str() {
                        "id" => sel.id = Some(Pattern::parse(s)?),
                        "text" => sel.text = Some(Pattern::parse(s)?),
                        "css" => sel.css = Some(s.to_string()),
                        "point" => sel.point = Some(Point::parse(s)?),
                        other => return Err(format!("unknown selector field '{other}' (id, text, css, point)")),
                    }
                }
                if sel.point.is_some() && (sel.id.is_some() || sel.text.is_some() || sel.css.is_some()) {
                    return Err("a point selector takes no other field".into());
                }
                if sel == Sel::default() {
                    return Err("a selector needs id, text, css or point".into());
                }
                Ok(sel)
            }
            other => Err(format!("a selector is a table {{id=, text=, css=, point=}} or a string, got {other}")),
        }
    }

    pub fn json(&self) -> Value {
        let mut m = Map::new();
        if let Some(p) = &self.id {
            m.insert("id".into(), json!(p.source()));
        }
        if let Some(p) = &self.text {
            m.insert("text".into(), json!(p.source()));
        }
        if let Some(c) = &self.css {
            m.insert("css".into(), json!(c));
        }
        if let Some(p) = &self.point {
            m.insert("point".into(), json!(p.show()));
        }
        Value::Object(m)
    }

    /// `{id='X', text='Y'}`, as a script writes it.
    pub fn show(&self) -> String {
        let fields: Vec<String> = self.json().as_object().into_iter().flatten().map(|(k, v)| format!("{k}='{}'", v.as_str().unwrap_or_default())).collect();
        format!("{{{}}}", fields.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(v: Value) -> Result<Sel, String> {
        Sel::from_json(&v)
    }

    #[test]
    fn given_the_shared_grammar_when_patterns_are_parsed_then_each_part_of_it_is_taken() {
        for ok in ["Continue", ".*Continue.*", "Run (local)", "a|b", "[a-z]+", "[^0-9]", "\\d{2,3}", "x{2}", "y{1,}", "a*?", "(?:ab)+", "^a$", "\\.\\*\\+\\?\\(\\)\\[\\]\\{\\}\\|\\/\\^\\$\\\\", "\\w\\s\\D\\W\\S", "தமிழ்", "a-b", ""] {
            assert!(Pattern::parse(ok).is_ok(), "{ok}: {:?}", Pattern::parse(ok).err());
        }
    }

    #[test]
    fn given_a_pattern_outside_the_grammar_when_parsed_then_it_is_refused_with_the_reason() {
        for (bad, why) in [
            ("(?i)a", "inline flag"),
            ("(?i:a)", "inline flag"),
            ("(?P<n>a)", "named group"),
            ("(?<n>a)", "named group"),
            ("[[:alpha:]]", "POSIX"),
            ("\\pL", "Unicode class"),
            ("\\p{Greek}", "Unicode class"),
            ("\\bword", "assertion"),
            ("\\Aa", "assertion"),
            ("a\\z", "assertion"),
            ("a(?=b)", "look-around"),
            ("(a)\\1", "backreference"),
            ("\\n", "escape"),
            ("\\-", "escape"),
            ("a}", "unescaped"),
            ("a]", "unescaped"),
            ("[a&&b]", "class operation"),
            ("[[a]]", "inside a class"),
            ("a{,3}", ""),
            ("^*", ""),
        ] {
            let e = Pattern::parse(bad).err().unwrap_or_else(|| panic!("{bad} was taken"));
            assert!(e.starts_with(&format!("pattern '{bad}': ")) && e.to_lowercase().contains(&why.to_lowercase()), "{bad}: {e}");
        }
    }

    #[test]
    fn given_maestros_rule_when_values_are_matched_then_it_is_a_full_case_insensitive_match_or_literal_equality() {
        let p = |s: &str| Pattern::parse(s).unwrap();
        assert!(p("Continue").is_match("continue") && p("Continue").is_match("CONTINUE"));
        assert!(!p("Continue").is_match("Continue later"), "a full match, not a substring");
        assert!(p(".*Continue.*").is_match("Tap Continue now"));
        assert!(p("Run (local)").is_match("Run (local)"), "literal equality where the parentheses are regex syntax");
        assert!(p("Run (local)").is_match("run local") && !p("Run (local)").is_match("Run (remote)"), "the regex reading still holds");
        assert!(p("Hello World").is_match("Hello\nWorld"), "a newline in the value reads as a space");
        assert!(p("line one.*").is_match("line one\n    line two"), "a dot matches a newline");
        assert!(p("^b$").is_match("b") && !p("^b$").is_match("a\nb"), "^ and $ do not make a substring match");
        assert!(p("\\d+").is_match("42") && !p("\\d+").is_match("௪௨"), "\\d is ASCII on every engine");
        assert!(p("a|ab").is_match("ab"), "a later alternative still makes the full match");
    }

    #[test]
    fn given_a_pattern_when_its_engine_forms_are_built_then_each_carries_the_flags_and_the_literal_alternative() {
        let p = Pattern::parse("Run (local) \\d").unwrap();
        assert_eq!(p.java(), "(?ismu)(?:Run[ \\n](local)[ \\n]\\d)|(?:Run[ \\n]\\(local\\)[ \\n]\\\\d)");
        let (src, flags) = p.js();
        assert_eq!((src, flags), ("(?<![\\s\\S])(?:(?:Run[ \\n](local)[ \\n]\\d)|(?:Run[ \\n]\\(local\\)[ \\n]\\\\d))(?![\\s\\S])", "imsu"));
        assert_eq!(Pattern::parse("\\s").unwrap().js().0, "(?<![\\s\\S])(?:(?:[\\t\\n\\v\\f\\r ])|(?:\\\\s))(?![\\s\\S])");
        let id = Pattern::parse("Check").unwrap();
        assert_eq!(id.java_id(), "(?ismu)(?:.*/(?=[^/]*\\z))?(?:(?:Check)|(?:Check))");
        assert!(id.is_id_match("com.app:id/check") && id.is_id_match("Check") && !id.is_id_match("Check/x") && !id.is_id_match(""));
    }

    #[test]
    fn given_a_selector_when_read_then_tables_strings_and_points_are_taken_and_old_forms_refused() {
        let s = sel(json!({ "id": "Go", "text": ".*3 / 3.*" })).unwrap();
        assert_eq!((s.id.as_ref().map(Pattern::source), s.text.as_ref().map(Pattern::source)), (Some("Go"), Some(".*3 / 3.*")));
        assert_eq!(s.show(), "{id='Go', text='.*3 / 3.*'}");
        assert_eq!(sel(json!("Continue")).unwrap().text.unwrap().source(), "Continue");
        for (old, new) in [("id:Go", "{id='Go'}"), ("text:Run (x)", "{text='.*Run \\(x\\).*'}"), ("css:.cm-line", "{css='.cm-line'}"), ("xy:1,2", "{point='1,2'}"), ("label:Back", "{text='.*Back.*'}")] {
            let e = sel(json!(old)).unwrap_err();
            assert!(e.starts_with("old selector form") && e.ends_with(&format!("use {new}")), "{old}: {e}");
        }
        assert!(sel(json!({ "point": "1,2", "id": "x" })).unwrap_err().contains("no other field"));
        assert!(sel(json!({ "label": "x" })).unwrap_err().contains("unknown selector field"));
        assert!(sel(json!({})).unwrap_err().contains("needs id, text, css or point"));
        assert!(sel(json!({ "id": "(?i)x" })).is_err());
    }

    #[test]
    fn given_a_point_when_parsed_then_tap_units_and_percentages_resolve_and_a_mixed_or_bad_one_is_refused() {
        let px = Point::parse("120, 340").unwrap();
        assert_eq!((px.resolve(393.0, 852.0), px.show(), px.is_pct()), ((120.0, 340.0), "120,340".to_string(), false));
        let pct = Point::parse("83%,20.5%").unwrap();
        assert_eq!((pct.resolve(400.0, 1000.0), pct.show(), pct.is_pct()), ((332.0, 205.0), "83%,20.5%".to_string(), true));
        for bad in ["1", "1,2,3", "a,b", "50%,10", "-1,2", "101%,5%"] {
            assert!(Point::parse(bad).is_err(), "{bad}");
        }
    }
}
