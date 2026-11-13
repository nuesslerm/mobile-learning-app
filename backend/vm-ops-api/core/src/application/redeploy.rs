//! `POST /services/<app>/redeploy` and `POST /services/<app>/retire`: another app is redeployed, or an app that left the list is torn
//! down, by a foreground sibling whose exit code is the answer. The redeploy is bracketed by two history appends. `POST
//! /services/orca-serve/pair` mints a pairing link in the deployed orca-serve container; the link is a credential and is never logged.
use crate::application::ports::{Clock, Event, EventLog, SiblingResult, Siblings};
use crate::application::scripts::redeploy::{append_history_args, app_redeploy_sibling_args, app_smoke_sibling_args, pair_sibling_args, retire_sibling_args, smoke_stop_sibling_args, AppRedeploy, HistoryEntry, RedeployEnv};
use crate::domain::apps::{compose_file, host_allowed_apps, is_known_app, is_retirable};
use crate::domain::js::JsString;
use crate::domain::provision_ref::resolve_provision_ref;
use crate::domain::validators::{is_valid_image_tag, is_valid_provision_ref, is_valid_recreate_mode, is_valid_requester_label};
use std::sync::Arc;

const REDEPLOY_TAIL_UNITS: usize = 4000;
const RETIRE_TAIL_UNITS: usize = 2000;
const UNKNOWN_REQUESTER: &str = "unknown";
const PAIRABLE_APP: &str = "orca-serve";

/// What a redeploy's query carried, each value as the request sent it and none when it sent none.
#[derive(Debug, Clone, Copy, Default)]
pub struct AppRequest<'a> {
    pub app: &'a str,
    pub tag: Option<&'a str>,
    pub reference: Option<&'a str>,
    pub recreate: Option<&'a str>,
    pub wipe_state: Option<&'a str>,
    pub requested_by: Option<&'a str>,
}

/// Why a request is refused with its own reply, before any effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    UnknownApp,
    /// The host label allows no deploy of this app; `allowed` is what it does allow.
    NotOnThisHost { host: String, allowed: Vec<&'static str> },
    InvalidTag,
    InvalidRef,
    InvalidRecreate,
    NotARetiredApp,
    NotAPairableApp,
}

/// A redeploy that ran, whatever its exit code was.
#[derive(Debug, Clone, PartialEq)]
pub struct AppDone {
    pub app: String,
    pub tag: String,
    pub reference: String,
    pub wipe_state: bool,
    /// None when a signal ended the sibling.
    pub exit_code: Option<i32>,
    pub server_script_sha: String,
    pub stdout: JsString,
    pub stderr: JsString,
}

impl AppDone {
    pub fn succeeded(&self) -> bool {
        self.exit_code == Some(0)
    }
}

/// A retire that ran, whatever its exit code was.
#[derive(Debug, Clone, PartialEq)]
pub struct RetireDone {
    pub app: String,
    pub exit_code: Option<i32>,
    pub stdout: JsString,
    pub stderr: JsString,
}

impl RetireDone {
    pub fn succeeded(&self) -> bool {
        self.exit_code == Some(0)
    }
}

/// What a pair that ran produced. The link is a credential: this type has no `Debug` and nothing here logs it.
pub enum PairOutcome {
    Minted(String),
    /// The sibling failed or printed no link; nothing of its output is kept.
    Failed,
}

/// `String(code)` of a sibling's exit code, which a signal leaves null.
fn code_text(code: Option<i32>) -> String {
    code.map_or_else(|| "null".to_string(), |code| code.to_string())
}

/// A request that passed every check.
struct Checked<'a> {
    compose: &'static str,
    tag: &'a str,
    reference: String,
}

pub struct Redeploy {
    siblings: Arc<dyn Siblings>,
    log: Arc<dyn EventLog>,
    clock: Arc<dyn Clock>,
    env: RedeployEnv,
    server_script_sha: String,
}

impl Redeploy {
    pub fn new(siblings: Arc<dyn Siblings>, log: Arc<dyn EventLog>, clock: Arc<dyn Clock>, env: RedeployEnv, server_script_sha: String) -> Self {
        Self { siblings, log, clock, env, server_script_sha }
    }

