//! A request target as the service reads it. Node parses `new URL(req.url, 'http://localhost')`, so a target starting with
//! `//` names a host and the pathname begins at its next segment, dot segments are resolved and a segment stays percent-encoded.
use std::borrow::Cow;
use url::Url;

/// The pathname, the search (`?x=1`, empty for none or a bare `?`) and the non-empty segments of the pathname.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub pathname: String,
    pub search: String,
    pub parts: Vec<String>,
}

/// The target is no URL against `http://localhost` (`//`, an unclosed `//[`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidUrl;

impl std::fmt::Display for InvalidUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Invalid URL")
    }
}

impl std::error::Error for InvalidUrl {}

/// WHATWG skips every slash after the first two of a scheme-relative target, while the `url` crate reads `///x` as an empty
/// host; a leading run of three or more slashes (backslashes count, tabs and newlines do not) is cut back to two.
fn cut_leading_slashes(raw: &str) -> Cow<'_, str> {
    let start = raw.len() - raw.trim_start_matches(|c: char| c <= ' ').len();
    let (mut run, mut end) = (0, start);
    for (at, c) in raw[start..].char_indices() {
        match c {
            '/' | '\\' => {
                run += 1;
                end = start + at + 1;
            }
            '\t' | '\n' | '\r' => {}
            _ => break,
        }
    }
    if run < 3 {
        Cow::Borrowed(raw)
    } else {
        Cow::Owned(format!("//{}", &raw[end..]))
    }
}

impl Target {
    /// A host the `url` crate's IDNA step refuses but Node's accepts (`//xn--a-/x`, a punycode label that decodes to ASCII) is
    /// `Invalid URL` here; over 18,889 hosts a retry without the host fixed 3,282 of them and broke 8,246, so there is none.
    pub fn parse(raw: &str) -> Result<Target, InvalidUrl> {
        let base = Url::parse("http://localhost").map_err(|_| InvalidUrl)?;
        let url = Url::options().base_url(Some(&base)).parse(&cut_leading_slashes(raw)).map_err(|_| InvalidUrl)?;
        let pathname = url.path().to_string();
        let search = match url.query() {
            None | Some("") => String::new(),
            Some(query) => format!("?{query}"),
        };
        let parts = pathname.split('/').filter(|part| !part.is_empty()).map(str::to_string).collect();
        Ok(Target { pathname, search, parts })
    }

    /// The pathname and search a signature covers.
    pub fn path_and_query(&self) -> String {
        format!("{}{}", self.pathname, self.search)
    }

    pub fn is_path(&self, pathname: &str) -> bool {
        self.pathname == pathname
    }

    /// The first value of a query parameter as `URLSearchParams#get` reads it: one leading `?` is dropped, `+` is a space and
    /// escapes are decoded.
    pub fn query_param(&self, name: &str) -> Option<String> {
        let query = self.search.strip_prefix('?').unwrap_or(&self.search);
        url::form_urlencoded::parse(query.as_bytes()).find(|(key, _)| key == name).map(|(_, value)| value.into_owned())
    }
}

#[cfg(test)]
mod query_param_tests {
    use super::*;

    #[test]
    fn given_a_query_when_a_parameter_is_read_then_the_first_value_is_decoded_as_url_search_params_decodes_it() {
        let target = Target::parse("/x?url=exp%3A%2F%2Fh%3A1&tail=a+b&tail=2&empty=&flag&q=%E2%82%AC").expect("a target");

        assert_eq!(target.query_param("url").as_deref(), Some("exp://h:1"));
        assert_eq!(target.query_param("tail").as_deref(), Some("a b"));
        assert_eq!(target.query_param("empty").as_deref(), Some(""));
        assert_eq!(target.query_param("flag").as_deref(), Some(""));
        assert_eq!(target.query_param("q").as_deref(), Some("\u{20ac}"));
        assert_eq!(target.query_param("missing"), None);
        assert_eq!(Target::parse("/x").expect("a target").query_param("url"), None);
    }

