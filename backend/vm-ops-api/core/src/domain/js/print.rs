//! How JavaScript prints and rounds a number: `String(x)`, what `JSON.stringify` makes of one, and `Math.round`.

/// `String(x)`: the shortest digits that read back as `x`, positional for decimal exponents -6 to 20 and in the `e` form beyond.
pub fn number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x == 0.0 {
        return "0".to_string();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-Infinity" } else { "Infinity" }.to_string();
    }
    let sign = if x < 0.0 { "-" } else { "" };
    let (digits, exponent) = shortest_digits(x.abs());
    // The value is 0.<digits> x 10^point.
    let point = exponent + 1;
    let count = digits.len() as i32;
    let body = if count <= point && point <= 21 {
        format!("{digits}{}", "0".repeat((point - count) as usize))
    } else if 0 < point && point <= 21 {
        format!("{}.{}", &digits[..point as usize], &digits[point as usize..])
    } else if -6 < point && point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else {
        let power = point - 1;
        let mantissa = if count == 1 { digits.clone() } else { format!("{}.{}", &digits[..1], &digits[1..]) };
        format!("{mantissa}e{}{}", if power < 0 { '-' } else { '+' }, power.abs())
    };
    format!("{sign}{body}")
}

/// The digits and decimal exponent of a positive finite `x`: the fewest digits that read back as `x`, the closest of those, an even
/// last digit on a tie. Rust's shortest form rounds a tie up, so the digits rounded exactly to the same count win if they read back.
fn shortest_digits(x: f64) -> (String, i32) {
    let split = |text: &str| -> (String, i32) {
        let (mantissa, exponent) = text.split_once('e').unwrap_or((text, "0"));
        (mantissa.chars().filter(char::is_ascii_digit).collect(), exponent.parse().unwrap_or(0))
    };
    let (shortest, exponent) = split(&format!("{x:e}"));
    let exact_text = format!("{x:.*e}", shortest.len() - 1);
    let (exact, exact_exponent) = split(&exact_text);
    if exact != shortest && exact_exponent == exponent && !exact.ends_with('0') && exact_text.parse() == Ok(x) {
        (exact, exponent)
    } else {
        (shortest, exponent)
    }
}

/// What `JSON.stringify(x)` writes for a number: `null` when it is NaN or infinite.
pub fn json_number(x: f64) -> String {
    if x.is_finite() {
        number_to_string(x)
    } else {
        "null".to_string()
    }
}

