//! The contract tracker: is the pinned reference of a phone app the newest release of the real server, and what a move of the pin
//! makes stale. `check` compares the pin with the newest release; `bump` rewrites the pin to it; `plan` reads the `[track <app>...]`
//! marker of a commit, checks each app it names and says which cassette sets the drifted ones must be re-recorded as. The newest
//! release is read through the `Upstream` port.
use crate::ports::{Tree, Upstream};
use crate::{App, Outcome};
use serde_json::{json, Value};

pub const SCHEMA: &str = "parity-track/1";
pub const MAX_CHUNK_MINUTES: u32 = 60;

const T3_NPM_PACKAGE: &str = "t3";
const T3_REPOSITORY: &str = "pingdotgg/t3code";

/// Where the newest release is read from; the defaults are the public services.
#[derive(Debug, Clone)]
pub struct Sources {
    pub npm: String,
    pub github_api: String,
    pub github_web: String,
}

impl Default for Sources {
    fn default() -> Sources {
        Sources { npm: "https://registry.npmjs.org".into(), github_api: "https://api.github.com".into(), github_web: "https://github.com".into() }
    }
}

struct Failure {
    error: String,
    next: String,
}

type Res<T> = Result<T, Failure>;

fn failure(error: impl Into<String>, next: impl Into<String>) -> Failure {
    Failure { error: error.into(), next: next.into() }
}

fn refusal(app: Option<App>, f: Failure) -> Outcome {
    Outcome { ok: false, report: json!({"schema": SCHEMA, "ok": false, "app": app.map(App::name), "error": f.error, "next": f.next}), writes: vec![] }
}

fn get_text(up: &dyn Upstream, url: &str) -> Res<String> {
    up.get(url).map_err(|e| failure(e, "check the network (HTTPS_PROXY) and, for api.github.com, GITHUB_TOKEN (60 requests an hour without it), then run again"))
}

fn get_json(up: &dyn Upstream, url: &str) -> Res<Value> {
    serde_json::from_str(&get_text(up, url)?).map_err(|e| failure(format!("{url}: not JSON: {e}"), "the service changed its answer: read it and update clis/parity/core/src/track.rs"))
}

fn text_at(v: &Value, pointer: &str, url: &str) -> Res<String> {
    v.pointer(pointer).and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from).ok_or_else(|| failure(format!("{url}: no {pointer}"), "the service changed its answer: read it and update clis/parity/core/src/track.rs"))
}

fn read_pin(app: App, tree: &dyn Tree) -> Res<Value> {
    let file = app.pin_file();
    let pin: Value = tree.read(file).and_then(|t| serde_json::from_str(&t).ok()).ok_or_else(|| failure(format!("{file} is missing or not JSON"), format!("restore {file}")))?;
    if pin["version"].as_str().is_none_or(str::is_empty) {
        return Err(failure(format!("{file} has no version"), format!("add the pinned version to {file}")));
    }
    Ok(pin)
}

fn repository_of(pin: &Value) -> Res<String> {
    let url = pin["repository"].as_str().unwrap_or("");
    url.strip_prefix("https://github.com/").map(|r| r.trim_end_matches('/').trim_end_matches(".git").to_string()).filter(|r| r.contains('/')).ok_or_else(|| failure(format!("{}: `repository` is not a https://github.com/<owner>/<name> URL", App::Orca.pin_file()), "fix the repository of the pin"))
}

struct Latest {
    version: String,
    tag: String,
}

fn latest(app: App, pin: &Value, up: &dyn Upstream, src: &Sources) -> Res<Latest> {
    match app {
        App::T3 => {
            let url = format!("{}/{T3_NPM_PACKAGE}/latest", src.npm);
            let version = text_at(&get_json(up, &url)?, "/version", &url)?;
            Ok(Latest { tag: format!("v{version}"), version })
        }
        App::Orca => {
            let url = format!("{}/repos/{}/releases/latest", src.github_api, repository_of(pin)?);
            let tag = text_at(&get_json(up, &url)?, "/tag_name", &url)?;
            Ok(Latest { version: tag.trim_start_matches('v').to_string(), tag })
        }
    }
}