    #[test]
    fn given_a_doubled_question_mark_when_a_parameter_is_read_then_only_the_first_is_dropped() {
        let target = Target::parse("/x??a=1").expect("a target");

        assert_eq!((target.query_param("a"), target.query_param("?a").as_deref().map(str::to_string)), (None, Some("1".to_string())));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(raw: &str) -> (String, String, Vec<String>) {
        let target = Target::parse(raw).unwrap_or_else(|e| panic!("{raw}: {e}"));
        (target.pathname, target.search, target.parts)
    }

    fn expect(raw: &str, pathname: &str, search: &str, parts: &[&str]) {
        let want = (pathname.to_string(), search.to_string(), parts.iter().map(|p| p.to_string()).collect::<Vec<_>>());
        assert_eq!(read(raw), want, "target {raw:?}");
    }

    #[test]
    fn given_targets_when_parsed_then_they_read_as_node_reads_them() {
        expect("/health", "/health", "", &["health"]);
        expect("/health/", "/health/", "", &["health"]);
        expect("/health?", "/health", "", &["health"]);
        expect("/health?a=2&b=%20c", "/health", "?a=2&b=%20c", &["health"]);
        expect("/x/../health", "/health", "", &["health"]);
        expect("/a/./b", "/a/b", "", &["a", "b"]);
        expect("/a/%2e%2e/b", "/b", "", &["b"]);
        expect("/a b", "/a%20b", "", &["a%20b"]);
        expect("/it's?q='x'", "/it's", "?q=%27x%27", &["it's"]);
        expect("/p?q=1#frag", "/p", "?q=1", &["p"]);
        expect("http://evil.example/x?y=1", "/x", "?y=1", &["x"]);
        expect("*", "/*", "", &["*"]);
        expect("/", "/", "", &[]);
    }

    #[test]
    fn given_doubled_slashes_when_parsed_then_the_segments_drop_the_empty_pieces_and_the_pathname_keeps_them() {
        expect("/services//leetcode-backend//ps", "/services//leetcode-backend//ps", "", &["services", "leetcode-backend", "ps"]);
    }

    #[test]
    fn given_a_target_starting_with_two_slashes_when_parsed_then_its_first_segment_is_the_host() {
        expect("//health", "/", "", &[]);
        expect("//services/leetcode-backend/ps", "/leetcode-backend/ps", "", &["leetcode-backend", "ps"]);
        expect("//services//leetcode-backend//ps", "//leetcode-backend//ps", "", &["leetcode-backend", "ps"]);
        expect("/\\x", "/", "", &[]);
    }

    #[test]
    fn given_a_target_starting_with_three_or_more_slashes_when_parsed_then_the_extra_slashes_are_skipped_as_node_skips_them() {
        expect("///x", "/", "", &[]);
        expect("////x", "/", "", &[]);
        expect("///services/a/ps", "/a/ps", "", &["a", "ps"]);
        expect("////services/a/ps", "/a/ps", "", &["a", "ps"]);
        expect("///services//a//ps", "//a//ps", "", &["a", "ps"]);
        expect("///x:80/y", "/y", "", &["y"]);
        expect("///[::1]/z", "/z", "", &["z"]);
        expect("//\\x", "/", "", &[]);
        expect("\\\\\\x", "/", "", &[]);
        expect("/\\/x", "/", "", &[]);
        expect("/\t//x", "/", "", &[]);
        expect("  ///x/y?q=1", "/y", "?q=1", &["y"]);
        expect("///.", "/", "", &[]);
        assert_eq!(Target::parse("///"), Err(InvalidUrl));
        assert_eq!(Target::parse("////"), Err(InvalidUrl));
        assert_eq!(Target::parse("///?q"), Err(InvalidUrl));
    }

    #[test]
    fn given_characters_the_path_and_the_query_encode_differently_when_parsed_then_each_follows_its_own_set() {
        expect("/a^b", "/a^b", "", &["a^b"]);
        expect("/a{b}", "/a%7Bb%7D", "", &["a%7Bb%7D"]);
        expect("/a`b", "/a%60b", "", &["a%60b"]);
        expect("/a|b", "/a|b", "", &["a|b"]);
        expect("/a<b>", "/a%3Cb%3E", "", &["a%3Cb%3E"]);
        expect("/a\"b", "/a%22b", "", &["a%22b"]);
        expect("/a\u{7f}b", "/a%7Fb", "", &["a%7Fb"]);
        expect("/a\tb", "/ab", "", &["ab"]);
        expect("/?q=a^b", "/", "?q=a^b", &[]);
        expect("/?q={a}", "/", "?q={a}", &[]);
        expect("/?q=a`b", "/", "?q=a`b", &[]);
        expect("/?q=a b", "/", "?q=a%20b", &[]);
        expect("/?q=\"x\"", "/", "?q=%22x%22", &[]);
        expect("/?q=<a>", "/", "?q=%3Ca%3E", &[]);
        expect("/\u{e9}", "/%C3%A9", "", &["%C3%A9"]);
        expect("/?q=\u{e9}", "/", "?q=%C3%A9", &[]);
        expect("/\u{1f600}", "/%F0%9F%98%80", "", &["%F0%9F%98%80"]);
        expect("/a%zz%41", "/a%zz%41", "", &["a%zz%41"]);
        expect("/a?q=%zz", "/a", "?q=%zz", &["a"]);
        expect("/a?b#c?d", "/a", "?b", &["a"]);
        expect("/health??", "/health", "??", &["health"]);
        expect("/health?=", "/health", "?=", &["health"]);
    }

    #[test]
    fn given_encoded_dot_segments_when_parsed_then_they_resolve_as_dots_and_an_encoded_slash_does_not() {
        expect("/%2e%2e/b", "/b", "", &["b"]);
        expect("/a/%2e./b", "/b", "", &["b"]);
        expect("/a/%2E%2e/b", "/b", "", &["b"]);
        expect("/a/..%2fb", "/a/..%2fb", "", &["a", "..%2fb"]);
    }

    #[test]
    fn given_an_encoded_segment_when_parsed_then_it_is_not_decoded() {
        expect("/services/leetcode%2Dbackend/ps", "/services/leetcode%2Dbackend/ps", "", &["services", "leetcode%2Dbackend", "ps"]);
        expect("/%7Efoo", "/%7Efoo", "", &["%7Efoo"]);
    }

    #[test]
    fn given_a_target_that_is_no_url_when_parsed_then_it_is_invalid() {
        assert_eq!(Target::parse("//"), Err(InvalidUrl));
        assert_eq!(Target::parse("//["), Err(InvalidUrl));
        assert_eq!(InvalidUrl.to_string(), "Invalid URL");
    }

    #[test]
    fn given_a_target_with_a_query_when_asked_for_its_signed_form_then_pathname_and_search_are_joined() {
        assert_eq!(Target::parse("/x/../health?a=1").map(|t| t.path_and_query()), Ok("/health?a=1".to_string()));
        assert_eq!(Target::parse("/health?").map(|t| t.path_and_query()), Ok("/health".to_string()));
    }
}
