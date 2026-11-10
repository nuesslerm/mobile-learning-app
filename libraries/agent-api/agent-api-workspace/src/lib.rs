//! The `workspace`, `files` and `files.edit` groups of the agent API, and (in `worktrees`) `workspace.worktrees`. `workspace.list` names the
//! workspaces whose executor is attached, `workspace.create` and `workspace.remove` add and forget one; the `file.*` methods act in one of them,
//! through that workspace's executor. Their results are the executor's (`exec-fs`, `exec-git`): the service relays them unchanged, and an
//! executor's failure with its own code. A workspace is the executor's root, a directory under it, or a worktree under the executor's worktree
//! base; paths are always relative to the workspace.
use agent_api_proto::{text, GroupName, Method, ParamError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub mod edit;
pub mod worktrees;

#[cfg(feature = "testing")]
pub mod testing;

pub const WORKSPACE: GroupName = GroupName::new("workspace");
pub const FILES: GroupName = GroupName::new("files");

pub const LIST: Method = Method::new(WORKSPACE, "workspace.list");
pub const WORKSPACE_CREATE: Method = Method::new(WORKSPACE, "workspace.create");
pub const WORKSPACE_REMOVE: Method = Method::new(WORKSPACE, "workspace.remove");
pub const REPO_LIST: Method = Method::new(WORKSPACE, "repo.list");
pub const FILE_READ: Method = Method::new(FILES, "file.read");
pub const FILE_LIST: Method = Method::new(FILES, "file.list");
pub const FILE_DIFF: Method = Method::new(FILES, "file.diff");

/// Every method of the `workspace` and `files` groups. The owner methods that are not here (`workspace.activate`, `repo.hooks`)
/// are pending: nothing defines their params yet.
pub const METHODS: &[Method] = &[LIST, WORKSPACE_CREATE, WORKSPACE_REMOVE, REPO_LIST, FILE_READ, FILE_LIST, FILE_DIFF];

/// The error codes of these groups, beside `INVALID_PARAMS` and the executor's own (`NO_EXECUTOR`, `NOT_FOUND`, ...).
pub mod code {
    /// A workspace of that name exists already.
    pub const WORKSPACE_EXISTS: &str = "WORKSPACE_EXISTS";
    /// No executor ever owned a workspace of that name.
    pub const UNKNOWN_WORKSPACE: &str = "UNKNOWN_WORKSPACE";
    /// An executor's own root workspace is not removed through the API.
    pub const NOT_REMOVABLE: &str = "NOT_REMOVABLE";
    /// Several executors are attached and the request did not name one.
    pub const AMBIGUOUS_EXECUTOR: &str = "AMBIGUOUS_EXECUTOR";
    /// The workspace is not a worktree.
    pub const NOT_A_WORKTREE: &str = "NOT_A_WORKTREE";
    /// No setup ran in the worktree.
    pub const NO_SETUP: &str = "NO_SETUP";
}

/// What the executor is given beside the path, copied from the params unchanged: the executor refuses a bad one with `INVALID_ARGS`.
const FORWARDED: [&str; 4] = ["offset", "limit", "depth", "base"];

/// The params copied unchanged to the executor for `method`, beside its path.
fn forwarded_keys(method: Method) -> &'static [&'static str] {
    match method {
        edit::FILE_READ_CHUNK => &["offset", "length"],
        edit::FILE_WRITE => &["content", "create_dirs"],
        edit::FILE_CREATE => &["content", "create_dirs"],
        edit::FILE_STAT => &[],
        edit::FILE_SEARCH => &["query", "limit"],
        edit::FILE_BROWSE => &["show_hidden"],
        edit::FILE_MKDIR => &["parents"],
        edit::TURN_DIFF | edit::THREAD_DIFF => &["base"],
        _ => &FORWARDED,
    }
}

/// The result of `workspace.create`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateResult {
    pub workspace: String,
    pub executor: String,
    /// The directory under the executor's root the workspace is, as the executor names it.
    pub path: String,
}

/// The params of `workspace.create`: a new workspace named `workspace`, on `executor` (the only one attached when absent), being the
/// directory `path` under that executor's root. The executor's root itself is a workspace already, named when it attached.
#[derive(Debug, Clone, PartialEq)]
pub struct CreateParams {
    pub workspace: String,
    pub executor: Option<String>,
    pub path: String,
}

impl CreateParams {
    pub fn parse(params: &Value) -> Result<CreateParams, ParamError> {
        let Some(workspace) = text(params, "workspace") else { return Err(ParamError("workspace is required".into())) };
        let optional = |key: &str| match params.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(v)) if !v.is_empty() => Ok(Some(v.clone())),
            Some(_) => Err(ParamError(format!("{key} must be a non-empty string"))),
        };
        let Some(path) = text(params, "path") else { return Err(ParamError("path is required".into())) };
        Ok(CreateParams { workspace: workspace.to_string(), executor: optional("executor")?, path: path.to_string() })
    }
}