/// `Math.round(x)`: halves go toward +Infinity, and a negative number that rounds to zero is -0.
pub fn round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let floor = x.floor();
    let rounded = if x - floor >= 0.5 { floor + 1.0 } else { floor };
    if rounded == 0.0 && x.is_sign_negative() {
        -0.0
    } else {
        rounded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_numbers_when_printed_then_they_read_as_node_prints_them() {
        let cases: [(f64, &str); 41] = [
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (-1.0, "-1"),
            (0.5, "0.5"),
            (12.3, "12.3"),
            (12.0, "12"),
            (100.0, "100"),
            (33.3, "33.3"),
            (8.8, "8.8"),
            (0.1, "0.1"),
            (99.9, "99.9"),
            (4.35, "4.35"),
            (25.5, "25.5"),
            (-1.5, "-1.5"),
            (1234.5678, "1234.5678"),
            (0.001, "0.001"),
            (100_000.0, "100000"),
            (4_294_967_296.0, "4294967296"),
            (1_700_000_000_000.0, "1700000000000"),
            (9_007_199_254_740_992.0, "9007199254740992"),
            (9_007_199_254_740_994.0, "9007199254740994"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1.0 / 3.0, "0.3333333333333333"),
            (2.0 / 3.0, "0.6666666666666666"),
            (100.0 / 3.0, "33.333333333333336"),
            (123_456_789.123_456_79, "123456789.12345679"),
            (1e20, "100000000000000000000"),
            (123_456_789_012_345_680_000.0, "123456789012345680000"),
            (1e21, "1e+21"),
            (1.5e21, "1.5e+21"),
            (-1e21, "-1e+21"),
            (1e300, "1e+300"),
            (f64::MAX, "1.7976931348623157e+308"),
            (0.000_001, "0.000001"),
            (0.000_001_5, "0.0000015"),
            (0.000_001_234, "0.000001234"),
            (1e-7, "1e-7"),
            (1.2345e-7, "1.2345e-7"),
            (-1e-7, "-1e-7"),
            (5e-324, "5e-324"),
        ];
        for (x, want) in cases {
            assert_eq!(number_to_string(x), want, "String({x:e})");
        }
    }

    #[test]
    fn given_a_float_exactly_between_two_shortest_candidates_when_printed_then_the_even_last_digit_wins() {
        let cases = [
            (866_208_980_082_996.25, "866208980082996.2"),
            (866_208_980_082_996.75, "866208980082996.8"),
            (1_158_876_749_163_475.25, "1158876749163475.2"),
            (1_125_899_907_026_539.25, "1125899907026539.2"),
            (1_125_899_907_026_539.75, "1125899907026539.8"),
            (-3_856_959_271_664.406_25, "-3856959271664.4062"),
            (4_503_599_627_370_497.5, "4503599627370498"),
        ];
        for (x, want) in cases {
            assert_eq!(number_to_string(x), want, "String({x:e})");
        }
    }

    #[test]
    fn given_a_number_that_is_no_finite_number_when_printed_then_it_is_named_and_in_json_it_is_null() {
        assert_eq!(number_to_string(f64::NAN), "NaN");
        assert_eq!(number_to_string(f64::INFINITY), "Infinity");
        assert_eq!(number_to_string(f64::NEG_INFINITY), "-Infinity");
        for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(json_number(x), "null");
        }
        assert_eq!(json_number(-0.0), "0");
        assert_eq!(json_number(12.0), "12");
        assert_eq!(json_number(12.5), "12.5");
    }

    #[test]
    fn given_a_spread_of_floats_when_printed_then_the_text_reads_back_as_the_same_number() {
        let mut x = 1.0_f64;
        for _ in 0..2000 {
            x = (x * 1.618_033_988_749_895 + 0.1).rem_euclid(1e9) / 7.0;
            for scaled in [x, x / 1e12, x * 1e15, -x] {
                let printed = number_to_string(scaled);
                assert_eq!(printed.parse::<f64>().ok(), Some(scaled), "String({scaled:e}) = {printed}");
            }
        }
    }

    #[test]
    fn given_halves_and_near_halves_when_rounded_then_they_match_math_round() {
        let cases: [(f64, f64); 24] = [
            (0.5, 1.0),
            (1.5, 2.0),
            (2.5, 3.0),
            (-0.5, -0.0),
            (-1.5, -1.0),
            (-2.5, -2.0),
            (0.499_999_999_999_999_94, 0.0),
            (0.4, 0.0),
            (-0.4, -0.0),
            (-0.2, -0.0),
            (2.499_999_999_999_999_6, 2.0),
            (4_503_599_627_370_495.5, 4_503_599_627_370_496.0),
            (4_503_599_627_370_497.0, 4_503_599_627_370_497.0),
            (1e21, 1e21),
            (-1e21, -1e21),
            (1234.5, 1235.0),
            (85.5, 86.0),
            (1.000_000_000_000_000_2, 1.0),
            (332.5, 333.0),
            (8.5, 9.0),
            (1e-7, 0.0),
            (9_007_199_254_740_991.0, 9_007_199_254_740_991.0),
            (12.345 * 10.0, 123.0),
            (0.1 * 3.0 * 1000.0, 300.0),
        ];
        for (x, want) in cases {
            let got = round(x);
            assert_eq!((got, got.is_sign_negative()), (want, want.is_sign_negative()), "Math.round({x:e})");
        }
        assert!(round(f64::NAN).is_nan());
        assert_eq!(round(f64::INFINITY), f64::INFINITY);
        assert_eq!(round(f64::NEG_INFINITY), f64::NEG_INFINITY);
    }
}
