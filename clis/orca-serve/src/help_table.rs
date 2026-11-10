//! The help text of `orca-serve`, hand-maintained under `help/`.
//! `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[
    ("serve", false, include_str!("help/serve.txt")),
    ("pair", false, include_str!("help/pair.txt")),
    ("api", false, include_str!("help/api.txt")),
];
