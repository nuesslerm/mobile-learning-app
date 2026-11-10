//! The source control rows of M11 (`t3-vcs` additions): what the executor's machine is signed in to and how a person signs it in (the CLI's own
//! device flow: t3 serve sees the one-time code and the address, never a token), looking a repository up, publishing one, and cloning one into a
//! new project with progress. Every request is the agent API's `hosted.auth` and `hosted.clone` groups, done by the executor with the machine's own sign-in.
use super::pr::{failure, provider_of};
use super::queries::{field, QueryFailure};
use super::service::Service;
use crate::domain::command::Family;
use crate::ports::{AgentError, Follow, Op};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use t3_contracts::time::iso;

const PROVIDERS: [(&str, &str, &str); 2] = [("github", "GitHub CLI", "gh"), ("gitlab", "GitLab CLI", "glab")];
/// How long one `signInWait` holds the socket before it says the sign-in is still open.
const SIGN_IN_PATIENCE_MS: u64 = 60_000;
const CLONES_KEY: &str = "projectClones";

fn some(v: &str) -> Value {
    json!({"_tag": "Some", "value": v})
}

fn none() -> Value {
    json!({"_tag": "None"})
}

fn repository_error(provider: &str, operation: &str, e: &AgentError) -> QueryFailure {
    let detail = e.message();
    QueryFailure::Error(json!({"_tag": "SourceControlRepositoryError", "provider": provider, "operation": operation, "detail": if detail.is_empty() { "the executor refused without saying why".to_string() } else { detail }}))
}

fn sign_in_error(provider: &str, host: &str, reason: &str, detail: &str) -> QueryFailure {
    QueryFailure::Error(json!({"_tag": "SourceControlSignInError", "provider": provider, "host": host, "reason": reason, "detail": detail}))
}

fn flow_key(provider: &str, host: &str) -> String {
    format!("sourceControl.flow:{provider}:{host}")
}

/// `owner/name` of a clone address (`https://host/owner/name.git`, `git@host:owner/name.git`) and the host it is on.
pub fn repository_of(remote: &str) -> Option<(String, String)> {
    let rest = remote.strip_prefix("https://").or_else(|| remote.strip_prefix("http://")).or_else(|| remote.strip_prefix("ssh://git@")).or_else(|| remote.strip_prefix("git@"))?;
    let (host, path) = rest.split_once(['/', ':'])?;
    let path = path.trim_matches('/').trim_end_matches(".git");
    (!host.is_empty() && path.contains('/')).then(|| (host.to_string(), path.to_string()))
}

/// What a clone of a project asked for, kept so it can be retried, cancelled and followed.
#[derive(Debug, Clone, PartialEq)]
struct CloneRecord {
    clone: String,
    provider: String,
    host: String,
    repo: String,
    destination: String,
    remote_url: String,
    started_ms: i64,
}

impl CloneRecord {
    fn to_json(&self) -> Value {
        json!({"clone": self.clone, "provider": self.provider, "host": self.host, "repo": self.repo, "destination": self.destination, "remoteUrl": self.remote_url, "startedMs": self.started_ms})
    }

    fn from_json(v: &Value) -> Option<CloneRecord> {
        let s = |k: &str| v[k].as_str().map(str::to_string);
        Some(CloneRecord { clone: s("clone")?, provider: s("provider")?, host: s("host")?, repo: s("repo")?, destination: s("destination")?, remote_url: s("remoteUrl")?, started_ms: v["startedMs"].as_i64()? })
    }
}

/// A clone's state as T3's snapshot. `now` stamps the end of one that ended.
fn snapshot(project: &str, record: &CloneRecord, state: &Value, now: i64) -> Value {
    let phase = match state["state"].as_str() {
        Some("done") => "done",
        Some("failed") => "failed",
        Some("cancelled") => "cancelled",
        _ => "running",
    };
    let error = (phase == "failed" || phase == "cancelled").then(|| state["detail"]["message"].as_str().unwrap_or(phase).chars().take(1000).collect::<String>());
    json!({
        "projectId": project, "remoteUrl": record.remote_url, "destinationPath": record.destination,
        "repository": {"provider": record.provider, "nameWithOwner": record.repo, "url": format!("https://{}/{}", record.host, record.repo), "sshUrl": format!("git@{}:{}.git", record.host, record.repo)},
        "phase": phase, "stage": if phase == "done" { "checkout" } else { "connecting" }, "percent": if phase == "done" { json!(100) } else { Value::Null }, "detail": null, "error": error,
        "startedAt": iso(record.started_ms), "endedAt": if phase == "running" { Value::Null } else { json!(iso(now)) }, "sequence": state["seq"].as_u64().unwrap_or(0),
    })
}