/// How a workspace lies at its executor, as `repo.list` says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// The executor's own root.
    Root,
    /// A directory below the root (`workspace.create`).
    Directory,
    /// A worktree under the executor's worktree base (`worktree.create`).
    Worktree,
}

/// One workspace of `repo.list`: its owner, how it lies there and, for a directory or a worktree, where.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Repo {
    pub workspace: String,
    pub executor: String,
    pub kind: Kind,
    /// Whether the owner has an open channel now.
    pub attached: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// The result of `repo.list`: every workspace the service knows, sorted by name, attached or not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepoListResult {
    pub repos: Vec<Repo>,
}

/// The result of `workspace.remove`: the workspace is forgotten; a worktree or a directory is left on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoveResult {
    pub workspace: String,
    pub removed: bool,
}

/// `{workspace}`, the params of `workspace.remove` and of every method that only names one.
pub fn workspace_param(params: &Value) -> Result<String, ParamError> {
    text(params, "workspace").map(str::to_string).ok_or_else(|| ParamError("workspace is required".into()))
}

/// The result of `workspace.list`: the workspaces with an open executor channel, sorted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListResult {
    pub workspaces: Vec<String>,
}

impl ListResult {
    pub fn to_value(&self) -> Value {
        json!({ "workspaces": self.workspaces })
    }
}

/// The params of a `file.*` method (or of `turn.diff`, `thread.diff`): the workspace it acts in and the path (absent for a diff of the whole
/// tree and for `file.search`, which searches all of the workspace).
#[derive(Debug, Clone, PartialEq)]
pub struct FileParams {
    pub workspace: String,
    pub path: Option<String>,
    forwarded: Vec<(&'static str, Value)>,
}

impl FileParams {
    pub fn parse(method: Method, params: &Value) -> Result<FileParams, ParamError> {
        let Some(workspace) = text(params, "workspace") else { return Err(ParamError("workspace is required".into())) };
        let path = params.get("path").and_then(Value::as_str);
        let pathless = matches!(method, FILE_DIFF | edit::FILE_SEARCH | edit::TURN_DIFF | edit::THREAD_DIFF);
        if path.is_none() && !pathless {
            return Err(ParamError("path is required".into()));
        }
        match method {
            edit::FILE_WRITE if !params.get("content").is_some_and(Value::is_string) => return Err(ParamError("content is required".into())),
            edit::FILE_SEARCH if text(params, "query").is_none() => return Err(ParamError("query is required".into())),
            edit::TURN_DIFF if text(params, "base").is_none() => return Err(ParamError("base is required".into())),
            _ => {}
        }
        let forwarded = forwarded_keys(method).iter().filter_map(|key| params.get(*key).map(|value| (*key, value.clone()))).collect();
        Ok(FileParams { workspace: workspace.to_string(), path: path.map(str::to_string), forwarded })
    }

