//! What a `.dockerignore` keeps out of a build context, with Docker's own rules (moby `patternmatcher`): a file the context never holds is
//! in no image, so an edit to it must not move the image's content hash (`image_hash`).

/// A parsed `.dockerignore`. A path is relative to the context root and uses `/`.
#[derive(Debug, Default, PartialEq)]
pub struct DockerIgnore {
    /// In file order: the pattern and whether it is an exception (`!`).
    patterns: Vec<(Vec<char>, bool)>,
}

impl DockerIgnore {
    pub fn parse(text: &str) -> DockerIgnore {
        let patterns = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|line| {
                let (exception, pattern) = line.strip_prefix('!').map_or((false, line), |rest| (true, rest.trim()));
                let cleaned = clean(pattern);
                (!cleaned.is_empty()).then(|| (cleaned.chars().collect(), exception))
            })
            .collect();
        DockerIgnore { patterns }
    }

    /// Whether the build context leaves `path` out: the last pattern that matches it or one of its parent directories decides.
    pub fn excludes(&self, path: &str) -> bool {
        let parents: Vec<String> = path.match_indices('/').map(|(i, _)| path[..i].to_string()).collect();
        let mut excluded = false;
        for (pattern, exception) in &self.patterns {
            if *exception != excluded {
                continue;
            }
            let hit = |candidate: &str| matches(pattern, &candidate.chars().collect::<Vec<_>>());
            if hit(path) || parents.iter().any(|parent| hit(parent)) {
                excluded = !exception;
            }
        }
        excluded
    }
}

/// `path.Clean` of a pattern without its leading `/`: no `.` or empty segment, no trailing `/`.
fn clean(pattern: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in pattern.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|p| *p != "..") {
                    parts.pop();
                } else {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// `*` within a segment, `**` across segments (`**/` also matches no directory), `?` one character, `[...]` a class, `\` escapes.
fn matches(pattern: &[char], text: &[char]) -> bool {
    match pattern.first() {
        None => text.is_empty(),
        Some('*') if pattern.get(1) == Some(&'*') => {
            let rest = &pattern[2..];
            match rest.first() {
                Some('/') => {
                    let after = &rest[1..];
                    matches(after, text) || (0..text.len()).any(|i| text[i] == '/' && matches(after, &text[i + 1..]))
                }
                _ => (0..=text.len()).any(|i| matches(rest, &text[i..])),
            }
        }
        Some('*') => {
            let reach = text.iter().position(|c| *c == '/').unwrap_or(text.len());
            (0..=reach).any(|i| matches(&pattern[1..], &text[i..]))
        }
        Some('?') => text.first().is_some_and(|c| *c != '/') && matches(&pattern[1..], &text[1..]),
        Some('\\') if pattern.len() > 1 => text.first() == Some(&pattern[1]) && matches(&pattern[2..], &text[1..]),
        Some('[') => match class(&pattern[1..]) {
            Some((set, consumed)) => text.first().is_some_and(|c| *c != '/' && set.contains(*c)) && matches(&pattern[1 + consumed..], &text[1..]),
            None => text.first() == Some(&'[') && matches(&pattern[1..], &text[1..]),
        },
        Some(c) => text.first() == Some(c) && matches(&pattern[1..], &text[1..]),
    }
}

struct Class {
    negated: bool,
    items: Vec<(char, char)>,
}

impl Class {
    fn contains(&self, c: char) -> bool {
        self.items.iter().any(|(lo, hi)| (*lo..=*hi).contains(&c)) != self.negated
    }
}

/// The class a `[` opens: what follows it up to the closing `]`, and how many pattern characters that took (the `]` included).
fn class(rest: &[char]) -> Option<(Class, usize)> {
    let negated = matches!(rest.first(), Some('^' | '!'));
    let start = usize::from(negated);
    let end = start + rest[start..].iter().skip(1).position(|c| *c == ']')? + 1;
    let body = &rest[start..end];
    let mut items = Vec::new();
    let mut i = 0;
    while i < body.len() {
        if i + 2 < body.len() && body[i + 1] == '-' {
            items.push((body[i], body[i + 2]));
            i += 3;
        } else {
            items.push((body[i], body[i]));
            i += 1;
        }
    }
    Some((Class { negated, items }, end + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn excluded(ignore: &str, paths: &[&str]) -> Vec<bool> {
        let parsed = DockerIgnore::parse(ignore);
        paths.iter().map(|p| parsed.excludes(p)).collect()
    }

    #[test]
    fn given_a_pattern_for_a_directory_when_a_file_inside_it_is_asked_then_it_is_excluded_and_a_sibling_is_not() {
        // Given a pattern that names the tests directory of every crate under a library workspace
        let ignore = "/libraries/agent-api/*/tests\n";
        // When files are asked, Then those under a crate's tests directory are excluded, and its sources and another library's tests are not
        let paths = ["libraries/agent-api/exec-fs/tests/fixtures/read_file.json", "libraries/agent-api/exec-fs/tests/x.rs", "libraries/agent-api/exec-fs/src/lib.rs", "libraries/agent-api/exec-fs/Cargo.toml", "libraries/other/exec-fs/tests/a.json", "libraries/agent-api/tests"];
        assert_eq!(excluded(ignore, &paths), [true, true, false, false, false, false]);
    }

    #[test]
    fn given_wildcards_when_paths_are_matched_then_star_stays_in_a_segment_and_double_star_crosses_them() {
        // Given patterns with a star, a double star in front and in the middle, and a question mark
        let ignore = "**/*.log\na/**/z\nb?\n";
        // When paths are asked
        let paths = ["x.log", "d/e/x.log", "x.logs", "a/z", "a/b/c/z", "a/z/inner", "bc", "bc/f", "bcd", "b"];
        // Then `**/` also matches no directory, `?` is exactly one character, and a longer name is not matched
        assert_eq!(excluded(ignore, &paths), [true, true, false, true, true, true, true, true, false, false]);
    }

    #[test]
    fn given_an_exception_when_it_follows_the_pattern_then_it_re_includes_what_it_names_and_only_after_it() {
        // Given every env file excluded except the one at the root, and the exception listed before and after
        let after = "**/.env.*\n!.env.example\n";
        let before = "!.env.example\n**/.env.*\n";
        // When the root example and a nested one are asked
        let paths = [".env.example", "svc/.env.example", ".env.local"];
        // Then after the pattern the exception re-includes the root file only; before it, the exception has nothing to undo
        assert_eq!(excluded(after, &paths), [false, true, true]);
        assert_eq!(excluded(before, &paths), [true, true, true]);
    }

    #[test]
    fn given_comments_blanks_and_odd_spellings_when_parsed_then_they_do_not_change_what_is_excluded() {
        // Given a file with a comment, a blank line, a trailing slash, a dot segment and a class
        let ignore = "# secrets\n\n./dist/\n/build/../out\nfile[0-9].txt\n";
        // Then the patterns are the cleaned ones
        assert_eq!(excluded(ignore, &["dist/a.js", "out/b", "build/out", "file3.txt", "filex.txt"]), [true, true, false, true, false]);
    }

    #[test]
    fn given_an_empty_ignore_file_when_asked_then_nothing_is_excluded() {
        assert_eq!(excluded("", &["a", "b/c"]), [false, false]);
    }
}
