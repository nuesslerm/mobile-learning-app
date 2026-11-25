//! Optional local OTLP capture shared by the full CLI and its thin binaries.
use std::process::Command;
use std::sync::{Arc, Mutex};
use telemetry::{Destination, Telemetry};

fn runner() -> Command {
    let mut cmd = Command::new("python3");
    cmd.args(["-c", include_str!("performance.py")]).env("WF_PERFORMANCE_ROOT", crate::root::repo_root());
    cmd
}

fn run(cmd: &mut Command) -> i32 {
    match cmd.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => { eprintln!("starting performance capture: {error}"); 1 }
    }
}

pub(crate) fn command(args: &[String]) -> i32 {
    run(runner().arg("--").arg(std::env::current_exe().expect("the running executable")).args(args))
}

/// Record the RCA integration suite or import a previously captured CI database.
pub fn integration(args: &[String]) -> Option<i32> {
    let opts = crate::args::parse_none(args, &["db", "profile", "merge", "report"], &["performance", "send", "catalog"])?;
    if [opts.flag("catalog"), opts.val("report").is_some(), opts.val("merge").is_some()].into_iter().filter(|active| *active).count() > 1 { return None; }
    let root = crate::root::repo_root();
    let mut cmd = runner();
    for key in ["db", "profile"] {
        if let Some(value) = opts.val(key) { cmd.arg(format!("--{key}")).arg(value); }
    }
    if opts.flag("send") { cmd.arg("--send"); }
    if let Some(path) = opts.val("report") {
        cmd.arg("--report").arg(path);
    } else if opts.val("merge").is_some() {
        cmd.arg("--merge").args(opts.values("merge"));
    } else {
        let filter = if opts.flag("catalog") {
            cmd.arg("--catalog");
            "catalog::"
        } else {
            cmd.arg("--budgets").arg(root.join("clis/app-bench/scenarios/rca-hosts-trace.json"));
            "performance::"
        };
        cmd.args(["--", "cargo", "test", "--locked", "--manifest-path"]).arg(root.join("clis/Cargo.toml"))
            .args(["-p", "orca-serve", "-p", "t3-serve", "--test", "integration", filter, "--", "--ignored", "--test-threads=1"]);
    }
    Some(run(&mut cmd))
}

#[track_caller]
pub fn external(client: &'static str) -> tracing::span::EnteredSpan {
    let caller = std::panic::Location::caller();
    tracing::info_span!("external.api", otel.name = %format!("external.{client}"), caller.file = caller.file(), caller.line = caller.line(), api.outcome = tracing::field::Empty).entered()
}

pub(crate) struct Capture(Arc<Mutex<Option<Telemetry>>>);

impl Drop for Capture {
    fn drop(&mut self) {
        if let Some(exporter) = self.0.lock().unwrap().take() {
            exporter.shutdown();
        }
    }
}

pub(crate) fn install() -> Option<Capture> {
    let endpoint = std::env::var("WF_PERFORMANCE_ENDPOINT").ok()?;
    // The performance subscriber should preserve machine-readable command stdout.
    let quiet_console = std::env::var_os("RUST_LOG").is_none();
    if quiet_console {
        std::env::set_var("RUST_LOG", "off");
    }
    let service = std::env::current_exe().ok().and_then(|path| path.file_name().map(|s| s.to_string_lossy().into_owned())).unwrap_or_else(|| "wf".into());
    let (dispatch, exporter) = telemetry::build(
        telemetry::source!(service),
        vec![],
        Some(Destination::new(endpoint, "")),
    );
    if quiet_console { std::env::remove_var("RUST_LOG"); }
    tracing::dispatcher::set_global_default(dispatch).expect("install the performance subscriber");
    let capture = Capture(Arc::new(Mutex::new(Some(exporter))));
    if std::env::var_os("WF_PERFORMANCE_FIXTURE").is_some() {
        let mut signals = signal_hook::iterator::Signals::new([signal_hook::consts::SIGUSR1])
            .expect("fixture shutdown signal");
        let worker = Capture(capture.0.clone());
        std::thread::spawn(move || {
            if signals.forever().next().is_some() {
                drop(worker);
                std::process::exit(0);
            }
        });
    }
    Some(capture)
}