impl Service {
    fn hosted(&self, method: &'static str, root: &str, params: Value) -> Result<Value, AgentError> {
        self.agent().run(&Op::Hosted { method, root: root.into(), params })
    }

    fn remember(&self, key: &str, value: &Value) {
        if let Err(e) = self.0.meta.set(key, &value.to_string()) {
            self.log("sourcecontrol.remember.failed", &[("key", key.to_string()), ("error", e.0)]);
        }
    }

    fn recall(&self, key: &str) -> Option<Value> {
        self.0.meta.get(key).ok().flatten().and_then(|t| serde_json::from_str(&t).ok())
    }

    /// `server.discoverSourceControl`: git and each provider's CLI on the executor's machine, and who it is signed in as.
    pub fn discover_source_control(&self) -> Value {
        let git = json!({"kind": "git", "implemented": true, "label": "Git", "executable": "git", "status": "available", "version": none(), "installHint": "Install git on the machine that runs the executor.", "detail": none()});
        let providers: Vec<Value> = PROVIDERS
            .iter()
            .map(|(kind, label, executable)| {
                let answer = self.hosted("sourceControl.discover", "", json!({"provider": kind}));
                let (status, auth) = match &answer {
                    Ok(answer) => {
                        let p = &answer["providers"][0];
                        let signed_in = p["signed_in"].as_bool().unwrap_or(false);
                        let host = p["host"].as_str().unwrap_or_default();
                        let account = p["account"].as_str();
                        let accounts = match (signed_in, account) {
                            (true, Some(account)) => json!([{"host": host, "account": account, "active": true, "authenticated": true}]),
                            _ => json!([]),
                        };
                        ("available", json!({"status": if signed_in { "authenticated" } else { "unauthenticated" }, "account": account.map_or_else(none, some), "host": if host.is_empty() { none() } else { some(host) }, "detail": none(), "accounts": accounts}))
                    }
                    Err(AgentError::Refused { code, message }) if code == "CLI_NOT_INSTALLED" => ("missing", json!({"status": "unknown", "account": none(), "host": none(), "detail": some(message)})),
                    Err(e) => ("available", json!({"status": "unknown", "account": none(), "host": none(), "detail": some(&e.message())})),
                };
                json!({"kind": kind, "label": label, "executable": executable, "status": status, "version": none(), "installHint": format!("Install the {label} ({executable}) on the machine that runs the executor and sign in."), "detail": none(), "auth": auth, "appSignIn": false})
            })
            .collect();
        json!({"versionControlSystems": [git], "sourceControlProviders": providers})
    }

    /// `sourceControl.lookupRepository`
    pub fn sc_lookup_repository(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let (provider, repository) = (field(payload, "provider")?, field(payload, "repository")?);
        let root = payload["cwd"].as_str().unwrap_or_default();
        let answer = self.hosted("sourceControl.lookupRepository", root, json!({"provider": provider, "repo": repository})).map_err(|e| repository_error(provider, "lookupRepository", &e))?;
        let repo = &answer["repo"];
        Ok(json!({"provider": provider, "nameWithOwner": repo["slug"], "url": repo["url"], "sshUrl": repo["ssh_url"]}))
    }

    /// `sourceControl.publishRepository`: the project's repository created on the host and pushed.
    pub fn sc_publish_repository(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let (cwd, provider, repository) = (field(payload, "cwd")?, field(payload, "provider")?, field(payload, "repository")?);
        let visibility = payload["visibility"].as_str().unwrap_or("private");
        let published = self.hosted("sourceControl.publishRepository", cwd, json!({"provider": provider, "repo": repository, "visibility": visibility})).map_err(|e| repository_error(provider, "publishRepository", &e))?;
        let branch = self.agent().run(&Op::GitStatus { root: cwd.into() }).ok().and_then(|s| s["branch"].as_str().map(str::to_string)).unwrap_or_else(|| "main".into());
        let url = published["url"].as_str().map_or_else(|| format!("https://{}/{repository}", host_of(provider)), str::to_string);
        Ok(json!({
            "repository": {"provider": provider, "nameWithOwner": repository, "url": url, "sshUrl": format!("git@{}:{repository}.git", host_of(provider))},
            "remoteName": payload["remoteName"].as_str().unwrap_or("origin"), "remoteUrl": url, "branch": branch, "status": "pushed",
        }))
    }