    /// The app's compose file, the tag and the ref a request names, checked in the order Node does (app, host, tag, ref, recreate).
    fn checked<'a>(&self, request: &AppRequest<'a>) -> Result<Checked<'a>, Refused> {
        let app = request.app;
        let compose = compose_file(app).filter(|_| is_known_app(app)).ok_or(Refused::UnknownApp)?;
        let host = self.env.host_label.as_str();
        let allowed = host_allowed_apps(host);
        if !allowed.is_some_and(|apps| apps.contains(&app)) {
            return Err(Refused::NotOnThisHost { host: host.to_string(), allowed: allowed.map(<[&str]>::to_vec).unwrap_or_default() });
        }
        let tag = request.tag.filter(|tag| !tag.is_empty()).unwrap_or("latest");
        if !is_valid_image_tag(tag) {
            return Err(Refused::InvalidTag);
        }
        let reference = resolve_provision_ref(tag, request.reference);
        if !is_valid_provision_ref(&reference) {
            return Err(Refused::InvalidRef);
        }
        if request.recreate.is_some_and(|mode| !is_valid_recreate_mode(mode)) {
            return Err(Refused::InvalidRecreate);
        }
        Ok(Checked { compose, tag, reference })
    }

    /// Checks the request, and only then runs the history append, the app's sibling and the closing append.
    pub async fn redeploy_app(&self, request: &AppRequest<'_>) -> Result<AppDone, Refused> {
        let app = request.app;
        let Checked { compose, tag, reference } = self.checked(request)?;
        let wipe_state = request.wipe_state.is_some_and(|value| matches!(value.to_lowercase().as_str(), "1" | "true"));
        let requested_by = request.requested_by.filter(|label| is_valid_requester_label(label)).unwrap_or(UNKNOWN_REQUESTER);
        let image = self.env.sibling_image();
        let event = |event: Event| event.with("app", app).with("tag", tag).with("requestedBy", requested_by);
        let history = |outcome: &'static str, exit_code: Option<i32>| {
            let key = format!("service-{app}");
            let entry = HistoryEntry { key: &key, requested_by, tag, reference: &reference, outcome, exit_code };
            append_history_args(&self.env.vm_dir, &image, &entry, self.clock.now_ms())
        };

        self.log.emit(event(Event::info("redeploy.started")));
        self.siblings.run(&history("redeploy.triggered", None)).await;
        let host_dir = format!("/home/ubuntu/{app}");
        let script = AppRedeploy { app, host_dir: &host_dir, compose_file: compose, tag, reference: &reference, wipe_state, recreate_all: request.recreate == Some("all") };
        let result = self.siblings.run(&app_redeploy_sibling_args(&self.env, &script)).await;
        let succeeded = result.code == Some(0);
        self.siblings.run(&history(if succeeded { "redeploy.succeeded" } else { "redeploy.failed" }, result.code)).await;
        let outcome = if succeeded { Event::info("redeploy.succeeded") } else { Event::error("redeploy.failed") };
        self.log.emit(event(outcome).with("exitCode", code_text(result.code)));
        Ok(AppDone {
            app: app.to_string(),
            tag: tag.to_string(),
            reference,
            wipe_state,
            exit_code: result.code,
            server_script_sha: self.server_script_sha.clone(),
            stdout: JsString::from(result.stdout.as_str()).last(REDEPLOY_TAIL_UNITS),
            stderr: JsString::from(result.stderr.as_str()).last(REDEPLOY_TAIL_UNITS),
        })
    }

    /// Starts the throwaway container of an app's pulled image on the spare port and leaves it running when it passes its own probe;
    /// a failure removes it and carries its log. An app with no smoke test answers exit 0 and says so.
    pub async fn smoke_app(&self, request: &AppRequest<'_>) -> Result<AppDone, Refused> {
        let app = request.app;
        let Checked { compose, tag, reference } = self.checked(request)?;
        let host_dir = format!("/home/ubuntu/{app}");
        let script = AppRedeploy { app, host_dir: &host_dir, compose_file: compose, tag, reference: &reference, wipe_state: false, recreate_all: false };
        let result = match app_smoke_sibling_args(&self.env, &script) {
            Some(args) => self.siblings.run(&args).await,
            None => SiblingResult { code: Some(0), stdout: format!("[smoke] {app} has no smoke test\n"), stderr: String::new() },
        };
        self.log.emit(Event::info("smoke.ran").with("app", app).with("tag", tag).with("exitCode", code_text(result.code)));
        Ok(AppDone {
            app: app.to_string(),
            tag: tag.to_string(),
            reference,
            wipe_state: false,
            exit_code: result.code,
            server_script_sha: self.server_script_sha.clone(),
            stdout: JsString::from(result.stdout.as_str()).last(REDEPLOY_TAIL_UNITS),
            stderr: JsString::from(result.stderr.as_str()).last(REDEPLOY_TAIL_UNITS),
        })
    }

    /// Removes the throwaway container of `app`'s smoke test, whether or not one is running.
    pub async fn smoke_stop(&self, app: &str) -> Result<RetireDone, Refused> {
        if !is_known_app(app) {
            return Err(Refused::UnknownApp);
        }
        let result = self.siblings.run(&smoke_stop_sibling_args(&self.env, app)).await;
        Ok(RetireDone {
            app: app.to_string(),
            exit_code: result.code,
            stdout: JsString::from(result.stdout.as_str()).last(RETIRE_TAIL_UNITS),
            stderr: JsString::from(result.stderr.as_str()).last(RETIRE_TAIL_UNITS),
        })
    }

    /// Takes down the compose project of an app that left the list. Neither a managed app nor the control plane may be retired, so
    /// this cannot stop anything the redeploy route manages or this service runs on.
    pub async fn retire(&self, app: &str) -> Result<RetireDone, Refused> {
        if !is_retirable(app) {
            return Err(Refused::NotARetiredApp);
        }
        self.log.emit(Event::info("retire.started").with("app", app));
        let result = self.siblings.run(&retire_sibling_args(&self.env, app)).await;
        let outcome = if result.code == Some(0) { Event::info("retire.succeeded") } else { Event::error("retire.failed") };
        self.log.emit(outcome.with("app", app).with("exitCode", code_text(result.code)));
        Ok(RetireDone {
            app: app.to_string(),
            exit_code: result.code,
            stdout: JsString::from(result.stdout.as_str()).last(RETIRE_TAIL_UNITS),
            stderr: JsString::from(result.stderr.as_str()).last(RETIRE_TAIL_UNITS),
        })
    }

    /// Mints a pairing link in the deployed orca-serve container by one fixed sibling script. Only `orca-serve` may be paired.
    pub async fn pair(&self, app: &str) -> Result<PairOutcome, Refused> {
        if app != PAIRABLE_APP {
            return Err(Refused::NotAPairableApp);
        }
        self.log.emit(Event::info("pair.started").with("app", app));
        let result = self.siblings.run(&pair_sibling_args(&self.env)).await;
        let url = if result.code == Some(0) { pairing_url(&result.stdout) } else { None };
        let outcome = if url.is_some() { Event::info("pair.succeeded") } else { Event::error("pair.failed") };
        self.log.emit(outcome.with("app", app).with("exitCode", code_text(result.code)));
        Ok(url.map_or(PairOutcome::Failed, PairOutcome::Minted))
    }
}

