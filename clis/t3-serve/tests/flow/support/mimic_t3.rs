//! What a replay of a T3 cassette set needs besides the interpreter: the set and its allowlist read from disk, the scopes the recorded session was
//! granted, the differences a verdict found, and the session prelude the recorder's `prelude: "session"` stands for. The client side itself is
//! the step interpreter (`mimic-script-host`), the same code that recorded the set.
use mimic_core::allowlist::Allowlist;
use mimic_core::cassette::{Cassette, Dir};
use mimic_core::differ::Verdict;
use mimic_core::ports::CassetteSet;
use mimic_core::rules::RuleSet;
use mimic_script_core::case::{Case, Entry};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::Path;
use std::rc::Rc;

pub const SET: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/cassettes/t3-v0.1.6");

pub fn load_set(dir: &str) -> CassetteSet {
    let read = |name: &str| std::fs::read_to_string(Path::new(dir).join(name)).unwrap_or_else(|e| panic!("{dir}/{name}: {e}"));
    let cassette = Cassette::parse(&read("cassette.json"), &read("interactions.jsonl")).unwrap_or_else(|e| panic!("{dir}: {e}"));
    let rules: RuleSet = serde_json::from_str(&read("rules.json")).expect("rules.json is a rule set");
    CassetteSet { cassette, rules, corpus: None }
}

pub fn load_allowlist(dir: &str) -> Allowlist {
    Allowlist::parse(&std::fs::read_to_string(Path::new(dir).join("allowlist.json")).expect("the set has its allowlist")).expect("the allowlist parses")
}

/// The scopes the recorded session was granted: the `scope` of the first token exchange any interaction recorded.
pub fn granted_scopes(set: &CassetteSet) -> Vec<String> {
    let grant = set.cassette.interactions.iter().flat_map(|i| i.steps.iter()).find(|s| s.payload["body"].get("scope").is_some()).expect("a token exchange answered with its scope");
    grant.payload["body"]["scope"].as_str().expect("scope is a string").split(' ').map(String::from).collect()
}

/// What a replay found that the allowlist does not name, one line each.
pub fn differences(v: &Verdict) -> Vec<String> {
    let mut out: Vec<String> = v.results.iter().flat_map(|r| r.diffs.iter().filter(|d| d.severity != mimic_core::differ::Severity::None).map(move |d| format!("{} {}: {:?} {:?}", r.id, d.path, d.kind, d.severity))).collect();
    out.extend(v.results.iter().filter_map(|r| r.error.as_ref().map(|e| format!("{}: {e}", r.id))));
    out
}

#[allow(dead_code)] // the integration replay uses it
fn replace_in(v: &mut Value, from: &str, to: &str) {
    match v {
        Value::String(s) => *s = s.replace(from, to),
        Value::Array(a) => a.iter_mut().for_each(|x| replace_in(x, from, to)),
        Value::Object(m) => m.values_mut().for_each(|x| replace_in(x, from, to)),
        _ => {}
    }
}

/// What the recorder's `prelude: "session"` is: a pairing, a socket, a project and a thread made once per server, whose steps open every
/// interaction that asks for it. The prelude's requests are the ones the cassette recorded (ids `p1` and `p2`), so nothing here names a method.
#[allow(dead_code)] // the integration replay uses it
pub fn prelude_of(set: &CassetteSet) -> Vec<Entry> {
    let recorded = |id: &str| set.cassette.interactions.iter().flat_map(|i| i.steps.iter()).find(|s| s.dir == Dir::C2s && s.channel.as_deref() == Some("ws") && s.payload["id"] == id).unwrap_or_else(|| panic!("no recorded prelude request {id}")).payload.clone();
    let exchange = set.cassette.interactions.iter().flat_map(|i| i.steps.iter()).find(|s| s.payload["path"] == "/oauth/token").expect("a recorded token exchange").payload.clone();
    let mut form = exchange["body"].clone();
    form["subject_token"] = json!("{credential}");
    let mut script = vec![
        json!({"http": {"method": "POST", "path": "/oauth/token", "headers": exchange["headers"], "body": form}}),
        json!({"http": {"method": "POST", "path": "/api/auth/websocket-ticket/rpc", "headers": {"accept": "*/*", "authorization": "Bearer {access_token}"}, "body": null}}),
        json!({"ws-open": {"path": "/ws?orchestrationProtocol=2&wsTicket={ticket}"}}),
    ];
    script.extend(["p1", "p2"].map(|id| {
        let mut frame = recorded(id);
        replace_in(&mut frame, "<workspace>", "{workspace}");
        json!({"ws-send": frame, "await": "exit"})
    }));
    Case::entries_of(&Value::Array(script)).expect("the prelude is a script")
}

/// A candidate that answers from memory on loopback: a frame that is coming arrives within this, so the recorded reference's longer quiet window
/// is not waited out per case.
pub const LOCAL_QUIET_MS: u64 = 120;

struct Local {
    addr: SocketAddr,
    workspace: String,
    mint: Rc<dyn Fn() -> String>,
}

impl mimic_script_host::driver::Server for Local {
    fn addr(&self) -> SocketAddr {
        self.addr
    }

    fn workspace(&self) -> String {
        self.workspace.clone()
    }

    fn mint(&self) -> Result<String, String> {
        Ok((self.mint)())
    }

    fn stop(&self) {}
}

struct LocalLaunch(Rc<Local>);

impl mimic_script_host::Launch for LocalLaunch {
    fn start(&mut self) -> Result<Rc<dyn mimic_script_host::driver::Server>, String> {
        Ok(self.0.clone())
    }
}

/// The step interpreter as the candidate of a replay against a server the test started: `mint` pairs with the scopes the recorded session was
/// granted. `session` keeps one prelude for every case that names one (the recorded slices) instead of running it for each.
pub fn driver(addr: SocketAddr, workspace: String, mint: Rc<dyn Fn() -> String>, prelude: Vec<Entry>, session: bool) -> mimic_script_host::ScriptDriver {
    let policy = mimic_script_core::Policy { quiet_cap_ms: Some(LOCAL_QUIET_MS), ..Default::default() };
    mimic_script_host::ScriptDriver::new(Box::new(LocalLaunch(Rc::new(Local { addr, workspace, mint }))), policy, prelude, session, None)
}