    /// `sourceControl.appAccess`: whether the signed-in account reaches the repository.
    pub fn sc_app_access(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let (provider, repository) = (field(payload, "provider")?, field(payload, "repository")?);
        let host = payload["host"].as_str().filter(|h| !["github.com", "gitlab.com"].contains(h));
        let mut params = json!({"provider": provider, "repo": repository});
        if let Some(host) = host {
            params["host"] = json!(host);
        }
        let answer = self.hosted("sourceControl.appAccess", "", params).map_err(|e| failure("appAccess", provider, &e))?;
        let status = if answer["status"]["signed_in"] != json!(true) {
            "signed-out"
        } else if answer["repo"].is_object() {
            "covered"
        } else {
            "not-covered"
        };
        Ok(json!({"status": status}))
    }

    /// `sourceControl.signInStart`: the CLI's web sign-in begun; the code and the address come back, and the flow is remembered for the wait.
    pub fn sc_sign_in_start(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let (provider, host) = (field(payload, "provider")?, field(payload, "host")?);
        let prompt = self.hosted("sourceControl.signInStart", "", json!({"provider": provider, "host": host})).map_err(|e| sign_in_error(provider, host, if matches!(&e, AgentError::Refused { code, .. } if code == "CLI_NOT_INSTALLED") { "unsupported" } else { "failed" }, &e.message()))?;
        let (Some(flow), Some(code), Some(uri)) = (prompt["flow"].as_str(), prompt["user_code"].as_str(), prompt["verification_uri"].as_str()) else {
            return Err(sign_in_error(provider, host, "failed", "the executor answered a sign-in with no code or address"));
        };
        self.remember(&flow_key(provider, host), &json!(flow));
        Ok(json!({"userCode": code, "verificationUri": uri, "expiresAt": self.now_ms() + 15 * 60_000}))
    }

    /// `sourceControl.signInWait`: holds the call for a while; a sign-in still open after it is told so.
    pub fn sc_sign_in_wait(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let (provider, host) = (field(payload, "provider")?, field(payload, "host")?);
        let Some(flow) = self.recall(&flow_key(provider, host)).and_then(|f| f.as_str().map(str::to_string)) else {
            return Err(sign_in_error(provider, host, "not-started", "no sign-in was started for this host"));
        };
        let waited = self.hosted("sourceControl.signInWait", "", json!({"provider": provider, "flow": flow, "timeout_ms": SIGN_IN_PATIENCE_MS})).map_err(|e| sign_in_error(provider, host, "failed", &e.message()))?;
        match waited["state"].as_str() {
            Some("signed_in") => {
                self.remember(&flow_key(provider, host), &Value::Null);
                match waited["account"].as_str() {
                    Some(account) => Ok(json!({"account": account})),
                    None => Err(sign_in_error(provider, host, "failed", "the CLI signed in but named no account")),
                }
            }
            Some("failed") => {
                self.remember(&flow_key(provider, host), &Value::Null);
                Err(sign_in_error(provider, host, "failed", waited["message"].as_str().unwrap_or("the sign-in failed")))
            }
            _ => Err(sign_in_error(provider, host, "expired", "the sign-in is still waiting for approval; ask to wait again or start a new one")),
        }
    }

    /// `sourceControl.signInCancel`
    pub fn sc_sign_in_cancel(&self, payload: &Value) -> Result<(), QueryFailure> {
        let (provider, host) = (field(payload, "provider")?, field(payload, "host")?);
        let Some(flow) = self.recall(&flow_key(provider, host)).and_then(|f| f.as_str().map(str::to_string)) else {
            return Err(sign_in_error(provider, host, "not-started", "no sign-in was started for this host"));
        };
        self.hosted("sourceControl.signInCancel", "", json!({"provider": provider, "flow": flow})).map_err(|e| sign_in_error(provider, host, "failed", &e.message()))?;
        self.remember(&flow_key(provider, host), &Value::Null);
        Ok(())
    }