fn verdict(app: App, pin: &Value, newest: &Latest) -> Value {
    let pinned = pin["version"].as_str().unwrap_or("");
    json!({"app": app.name(), "pinned": pinned, "latest": newest.version, "drifted": pinned != newest.version})
}

fn verdict_of(app: App, tree: &dyn Tree, up: &dyn Upstream, src: &Sources) -> Res<Value> {
    let pin = read_pin(app, tree)?;
    Ok(verdict(app, &pin, &latest(app, &pin, up, src)?))
}

/// `wf parity track check`: one verdict `{app, pinned, latest, drifted}`; a pin behind the newest release is data, not a failure.
pub fn check(app: App, tree: &dyn Tree, up: &dyn Upstream, src: &Sources) -> Outcome {
    match verdict_of(app, tree, up, src) {
        Ok(v) => {
            let next = if v["drifted"] == true { format!("wf parity track bump --app {}, then re-record what it lists (the track-contracts workflow does: git commit --allow-empty -m \"[track {}]\")", app.name(), app.name()) } else { "nothing to do".into() };
            let mut report = json!({"schema": SCHEMA, "ok": true});
            report.as_object_mut().expect("object").extend(v.as_object().expect("object").clone());
            report["next"] = next.into();
            Outcome { ok: true, report, writes: vec![] }
        }
        Err(f) => refusal(Some(app), f),
    }
}

/// `SHA256SUMS` of a release: `<hex>  <file>` per line (a `*` before the file name marks binary mode).
fn parse_sums(text: &str) -> Vec<(String, String)> {
    text.lines().filter_map(|l| l.split_once(char::is_whitespace)).map(|(hash, name)| (name.trim().trim_start_matches('*').to_string(), hash.to_string())).collect()
}

fn t3_pin(pin: &Value, newest: &Latest, up: &dyn Upstream, src: &Sources) -> Res<Value> {
    let (version, tag) = (&newest.version, &newest.tag);
    let manifest = format!("{}/{T3_NPM_PACKAGE}/{version}", src.npm);
    let integrity = text_at(&get_json(up, &manifest)?, "/dist/integrity", &manifest)?;
    let commit_url = format!("{}/repos/{T3_REPOSITORY}/commits/{tag}", src.github_api);
    let commit = text_at(&get_json(up, &commit_url)?, "/sha", &commit_url)?;
    let release = format!("{}/{T3_REPOSITORY}/releases/download/{tag}", src.github_web);
    let sums = parse_sums(&get_text(up, &format!("{release}/SHA256SUMS"))?);
    let assets_url = format!("{}/repos/{T3_REPOSITORY}/releases/tags/{tag}", src.github_api);
    let assets = get_json(up, &assets_url)?;
    let mut platforms = serde_json::Map::new();
    for platform in pin["platforms"].as_object().map(|p| p.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() {
        let asset = format!("t3-{version}-{platform}.tar.gz");
        let sha256 = sums.iter().find(|(n, _)| *n == asset).map(|(_, h)| h.clone()).ok_or_else(|| failure(format!("{release}/SHA256SUMS does not list {asset}"), "the release dropped a platform the pin names: decide whether to drop it from t3-pin.json"))?;
        let bytes = assets["assets"].as_array().into_iter().flatten().find(|a| a["name"] == asset.as_str()).and_then(|a| a["size"].as_u64()).ok_or_else(|| failure(format!("{assets_url}: no asset {asset}"), "the release has no tarball for a platform the pin names"))?;
        platforms.insert(platform, json!({"bytes": bytes, "sha256": sha256}));
    }
    let mut next = pin.clone();
    next["version"] = version.clone().into();
    next["commit"] = commit.into();
    next["release"] = release.into();
    next["platforms"] = Value::Object(platforms);
    next["npm"] = json!({"integrity": integrity});
    Ok(next)
}

fn orca_pin(pin: &Value, newest: &Latest, up: &dyn Upstream, src: &Sources) -> Res<Value> {
    let url = format!("{}/repos/{}/commits/{}", src.github_api, repository_of(pin)?, newest.tag);
    let commit = text_at(&get_json(up, &url)?, "/sha", &url)?;
    let mut next = pin.clone();
    next["version"] = newest.version.clone().into();
    next["commit"] = commit.into();
    Ok(next)
}

/// The cassette sets a runner can re-record for the app: those whose `mimic.toml` takes the app's provision file.
pub fn record_sets(app: App, tree: &dyn Tree) -> Vec<String> {
    let provision = format!("provision/{}.toml", app.name());
    let mut sets: Vec<String> = tree
        .files("clis")
        .iter()
        .filter(|f| f.contains("/cassettes/") && f.ends_with("/mimic.toml"))
        .filter(|f| tree.read(f).is_some_and(|t| t.lines().any(|l| l.trim_start().starts_with("provision_file") && l.contains(&provision))))
        .filter_map(|f| f.trim_end_matches("/mimic.toml").rsplit('/').next().map(String::from))
        .collect();
    sets.sort();
    sets
}

fn stale_artifacts(app: App, sets: &[String], commit: &str) -> Vec<String> {
    let name = app.name();
    let reference = app.reference();
    let mut steps = vec![
        format!("re-record the cassette sets {} (CI: git commit --allow-empty -m \"[track {name}]\" && git push origin dev; by hand: wf mimic record <set>, docs/runbooks/record-reference-servers-on-the-vm.md)", sets.join(", ")),
        format!("git submodule update --init {reference} and check it out at {commit}, then wf parity coverage --app {name} (the methods list and the coverage table; a commit marked 'parity: pin-bump' may raise pending)"),
        format!("wf mimic crawl --all (the crawl sets of {name}: inventory, corpus and coverage of the new source), then node tooling/mimic-drivers/src/cli.ts scenarios {name}"),
        format!("derive the known gaps again after the record (WF_MIMIC_WRITE_GAPS=<set>, runbook section 7), then wf parity coverage --app {name} --traces <dir> for the evidence"),
    ];
    if app == App::T3 {
        steps.push("tooling/t3-conformance/goldens, if the T3 conformance tests still read them".into());
    }
    steps
}

fn review_fields(app: App) -> Vec<&'static str> {
    match app {
        App::T3 => vec!["protocol", "databaseMigration", "conformedStable"],
        App::Orca => vec![],
    }
}

