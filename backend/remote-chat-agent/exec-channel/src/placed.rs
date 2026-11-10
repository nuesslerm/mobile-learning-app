//! A workspace that is a directory or a worktree as its executor serves it: the channel of the owner with each action's paths moved under the
//! workspace's place. A model's tools reach the executor through it, so a turn in a worktree reads, writes and runs in the worktree and cannot
//! name anything outside it; a workspace at the executor's root is passed through unchanged.
use crate::registry::Place;
use crate::remote::{ActionOutput, ExecutorChannel, RemoteError};
use async_trait::async_trait;
use exec_proto::Code;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct Placed {
    inner: Arc<dyn ExecutorChannel>,
    place: Place,
}

impl Placed {
    /// The channel for `place`: `inner` itself at the root.
    pub fn wrap(inner: Arc<dyn ExecutorChannel>, place: Place) -> Arc<dyn ExecutorChannel> {
        if place == Place::default() {
            inner
        } else {
            Arc::new(Placed { inner, place })
        }
    }

    fn args(&self, tool: &str, mut args: Value) -> Result<Value, RemoteError> {
        let refuse = |message: String| RemoteError::Action { code: Code::InvalidArgs.as_str().into(), message };
        let join = |path: &str| self.place.path(path).map_err(refuse);
        let string = |args: &Value, key: &str| args.get(key).and_then(Value::as_str).map(str::to_string);
        match tool {
            exec_proc::RUN_COMMAND => args["cwd"] = json!(join(&string(&args, "cwd").unwrap_or_else(|| ".".into()))?),
            exec_git::GIT_DIFF => match &self.place.worktree {
                Some(name) => args["worktree"] = json!(name),
                None => args["path"] = json!(join(&string(&args, "path").unwrap_or_else(|| ".".into()))?),
            },
            exec_fs::SEARCH => args["path"] = json!(join(&string(&args, "path").unwrap_or_else(|| ".".into()))?),
            exec_fs::SEARCH_ENTRIES => args["path"] = self.place.prefix.clone().map_or(Value::Null, Value::from),
            exec_fs::BROWSE => {
                if let Some(path) = string(&args, "path").filter(|p| !std::path::Path::new(p).is_absolute()) {
                    args["path"] = json!(join(&path)?);
                }
            }
            exec_proc::CANCEL | exec_git::GIT_WORKTREE_ADD | exec_git::GIT_WORKTREE_REMOVE | exec_git::GIT_WORKTREE_LIST | exec_git::WORKTREE_SETUP => {}
            _ => {
                if let Some(path) = string(&args, "path") {
                    args["path"] = json!(join(&path)?);
                }
            }
        }
        Ok(args)
    }
}

#[async_trait]
impl ExecutorChannel for Placed {
    async fn call(&self, tool: &str, args: Value) -> Result<ActionOutput, RemoteError> {
        self.inner.call(tool, self.args(tool, args)?).await
    }

    fn close(&self, code: u16) {
        self.inner.close(code)
    }

    fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }

    async fn closed(&self) {
        self.inner.closed().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::ScriptedChannel;

    fn done() -> Result<ActionOutput, RemoteError> {
        Ok(ActionOutput { result: json!({}), output: String::new() })
    }

    #[tokio::test]
    async fn given_a_worktree_place_when_a_model_tool_is_called_then_the_executor_is_asked_below_the_worktree_and_a_path_that_climbs_out_is_refused_before_it() {
        // Given the executor of a worktree wt under /base, seen through its place
        let executor = ScriptedChannel::new(|_, _| done());
        let placed = Placed::wrap(executor.clone(), Place { prefix: Some("/base/wt".into()), worktree: Some("wt".into()) });
        // When each kind of tool is called with paths relative to the worktree
        for (tool, args) in [
            ("read_file", json!({"path": "src/a.rs", "limit": 3})),
            ("write_file", json!({"path": "b.txt", "content": "x"})),
            ("run_command", json!({"command": "ls"})),
            ("run_command", json!({"command": "ls", "cwd": "sub"})),
            ("search", json!({"pattern": "p", "kind": "content"})),
            ("git_diff", json!({"path": "a.rs"})),
            ("search_entries", json!({"query": "q"})),
            ("cancel", json!({"id": "a1"})),
        ] {
            placed.call(tool, args).await.unwrap();
        }
        // Then the executor was given absolute paths in the worktree, a command that runs in it, a diff of it, and the cancel as it came
        let seen: Vec<Value> = executor.calls().into_iter().map(|(_, args)| args).collect();
        assert_eq!(
            seen,
            [
                json!({"path": "/base/wt/src/a.rs", "limit": 3}),
                json!({"path": "/base/wt/b.txt", "content": "x"}),
                json!({"command": "ls", "cwd": "/base/wt"}),
                json!({"command": "ls", "cwd": "/base/wt/sub"}),
                json!({"pattern": "p", "kind": "content", "path": "/base/wt"}),
                json!({"path": "a.rs", "worktree": "wt"}),
                json!({"query": "q", "path": "/base/wt"}),
                json!({"id": "a1"}),
            ]
        );
        // When a path or a working directory leaves the worktree
        let before = executor.calls().len();
        for (tool, args) in [("read_file", json!({"path": "../other/a"})), ("write_file", json!({"path": "/etc/passwd", "content": "x"})), ("run_command", json!({"command": "ls", "cwd": "../.."})), ("list_dir", json!({"path": "a/../../x"}))] {
            // Then the call is refused as INVALID_ARGS and never reaches the executor
            assert_eq!(placed.call(tool, args).await.unwrap_err().code(), "INVALID_ARGS", "{tool}");
        }
        assert_eq!(executor.calls().len(), before);
    }

    #[tokio::test]
    async fn given_the_root_place_when_wrapped_then_the_channel_is_the_executors_own_and_a_directory_place_prefixes_relative_paths() {
        // Given an executor, wrapped for its root and for a directory below it
        let executor = ScriptedChannel::new(|_, _| done());
        let root = Placed::wrap(executor.clone(), Place::default());
        let dir = Placed::wrap(executor.clone(), Place::directory("apps/web").unwrap());
        // When the same tool is called through each
        root.call("read_file", json!({"path": "../x"})).await.unwrap();
        dir.call("read_file", json!({"path": "index.ts"})).await.unwrap();
        // Then the root passes the path as it came (the executor confines it), and the directory prefixes it
        let seen: Vec<Value> = executor.calls().into_iter().map(|(_, args)| args).collect();
        assert_eq!(seen, [json!({"path": "../x"}), json!({"path": "apps/web/index.ts"})]);
        assert!(!root.is_closed() && !dir.is_closed());
    }
}
