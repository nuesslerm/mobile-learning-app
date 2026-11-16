//! Pure helpers for replaying rca's byte tail to a phone: UTF-8 carry, UTF-16 units, query stripping and the byte budget.
const ESC: char = '\x1b';
const MAX_PENDING: usize = 4096;
const REPLACEMENT: char = '\u{fffd}';

#[derive(Debug, Default)]
pub struct Utf8Carry {
    carry: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Decoded {
    pub text: String,
    /// Bytes of carry plus input turned into `text`; a held tail counts when it is finally decoded.
    pub consumed: usize,
}

impl Utf8Carry {
    pub fn push(&mut self, bytes: &[u8]) -> Decoded {
        self.carry.extend_from_slice(bytes);
        let mut text = String::new();
        let mut at = 0;
        loop {
            match std::str::from_utf8(&self.carry[at..]) {
                Ok(valid) => {
                    text.push_str(valid);
                    at = self.carry.len();
                    break;
                }
                Err(err) => {
                    let good = err.valid_up_to();
                    text.push_str(std::str::from_utf8(&self.carry[at..at + good]).unwrap_or_default());
                    at += good;
                    match err.error_len() {
                        Some(bad) => {
                            text.push(REPLACEMENT);
                            at += bad;
                        }
                        None => break,
                    }
                }
            }
        }
        self.carry.drain(..at);
        Decoded { text, consumed: at }
    }