/// `wf parity track bump`: the pin names the newest release, with its commit (and for T3 the release URL, each platform's tarball size
/// and sha256, and the npm integrity). A pin at the newest release stays as it is.
pub fn bump(app: App, tree: &dyn Tree, up: &dyn Upstream, src: &Sources) -> Outcome {
    let done = || -> Res<Outcome> {
        let pin = read_pin(app, tree)?;
        let newest = latest(app, &pin, up, src)?;
        let seen = verdict(app, &pin, &newest);
        let mut report = json!({"schema": SCHEMA, "ok": true, "app": app.name(), "pinned": seen["pinned"], "latest": seen["latest"], "drifted": seen["drifted"], "bumped": false, "wrote": []});
        if seen["drifted"] != true {
            report["next"] = "nothing to do".into();
            return Ok(Outcome { ok: true, report, writes: vec![] });
        }
        let next = match app {
            App::T3 => t3_pin(&pin, &newest, up, src)?,
            App::Orca => orca_pin(&pin, &newest, up, src)?,
        };
        let sets = record_sets(app, tree);
        let commit = next["commit"].as_str().unwrap_or("").to_string();
        report["bumped"] = true.into();
        report["wrote"] = json!([app.pin_file()]);
        report["commit"] = commit.clone().into();
        report["rerecord"] = json!(sets);
        report["stale"] = json!(stale_artifacts(app, &sets, &commit));
        report["review"] = json!(review_fields(app).iter().map(|f| format!("{} `{f}` is not derived from the release: read it in the new source and set it by hand", app.pin_file())).collect::<Vec<_>>());
        report["next"] = format!("commit the pin in a commit marked '{}', then re-record: git commit --allow-empty -m \"[track {}]\"", crate::coverage::PIN_BUMP, app.name()).into();
        let text = serde_json::to_string_pretty(&next).map_err(|e| failure(e.to_string(), "report this"))? + "\n";
        Ok(Outcome { ok: true, report, writes: vec![(app.pin_file().to_string(), text)] })
    };
    done().unwrap_or_else(|f| refusal(Some(app), f))
}