    /// The `args` of the executor request: the path and each forwarded param the caller sent.
    pub fn executor_args(&self) -> Value {
        let mut args = json!({});
        if let Some(path) = &self.path {
            args["path"] = json!(path);
        }
        for (key, value) in &self.forwarded {
            args[*key] = value.clone();
        }
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_file_params_when_read_then_the_workspace_and_a_path_are_required_except_for_the_diff_of_the_whole_tree() {
        // Given params with no workspace, none with a path, and a diff with no path
        // Then the first two are refused saying what is required, and the diff is the whole tree
        assert_eq!(FileParams::parse(FILE_READ, &json!({"path": "a"})).unwrap_err().0, "workspace is required");
        assert_eq!(FileParams::parse(FILE_LIST, &json!({"workspace": ""})).unwrap_err().0, "workspace is required");
        assert_eq!(FileParams::parse(FILE_READ, &json!({"workspace": "w"})).unwrap_err().0, "path is required");
        assert_eq!(FileParams::parse(FILE_DIFF, &json!({"workspace": "w"})).unwrap().path, None);
        // And a diff whose path is not a string is the whole tree, a read whose path is not a string has no path
        assert_eq!(FileParams::parse(FILE_DIFF, &json!({"workspace": "w", "path": 5})).unwrap().path, None);
        assert!(FileParams::parse(FILE_READ, &json!({"workspace": "w", "path": 5})).is_err());
    }

    #[test]
    fn given_params_beside_the_path_when_the_executor_args_are_built_then_the_four_forwarded_ones_go_unchanged_and_nothing_else() {
        // Given params with every forwarded key (one of them a wrong kind and one null) and a key that is not forwarded
        let params = json!({"workspace": "w", "path": "a", "offset": 2, "limit": "ten", "depth": null, "base": "HEAD", "other": 1});
        // When the executor args are built
        let args = FileParams::parse(FILE_READ, &params).unwrap().executor_args();
        // Then the forwarded ones are copied as they came and the other is left out
        assert_eq!(args, json!({"path": "a", "offset": 2, "limit": "ten", "depth": null, "base": "HEAD"}));
        // And the diff of the whole tree sends no path
        assert_eq!(FileParams::parse(FILE_DIFF, &json!({"workspace": "w", "base": "HEAD"})).unwrap().executor_args(), json!({"base": "HEAD"}));
    }

    #[test]
    fn given_the_methods_of_the_groups_when_listed_then_each_belongs_to_its_group() {
        // Given the methods
        // Then the listing is in the group's order and each name starts with its group's prefix
        assert_eq!(METHODS, [LIST, WORKSPACE_CREATE, WORKSPACE_REMOVE, REPO_LIST, FILE_READ, FILE_LIST, FILE_DIFF]);
        for method in METHODS {
            let prefixes: &[&str] = if method.group() == FILES { &["file."] } else { &["workspace.", "repo."] };
            assert!(prefixes.iter().any(|p| method.name().starts_with(p)), "{}", method.name());
        }
    }

    #[test]
    fn given_the_params_of_the_edit_methods_when_read_then_each_requires_what_it_cannot_do_without_and_forwards_only_its_own_keys() {
        // Given params with every key any method forwards, and a stray one
        let all = json!({"workspace": "w", "path": "p", "content": "c", "create_dirs": true, "offset": 1, "length": 2, "limit": 3, "query": "q", "base": "b", "show_hidden": true, "depth": 4, "other": 5});
        // When each method's params are read, Then the executor is given its own keys and path only
        let args = |method| FileParams::parse(method, &all).unwrap().executor_args();
        assert_eq!(args(edit::FILE_READ_CHUNK), json!({"path": "p", "offset": 1, "length": 2}));
        assert_eq!(args(edit::FILE_WRITE), json!({"path": "p", "content": "c", "create_dirs": true}));
        assert_eq!(args(edit::FILE_CREATE), json!({"path": "p", "content": "c", "create_dirs": true}));
        assert_eq!(args(edit::FILE_STAT), json!({"path": "p"}));
        assert_eq!(args(edit::FILE_SEARCH), json!({"path": "p", "query": "q", "limit": 3}));
        assert_eq!(args(edit::FILE_BROWSE), json!({"path": "p", "show_hidden": true}));
        assert_eq!(args(edit::TURN_DIFF), json!({"path": "p", "base": "b"}));
        assert_eq!(args(edit::THREAD_DIFF), json!({"path": "p", "base": "b"}));
        // And what a method cannot do without is refused naming it: a write needs a string content, a search a query, a turn diff its base
        let without = |method, key: &str| {
            let mut params = all.clone();
            params.as_object_mut().unwrap().remove(key);
            FileParams::parse(method, &params).unwrap_err().0
        };
        assert_eq!(without(edit::FILE_WRITE, "content"), "content is required");
        assert_eq!(FileParams::parse(edit::FILE_WRITE, &json!({"workspace": "w", "path": "p", "content": 5})).unwrap_err().0, "content is required");
        assert_eq!(without(edit::FILE_SEARCH, "query"), "query is required");
        assert_eq!(without(edit::TURN_DIFF, "base"), "base is required");
        assert_eq!(without(edit::FILE_STAT, "path"), "path is required");
        // And a search, a thread diff and a turn diff need no path, and a created file needs no content
        assert_eq!(FileParams::parse(edit::FILE_SEARCH, &json!({"workspace": "w", "query": "q"})).unwrap().path, None);
        assert_eq!(FileParams::parse(edit::THREAD_DIFF, &json!({"workspace": "w"})).unwrap().executor_args(), json!({}));
        assert!(FileParams::parse(edit::FILE_CREATE, &json!({"workspace": "w", "path": "p"})).is_ok());
    }

    #[test]
    fn given_the_params_of_workspace_create_when_read_then_a_name_and_a_directory_are_required_and_the_executor_must_be_a_string() {
        // Given a name with a directory, one that also names an executor, and values of the wrong kind
        let bare = CreateParams::parse(&json!({"workspace": "w", "path": "sub"})).unwrap();
        let full = CreateParams::parse(&json!({"workspace": "w", "executor": "e", "path": "sub/dir"})).unwrap();
        // Then the executor is absent or as sent (null counts as absent), and a missing name or directory, or a wrong kind, is refused naming it
        assert_eq!((bare.executor, bare.path.as_str()), (None, "sub"));
        assert_eq!((full.executor.as_deref(), full.path.as_str()), (Some("e"), "sub/dir"));
        assert_eq!(CreateParams::parse(&json!({"workspace": "w", "path": "p", "executor": null})).unwrap().executor, None);
        assert_eq!(CreateParams::parse(&json!({"path": "p"})).unwrap_err().0, "workspace is required");
        assert_eq!(CreateParams::parse(&json!({"workspace": "w"})).unwrap_err().0, "path is required");
        assert_eq!(CreateParams::parse(&json!({"workspace": "w", "path": 3})).unwrap_err().0, "path is required");
        assert_eq!(CreateParams::parse(&json!({"workspace": "w", "path": "p", "executor": ""})).unwrap_err().0, "executor must be a non-empty string");
        assert_eq!(workspace_param(&json!({"workspace": ""})).unwrap_err().0, "workspace is required");
    }
}
