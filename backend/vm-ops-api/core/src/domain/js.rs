//! The JavaScript behaviours the wire contract inherits from Node: `Number(text)` and `parseInt(text, 10)`, the UTC date and
//! `Date#toISOString` of a millisecond count, `new Date(value)`, `JSON.stringify`, how a number prints and rounds, and a string's
//! UTF-16 slices.

mod date;
mod json;
mod print;
mod utf16;

pub use date::{new_date_ms, parse_date};
pub use json::{stringify, stringify_pretty};
pub use print::{json_number, number_to_string, round};
pub use utf16::JsString;

const MS_PER_DAY: i64 = 86_400_000;

/// What `String#trim` strips and `\s` matches: white space and line terminators.
pub(crate) fn is_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

/// `Number(text)`: NaN for anything that is no number literal, 0 for the empty text, the hex, octal and binary forms
/// without a sign, and Infinity only spelled `Infinity`.
pub fn to_number(text: &str) -> f64 {
    let text = text.trim_matches(is_space);
    if text.is_empty() {
        return 0.0;
    }
    radix_literal(text).unwrap_or_else(|| decimal_literal(text).unwrap_or(f64::NAN))
}

/// `parseInt(text, 10)`: after the leading white space, an optional sign and the ASCII digits up to the first other character;
/// NaN when there are none.
pub fn parse_int(text: &str) -> f64 {
    let text = text.trim_start_matches(is_space);
    let (negative, unsigned) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let digits = unsigned.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return f64::NAN;
    }
    let magnitude = unsigned[..digits].parse::<f64>().unwrap_or(f64::NAN);
    if negative {
        -magnitude
    } else {
        magnitude
    }
}

/// `None` when the text is no `0x`, `0o` or `0b` literal at all; NaN when it starts like one and is not one.
fn radix_literal(text: &str) -> Option<f64> {
    let bits_per_digit = match text.as_bytes() {
        [b'0', b'x' | b'X', ..] => 4,
        [b'0', b'o' | b'O', ..] => 3,
        [b'0', b'b' | b'B', ..] => 1,
        _ => return None,
    };
    let digits = &text[2..];
    if digits.is_empty() {
        return Some(f64::NAN);
    }
    // The exact value rounded once, to nearest even: the first 64 significant bits, with a sticky bit for the rest.
    let (mut top, mut significant, mut sticky) = (0u64, 0usize, false);
    for c in digits.chars() {
        let Some(digit) = c.to_digit(1 << bits_per_digit) else { return Some(f64::NAN) };
        for shift in (0..bits_per_digit).rev() {
            let bit = (digit >> shift) & 1 == 1;
            if significant == 0 && !bit {
                continue;
            }
            if significant < 64 {
                top = (top << 1) | u64::from(bit);
            } else {
                sticky |= bit;
            }
            significant += 1;
        }
    }
    if significant <= 64 {
        return Some(top as f64);
    }
    let scale = 2f64.powi(i32::try_from(significant - 64).unwrap_or(i32::MAX));
    Some((top | u64::from(sticky)) as f64 * scale)
}

/// `[+-]? (Infinity | digits [. digits?] [exp] | . digits [exp])`, with `exp` as `[eE][+-]?digits`.
fn decimal_literal(text: &str) -> Option<f64> {
    let (negative, body) = match text.as_bytes().first()? {
        b'-' => (true, &text[1..]),
        b'+' => (false, &text[1..]),
        _ => (false, text),
    };
    let magnitude = if body == "Infinity" {
        f64::INFINITY
    } else if is_decimal_body(body.as_bytes()) {
        body.parse::<f64>().ok()?
    } else {
        return None;
    };
    Some(if negative { -magnitude } else { magnitude })
}

fn is_decimal_body(body: &[u8]) -> bool {
    let digits = |from: usize| body[from..].iter().take_while(|b| b.is_ascii_digit()).count();
    let int = digits(0);
    let mut at = int;
    let mut frac = 0;
    if body.get(at) == Some(&b'.') {
        frac = digits(at + 1);
        at += 1 + frac;
    }
    if int == 0 && frac == 0 {
        return false;
    }
    if matches!(body.get(at), Some(b'e' | b'E')) {
        at += 1;
        if matches!(body.get(at), Some(b'+' | b'-')) {
            at += 1;
        }
        let exp = digits(at);
        if exp == 0 {
            return false;
        }
        at += exp;
    }
    at == body.len()
}