#[derive(Debug, PartialEq, Eq)]
pub struct Marker {
    pub apps: Vec<App>,
    pub chunk: Option<u32>,
    pub fresh: bool,
}

/// The `[track <app>... [chunk=<minutes>] [fresh]]` line of a commit message: `Ok(None)` when it has none (a mention mid-line is none),
/// an error when it is malformed or the message carries two.
pub fn parse_marker(message: &str) -> Result<Option<Marker>, String> {
    let lines: Vec<&str> = message.lines().map(str::trim_end).filter(|l| l.starts_with("[track ") || *l == "[track]").collect();
    let line = match lines.as_slice() {
        [] => return Ok(None),
        [one] => *one,
        _ => return Err("more than one [track ...] marker in the message".into()),
    };
    let inner = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')).ok_or_else(|| format!("{line}: a marker ends with ]"))?;
    let (mut apps, mut chunk, mut fresh) = (vec![], None, false);
    for word in inner.split_whitespace().skip(1) {
        if let Some(minutes) = word.strip_prefix("chunk=") {
            let n = minutes.parse::<u32>().ok().filter(|n| (1..=MAX_CHUNK_MINUTES).contains(n)).ok_or_else(|| format!("{word}: chunk is minutes, 1 to {MAX_CHUNK_MINUTES}"))?;
            if chunk.replace(n).is_some() {
                return Err("chunk= given twice".into());
            }
        } else if word == "fresh" {
            fresh = true;
        } else {
            let app = App::parse(word).ok_or_else(|| format!("{word}: not an app (t3 or orca), chunk=<minutes> or fresh"))?;
            if apps.contains(&app) {
                return Err(format!("{word} named twice"));
            }
            apps.push(app);
        }
    }
    if apps.is_empty() {
        return Err("[track] names no app: [track t3], [track orca] or [track t3 orca]".into());
    }
    Ok(Some(Marker { apps, chunk, fresh }))
}

