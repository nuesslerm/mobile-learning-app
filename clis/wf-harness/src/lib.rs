//! The device test harness as the verbs mount it: `wf android` and `wf browser` (one command table and one Lua script API over two
//! drivers) from `harness-android` and `harness-browser`, on the run engine and goldens of `harness-core`. This crate names them
//! together: the thin binary, the cross-platform flow tests, and the paths the crates above have always used.
pub use harness_android::{android, android_lab, android_lease};
pub use harness_browser::{browser, browser_daemon};
pub use harness_core::{device, golden, pixelmatch, run, run_report, run_stop, staticd};
#[cfg(feature = "testing")]
pub use harness_browser::testing;

/// What the thin binary mounts: the two device nouns, the browser daemon, which `wf browser` starts again through the executable, and
/// the verb of `wf test` that turns a published run's screenshots into goldens.
pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[("android", android::run), ("browser", browser::run), ("browser-daemon", browser_daemon::run), ("test", run_test)];

/// `wf test goldens <run>`: the words after `test`, for the router of `wf` and the thin binary.
pub fn run_test(rest: &[String]) -> Option<i32> {
    use wf_kit::args::{parse_spec, Positional, Spec};
    let p = parse_spec(rest, &Spec { values: &["goldens-dir"], positional: Positional::Any, ..Spec::NONE })?;
    match p.positional.first().map(String::as_str)? {
        "goldens" => Some(harness_core::test_goldens::run(p.positional.get(1).map(String::as_str), p.val("goldens-dir"))),
        _ => None,
    }
}

/// The help text of both nouns.
pub const HELP: [&[(&str, bool, &str)]; 2] = [harness_android::HELP, harness_browser::HELP];
