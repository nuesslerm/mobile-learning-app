//! The rules of the two env helpers the redeploy script runs around `docker compose`: which resolved keys a hand-appended `.env`
//! would clobber, and which lines of `.env.local` may be appended. Text is read as JavaScript reads it (`trim`, `JSON.parse`).
use super::js::is_space;
use std::collections::HashMap;

fn js_trim(text: &str) -> &str {
    text.trim_matches(is_space)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvLine<'a> {
    pub key: &'a str,
    pub value: &'a str,
}

/// `KEY=value` of a line trimmed; no blank line, comment or line with no key before its first `=`. The key is not trimmed.
pub fn parse_env_line(line: &str) -> Option<EnvLine<'_>> {
    let trimmed = js_trim(line);
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let eq = trimmed.find('=').filter(|&eq| eq > 0)?;
    Some(EnvLine { key: &trimmed[..eq], value: &trimmed[eq + 1..] })
}

/// The value each key ends with, in the order the keys first appeared.
#[derive(Debug, Default)]
pub struct EnvValues<'a> {
    entries: Vec<EnvLine<'a>>,
    position: HashMap<&'a str, usize>,
}

impl<'a> EnvValues<'a> {
    pub fn get(&self, key: &str) -> Option<&'a str> {
        self.position.get(key).map(|&at| self.entries[at].value)
    }

    pub fn iter(&self) -> impl Iterator<Item = EnvLine<'a>> + '_ {
        self.entries.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Last occurrence wins, as docker compose reads an env_file.
pub fn last_values(text: &str) -> EnvValues<'_> {
    let mut values = EnvValues::default();
    for line in text.split('\n').filter_map(parse_env_line) {
        match values.position.get(line.key) {
            Some(&at) => values.entries[at].value = line.value,
            None => {
                values.position.insert(line.key, values.entries.len());
                values.entries.push(line);
            }
        }
    }
    values
}

/// A missing value is as empty as one written `""` or `''`.
pub fn is_empty_value(value: Option<&str>) -> bool {
    matches!(js_trim(value.unwrap_or("")), "" | "\"\"" | "''")
}

fn is_space_unit(unit: u16) -> bool {
    char::from_u32(u32::from(unit)).is_some_and(is_space)
}

fn trim_units(units: &[u16]) -> &[u16] {
    let start = units.iter().position(|u| !is_space_unit(*u)).unwrap_or(units.len());
    let end = units.iter().rposition(|u| !is_space_unit(*u)).map_or(start, |last| last + 1);
    &units[start..end]
}

fn hex4(units: &mut impl Iterator<Item = u16>) -> Option<u16> {
    (0..4).try_fold(0u16, |acc, _| Some((acc << 4) | char::from_u32(u32::from(units.next()?))?.to_digit(16)? as u16))
}

const QUOTE: u16 = b'"' as u16;
const BACKSLASH: u16 = b'\\' as u16;

/// `JSON.parse(text)` when `text` is one string literal, as the UTF-16 units it holds: a lone surrogate escape survives.
fn json_string(units: &[u16]) -> Option<Vec<u16>> {
    let (&open, rest) = units.split_first()?;
    let (&close, body) = rest.split_last()?;
    if open != QUOTE || close != QUOTE {
        return None;
    }
    let mut decoded = Vec::with_capacity(body.len());
    let mut body = body.iter().copied();
    while let Some(unit) = body.next() {
        match unit {
            QUOTE | 0..=0x1f => return None,
            BACKSLASH => decoded.push(match u8::try_from(body.next()?).ok()? {
                b'"' => QUOTE,
                b'\\' => BACKSLASH,
                b'/' => u16::from(b'/'),
                b'b' => 0x08,
                b'f' => 0x0c,
                b'n' => 0x0a,
                b'r' => 0x0d,
                b't' => 0x09,
                b'u' => hex4(&mut body)?,
                _ => return None,
            }),
            _ => decoded.push(unit),
        }
    }
    Some(decoded)
}