    pub fn held(&self) -> usize {
        self.carry.len()
    }
}

pub fn units(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

fn is_query_csi(seq: &str) -> bool {
    let body = &seq[2..];
    let Some(last) = body.chars().last() else { return false };
    let params = &body[..body.len() - last.len_utf8()];
    let digits = |s: &str| s.chars().all(|c| c.is_ascii_digit() || c == ';');
    match last {
        'c' => digits(params.strip_prefix(['?', '>', '=']).unwrap_or(params)),
        'p' => params
            .strip_suffix('$')
            .map(|p| p.strip_prefix('?').unwrap_or(p))
            .is_some_and(|p| !p.is_empty() && digits(p)),
        'n' => matches!(params, "5" | "6" | "?6" | "?996"),
        'q' => params == ">",
        't' => matches!(params, "14" | "16" | "18"),
        'u' => params == "?",
        _ => false,
    }
}

fn csi_end(input: &str, from: usize) -> Option<usize> {
    input[from..].char_indices().find(|(_, c)| ('\u{40}'..='\u{7e}').contains(c)).map(|(i, c)| from + i + c.len_utf8())
}

/// Removes terminal reply queries across chunks, holding an unfinished escape until the next push.
#[derive(Debug, Default)]
pub struct QueryStrip {
    pending: String,
}

impl QueryStrip {
    pub fn push(&mut self, text: &str) -> String {
        let input = std::mem::take(&mut self.pending) + text;
        let mut out = String::with_capacity(input.len());
        let mut at = 0;
        while let Some(rel) = input[at..].find(ESC) {
            let start = at + rel;
            out.push_str(&input[at..start]);
            let rest = &input[start + 1..];
            let end = match rest.chars().next() {
                None => None,
                Some('[') => csi_end(&input, start + 2).map(|e| (e, is_query_csi(&input[start..e]))),
                Some(']') => osc_end(&input, start + 2).map(|(e, body)| (e, matches!(body, "10;?" | "11;?" | "12;?"))),
                Some('P') => input[start + 2..]
                    .find("\x1b\\")
                    .map(|i| (start + 2 + i + 2, input[start + 2..].starts_with("$q") || input[start + 2..].starts_with("+q"))),
                Some(_) => {
                    out.push(ESC);
                    at = start + 1;
                    continue;
                }
            };
            match end {
                Some((e, true)) => at = e,
                Some((e, false)) => {
                    if input[start + 1..].starts_with('[') {
                        out.push_str(&input[start..e]);
                        at = e;
                    } else {
                        out.push_str(&input[start..start + 2]);
                        at = start + 2;
                    }
                }
                None if input.len() - start > MAX_PENDING => {
                    out.push(ESC);
                    at = start + 1;
                }
                None => {
                    self.pending = input[start..].to_string();
                    return out;
                }
            }
        }
        out.push_str(&input[at..]);
        out
    }

    pub fn finish(&mut self) -> String {
        std::mem::take(&mut self.pending)
    }
}

fn osc_end(input: &str, from: usize) -> Option<(usize, &str)> {
    let rest = &input[from..];
    let bel = rest.find('\x07').map(|i| (i, 1));
    let st = rest.find("\x1b\\").map(|i| (i, 2));
    let (i, n) = match (bel, st) {
        (Some(a), Some(b)) => if a.0 < b.0 { a } else { b },
        (a, b) => a.or(b)?,
    };
    Some((from + i + n, &rest[..i]))
}

pub fn strip_queries(text: &str) -> String {
    let mut strip = QueryStrip::default();
    let mut out = strip.push(text);
    out.push_str(&strip.finish());
    out
}

/// Keeps the newest bytes that fit `budget - frame_overhead`, cut after a newline on a char boundary so the head is not half an escape.
pub fn trim_to_budget(text: &str, budget: usize, frame_overhead: usize) -> (&str, bool) {
    let room = budget.saturating_sub(frame_overhead);
    if text.len() <= room {
        return (text, false);
    }
    let mut start = text.len() - room;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    if let Some(nl) = text[start..].find('\n') {
        start += nl + 1;
    }
    (&text[start..], true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_an_emoji_split_across_chunks_when_decoded_then_it_appears_once_with_exact_counts() {
        let mut carry = Utf8Carry::default();
        let bytes = "a😀b".as_bytes();
        let first = carry.push(&bytes[..3]);
        assert_eq!(first, Decoded { text: "a".into(), consumed: 1 });
        assert_eq!(carry.held(), 2);
        let second = carry.push(&bytes[3..]);
        assert_eq!(second, Decoded { text: "😀b".into(), consumed: 5 });
        assert_eq!(carry.held(), 0);
    }

    #[test]
    fn given_invalid_bytes_when_decoded_then_each_run_becomes_a_replacement_char() {
        let mut carry = Utf8Carry::default();
        let got = carry.push(&[b'x', 0xff, b'y']);
        assert_eq!(got, Decoded { text: "x\u{fffd}y".into(), consumed: 3 });
    }

    #[test]
    fn given_text_when_counting_units_then_astral_chars_count_two() {
        assert_eq!(units("a😀"), 3);
        assert_eq!(units(""), 0);
    }

    #[test]
    fn given_queries_in_a_snapshot_when_stripped_then_colors_and_moves_survive() {
        let text = "a\x1b[6nb\x1b]11;?\x07c\x1b[31mred\x1b[2;3Hd\x1b[0c\x1b[?2004$pe\x1b[>qf\x1bP$qm\x1b\\g";
        assert_eq!(strip_queries(text), "abc\x1b[31mred\x1b[2;3Hdefg");
    }

    #[test]
    fn given_an_osc_title_when_stripped_then_it_is_kept() {
        let text = "\x1b]0;title\x07\x1b]11;rgb:00/00/00\x1b\\";
        assert_eq!(strip_queries(text), text);
    }

    #[test]
    fn given_a_query_split_across_scans_when_stripped_then_it_is_still_removed() {
        for cut in 1..="x\x1b[6ny".len() - 1 {
            let whole = "x\x1b[6ny";
            let mut strip = QueryStrip::default();
            let mut out = strip.push(&whole[..cut]);
            out.push_str(&strip.push(&whole[cut..]));
            out.push_str(&strip.finish());
            assert_eq!(out, "xy", "cut at {cut}");
        }
        let mut strip = QueryStrip::default();
        let mut out = strip.push("\x1b]11;");
        out.push_str(&strip.push("?\x1b"));
        out.push_str(&strip.push("\\z"));
        assert_eq!(out, "z");
    }

    #[test]
    fn given_an_unfinished_escape_at_the_end_when_finished_then_it_is_released() {
        let mut strip = QueryStrip::default();
        assert_eq!(strip.push("a\x1b[1"), "a");
        assert_eq!(strip.finish(), "\x1b[1");
    }

    #[test]
    fn given_a_tail_over_budget_when_trimmed_then_it_cuts_after_a_newline_on_a_char_boundary() {
        let text = "😀😀😀\nline two\nline three";
        let (kept, truncated) = trim_to_budget(text, 22, 2);
        assert!(truncated);
        assert_eq!(kept, "line two\nline three");
        let (all, truncated) = trim_to_budget("short", 100, 10);
        assert!(!truncated);
        assert_eq!(all, "short");
        let (cut, truncated) = trim_to_budget("😀😀😀😀", 9, 0);
        assert!(truncated);
        assert_eq!(cut, "😀😀");
    }
}