    fn clone_index(&self) -> Vec<String> {
        self.recall(CLONES_KEY).and_then(|v| v.as_array().map(|a| a.iter().filter_map(|p| p.as_str().map(str::to_string)).collect())).unwrap_or_default()
    }

    fn clone_record(&self, project: &str) -> Option<CloneRecord> {
        CloneRecord::from_json(&self.recall(&format!("projectClone:{project}"))?)
    }

    fn start_clone(&self, project: &str, record: &CloneRecord) -> Result<CloneRecord, QueryFailure> {
        let started = self.hosted("clone.start", "", json!({"provider": record.provider, "host": record.host, "repo": record.repo, "dest": record.destination})).map_err(|e| repository_error(&record.provider, "cloneRepository", &e))?;
        let clone = started["clone"].as_str().ok_or_else(|| repository_error(&record.provider, "cloneRepository", &AgentError::Failed("the agent API started a clone with no id".into())))?;
        let record = CloneRecord { clone: clone.to_string(), started_ms: self.now_ms(), ..record.clone() };
        self.remember(&format!("projectClone:{project}"), &record.to_json());
        self.remember(&format!("projectClone.by:{clone}"), &json!(project));
        let mut index = self.clone_index();
        if !index.iter().any(|p| p == project) {
            index.push(project.to_string());
            self.remember(CLONES_KEY, &json!(index));
        }
        Ok(record)
    }

    /// `projectClone.start`: the project made on the destination and the clone started; the progress is the subscription's.
    pub fn project_clone_start(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let (project, title, destination) = (field(payload, "projectId")?, field(payload, "title")?, field(payload, "destinationPath")?);
        let named = payload["repository"].as_str().filter(|r| !r.is_empty());
        let remote = payload["remoteUrl"].as_str().filter(|r| !r.is_empty());
        let from_remote = remote.and_then(repository_of);
        let provider = payload["provider"].as_str().map(str::to_string).unwrap_or_else(|| provider_of(from_remote.as_ref().map(|(h, _)| h.as_str())).to_string());
        let (host, repo) = match (named, from_remote) {
            (Some(repo), found) => (found.map_or_else(|| host_of(&provider), |(h, _)| h), repo.to_string()),
            (None, Some((host, repo))) => (host, repo),
            (None, None) => return Err(repository_error(&provider, "cloneRepository", &AgentError::Failed("a clone needs a repository or a remote address".into()))),
        };
        let remote_url = remote.map_or_else(|| format!("https://{host}/{repo}.git"), str::to_string);
        let record = CloneRecord { clone: String::new(), provider: provider.clone(), host: host.clone(), repo: repo.clone(), destination: destination.to_string(), remote_url: remote_url.clone(), started_ms: 0 };
        let command = json!({"type": "project.create", "commandId": format!("clone-{project}"), "projectId": project, "title": title, "workspaceRoot": destination});
        self.engine().dispatch(Family::Project, &command).map_err(|_| repository_error(&provider, "cloneRepository", &AgentError::Failed(format!("the project {project} could not be made"))))?;
        self.start_clone(project, &record)?;
        Ok(json!({"projectId": project, "cwd": destination, "remoteUrl": remote_url, "repository": {"provider": provider, "nameWithOwner": repo, "url": format!("https://{host}/{repo}"), "sshUrl": format!("git@{host}:{repo}.git")}}))
    }

    /// `projectClone.cancel`
    pub fn project_clone_cancel(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let project = field(payload, "projectId")?;
        let Some(record) = self.clone_record(project) else { return Ok(json!({"applied": false})) };
        match self.hosted("clone.cancel", "", json!({"clone": record.clone})) {
            Ok(answer) => Ok(json!({"applied": answer["cancelled"].as_bool().unwrap_or(false)})),
            Err(e) => Err(QueryFailure::Error(json!({"_tag": "ExecutionHostError", "message": e.message()}))),
        }
    }

    /// `projectClone.retry`: the clone started again with what it asked for.
    pub fn project_clone_retry(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let project = field(payload, "projectId")?;
        let Some(record) = self.clone_record(project) else { return Ok(json!({"applied": false})) };
        self.start_clone(project, &record)?;
        Ok(json!({"applied": true}))
    }

