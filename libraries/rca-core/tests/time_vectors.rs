//! Fixed vectors for `rca_core::time`, computed once from the `time` crate the module replaced: leap years, zone offsets, leap
//! seconds, missing seconds and out-of-range inputs.

use rca_core::time::{date_ms, iso, next_utc_midnight, parse, utc_date};

#[test]
fn given_unix_milliseconds_when_printed_then_iso_and_date_match_the_recorded_vectors() {
    // Given instants around leap days, the epoch and both ends of the supported range
    let vectors: &[(i64, &str, &str)] = &[
        (0, "1970-01-01T00:00:00.000Z", "1970-01-01"),
        (-1, "1969-12-31T23:59:59.999Z", "1969-12-31"),
        (1, "1970-01-01T00:00:00.001Z", "1970-01-01"),
        (951782400000, "2000-02-29T00:00:00.000Z", "2000-02-29"),
        (951868799999, "2000-02-29T23:59:59.999Z", "2000-02-29"),
        (4107542400000, "2100-03-01T00:00:00.000Z", "2100-03-01"),
        (4107628799999, "2100-03-01T23:59:59.999Z", "2100-03-01"),
        (253402300799999, "9999-12-31T23:59:59.999Z", "9999-12-31"),
        (253402300800000, "1970-01-01T00:00:00.000Z", "1970-01-01"),
        (-62167219200000, "0000-01-01T00:00:00.000Z", "0000-01-01"),
        (-62167219200001, "-0001-12-31T23:59:59.999Z", "-0001-12-31"),
        (-377705116800000, "-9999-01-01T00:00:00.000Z", "-9999-01-01"),
        (-377705116800001, "1970-01-01T00:00:00.000Z", "1970-01-01"),
        (1791288000123, "2026-10-06T12:00:00.123Z", "2026-10-06"),
        (-2203889399500, "1900-03-01T00:30:00.500Z", "1900-03-01"),
        (1483228799999, "2016-12-31T23:59:59.999Z", "2016-12-31"),
        (4107456000000, "2100-02-28T00:00:00.000Z", "2100-02-28"),
        (4107542399999, "2100-02-28T23:59:59.999Z", "2100-02-28"),
    ];
    for &(ms, want_iso, want_date) in vectors {
        // When they are printed
        // Then the text is the recorded one
        assert_eq!(iso(ms), want_iso, "{ms}");
        assert_eq!(utc_date(ms), want_date, "{ms}");
    }
}

#[test]
fn given_timestamp_text_when_parsed_then_milliseconds_match_the_recorded_vectors() {
    // Given RFC 3339 and zoneless texts, with offsets, leap seconds, missing seconds and malformed shapes
    let vectors: &[(&str, Option<i64>)] = &[
        ("2026-10-06T12:00:00.000Z", Some(1791288000000)),
        ("2026-10-06T14:00:00+02:00", Some(1791288000000)),
        ("2026-10-06T12:00:00.5Z", Some(1791288000500)),
        ("2026-10-06T12:00Z", None),
        ("2026-10-06T12:00", Some(1791288000000)),
        ("2026-10-06T12:00:00", Some(1791288000000)),
        ("2026-10-06T12:00:00.123456789", Some(1791288000123)),
        ("2026-10-06t12:00:00z", Some(1791288000000)),
        ("2026-10-06 12:00:00Z", Some(1791288000000)),
        ("2026-10-06X12:00:00Z", Some(1791288000000)),
        ("garbage", None),
        ("", None),
        ("2000-02-29T00:00:00Z", Some(951782400000)),
        ("1900-02-29T00:00:00Z", None),
        ("2100-02-29T00:00:00Z", None),
        ("2000-02-30T00:00:00Z", None),
        ("2026-13-01T00:00:00Z", None),
        ("2026-00-10T00:00:00Z", None),
        ("2026-10-06T24:00:00Z", None),
        ("2026-10-06T23:60:00Z", None),
        ("2026-10-06T23:59:60", None),
        ("2026-10-06T00:30:00+01:00", Some(1791243000000)),
        ("2026-10-06T00:30:00-23:59", Some(1791332940000)),
        ("2026-10-06T00:30:00+24:00", None),
        ("2026-10-06T00:30:00+0100", None),
        ("2016-12-31T23:59:60Z", Some(1483228799999)),
        ("2016-06-30T23:59:60Z", Some(1467331199999)),
        ("2016-12-30T23:59:60Z", None),
        ("2016-06-30T23:59:60+01:00", None),
        ("2016-07-01T00:59:60+01:00", Some(1467331199999)),
        ("2016-02-29T23:59:60Z", Some(1456790399999)),
        ("2016-02-28T23:59:60Z", None),
        ("1969-12-31T23:59:59.999Z", Some(-1)),
        ("1969-12-31T23:59:59.9995Z", Some(-1)),
        ("0000-01-01T00:00:00Z", Some(-62167219200000)),
        ("9999-12-31T23:59:59.999Z", Some(253402300799999)),
        ("9999-12-31T23:59:59.999-01:00", Some(253402304399999)),
        ("0000-01-01T00:00:00+01:00", Some(-62167222800000)),
        ("-0001-01-01T00:00:00", Some(-62198755200000)),
        ("+2026-10-06T12:00:00", Some(1791288000000)),
        ("-0001-01-01T00:00:00Z", None),
        ("2026-10-06T12:00:00.", None),
        ("2026-10-06T12:00:00Z ", None),
        ("1969-12-31T23:59:59.999999999Z", Some(-1)),
    ];
    for &(text, want) in vectors {
        // When they are parsed
        // Then the result is the recorded one
        assert_eq!(parse(text), want, "{text:?}");
    }
}

#[test]
fn given_calendar_dates_when_read_then_midnights_match_the_recorded_vectors() {
    // Given valid, impossible and oddly shaped dates
    let vectors: &[(&str, Option<i64>)] = &[
        ("2026-10-06", Some(1791244800000)),
        ("2026-02-30", None),
        ("2000-02-29", Some(951782400000)),
        ("1900-02-29", None),
        ("2100-02-29", None),
        ("2024-02-29", Some(1709164800000)),
        ("0000-01-01", Some(-62167219200000)),
        ("9999-12-31", Some(253402214400000)),
        ("-0001-12-31", Some(-62167305600000)),
        ("+2026-01-01", Some(1767225600000)),
        ("2026-1-01", None),
        ("2026-10-06T00:00", None),
        ("", None),
        ("20261006", None),
        ("2026-12-31", Some(1798675200000)),
        ("2026-04-31", None),
        ("1969-12-31", Some(-86400000)),
        ("1970-01-01", Some(0)),
    ];
    for &(text, want) in vectors {
        // When they are read
        // Then the result is the recorded one
        assert_eq!(date_ms(text), want, "{text:?}");
    }
}

#[test]
fn given_every_day_of_four_centuries_when_round_tripped_then_text_and_milliseconds_agree() {
    for day in -146_097i64..146_097 * 2 {
        // Given the midnight of a day
        let ms = day * 86_400_000;
        // When it is printed and read back
        let date = utc_date(ms);
        // Then the date and the full timestamp both return the same instant, and the next midnight is one day on
        assert_eq!(date_ms(&date), Some(ms), "{date}");
        assert_eq!(parse(&iso(ms)), Some(ms), "{date}");
        assert_eq!(next_utc_midnight(ms), ms + 86_400_000);
    }
}
