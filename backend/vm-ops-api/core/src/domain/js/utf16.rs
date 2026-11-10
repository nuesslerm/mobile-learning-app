//! A JavaScript string is UTF-16 units, so `slice` can cut a surrogate pair: Node then sends the lone half, which
//! `JSON.stringify` escapes as `\udXXX` and no Rust `String` can hold.
use std::char::decode_utf16;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JsString(Vec<u16>);

impl From<&str> for JsString {
    fn from(text: &str) -> Self {
        JsString(text.encode_utf16().collect())
    }
}

impl JsString {
    pub fn from_units(units: &[u16]) -> Self {
        JsString(units.to_vec())
    }

    /// `string.length`.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `string.slice(0, units)`.
    pub fn first(&self, units: usize) -> JsString {
        JsString(self.0[..units.min(self.0.len())].to_vec())
    }

    /// The last `units` code units, as `string.slice(-units)` for a positive count. `slice(-0)` keeps everything; this keeps nothing.
    pub fn last(&self, units: usize) -> JsString {
        JsString(self.0[self.0.len().saturating_sub(units)..].to_vec())
    }

    /// The text a UTF-8 encoder writes for it: each lone surrogate becomes U+FFFD.
    pub fn to_text_lossy(&self) -> String {
        String::from_utf16_lossy(&self.0)
    }

    /// `JSON.stringify(string)`, with every lone surrogate escaped in lower case.
    pub fn to_json(&self) -> String {
        let mut out = String::with_capacity(self.0.len() + 2);
        out.push('"');
        for unit in decode_utf16(self.0.iter().copied()) {
            match unit {
                Ok('"') => out.push_str("\\\""),
                Ok('\\') => out.push_str("\\\\"),
                Ok('\u{8}') => out.push_str("\\b"),
                Ok('\t') => out.push_str("\\t"),
                Ok('\n') => out.push_str("\\n"),
                Ok('\u{c}') => out.push_str("\\f"),
                Ok('\r') => out.push_str("\\r"),
                Ok(c) if c < ' ' => {
                    let _ = write!(out, "\\u{:04x}", u32::from(c));
                }
                Ok(c) => out.push(c),
                Err(lone) => {
                    let _ = write!(out, "\\u{:04x}", lone.unpaired_surrogate());
                }
            }
        }
        out.push('"');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRIN: &str = "\u{1f600}";

    fn json_of(units: &[u16]) -> String {
        JsString::from_units(units).to_json()
    }

    #[test]
    fn given_strings_when_written_as_json_then_they_are_what_json_stringify_writes() {
        let cases = [
            ("plain", r#""plain""#),
            ("quote\"back\\slash", r#""quote\"back\\slash""#),
            ("\u{0}\u{1}\u{8}\t\n\u{b}\u{c}\r\u{1f}\u{7f}", "\"\\u0000\\u0001\\b\\t\\n\\u000b\\f\\r\\u001f\u{7f}\""),
            ("\u{2028}\u{2029}", "\"\u{2028}\u{2029}\""),
            ("\u{e9}\u{1f600}x", "\"\u{e9}\u{1f600}x\""),
            ("", r#""""#),
        ];
        for (text, want) in cases {
            assert_eq!(JsString::from(text).to_json(), want, "JSON.stringify({text:?})");
        }
    }

    #[test]
    fn given_lone_surrogates_when_written_as_json_then_each_is_escaped_in_lower_case() {
        assert_eq!(json_of(&[0xd800]), r#""\ud800""#);
        assert_eq!(json_of(&[0xdc00]), r#""\udc00""#);
        assert_eq!(json_of(&[0xde00, 0x61, 0x62, 0x63]), r#""\ude00abc""#);
        assert_eq!(json_of(&[0x61, 0x62, 0x63, 0xd83d]), r#""abc\ud83d""#);
        assert_eq!(json_of(&[0xd83d, 0xde00]), "\"\u{1f600}\"");
        assert_eq!(json_of(&[0x61, 0xd83d, 0xd83d, 0xde00, 0xde00, 0x62]), "\"a\\ud83d\u{1f600}\\ude00b\"");
        assert_eq!(json_of(&[0xd83d, 0x78, 0xde00]), r#""\ud83dx\ude00""#);
    }

    #[test]
    fn given_well_formed_text_when_written_as_json_then_it_equals_what_serde_json_writes() {
        for text in ["", "abc", "\u{e9}\u{1f600}\u{2028}", "a\"b\\c\n\t\u{1}\u{1f}\u{7f}", "\u{ffff}\u{10ffff}"] {
            assert_eq!(JsString::from(text).to_json(), serde_json::to_string(text).expect("a string is JSON"), "{text:?}");
        }
    }

    #[test]
    fn given_a_string_with_a_non_bmp_character_when_measured_then_it_counts_two_units_for_it() {
        assert_eq!(JsString::from(GRIN).len(), 2);
        assert_eq!(JsString::from("a\u{e9}").len(), 2);
        assert!(JsString::from("").is_empty());
    }

    #[test]
    fn given_5000_units_whose_last_4000_start_with_a_non_bmp_character_when_the_tail_is_taken_then_the_pair_is_whole() {
        let text = format!("{}{GRIN}{}", "x".repeat(1000), "y".repeat(3998));
        let tail = JsString::from(text.as_str()).last(4000);
        assert_eq!(tail.len(), 4000);
        assert_eq!(tail.to_text_lossy(), format!("{GRIN}{}", "y".repeat(3998)));
        assert!(tail.to_json().starts_with("\"\u{1f600}yyy"));
    }

    #[test]
    fn given_5000_units_cut_between_the_halves_of_a_pair_when_the_tail_is_taken_then_the_lone_low_half_is_kept() {
        let text = format!("{}{GRIN}{}", "x".repeat(999), "y".repeat(3999));
        let tail = JsString::from(text.as_str()).last(4000);
        assert_eq!(tail.len(), 4000);
        assert_eq!(tail.to_json(), format!("\"\\ude00{}\"", "y".repeat(3999)));
        assert_eq!(tail.to_text_lossy(), format!("\u{fffd}{}", "y".repeat(3999)));
    }

    #[test]
    fn given_three_non_bmp_characters_when_sliced_then_each_cut_matches_what_slice_keeps() {
        let text = JsString::from(GRIN.repeat(3).as_str());
        let json = |s: JsString| s.to_json();
        assert_eq!(json(text.first(3)), "\"\u{1f600}\\ud83d\"");
        assert_eq!(json(text.first(4)), "\"\u{1f600}\u{1f600}\"");
        assert_eq!(json(text.first(1)), r#""\ud83d""#);
        assert_eq!(json(text.first(0)), r#""""#);
        assert_eq!(json(text.first(100)), "\"\u{1f600}\u{1f600}\u{1f600}\"");
        assert_eq!(json(text.last(3)), "\"\\ude00\u{1f600}\"");
        assert_eq!(json(text.last(1)), r#""\ude00""#);
        assert_eq!(json(text.last(100)), "\"\u{1f600}\u{1f600}\u{1f600}\"");
        assert_eq!(json(text.last(0)), r#""""#);
    }
}