    fn clone_list(&self, states: &BTreeMap<String, Value>) -> Value {
        let now = self.now_ms();
        Value::Array(self.clone_index().iter().filter_map(|p| {
            let record = self.clone_record(p)?;
            let state = states.get(&record.clone)?;
            Some(snapshot(p, &record, state, now))
        }).collect())
    }

    /// `subscribeProjectClones`: the snapshots of every clone this server started, now and after each change.
    pub fn open_project_clones(&self) -> (CloneFold, Value) {
        let mut states = BTreeMap::new();
        for p in self.clone_index() {
            if let Some(record) = self.clone_record(&p) {
                if let Ok(state) = self.hosted("clone.status", "", json!({"clone": record.clone})) {
                    states.insert(record.clone, state);
                }
            }
        }
        let first = self.clone_list(&states);
        (CloneFold { states, last: first.clone() }, first)
    }

    /// Follows the clones on a thread of its own, sending the list again whenever a snapshot changed.
    pub fn follow_project_clones(&self, mut fold: CloneFold, id: Value, to: Sender<(Value, Value)>, stop: Arc<AtomicBool>) {
        let svc = self.clone();
        std::thread::spawn(move || {
            let result = svc.agent().clone_follow("", &mut |event| {
                if stop.load(Ordering::SeqCst) {
                    return Follow::Stop;
                }
                let Some(event) = event else { return Follow::Continue };
                if let Some(clone) = event["clone"].as_str() {
                    fold.states.insert(clone.to_string(), event.clone());
                }
                let list = svc.clone_list(&fold.states);
                if list != fold.last {
                    fold.last = list.clone();
                    if to.send((id.clone(), list)).is_err() {
                        return Follow::Stop;
                    }
                }
                Follow::Continue
            });
            if let Err(e) = result {
                svc.log("projectClone.follow.stopped", &[("message", e.message())]);
            }
        });
    }
}

/// What the clone stream last told the app.
pub struct CloneFold {
    states: BTreeMap<String, Value>,
    last: Value,
}

fn host_of(provider: &str) -> String {
    if provider == "gitlab" { "gitlab.com" } else { "github.com" }.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_clone_addresses_of_each_shape_when_read_then_the_host_and_repository_come_out_and_a_bare_word_is_none() {
        for (address, expected) in [
            ("https://github.com/octo/hello.git", Some(("github.com", "octo/hello"))),
            ("git@gitlab.example.com:grp/sub/proj.git", Some(("gitlab.example.com", "grp/sub/proj"))),
            ("ssh://git@github.com/octo/hello", Some(("github.com", "octo/hello"))),
            ("hello", None),
            ("https://github.com/", None),
        ] {
            assert_eq!(repository_of(address), expected.map(|(h, r)| (h.to_string(), r.to_string())), "{address}");
        }
    }

    #[test]
    fn given_the_states_a_clone_goes_through_when_made_snapshots_then_the_phase_follows_the_state_and_an_ended_one_has_an_end() {
        // Given a clone and the states the agent API logs for it
        let record = CloneRecord { clone: "c1".into(), provider: "github".into(), host: "github.com".into(), repo: "octo/hello".into(), destination: "/w/hello".into(), remote_url: "https://github.com/octo/hello.git".into(), started_ms: 1_000 };
        let at = |state: Value| snapshot("p1", &record, &state, 5_000);
        // When each state is made a snapshot
        let (running, done, failed) = (at(json!({"state": "running", "seq": 1})), at(json!({"state": "done", "seq": 2})), at(json!({"state": "failed", "seq": 3, "detail": {"message": "fatal: boom"}})));
        // Then the phase, the progress, the error and the end follow it
        assert_eq!((running["phase"].clone(), running["endedAt"].clone(), running["error"].clone()), (json!("running"), Value::Null, Value::Null));
        assert_eq!((done["phase"].clone(), done["percent"].clone(), done["endedAt"].is_string()), (json!("done"), json!(100), true));
        assert_eq!((failed["phase"].clone(), failed["error"].clone(), failed["sequence"].clone()), (json!("failed"), json!("fatal: boom"), json!(3)));
    }
}
