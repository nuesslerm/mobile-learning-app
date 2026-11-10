//! The recorded exchanges of an executor action (`tests/fixtures/<action>.json` of each family crate), as both sides replay them: the
//! executor runs the request in a directory made by `setup` and must answer `stream` and `response`; the service's double answers a request
//! with the same `args` by sending them.
use agent_api_proto::testing::{load_dir, mask, pointer_masks};
pub use agent_api_proto::testing::{Mask, VOLATILE};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

/// The directory an exchange runs in: `files` are written, then (with `commit`) committed to a new git repository, then `worktrees` (name to the
/// new branch each is made on, under the executor's worktree base) are added, then (with `remote`) the repository gets an `origin`, then (with
/// `conflict`) a merge is left stopped on a conflict, then `edit` and `worktree_edit` are written. The repository's first branch is `main`, its
/// commits have fixed dates and its identity is `t <t@t>`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub commit: bool,
    #[serde(default)]
    pub worktrees: BTreeMap<String, String>,
    /// A bare repository `remote.git` in the worktree base, added as `origin` with the first branch pushed and tracking it (needs `commit`).
    #[serde(default)]
    pub remote: bool,
    /// For each number, a commit on top of the first branch pushed to the remote as `refs/pull/<n>/head` (how a host keeps a pull request's
    /// head) with a file `pull-<n>.txt`; needs `remote`.
    #[serde(default)]
    pub remote_pulls: Vec<u64>,
    /// Merging a branch `other` into the first branch, both having changed `path` (needs `commit` and a committed `path`): left stopped.
    #[serde(default)]
    pub conflict: Option<Conflict>,
    #[serde(default)]
    pub edit: BTreeMap<String, String>,
    /// Files written into a worktree after it was added, keyed `<worktree name>/<path>`.
    #[serde(default)]
    pub worktree_edit: BTreeMap<String, String>,
    /// Agent CLIs on the executor's lookup path: program name to the text its `--version` prints.
    #[serde(default)]
    pub clis: BTreeMap<String, String>,
    /// Files in the executor's own home directory, outside the workspace (`.claude/skills/<name>/SKILL.md`).
    #[serde(default)]
    pub home: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conflict {
    pub path: String,
    pub ours: String,
    pub theirs: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exchange {
    pub name: String,
    #[serde(default)]
    pub setup: Setup,
    /// `{id, tool, args}`
    pub request: Value,
    /// The output frames the request makes, without their `id`: `{stream, data}`.
    #[serde(default)]
    pub stream: Vec<Value>,
    /// The `event` of each subscribe frame the request makes, in order (their `seq` counts from 1).
    #[serde(default)]
    pub events: Vec<Value>,
    /// `{id, ok, result}` or `{id, ok: false, error}`
    pub response: Value,
    /// JSON pointers into the response of fields that depend on the machine (the size of a directory) and are compared by presence.
    #[serde(default)]
    pub volatile: Vec<String>,
}

impl Exchange {
    pub fn id(&self) -> &str {
        self.request["id"].as_str().unwrap_or_default()
    }

    pub fn tool(&self) -> &str {
        self.request["tool"].as_str().unwrap_or_default()
    }

    pub fn succeeds(&self) -> bool {
        self.response["ok"] == true
    }

    /// The `volatile` pointers as mask rules.
    pub fn masks(&self) -> Vec<Mask> {
        pointer_masks(&self.volatile)
    }

    /// `response` with the volatile fields replaced, so a recorded response and an actual one compare equal when only those differ. A
    /// volatile field the response lacks stays absent, so the comparison still fails.
    pub fn masked(&self, response: Value) -> Value {
        mask(&self.masks(), response)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    pub action: String,
    pub exchanges: Vec<Exchange>,
}

/// Every `*.json` file of `dir` as an executor fixture (the loader is `agent_api_proto::testing::load_dir`).
pub fn load(dir: &Path) -> Vec<Fixture> {
    load_dir(dir)
}