/// Year, month and day of a day count since 1970-01-01 (Howard Hinnant's civil_from_days).
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    (year_of_era + era * 400 + i64::from(month <= 2), month, day)
}

/// `YYYY-MM-DD` of the UTC day `ms` falls on: `new Date(ms).toISOString().slice(0, 10)`.
pub fn utc_date(ms: i64) -> String {
    let (year, month, day) = civil(ms.div_euclid(MS_PER_DAY));
    format!("{year:04}-{month:02}-{day:02}")
}

/// `new Date(ms).toISOString()`, for the years 0000 to 9999.
pub fn iso_string(ms: i64) -> String {
    let in_day = ms.rem_euclid(MS_PER_DAY);
    format!("{}T{:02}:{:02}:{:02}.{:03}Z", utc_date(ms), in_day / 3_600_000, in_day / 60_000 % 60, in_day / 1000 % 60, in_day % 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_characters_when_asked_for_white_space_then_exactly_the_javascript_set_is_taken() {
        let spaces = ['\u{9}', '\u{a}', '\u{b}', '\u{c}', '\u{d}', ' ', '\u{a0}', '\u{1680}', '\u{2000}', '\u{200a}', '\u{2028}', '\u{2029}', '\u{202f}', '\u{205f}', '\u{3000}', '\u{feff}'];
        let others = ['\u{8}', '\u{e}', '\u{1f}', '\u{85}', '\u{180e}', '\u{1fff}', '\u{200b}', '\u{200c}', '\u{2060}', '\u{3001}', '\u{fffe}', 'a', '0'];
        assert!(spaces.iter().all(|c| is_space(*c)), "a white space was refused");
        assert!(others.iter().all(|c| !is_space(*c)), "a non-space was taken");
    }

    #[test]
    fn given_number_literals_when_read_as_number_then_they_match_what_node_prints() {
        let cases: [(&str, f64); 25] = [
            ("", 0.0),
            (" ", 0.0),
            ("  1700000000000  ", 1_700_000_000_000.0),
            ("1700000000000e0", 1_700_000_000_000.0),
            ("1700000000000.0", 1_700_000_000_000.0),
            ("+1700000000000", 1_700_000_000_000.0),
            ("0x18b8d1ab400", 1_698_879_419_392.0),
            ("0X1F", 31.0),
            ("0o17", 15.0),
            ("0b101", 5.0),
            ("0B11", 3.0),
            ("1e3", 1000.0),
            ("1E3", 1000.0),
            ("1e+3", 1000.0),
            ("1e-3", 0.001),
            (".5", 0.5),
            ("5.", 5.0),
            ("5.e3", 5000.0),
            ("0.1e1", 1.0),
            ("00012", 12.0),
            ("\u{a0} 42 \u{feff}", 42.0),
            ("\u{2028} 7 \u{2029}", 7.0),
            ("Infinity", f64::INFINITY),
            ("-Infinity", f64::NEG_INFINITY),
            ("1e400", f64::INFINITY),
        ];
        for (text, want) in cases {
            assert_eq!(to_number(text), want, "Number({text:?})");
        }
    }

    #[test]
    fn given_text_that_is_no_number_literal_when_read_as_number_then_it_is_nan() {
        let cases = [
            "-0x10", "+0x10", "0x", "0xg", "0b2", "0o8", ".", "e3", "1e", "1_000", "infinity", "inf", "NaN", "12abc", "1 2", "--1", "+-1", "1,5",
            "\u{661}\u{662}\u{663}", "\u{180e}5", "\u{200b}5",
        ];
        for text in cases {
            assert!(to_number(text).is_nan(), "Number({text:?}) is {}", to_number(text));
        }
    }

    #[test]
    fn given_a_hex_literal_longer_than_a_double_when_read_as_number_then_it_is_infinity() {
        assert_eq!(to_number(&format!("0x{}", "f".repeat(300))), f64::INFINITY);
    }

    #[test]
    fn given_radix_literals_longer_than_53_bits_when_read_as_number_then_they_round_once_to_nearest_even() {
        let ones = |n: usize| "1".repeat(n);
        let cases: [(String, &str); 22] = [
            ("0x20000000000001".into(), "9007199254740992"),
            ("0x20000000000003".into(), "9007199254740996"),
            ("0x20000000000005".into(), "9007199254740996"),
            ("0x1fffffffffffff".into(), "9007199254740991"),
            ("0x1fffffffffffff8".into(), "144115188075855870"),
            ("0xffffffffffffffff".into(), "18446744073709552000"),
            ("0x7fffffffffffffc1".into(), "9223372036854776000"),
            ("0xfffffffffffffbff".into(), "18446744073709550000"),
            ("0x100000000000000000000000000000000".into(), "3.402823669209385e+38"),
            (format!("0b{}", ones(55)), "36028797018963970"),
            (format!("0b1{}11", "0".repeat(52)), "18014398509481988"),
            (format!("0b1{}1", "0".repeat(53)), "18014398509481984"),
            (format!("0o{}", "7".repeat(30)), "1.2379400392853803e+27"),
            ("0x49D720E6FD6B3A987A22513".into(), "1.4282800090571674e+27"),
            ("0o7324333662604263210611".into(), "68390391995857445000"),
            (format!("0x{}", "f".repeat(255)), "1.1235582092889474e+307"),
            (format!("0x{}", "f".repeat(256)), "Infinity"),
            (format!("0x{}1f", "0".repeat(400)), "31"),
            (format!("0b1{}1{}", "0".repeat(52), "0".repeat(10)), "9223372036854776000"),
            (format!("0b1{}1{}1", "0".repeat(52), "0".repeat(10)), "18446744073709556000"),
            (format!("0b1{}0{}1", "0".repeat(52), ones(10)), "18446744073709552000"),
            ("0x100000000000008001".into(), "295147905179352900000"),
        ];
        for (text, want) in cases {
            assert_eq!(number_to_string(to_number(&text)), want, "Number({text:.40?})");
        }
    }

    #[test]
    fn given_texts_with_a_leading_integer_when_parsed_as_int_then_they_match_what_node_prints() {
        let cases: [(&str, f64); 22] = [
            ("200", 200.0),
            ("0", 0.0),
            ("-5", -5.0),
            ("5000", 5000.0),
            ("12abc", 12.0),
            (" 12", 12.0),
            ("\t\n 7 ", 7.0),
            ("+5", 5.0),
            ("1e3", 1.0),
            ("0x1F", 0.0),
            ("0b11", 0.0),
            ("1.9", 1.0),
            ("00012", 12.0),
            ("12 34", 12.0),
            ("5-", 5.0),
            ("\u{a0} 42", 42.0),
            ("9007199254740993", 9_007_199_254_740_992.0),
            ("99999999999999999999", 1e20),
            ("123456789012345678901234567890", 1.234_567_890_123_456_8e29),
            ("4000", 4000.0),
            (&"1".repeat(400), f64::INFINITY),
            (&format!("-{}", "1".repeat(400)), f64::NEG_INFINITY),
        ];
        for (text, want) in cases {
            assert_eq!(parse_int(text), want, "parseInt({text:?}, 10)");
        }
        assert!(parse_int("-0").is_sign_negative() && parse_int("-0") == 0.0);
        assert!(parse_int("-0.5").is_sign_negative() && parse_int("-0.5") == 0.0);
    }

    #[test]
    fn given_texts_with_no_leading_integer_when_parsed_as_int_then_they_are_nan() {
        let cases = ["abc", "- 5", "", "  ", ".5", "\u{967}\u{968}", "\u{ff11}\u{ff12}", "\u{200b}5", "\u{180e}5", "+-5", "--5", "-", "+"];
        for text in cases {
            assert!(parse_int(text).is_nan(), "parseInt({text:?}, 10) is {}", parse_int(text));
        }
    }

    #[test]
    fn given_instants_when_formatted_then_they_match_to_iso_string() {
        let cases = [
            (0, "1970-01-01T00:00:00.000Z"),
            (86_399_999, "1970-01-01T23:59:59.999Z"),
            (86_400_000, "1970-01-02T00:00:00.000Z"),
            (951_782_400_000, "2000-02-29T00:00:00.000Z"),
            (1_700_000_000_123, "2023-11-14T22:13:20.123Z"),
            (4_102_444_800_000, "2100-01-01T00:00:00.000Z"),
            (-1, "1969-12-31T23:59:59.999Z"),
            (253_402_300_799_999, "9999-12-31T23:59:59.999Z"),
        ];
        for (ms, want) in cases {
            assert_eq!(iso_string(ms), want, "{ms} ms");
            assert_eq!(utc_date(ms), want[..10], "{ms} ms");
        }
    }
}