/// The `url` of the last JSON line of the pair verb's stdout, when it is an `orca://pair` link.
fn pairing_url(stdout: &str) -> Option<String> {
    let line = stdout.lines().map(str::trim).rfind(|line| !line.is_empty())?;
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let url = value.get("url")?.as_str()?;
    (url.starts_with("orca://pair") && !url.chars().any(char::is_whitespace)).then(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::{Level, SiblingResult};
    use crate::testing::{FakeClock, FakeEventLog, FakeSiblings, SiblingCall};
    use serde_json::Value;

    const NOON: i64 = 1_791_115_200_000;
    const HISTORY: &str = ">> redeploy-history.jsonl";
    const APP_SCRIPT: &str = "[redeploy] start";

    struct Rig {
        siblings: Arc<FakeSiblings>,
        log: Arc<FakeEventLog>,
        redeploy: Redeploy,
    }

    fn rig(siblings: FakeSiblings, host: &str) -> Rig {
        let (siblings, log) = (Arc::new(siblings), Arc::new(FakeEventLog::default()));
        let env = RedeployEnv { host_label: host.into(), image_namespace: "ns".into(), provision_token: "tok".into(), vm_dir: "/home/ubuntu/vm-ops-api".into(), ..RedeployEnv::default() };
        let redeploy = Redeploy::new(siblings.clone(), log.clone(), Arc::new(FakeClock::at(NOON)), env, "0123456789ab".into());
        Rig { siblings, log, redeploy }
    }

    fn running(code: i32, stdout: &str) -> FakeSiblings {
        FakeSiblings::new().with_run(HISTORY, vec![SiblingResult::exited(0, "", "")]).with_run(APP_SCRIPT, vec![SiblingResult::exited(code, stdout, "boom")])
    }

    fn ran(rig: &Rig) -> Vec<Vec<String>> {
        rig.siblings.calls().into_iter().map(|call| match call {
            SiblingCall::Run(args) | SiblingCall::Detached(args) => args,
        }).collect()
    }

    fn history_line(args: &[String]) -> Value {
        let script = args.last().cloned().unwrap_or_default();
        let line = script.strip_prefix("printf '%s\\n' '").and_then(|rest| rest.split_once("' >> ")).map(|(line, _)| line.to_string());
        serde_json::from_str(&line.expect("a history line")).expect("JSON")
    }

    fn request(app: &str) -> AppRequest<'_> {
        AppRequest { app, ..AppRequest::default() }
    }

    #[tokio::test]
    async fn given_a_redeploy_when_it_passes_then_two_history_lines_bracket_the_app_sibling_and_an_explicit_ref_wins() {
        // Given leetcode-backend on host main, its sibling exiting 0
        let rig = rig(running(0, "ok"), "main");

        // When it is redeployed at a sha tag with an explicit ref and a requester
        let done = rig.redeploy.redeploy_app(&AppRequest { tag: Some("sha-89abcde"), reference: Some("feature/x"), requested_by: Some("wf ship"), ..request("leetcode-backend") }).await.expect("a redeploy");

        // Then the explicit ref won, and three siblings ran: triggered, the app, succeeded
        assert_eq!((done.reference.as_str(), done.exit_code, done.succeeded()), ("feature/x", Some(0), true));
        let argvs = ran(&rig);
        assert_eq!(argvs.len(), 3);
        let outcomes: Vec<(Value, Value)> = [&argvs[0], &argvs[2]].map(|args| (history_line(args)["outcome"].clone(), history_line(args)["exitCode"].clone())).to_vec();
        assert_eq!(outcomes, [("redeploy.triggered".into(), Value::Null), ("redeploy.succeeded".into(), 0.into())]);
        assert!(argvs[1].last().is_some_and(|script| script.contains("/mobile-learning-app/feature/x/backend/leetcode-backend/")));
        // And both events carry the app, tag and requester, the second the exit code as text
        let events = rig.log.events();
        let expected_started = Event::info("redeploy.started").with("app", "leetcode-backend").with("tag", "sha-89abcde").with("requestedBy", "wf ship");
        let expected_done = Event::info("redeploy.succeeded").with("app", "leetcode-backend").with("tag", "sha-89abcde").with("requestedBy", "wf ship").with("exitCode", "0");
        assert_eq!(events, [expected_started, expected_done]);
    }

    #[tokio::test]
    async fn given_a_sibling_that_fails_when_redeployed_then_the_failure_is_recorded_and_the_output_keeps_its_last_4000_units() {
        // Given a sibling that prints 5000 units and exits 1
        let rig = rig(running(1, &"o".repeat(5000)), "main");

        // When leetcode-backend is redeployed with no ref
        let done = rig.redeploy.redeploy_app(&request("leetcode-backend")).await.expect("a redeploy");

        // Then the trunk is the ref, the exit code is 1 and the history and the event say failed
        assert_eq!((done.reference.as_str(), done.exit_code, done.stdout.len()), ("dev", Some(1), 4000));
        assert_eq!(history_line(&ran(&rig)[2])["outcome"], "redeploy.failed");
        let failed = rig.log.events().pop().expect("an event");
        assert_eq!((failed.level, failed.message), (Level::Error, "redeploy.failed"));
        assert_eq!(failed.attributes.last(), Some(&("exitCode", Value::from("1"))));
    }

    #[tokio::test]
    async fn given_bad_input_when_redeployed_then_it_is_refused_in_node_s_order_before_any_effect() {
        // Given a service on host main, and one on another host
        let main = rig(running(0, ""), "main");
        let other_host = rig(running(0, ""), "other");

        // When an unknown app, a bad tag with a bad ref, a bad ref and a bad recreate are sent
        let bad_tag = AppRequest { tag: Some("-bad"), reference: Some("a..b"), recreate: Some("x"), ..request("leetcode-backend") };
        let bad_ref = AppRequest { reference: Some("a..b"), recreate: Some("x"), ..request("leetcode-backend") };
        let bad_recreate = AppRequest { recreate: Some("ALL"), ..request("leetcode-backend") };
        let got = [
            main.redeploy.redeploy_app(&request("vm-ops-api")).await.err(),
            main.redeploy.redeploy_app(&bad_tag).await.err(),
            main.redeploy.redeploy_app(&bad_ref).await.err(),
            main.redeploy.redeploy_app(&bad_recreate).await.err(),
            other_host.redeploy.redeploy_app(&bad_tag).await.err(),
        ];

        // Then each is refused as named, and nothing ran
        let host = Refused::NotOnThisHost { host: "other".into(), allowed: Vec::new() };
        let expected = [Refused::UnknownApp, Refused::InvalidTag, Refused::InvalidRef, Refused::InvalidRecreate, host].map(Some);
        assert_eq!(got, expected);
        for rig in [&main, &other_host] {
            assert_eq!((ran(rig), rig.log.events()), (Vec::<Vec<String>>::new(), Vec::new()));
        }
    }

    #[tokio::test]
    async fn given_a_name_when_retired_then_only_a_pattern_name_outside_the_control_plane_runs_a_sibling_and_its_output_keeps_2000_units() {
        // Given a retire sibling that exits 0 with 3000 units
        let siblings = FakeSiblings::new().with_run("docker compose -p", vec![SiblingResult::exited(0, &"o".repeat(3000), "")]);
        let rig = rig(siblings, "main");

        // When the managed apps, the control plane and names outside the pattern are retired
        for name in ["leetcode-backend", "vm-ops-api", "watchdog", "Old-app", "a"] {
            assert_eq!(rig.redeploy.retire(name).await, Err(Refused::NotARetiredApp), "{name}");
        }
        assert_eq!(ran(&rig), Vec::<Vec<String>>::new());
        // And old-app, whose compose file left the repo, is retired
        let done = rig.redeploy.retire("old-app").await.expect("a retire");

        // Then its sibling took the compose project down and the reply keeps the last 2000 units; both events name the app
        assert_eq!((done.succeeded(), done.stdout.len()), (true, 2000));
        assert_eq!(ran(&rig)[0].last().map(String::as_str), Some("docker compose -p old-app down --volumes --remove-orphans"));
        let expected = [Event::info("retire.started").with("app", "old-app"), Event::info("retire.succeeded").with("app", "old-app").with("exitCode", "0")];
        assert_eq!(rig.log.events(), expected);
    }

    #[tokio::test]
    async fn given_an_app_when_paired_then_only_orca_serve_runs_the_one_fixed_script_and_the_link_is_in_no_event() {
        // Given a pair sibling that prints a log line and then the verb's JSON line with a recognisable fake link
        const LINK: &str = "orca://pair?code=FAKE-CREDENTIAL-123";
        let stdout = format!("starting\n{{\"ok\":true,\"url\":\"{LINK}\",\"deviceId\":\"d1\"}}\n");
        let rig = rig(FakeSiblings::new().with_run("docker exec orca-serve", vec![SiblingResult::exited(0, &stdout, "")]), "main");

        // When other apps and orca-serve are paired
        for name in ["leetcode-backend", "vm-ops-api", "old-app", "Orca-serve", ""] {
            assert_eq!(rig.redeploy.pair(name).await.err(), Some(Refused::NotAPairableApp), "{name}");
        }
        assert_eq!(ran(&rig), Vec::<Vec<String>>::new());
        let outcome = rig.redeploy.pair("orca-serve").await.expect("a pair");

        // Then the one fixed script ran, the link came back, and no event carries it
        assert!(matches!(outcome, PairOutcome::Minted(ref url) if url == LINK));
        assert_eq!(ran(&rig)[0].last().map(String::as_str), Some("docker exec orca-serve orca-serve pair --endpoint ws://orca-serve:6768"));
        let events = rig.log.events();
        assert_eq!(events, [Event::info("pair.started").with("app", "orca-serve"), Event::info("pair.succeeded").with("app", "orca-serve").with("exitCode", "0")]);
        assert!(!format!("{events:?}").contains("FAKE-CREDENTIAL"));
    }

    #[tokio::test]
    async fn given_a_pair_sibling_that_fails_or_prints_no_link_when_paired_then_it_is_failed_and_nothing_is_logged_of_its_output() {
        // Given siblings that exit 1 with a link in stdout, exit 0 with no url, and exit 0 with a url that is no pairing link
        let link_line = "{\"url\":\"orca://pair?code=FAKE-CREDENTIAL-123\"}\n";
        let cases = [(1, link_line), (0, "{\"ok\":true}\n"), (0, "{\"url\":\"https://example.com/x\"}\n"), (0, "not json\n"), (0, "")];
        for (code, stdout) in cases {
            let rig = rig(FakeSiblings::new().with_run("docker exec orca-serve", vec![SiblingResult::exited(code, stdout, "FAKE-CREDENTIAL-STDERR")]), "main");

            // When orca-serve is paired
            let outcome = rig.redeploy.pair("orca-serve").await.expect("a pair");

            // Then it failed, and the failure event carries only the exit code
            assert!(matches!(outcome, PairOutcome::Failed), "{code} {stdout:?}");
            let events = rig.log.events();
            assert_eq!(events[1], Event::error("pair.failed").with("app", "orca-serve").with("exitCode", code.to_string()));
            assert!(!format!("{events:?}").contains("FAKE-CREDENTIAL"));
        }
    }
}
