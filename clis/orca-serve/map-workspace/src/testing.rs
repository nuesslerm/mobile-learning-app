//! A double of the agent API's workspace and file methods: workspaces holding files in memory, answered in the shapes the executor's actions
//! return (`exec-fs`). The chain runs the real service and executor against the same calls, so a shape that drifts fails there.
use orca_core::testing::ScriptedAgent;
use orca_core::AgentError;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct State {
    /// workspace name -> (kind, path) as `repo.list` says it.
    workspaces: BTreeMap<String, (String, Option<String>)>,
    files: BTreeMap<(String, String), Vec<u8>>,
    /// repo workspace -> its linked worktrees as (workspace name, branch).
    linked: BTreeMap<String, Vec<(String, String)>>,
    /// workspace -> (ref name, kind) as `git.refs` lists them.
    refs: BTreeMap<String, Vec<(String, String)>>,
    remotes: BTreeSet<String>,
    fetched: Vec<String>,
}

#[derive(Clone, Default)]
pub struct Workspaces(Arc<Mutex<State>>);

fn refused(code: &str, message: impl Into<String>) -> AgentError {
    AgentError::Refused { code: code.into(), message: message.into() }
}

impl Workspaces {
    pub fn workspace(&self, name: &str, kind: &str, path: Option<&str>) -> &Self {
        self.0.lock().unwrap().workspaces.insert(name.into(), (kind.into(), path.map(str::to_string)));
        self
    }

    pub fn put(&self, workspace: &str, path: &str, bytes: &[u8]) -> &Self {
        self.0.lock().unwrap().files.insert((workspace.into(), path.into()), bytes.to_vec());
        self
    }

    pub fn read(&self, workspace: &str, path: &str) -> Option<Vec<u8>> {
        self.0.lock().unwrap().files.get(&(workspace.into(), path.into())).cloned()
    }

    /// The refs `git.refs` lists for a workspace.
    pub fn refs(&self, workspace: &str, refs: &[(&str, &str)]) -> &Self {
        self.0.lock().unwrap().refs.insert(workspace.into(), refs.iter().map(|(n, k)| (n.to_string(), k.to_string())).collect());
        self
    }

    /// Gives a workspace a remote, so that `git.fetch` has something to fetch.
    pub fn remote(&self, workspace: &str) -> &Self {
        self.0.lock().unwrap().remotes.insert(workspace.into());
        self
    }

    /// The workspaces `git.fetch` was answered for.
    pub fn fetched(&self) -> Vec<String> {
        self.0.lock().unwrap().fetched.clone()
    }

    /// Makes `agent` answer the methods of `USES` from this workspace set.
    pub fn install(&self, agent: &ScriptedAgent) {
        let me = self.clone();
        agent.reply("repo.list", move |_| {
            let s = me.0.lock().unwrap();
            let repos: Vec<Value> = s.workspaces.iter().map(|(w, (kind, path))| json!({"workspace": w, "executor": "e1", "kind": kind, "attached": true}).as_object().cloned().map(|mut o| { if let Some(p) = path { o.insert("path".into(), p.clone().into()); } Value::Object(o) }).unwrap()).collect();
            Ok(json!({"repos": repos}))
        });
        let me = self.clone();
        agent.reply("file.list", move |p| me.list(p));
        let me = self.clone();
        agent.reply("file.readChunk", move |p| me.chunk(p));
        let me = self.clone();
        agent.reply("file.stat", move |p| me.stat(p));
        let me = self.clone();
        agent.reply("file.create", move |p| me.create(p));
        let me = self.clone();
        agent.reply("file.search", move |p| me.search(p));
        let me = self.clone();
        agent.reply("worktree.list", move |p| me.worktree_list(p));
        let me = self.clone();
        agent.reply("worktree.create", move |p| me.worktree_create(p));
        let me = self.clone();
        agent.reply("worktree.remove", move |p| me.worktree_remove(p));
        let me = self.clone();
        agent.reply("git.refs", move |p| me.git_refs(p));
        let me = self.clone();
        agent.reply("git.fetch", move |p| me.git_fetch(p));
    }

    fn ws(&self, p: &Value) -> Result<String, AgentError> {
        let name = p["workspace"].as_str().unwrap_or_default();
        if self.0.lock().unwrap().workspaces.contains_key(name) {
            Ok(name.to_string())
        } else {
            Err(refused("UNKNOWN_WORKSPACE", format!("no workspace {name}")))
        }
    }

    fn list(&self, p: &Value) -> Result<Value, AgentError> {
        let w = self.ws(p)?;
        let base = p["path"].as_str().unwrap_or(".");
        let base = if base == "." { String::new() } else { format!("{}/", base.trim_end_matches('/')) };
        let depth = p["depth"].as_u64().unwrap_or(1) as usize;
        let s = self.0.lock().unwrap();
        let mut seen: BTreeMap<String, (&str, u64)> = BTreeMap::new();
        for ((fw, path), bytes) in &s.files {
            let Some(rest) = (fw == &w).then(|| path.strip_prefix(&base)).flatten() else { continue };
            let parts: Vec<&str> = rest.split('/').collect();
            for n in 1..=parts.len().min(depth) {
                let name = parts[..n].join("/");
                if n == parts.len() {
                    seen.insert(name, ("file", bytes.len() as u64));
                } else {
                    seen.entry(name).or_insert(("dir", 0));
                }
            }
        }
        if !base.is_empty() && seen.is_empty() {
            return Err(refused("NOT_FOUND", format!("{base}: not found")));
        }
        Ok(json!({"entries": seen.into_iter().map(|(name, (kind, size))| json!({"name": name, "kind": kind, "size": size})).collect::<Vec<_>>()}))
    }