/// `wf parity track plan`: the marker of a commit message, the verdict of each app it names, and the `[record ...]` marker for the sets of the
/// drifted ones (`null` when none drifted, so the workflow records nothing).
pub fn plan(message: &str, tree: &dyn Tree, up: &dyn Upstream, src: &Sources) -> Outcome {
    let marker = match parse_marker(message) {
        Ok(Some(m)) => m,
        Ok(None) => return Outcome { ok: true, report: json!({"schema": SCHEMA, "ok": true, "go": false, "apps": [], "drifted": [], "verdicts": [], "sets": [], "record_marker": null, "next": "put [track t3], [track orca] or [track t3 orca] on a line of its own"}), writes: vec![] },
        Err(e) => return refusal(None, failure(e, "[track <t3|orca>... [chunk=<minutes>] [fresh]], one marker per message")),
    };
    let (mut verdicts, mut drifted, mut sets) = (vec![], vec![], vec![]);
    for app in &marker.apps {
        match verdict_of(*app, tree, up, src) {
            Ok(v) => {
                if v["drifted"] == true {
                    drifted.push(app.name());
                    sets.extend(record_sets(*app, tree));
                }
                verdicts.push(v);
            }
            Err(f) => return refusal(Some(*app), f),
        }
    }
    let sets_named = sets.clone();
    let record_marker = (!sets.is_empty()).then(|| {
        let mut words = vec!["record".to_string()];
        words.extend(sets);
        words.extend(marker.chunk.map(|n| format!("chunk={n}")));
        words.extend(marker.fresh.then(|| "fresh".to_string()));
        format!("[{}]", words.join(" "))
    });
    let report = json!({"schema": SCHEMA, "ok": true, "go": true, "apps": marker.apps.iter().map(|a| a.name()).collect::<Vec<_>>(), "chunk": marker.chunk, "fresh": marker.fresh, "drifted": drifted, "verdicts": verdicts, "sets": sets_named, "record_marker": record_marker,
        "next": if record_marker.is_some() { "wf mimic trigger --message <record_marker> gives the capture matrix" } else { "every pin is at the newest release: nothing to record" }});
    Outcome { ok: true, report, writes: vec![] }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeUpstream, MemoryTree};

    const SRC: &str = "https://registry.test";
    const API: &str = "https://api.test";
    const WEB: &str = "https://web.test";

    fn src() -> Sources {
        Sources { npm: SRC.into(), github_api: API.into(), github_web: WEB.into() }
    }

    fn orca_tree() -> MemoryTree {
        MemoryTree::of(&[
            ("tooling/orca-conformance/orca-pin.json", r#"{"version": "1.4.214", "commit": "aaa", "repository": "https://github.com/stablyai/orca"}"#),
            ("clis/orca-serve/cassettes/orca-recorded/mimic.toml", "provision_file = \"//tooling/mimic-drivers/provision/orca.toml\"\n"),
            ("clis/orca-serve/cassettes/orca-crawl/mimic.toml", "corpus = \"corpus.json\"\n"),
            ("clis/t3-serve/cassettes/t3-recorded-1/mimic.toml", "provision_file = \"//tooling/mimic-drivers/provision/t3.toml\"\n"),
        ])
    }

    fn orca_upstream(tag: &str) -> FakeUpstream {
        FakeUpstream::of(&[(&format!("{API}/repos/stablyai/orca/releases/latest"), &format!(r#"{{"tag_name": "{tag}"}}"#)), (&format!("{API}/repos/stablyai/orca/commits/{tag}"), r#"{"sha": "bbb"}"#)])
    }

    fn t3_tree() -> MemoryTree {
        let mut tree = MemoryTree::of(&[
            ("clis/t3-serve/cassettes/t3-recorded-1/mimic.toml", "provision_file = \"//tooling/mimic-drivers/provision/t3.toml\"\n"),
            ("clis/t3-serve/cassettes/t3-recorded-2/mimic.toml", "provision_file = \"//tooling/mimic-drivers/provision/t3.toml\"\n"),
        ]);
        tree.files.insert(
            "tooling/t3-conformance/t3-pin.json".into(),
            r#"{"version": "0.1.6", "conformedStable": "0.1.6", "commit": "old", "protocol": 2, "databaseMigration": 76, "release": "old", "platforms": {"linux-x64": {"bytes": 1, "sha256": "x"}, "linux-arm64": {"bytes": 2, "sha256": "y"}}, "npm": {"integrity": "old"}}"#.into(),
        );
        tree
    }

    fn t3_upstream(version: &str) -> FakeUpstream {
        FakeUpstream::of(&[
            (&format!("{SRC}/t3/latest"), &format!(r#"{{"version": "{version}"}}"#)),
            (&format!("{SRC}/t3/{version}"), r#"{"dist": {"integrity": "sha512-new"}}"#),
            (&format!("{API}/repos/pingdotgg/t3code/commits/v{version}"), r#"{"sha": "ccc"}"#),
            (&format!("{WEB}/pingdotgg/t3code/releases/download/v{version}/SHA256SUMS"), &format!("1111  t3-{version}-linux-x64.tar.gz\n2222  t3-{version}-linux-arm64.tar.gz\n3333  t3-{version}-darwin-arm64.tar.gz\n")),
            (&format!("{API}/repos/pingdotgg/t3code/releases/tags/v{version}"), &format!(r#"{{"assets": [{{"name": "t3-{version}-linux-x64.tar.gz", "size": 10}}, {{"name": "t3-{version}-linux-arm64.tar.gz", "size": 20}}]}}"#)),
        ])
    }

    #[test]
    fn a_pin_behind_the_newest_release_is_drift_and_a_pin_at_it_is_not() {
        // Given the Orca pin at 1.4.214
        // When the newest release is 1.4.215 (tag v1.4.215)
        let behind = check(App::Orca, &orca_tree(), &orca_upstream("v1.4.215"), &src());
        // Then the verdict names both versions, without the tag's v, and says drifted
        assert!(behind.ok);
        assert_eq!(behind.report["app"], "orca");
        assert_eq!((behind.report["pinned"].as_str(), behind.report["latest"].as_str(), behind.report["drifted"].as_bool()), (Some("1.4.214"), Some("1.4.215"), Some(true)));
        // When the newest release is the pinned one, Then it is not drifted
        let level = check(App::Orca, &orca_tree(), &orca_upstream("v1.4.214"), &src());
        assert_eq!(level.report["drifted"], false);
    }

    #[test]
    fn t3_is_tracked_by_the_npm_package() {
        // Given the T3 pin at 0.1.6 and npm's latest at 0.1.7
        let v = check(App::T3, &t3_tree(), &t3_upstream("0.1.7"), &src());
        // Then the verdict is drifted from the version npm names
        assert_eq!((v.report["pinned"].as_str(), v.report["latest"].as_str(), v.report["drifted"].as_bool()), (Some("0.1.6"), Some("0.1.7"), Some(true)));
    }

    #[test]
    fn an_unreachable_upstream_is_a_failure_with_the_url_not_a_verdict() {
        // Given no answer from the service
        let v = check(App::Orca, &orca_tree(), &FakeUpstream::default(), &src());
        // Then it is a failure that names the URL and the next step
        assert!(!v.ok);
        assert!(v.report["error"].as_str().unwrap().contains("releases/latest"), "{}", v.report);
        assert!(v.report["next"].as_str().is_some_and(|n| !n.is_empty()));
        assert!(v.report.get("drifted").is_none());
    }

    #[test]
    fn bumping_orca_writes_the_release_and_its_commit_and_keeps_the_rest_of_the_pin() {
        // Given the Orca pin behind the newest release
        let out = bump(App::Orca, &orca_tree(), &orca_upstream("v1.4.215"), &src());
        // Then the pin file is the only write, at the new version and the commit of the release tag, with its repository kept
        assert!(out.ok, "{}", out.report);
        let [(path, text)] = out.writes.as_slice() else { panic!("{:?}", out.writes) };
        assert_eq!(path, "tooling/orca-conformance/orca-pin.json");
        let pin: Value = serde_json::from_str(text).unwrap();
        assert_eq!((pin["version"].as_str(), pin["commit"].as_str(), pin["repository"].as_str()), (Some("1.4.215"), Some("bbb"), Some("https://github.com/stablyai/orca")));
        assert!(text.ends_with('\n'));
        // And the report lists the sets of Orca a runner re-records, and no others
        assert_eq!(out.report["rerecord"], json!(["orca-recorded"]));
        assert!(out.report["stale"].as_array().unwrap().iter().any(|s| s.as_str().unwrap().contains("wf parity coverage --app orca")));
    }

    #[test]
    fn bumping_t3_takes_the_hashes_of_the_release_for_the_platforms_the_pin_names() {
        // Given the T3 pin with two platforms and a release whose sums also list a third
        let out = bump(App::T3, &t3_tree(), &t3_upstream("0.1.7"), &src());
        assert!(out.ok, "{}", out.report);
        let pin: Value = serde_json::from_str(&out.writes[0].1).unwrap();
        // Then version, commit, release URL, npm integrity and each pinned platform's size and sha256 are the new release's
        assert_eq!((pin["version"].as_str(), pin["commit"].as_str(), pin["npm"]["integrity"].as_str()), (Some("0.1.7"), Some("ccc"), Some("sha512-new")));
        assert_eq!(pin["release"], format!("{WEB}/pingdotgg/t3code/releases/download/v0.1.7"));
        assert_eq!(pin["platforms"], json!({"linux-x64": {"bytes": 10, "sha256": "1111"}, "linux-arm64": {"bytes": 20, "sha256": "2222"}}));
        // And what the release does not say stays as it was and is listed for review
        assert_eq!((pin["protocol"].as_u64(), pin["databaseMigration"].as_u64(), pin["conformedStable"].as_str()), (Some(2), Some(76), Some("0.1.6")));
        assert_eq!(out.report["review"].as_array().unwrap().len(), 3);
        assert_eq!(out.report["rerecord"], json!(["t3-recorded-1", "t3-recorded-2"]));
    }

    #[test]
    fn a_release_that_lacks_a_pinned_platform_is_not_written_half() {
        // Given a release whose sums miss linux-arm64
        let mut up = t3_upstream("0.1.7");
        up.answers.insert(format!("{WEB}/pingdotgg/t3code/releases/download/v0.1.7/SHA256SUMS"), "1111  t3-0.1.7-linux-x64.tar.gz\n".into());
        // When bumped, Then nothing is written and the failure names the missing tarball
        let out = bump(App::T3, &t3_tree(), &up, &src());
        assert!(!out.ok && out.writes.is_empty());
        assert!(out.report["error"].as_str().unwrap().contains("t3-0.1.7-linux-arm64.tar.gz"), "{}", out.report);
    }

    #[test]
    fn bumping_a_pin_at_the_newest_release_writes_nothing_and_asks_nothing_more() {
        // Given a pin at the newest release
        let up = orca_upstream("v1.4.214");
        let out = bump(App::Orca, &orca_tree(), &up, &src());
        // Then nothing is written and only the release was asked
        assert!(out.ok && out.writes.is_empty());
        assert_eq!(out.report["bumped"], false);
        assert_eq!(up.asked.borrow().len(), 1);
    }

    #[test]
    fn the_marker_names_the_apps_and_passes_chunk_and_fresh_on() {
        // Given a message with prose, a marker line and a trailer
        let m = parse_marker("track the contracts\n\n[track t3 orca chunk=15 fresh]\n\nCo-Authored-By: x").unwrap().unwrap();
        // Then both apps, the chunk and fresh are read
        assert_eq!(m, Marker { apps: vec![App::T3, App::Orca], chunk: Some(15), fresh: true });
        // And a message without a marker line, or with the text mid-line, starts nothing
        assert_eq!(parse_marker("fix: the [track t3] marker").unwrap(), None);
        assert_eq!(parse_marker("[record orca-recorded]").unwrap(), None);
    }

    #[test]
    fn a_malformed_marker_is_refused_with_the_reason() {
        for (message, reason) in [("[track]", "names no app"), ("[track t4]", "t4"), ("[track t3 t3]", "twice"), ("[track t3 chunk=0]", "chunk"), ("[track t3]\n[track orca]", "more than one")] {
            // Given the message, When parsed, Then it is an error naming the cause
            let e = parse_marker(message).unwrap_err();
            assert!(e.contains(reason), "{message}: {e}");
        }
    }

    #[test]
    fn the_plan_records_only_the_sets_of_the_apps_that_drifted() {
        // Given a marker for both apps, Orca behind and T3 at the newest release
        let mut tree = orca_tree();
        tree.files.extend(t3_tree().files);
        let mut up = orca_upstream("v1.4.215");
        up.answers.extend(t3_upstream("0.1.6").answers);
        let out = plan("[track t3 orca chunk=20]", &tree, &up, &src());
        // Then the verdicts are both there, and the record marker names the Orca sets only, with the chunk
        assert!(out.ok, "{}", out.report);
        assert_eq!(out.report["drifted"], json!(["orca"]));
        assert_eq!(out.report["verdicts"].as_array().unwrap().len(), 2);
        assert_eq!(out.report["record_marker"], "[record orca-recorded chunk=20]");
    }

    #[test]
    fn the_plan_has_nothing_to_record_when_no_pin_drifted_and_starts_nothing_without_a_marker() {
        // Given a marker and every pin at the newest release
        let none = plan("[track orca]", &orca_tree(), &orca_upstream("v1.4.214"), &src());
        // Then there is no record marker
        assert!(none.ok && none.report["go"] == true && none.report["record_marker"].is_null());
        // And a message without a marker is go: false, with no request made
        let up = FakeUpstream::default();
        let quiet = plan("chore: an ordinary commit", &orca_tree(), &up, &src());
        assert!(quiet.ok && quiet.report["go"] == false);
        assert!(up.asked.borrow().is_empty());
    }
}