/// JSON-quoted twice (`"\"real\""`): the app would receive the quotes as part of the secret.
pub fn is_double_quoted(value: Option<&str>) -> bool {
    let units: Vec<u16> = js_trim(value.unwrap_or("")).encode_utf16().collect();
    json_string(&units).is_some_and(|once| json_string(trim_units(&once)).is_some())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clobbered {
    pub clobbered: Vec<String>,
    pub mangled: Vec<String>,
    pub synced_count: usize,
}

/// Keys sync-env resolved that would reach the container empty or double-encoded; a non-empty override passes.
pub fn clobbered_keys(synced_text: &str, final_text: &str) -> Clobbered {
    let synced = last_values(synced_text);
    let last = last_values(final_text);
    let (mut clobbered, mut mangled) = (Vec::new(), Vec::new());
    for EnvLine { key, value } in synced.iter() {
        if is_empty_value(Some(value)) {
            continue;
        }
        let end = last.get(key);
        if is_empty_value(end) {
            clobbered.push(key.to_string());
        } else if is_double_quoted(end) {
            mangled.push(key.to_string());
        }
    }
    Clobbered { clobbered, mangled, synced_count: synced.len() }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendPlan {
    /// The `.env.local` lines to append, as written; blank lines are left out.
    pub keep: Vec<String>,
    /// The keys of the empty overrides left out, once per line.
    pub dropped: Vec<String>,
}

/// Appends `.env.local` but drops an empty override that would shadow a resolved value.
pub fn plan_append(env_text: &str, env_local_text: &str) -> AppendPlan {
    let resolved = last_values(env_text);
    let (mut keep, mut dropped) = (Vec::new(), Vec::new());
    for line in env_local_text.split('\n') {
        let Some(parsed) = parse_env_line(line) else {
            if !js_trim(line).is_empty() {
                keep.push(line.to_string());
            }
            continue;
        };
        let shadows = is_empty_value(Some(parsed.value)) && resolved.get(parsed.key).is_some_and(|v| !is_empty_value(Some(v)));
        if shadows {
            dropped.push(parsed.key.to_string());
        } else {
            keep.push(line.to_string());
        }
    }
    AppendPlan { keep, dropped }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYNCED: &str = "PORT=8080\nVM_GATEWAY_AUTH_SEED=realsecretvalue\nTS_VM_AUTH_KEY=tskey-auth-abc";
    const SEED: &str = "VM_GATEWAY_AUTH_SEED";

    fn verify(final_text: &str) -> Clobbered {
        clobbered_keys(&format!("{SYNCED}\n"), final_text)
    }

    fn names(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|k| k.to_string()).collect()
    }

    #[test]
    fn given_harmless_appends_a_commented_key_or_a_real_override_when_verified_then_no_key_is_clobbered() {
        let tails = [
            "HOST_LABEL=main",
            "#VM_GATEWAY_AUTH_SEED=",
            "VM_GATEWAY_AUTH_SEED=deliberate",
            "TS_VM_AUTH_KEY=\"tskey-auth-abc\"",
            "VM_GATEWAY_AUTH_SEED=\"he said \\\"hi\\\" loudly\"",
        ];
        for tail in tails {
            let verdict = verify(&format!("{SYNCED}\n{tail}\n"));
            assert_eq!((verdict.clobbered, verdict.mangled), (vec![], vec![]), "{tail}");
        }
    }

    #[test]
    fn given_a_resolved_secret_emptied_or_dropped_when_verified_then_it_is_clobbered_by_name() {
        for final_text in [
            format!("{SYNCED}\nVM_GATEWAY_AUTH_SEED=\n"),
            format!("{SYNCED}\nVM_GATEWAY_AUTH_SEED=\"\"\n"),
            format!("{SYNCED}\nVM_GATEWAY_AUTH_SEED=''\n"),
            format!("{SYNCED}\nVM_GATEWAY_AUTH_SEED=  \n"),
        ] {
            let verdict = verify(&final_text);
            assert_eq!((verdict.clobbered, verdict.mangled), (names(&[SEED]), vec![]), "{final_text:?}");
        }
        let vanished = verify("PORT=8080\n");
        assert_eq!(vanished.clobbered, names(&[SEED, "TS_VM_AUTH_KEY"]));
    }

    #[test]
    fn given_a_secret_json_quoted_twice_when_verified_then_it_is_mangled_not_clobbered() {
        let verdict = verify(&format!("{SYNCED}\nVM_GATEWAY_AUTH_SEED={}\n", r#""\"realsecretvalue\"""#));
        assert_eq!((verdict.clobbered, verdict.mangled), (vec![], names(&[SEED])));
    }

    #[test]
    fn given_one_key_emptied_and_another_quoted_twice_when_verified_then_both_lists_are_filled() {
        let verdict = verify(&format!("{SYNCED}\nVM_GATEWAY_AUTH_SEED=\nTS_VM_AUTH_KEY={}\n", r#""\"tskey-auth-abc\"""#));
        assert_eq!((verdict.clobbered, verdict.mangled), (names(&[SEED]), names(&["TS_VM_AUTH_KEY"])));
    }

    #[test]
    fn given_synced_keys_when_verified_then_the_count_is_of_distinct_keys_with_empty_ones_included() {
        let synced = "API_KEY=s3cret-value\nDB_URL=postgres://u:pw@h/db\nEMPTY=\n";
        let verdict = clobbered_keys(synced, &format!("{synced}HOST_LABEL=main\nLOCAL_ONLY=1\nAPI_KEY=local-override\n"));
        assert_eq!(verdict, Clobbered { clobbered: vec![], mangled: vec![], synced_count: 3 });
        assert_eq!(clobbered_keys("A=1\nB=2\nA=3\n# C=1\n\n=x\n", "A=1\nB=2\n").synced_count, 2);
        assert_eq!(clobbered_keys("", "A=1\n").synced_count, 0);
    }

    #[test]
    fn given_a_key_synced_twice_when_verified_then_it_is_listed_where_it_first_appeared_and_its_last_value_counts() {
        let verdict = clobbered_keys("A=1\nB=2\nA=3\nC=4\n", "A=\nB=\nC=\n");
        assert_eq!(verdict.clobbered, names(&["A", "B", "C"]));
        let verdict = clobbered_keys("A=real\nA=\nB=2\n", "B=2\n");
        assert_eq!((verdict.clobbered, verdict.synced_count), (vec![], 2), "a key that ends empty in the snapshot was never resolved");
    }

    #[test]
    fn given_a_key_written_twice_in_the_final_env_when_verified_then_the_last_line_decides() {
        assert_eq!(clobbered_keys("A=real\n", "A=real\nA=\n").clobbered, names(&["A"]));
        assert_eq!(clobbered_keys("A=real\n", "A=\nA=real\n").clobbered, Vec::<String>::new());
        assert_eq!(clobbered_keys("A=real\n", "A=real\nA=\"\\\"x\\\"\"\n").mangled, names(&["A"]));
    }

    #[test]
    fn given_an_empty_override_of_a_resolved_key_when_planned_then_it_is_dropped_by_name_and_real_overrides_stay() {
        let env = "PORT=8080\nVM_GATEWAY_AUTH_SEED=realsecretvalue\nBETTERSTACK_OTLP_TOKEN=realtoken\n";
        let local = "BETTERSTACK_OTLP_TOKEN=\nVM_GATEWAY_AUTH_SEED=\"\"\nBWS_ACCESS_TOKEN=bws-real\nPORT=9090\n";
        let plan = plan_append(env, local);
        assert_eq!(plan.dropped, names(&["BETTERSTACK_OTLP_TOKEN", "VM_GATEWAY_AUTH_SEED"]));
        assert_eq!(plan.keep, names(&["BWS_ACCESS_TOKEN=bws-real", "PORT=9090"]), "last-wins stays with compose, in order");
    }

    #[test]
    fn given_a_local_file_with_comments_blanks_and_keys_resolved_empty_when_planned_then_only_the_shadowing_lines_go() {
        let env = "API_KEY=resolved-key\nDB_URL=postgres://u:pw@h/db\nOPTIONAL=\nHOST_LABEL=main\n";
        let local = "API_KEY=\nOPTIONAL=\nNEW_FLAG=1\n# set by hand\n\nDB_URL=\"\"\n";
        let plan = plan_append(env, local);
        assert_eq!(plan.dropped, names(&["API_KEY", "DB_URL"]));
        assert_eq!(plan.keep, names(&["OPTIONAL=", "NEW_FLAG=1", "# set by hand"]));
    }

    #[test]
    fn given_lines_that_are_no_assignment_when_planned_then_non_blank_ones_are_kept_as_written() {
        let plan = plan_append("A=1\n", "  \n\t\n\u{feff}\n=x\n  B=2  \r\n#c\r\nnot an assignment\n\u{85}\nA=\r\n");
        assert_eq!(plan.keep, names(&["=x", "  B=2  \r", "#c\r", "not an assignment", "\u{85}"]));
        assert_eq!(plan.dropped, names(&["A"]));
    }

    #[test]
    fn given_an_empty_override_of_a_key_that_is_unresolved_or_resolved_empty_when_planned_then_it_is_kept() {
        let plan = plan_append("A=\nB=''\nC=\"\"\n", "A=\nB=\nC=\nD=\nA = \n");
        assert_eq!(plan.dropped, Vec::<String>::new());
        assert_eq!(plan.keep, names(&["A=", "B=", "C=", "D=", "A = "]));
    }

    #[test]
    fn given_a_resolved_key_written_twice_when_planned_then_its_last_value_decides_whether_an_override_shadows_it() {
        assert_eq!(plan_append("A=real\nA=\n", "A=\n").dropped, Vec::<String>::new());
        assert_eq!(plan_append("A=\nA=real\n", "A=\n").dropped, names(&["A"]));
        assert_eq!(plan_append("A=real\n", "A=\nA=\nA=x\nA=\n").dropped, names(&["A", "A", "A"]));
    }

    #[test]
    fn given_assignment_lines_when_parsed_then_the_key_is_untrimmed_and_the_value_runs_to_the_end_of_the_trimmed_line() {
        let cases = [
            ("KEY=value", Some(("KEY", "value"))),
            ("  KEY=value \t\r", Some(("KEY", "value"))),
            ("KEY = v", Some(("KEY ", " v"))),
            ("K=", Some(("K", ""))),
            ("K==v=", Some(("K", "=v="))),
            ("\u{feff}K=v\u{a0}", Some(("K", "v"))),
            ("K=a b  c", Some(("K", "a b  c"))),
            ("A B=c", Some(("A B", "c"))),
            ("\u{85}K=v", Some(("\u{85}K", "v"))),
            ("=v", None),
            ("  =v", None),
            ("# K=v", None),
            ("  #K=v", None),
            ("K", None),
            ("", None),
            (" \t\u{3000}", None),
        ];
        for (line, want) in cases {
            assert_eq!(parse_env_line(line).map(|l| (l.key, l.value)), want, "{line:?}");
        }
    }

    #[test]
    fn given_a_key_written_twice_when_read_then_the_last_value_wins_at_the_first_position() {
        let values = last_values("A=1\nB=2\n# A=9\nA=3\n\nC=\n=x\n");
        assert_eq!(values.iter().map(|l| (l.key, l.value)).collect::<Vec<_>>(), [("A", "3"), ("B", "2"), ("C", "")]);
        assert_eq!((values.get("A"), values.get("C"), values.get("D"), values.len(), values.is_empty()), (Some("3"), Some(""), None, 3, false));
        assert!(last_values("").is_empty());
    }

    #[test]
    fn given_values_when_asked_if_empty_then_only_nothing_or_a_pair_of_the_same_quote_is() {
        let empty = [None, Some(""), Some("  "), Some("\"\""), Some("''"), Some(" \"\" "), Some("\u{a0}''\u{feff}")];
        let not_empty = ["x", "\" \"", "' '", "\"", "'", "\"'", "'\"", "\"\"\"\"", "\"\\\"\\\"\"", "\u{85}", "0"];
        for value in empty {
            assert!(is_empty_value(value), "{value:?}");
        }
        for value in not_empty {
            assert!(!is_empty_value(Some(value)), "{value:?}");
        }
    }

    #[test]
    fn given_values_when_asked_if_quoted_twice_then_only_a_json_string_that_holds_a_json_string_is() {
        let cases: &[(Option<&str>, bool)] = &[
            (None, false),
            (Some(""), false),
            (Some("\""), false),
            (Some("\"\""), false),
            (Some("\"x\""), false),
            (Some("realsecretvalue"), false),
            (Some("\"he said \\\"hi\\\" loudly\""), false),
            (Some(r#""\"x\"""#), true),
            (Some(r#""\"\"""#), true),
            (Some(r#""\"real\" ""#), true),
            (Some(r#"" \"x\"""#), true),
            (Some(r#""\"x\"" "#), true),
            (Some("  \"\\\"x\\\"\"  \n"), true),
            (Some(r#""\"x""#), false),
            (Some(r#""\"x\"" x"#), false),
            (Some(r#""\u0022x\u0022""#), true),
            (Some(r#""\u0022X\u0022""#), true),
            (Some(r#""\U0022x\U0022""#), false),
            (Some(r#""\"a\"b\"""#), false),
            (Some(r#""\"a\\\"b\"""#), true),
            (Some(r#""\q""#), false),
            (Some(r#""\u00zz""#), false),
            (Some(r#""\u22""#), false),
            (Some("\"a\tb\""), false),
            (Some(r#""\"\n\"""#), false),
            (Some(r#""\"\\n\"""#), true),
            (Some(r#""\"\u2028\"""#), true),
            (Some(r#""\"\ud800\"""#), true),
            (Some(r#""\ud800""#), false),
            (Some(r#""\"\ud83d\ude00\"""#), true),
            (Some(r#""\ud83d\ude00""#), false),
            (Some("\"\\\"\u{1f600}\\\"\""), true),
            (Some("\"\u{1f600}\""), false),
            (Some("\u{a0}\"\\\"x\\\"\"\u{a0}"), true),
            (Some("\u{feff}\"\\\"x\\\"\""), true),
            (Some("\u{85}\"\\\"x\\\"\""), false),
            (Some(r#""\"x\"\u00a0""#), true),
            (Some(r#""\"x\"\u0085""#), false),
            (Some(r#""\"x\"""y""#), false),
            (Some(r#""\"a\/b\"""#), true),
            (Some(r#""\/""#), false),
            (Some(r#""\"\b\"""#), false),
            (Some(r#""\"\f\"""#), false),
            (Some(r#""\"\r\"""#), false),
            (Some(r#""\"\t\"""#), false),
            (Some(r#""\"\\b\\f\\r\\t\"""#), true),
            (Some(r#""\"é\"""#), true),
            (Some(r#""\"é\"""#), true),
            (Some(r#""\"\u00g9\"""#), false),
            (Some(r#""\"\u0009\"""#), false),
            (Some(r#""\"\u001f\"""#), false),
            (Some(r#""\"\u007f\"""#), true),
            (Some("\"\\\"\u{7f}\\\"\""), true),
            (Some("''"), false),
        ];
        for &(value, want) in cases {
            assert_eq!(is_double_quoted(value), want, "{value:?}");
        }
    }
}