    fn chunk(&self, p: &Value) -> Result<Value, AgentError> {
        let w = self.ws(p)?;
        let path = p["path"].as_str().unwrap_or_default();
        let s = self.0.lock().unwrap();
        let bytes = s.files.get(&(w, path.to_string())).ok_or_else(|| refused("NOT_FOUND", format!("{path}: not found")))?;
        let offset = (p["offset"].as_u64().unwrap_or(0) as usize).min(bytes.len());
        let length = (p["length"].as_u64().unwrap_or(1 << 20) as usize).min(1 << 20);
        let piece = &bytes[offset..(offset + length).min(bytes.len())];
        use base64::Engine;
        Ok(json!({"data": base64::engine::general_purpose::STANDARD.encode(piece), "offset": offset, "bytes": piece.len(), "total_bytes": bytes.len(), "eof": offset + piece.len() >= bytes.len()}))
    }

    fn stat(&self, p: &Value) -> Result<Value, AgentError> {
        let w = self.ws(p)?;
        let path = p["path"].as_str().unwrap_or_default();
        let s = self.0.lock().unwrap();
        match s.files.get(&(w.clone(), path.to_string())) {
            Some(b) => Ok(json!({"kind": "file", "size": b.len(), "modified_ms": 1_700_000_000_000u64})),
            None if path == "." || s.files.keys().any(|(fw, f)| fw == &w && f.starts_with(&format!("{path}/"))) => Ok(json!({"kind": "dir", "size": 0, "modified_ms": 1_700_000_000_000u64})),
            None => Err(refused("NOT_FOUND", format!("{path}: not found"))),
        }
    }

    fn create(&self, p: &Value) -> Result<Value, AgentError> {
        let w = self.ws(p)?;
        let path = p["path"].as_str().unwrap_or_default().to_string();
        let mut s = self.0.lock().unwrap();
        if s.files.contains_key(&(w.clone(), path.clone())) {
            return Err(refused("ALREADY_EXISTS", format!("{path} already exists")));
        }
        let content = p["content"].as_str().unwrap_or("").as_bytes().to_vec();
        let bytes = content.len();
        s.files.insert((w, path), content);
        Ok(json!({"bytes": bytes}))
    }

    fn search(&self, p: &Value) -> Result<Value, AgentError> {
        let w = self.ws(p)?;
        let q = p["query"].as_str().unwrap_or_default().to_lowercase();
        let s = self.0.lock().unwrap();
        let mut hits: Vec<&String> = s.files.keys().filter(|(fw, f)| fw == &w && f.to_lowercase().contains(&q)).map(|(_, f)| f).collect();
        hits.sort_by_key(|f| (f.len(), (*f).clone()));
        Ok(json!({"entries": hits.iter().map(|f| json!({"path": f, "kind": "file"})).collect::<Vec<_>>(), "truncated": false}))
    }

    fn worktree_list(&self, p: &Value) -> Result<Value, AgentError> {
        let w = self.ws(p)?;
        let s = self.0.lock().unwrap();
        let mut rows = vec![json!({"path": format!("/{w}"), "head": "0000000", "branch": "main", "is_main": true})];
        for (name, branch) in s.linked.get(&w).into_iter().flatten() {
            rows.push(json!({"path": format!("/base/{name}"), "head": "0000000", "branch": branch, "is_main": false}));
        }
        Ok(json!({"worktrees": rows}))
    }

    fn worktree_create(&self, p: &Value) -> Result<Value, AgentError> {
        let repo = self.ws(p)?;
        let name = p["worktree"].as_str().unwrap_or_default().to_string();
        let branch = p["branch"].as_str().unwrap_or_default().to_string();
        let mut s = self.0.lock().unwrap();
        if s.workspaces.contains_key(&name) {
            return Err(refused("WORKSPACE_EXISTS", format!("workspace {name} exists")));
        }
        let path = format!("/base/{name}");
        s.workspaces.insert(name.clone(), ("worktree".into(), Some(path.clone())));
        s.linked.entry(repo.clone()).or_default().push((name.clone(), branch.clone()));
        Ok(json!({"workspace": name, "repo": repo, "executor": "e1", "path": path, "branch": branch, "head": "0000000", "setup": false}))
    }

    fn worktree_remove(&self, p: &Value) -> Result<Value, AgentError> {
        let name = self.ws(p)?;
        let mut s = self.0.lock().unwrap();
        if s.workspaces[&name].0 != "worktree" {
            return Err(refused("NOT_A_WORKTREE", format!("{name} is not a worktree")));
        }
        s.workspaces.remove(&name);
        for list in s.linked.values_mut() {
            list.retain(|(n, _)| n != &name);
        }
        Ok(json!({"workspace": name, "removed": true}))
    }

    fn git_refs(&self, p: &Value) -> Result<Value, AgentError> {
        let w = self.ws(p)?;
        let s = self.0.lock().unwrap();
        let all = s.refs.get(&w).cloned().unwrap_or_else(|| vec![("main".into(), "branch".into())]);
        let query = p["query"].as_str().unwrap_or_default().to_lowercase();
        let rows: Vec<Value> = all.iter().filter(|(n, k)| p["kind"].as_str().is_none_or(|want| want == k) && n.to_lowercase().contains(&query)).map(|(n, k)| json!({"name": n, "kind": k, "current": n == "main"})).collect();
        Ok(json!({"current": "main", "detached": false, "refs": rows, "truncated": false}))
    }

    fn git_fetch(&self, p: &Value) -> Result<Value, AgentError> {
        let w = self.ws(p)?;
        let mut s = self.0.lock().unwrap();
        if !s.remotes.contains(&w) {
            return Err(refused("INVALID_ARGS", "this repository has no remote configured"));
        }
        s.fetched.push(w);
        Ok(json!({}))
    }
}
