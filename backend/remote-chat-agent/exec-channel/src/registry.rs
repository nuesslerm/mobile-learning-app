//! The open executor channels, keyed by executor, and `Executors`, the port the agent API's groups reach them through. An executor holds
//! workspaces: its root workspace, named when it attaches, and the ones made later (`workspace.create`, `worktree.create`), each of which
//! it owns. A workspace's actions go to its owner's channel; a new workspace is routed to the executor that owns its repository.
use crate::remote::{ExecutorChannel, REPLACED};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Why a workspace could not be given to an executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceError {
    /// The executor is not attached.
    UnknownExecutor(String),
    /// Another executor owns the workspace.
    Taken { workspace: String, owner: String },
}

/// Why no executor could be chosen for new work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteError {
    /// No executor is attached.
    NoExecutor,
    /// The named executor is not attached.
    UnknownExecutor(String),
    /// None was named and several are attached; they are listed sorted.
    AmbiguousExecutor(Vec<String>),
    /// No executor ever owned the workspace.
    UnknownWorkspace(String),
}

/// The executor chosen for new work and its channel.
#[derive(Clone)]
pub struct Route {
    pub executor: String,
    pub channel: Arc<dyn ExecutorChannel>,
}

/// Where a workspace lies at its executor: its root (no prefix), a directory under the root, or a worktree under the worktree base.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Place {
    /// The directory every path of the workspace is under, as the executor names it (absolute for a worktree).
    pub prefix: Option<String>,
    /// The worktree's name, for the actions that take one (`git_diff`, `worktree_setup`).
    pub worktree: Option<String>,
}

impl Place {
    /// A directory below the root: `path` relative, with no `..`, naming something (not the root itself).
    pub fn directory(path: &str) -> Result<Place, String> {
        let prefix = Place { prefix: Some(String::new()), worktree: None }.path(path)?;
        let prefix = prefix.trim_start_matches('/').to_string();
        if prefix.is_empty() {
            return Err("path must name a directory below the root".into());
        }
        Ok(Place { prefix: Some(prefix), worktree: None })
    }

    /// The path the executor is given for `path` of the workspace. Under a prefix a path is relative and stays below it, so a workspace
    /// that is a directory or a worktree cannot name anything outside it; at the root the path is the executor's to confine.
    pub fn path(&self, path: &str) -> Result<String, String> {
        let Some(prefix) = &self.prefix else { return Ok(path.to_string()) };
        let given = std::path::Path::new(path);
        if given.is_absolute() || given.components().any(|c| matches!(c, std::path::Component::ParentDir | std::path::Component::Prefix(_))) {
            return Err(format!("{path} must be a path inside the workspace"));
        }
        let rest: std::path::PathBuf = given.components().filter(|c| !matches!(c, std::path::Component::CurDir)).collect();
        Ok(if rest.as_os_str().is_empty() { prefix.clone() } else { format!("{}/{}", prefix.trim_end_matches('/'), rest.to_string_lossy()) })
    }
}

/// What a group of the agent API needs of the executors: which workspaces have one, the channel of one, and where new workspaces go.
pub trait Executors: Send + Sync {
    /// The workspaces whose executor has an open channel, sorted.
    fn workspaces(&self) -> Vec<String>;
    /// The channel of the executor that owns the workspace, when one is open.
    fn get(&self, workspace: &str) -> Option<Arc<dyn ExecutorChannel>>;
    /// Records that `thread` acts on `workspace` until `unbind`: how the MCP server a turn's CLI is given finds the thread's executor.
    fn bind(&self, thread: &str, workspace: &str);
    fn unbind(&self, thread: &str);
    /// The attached executors, sorted.
    fn executors(&self) -> Vec<String>;
    /// The executor that owns the workspace, attached or not.
    fn owner(&self, workspace: &str) -> Option<String>;
    /// Every workspace some executor owns, attached or not, sorted.
    fn known_workspaces(&self) -> Vec<String>;
    /// The workspaces the executor owns, sorted, attached or not.
    fn workspaces_of(&self, executor: &str) -> Vec<String>;
    /// Gives a new workspace to an attached executor; again for the owner is a no-op.
    fn add_workspace(&self, executor: &str, workspace: &str) -> Result<(), WorkspaceError> {
        self.add_workspace_at(executor, workspace, Place::default())
    }
    /// Gives a new workspace to an attached executor, lying at `place`; again for the owner replaces the place.
    fn add_workspace_at(&self, executor: &str, workspace: &str, place: Place) -> Result<(), WorkspaceError>;
    /// Where the workspace lies at its executor: the root when it was never placed.
    fn place(&self, workspace: &str) -> Place;
    /// Forgets the workspace; false when it was not known.
    fn remove_workspace(&self, workspace: &str) -> bool;
    /// The executor for a new repository workspace: the named one, else the only one attached.
    fn route_new(&self, executor: Option<&str>) -> Result<Route, RouteError>;
    /// The executor that owns `workspace`, for new work in the same repository (a worktree).
    fn route_worktree(&self, workspace: &str) -> Result<Route, RouteError>;
}

/// Where the registry keeps which executor owns which workspace and where it lies, so a restart of the service forgets none of them.
pub trait WorkspaceLedger: Send + Sync {
    /// Every persisted `(workspace, executor, place)`; empty when there is nothing yet or it cannot be read.
    fn load(&self) -> Vec<(String, String, Place)>;
    /// Replaces the persisted ownership and places with these.
    fn save(&self, owners: &HashMap<String, String>, places: &HashMap<String, Place>);
}

#[derive(Default)]
struct Attached {
    channels: HashMap<String, Arc<dyn ExecutorChannel>>,
    owners: HashMap<String, String>,
    places: HashMap<String, Place>,
    threads: HashMap<String, String>,
    /// Executors the ledger named that have not attached since this boot: a new executor may take over their root.
    restored: std::collections::HashSet<String>,
}

/// The open channel of each executor, the owner of each workspace, and the workspace each running thread acts on. A second open for an
/// executor replaces the first and closes it with `REPLACED`; the workspaces stay with the executor across the replacement and across a
/// disconnect, so they route again when it reattaches.
#[derive(Default)]
pub struct Registry {
    attached: Mutex<Attached>,
    ledger: Option<Arc<dyn WorkspaceLedger>>,
}

impl Registry {
    /// A registry that starts from what `ledger` holds (owned, not routable until the owner attaches) and saves every change to ownership or places.
    pub fn with_ledger(ledger: Arc<dyn WorkspaceLedger>) -> Registry {
        let mut attached = Attached::default();
        for (workspace, executor, place) in ledger.load() {
            if place != Place::default() {
                attached.places.insert(workspace.clone(), place);
            }
            attached.restored.insert(executor.clone());
            attached.owners.insert(workspace, executor);
        }
        Registry { attached: Mutex::new(attached), ledger: Some(ledger) }
    }

    fn persist(&self, attached: &Attached) {
        if let Some(ledger) = &self.ledger {
            ledger.save(&attached.owners, &attached.places);
        }
    }

    fn attached(&self) -> std::sync::MutexGuard<'_, Attached> {
        self.attached.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Attaches `executor` with `root` as its root workspace. Refused, and nothing changes, when another executor owns `root`.
    pub fn open_executor(&self, executor: &str, root: &str, channel: Arc<dyn ExecutorChannel>) -> Result<(), WorkspaceError> {
        let replaced = {
            let mut attached = self.attached();
            if let Some(owner) = attached.owners.get(root).filter(|o| o.as_str() != executor) {
                if !attached.restored.contains(owner.as_str()) || attached.channels.contains_key(owner.as_str()) {
                    return Err(WorkspaceError::Taken { workspace: root.to_string(), owner: owner.clone() });
                }
            }
            attached.restored.remove(executor);
            if attached.owners.insert(root.to_string(), executor.to_string()).as_deref() != Some(executor) {
                self.persist(&attached);
            }
            attached.channels.insert(executor.to_string(), channel)
        };
        if let Some(old) = replaced {
            old.close(REPLACED);
        }
        Ok(())
    }

    /// Attaches the executor of one workspace: its id is the workspace's.
    pub fn open(&self, workspace: &str, channel: Arc<dyn ExecutorChannel>) -> Result<(), WorkspaceError> {
        self.open_executor(workspace, workspace, channel)
    }

    /// The workspaces whose executor has an open channel, sorted.
    pub fn workspaces(&self) -> Vec<String> {
        let attached = self.attached();
        let mut names: Vec<String> = attached.owners.iter().filter(|(_, executor)| attached.channels.get(*executor).is_some_and(|c| !c.is_closed())).map(|(name, _)| name.clone()).collect();
        names.sort();
        names
    }

    /// The channel of the executor that owns the workspace, when one is open.
    pub fn get(&self, workspace: &str) -> Option<Arc<dyn ExecutorChannel>> {
        let attached = self.attached();
        attached.channels.get(attached.owners.get(workspace)?).filter(|c| !c.is_closed()).cloned()
    }

    /// Forgets the executor's channel when it is still `channel` (a replaced one must not remove its successor); its workspaces stay.
    pub fn release(&self, executor: &str, channel: &Arc<dyn ExecutorChannel>) {
        let mut attached = self.attached();
        if attached.channels.get(executor).is_some_and(|current| Arc::ptr_eq(current, channel)) {
            attached.channels.remove(executor);
        }
    }

    /// Records that `thread` acts on `workspace` (until `unbind`), which is how the MCP bridge of a thread finds its executor.
    pub fn bind(&self, thread: &str, workspace: &str) {
        self.attached().threads.insert(thread.to_string(), workspace.to_string());
    }

    pub fn unbind(&self, thread: &str) {
        self.attached().threads.remove(thread);
    }

    /// The channel of the workspace `thread` is bound to, when both exist: for a worktree or a directory, seen through its place (`Placed`).
    pub fn for_thread(&self, thread: &str) -> Option<Arc<dyn ExecutorChannel>> {
        let workspace = self.attached().threads.get(thread).cloned()?;
        let channel = Registry::get(self, &workspace)?;
        Some(crate::placed::Placed::wrap(channel, Executors::place(self, &workspace)))
    }

    fn open_executors(&self) -> Vec<(String, Arc<dyn ExecutorChannel>)> {
        let mut open: Vec<_> = self.attached().channels.iter().filter(|(_, c)| !c.is_closed()).map(|(id, c)| (id.clone(), c.clone())).collect();
        open.sort_by(|a, b| a.0.cmp(&b.0));
        open
    }
}

impl Executors for Registry {
    fn workspaces(&self) -> Vec<String> {
        Registry::workspaces(self)
    }

    fn get(&self, workspace: &str) -> Option<Arc<dyn ExecutorChannel>> {
        Registry::get(self, workspace)
    }

    fn bind(&self, thread: &str, workspace: &str) {
        Registry::bind(self, thread, workspace);
    }

    fn unbind(&self, thread: &str) {
        Registry::unbind(self, thread);
    }

    fn executors(&self) -> Vec<String> {
        self.open_executors().into_iter().map(|(id, _)| id).collect()
    }

    fn owner(&self, workspace: &str) -> Option<String> {
        self.attached().owners.get(workspace).cloned()
    }

    fn known_workspaces(&self) -> Vec<String> {
        let mut names: Vec<String> = self.attached().owners.keys().cloned().collect();
        names.sort();
        names
    }

    fn workspaces_of(&self, executor: &str) -> Vec<String> {
        let mut names: Vec<String> = self.attached().owners.iter().filter(|(_, owner)| owner.as_str() == executor).map(|(name, _)| name.clone()).collect();
        names.sort();
        names
    }

    fn place(&self, workspace: &str) -> Place {
        self.attached().places.get(workspace).cloned().unwrap_or_default()
    }

    fn add_workspace_at(&self, executor: &str, workspace: &str, place: Place) -> Result<(), WorkspaceError> {
        let mut attached = self.attached();
        if !attached.channels.get(executor).is_some_and(|c| !c.is_closed()) {
            return Err(WorkspaceError::UnknownExecutor(executor.to_string()));
        }
        match attached.owners.get(workspace) {
            Some(owner) if owner != executor => Err(WorkspaceError::Taken { workspace: workspace.to_string(), owner: owner.clone() }),
            _ => {
                attached.owners.insert(workspace.to_string(), executor.to_string());
                attached.places.insert(workspace.to_string(), place);
                self.persist(&attached);
                Ok(())
            }
        }
    }

    fn remove_workspace(&self, workspace: &str) -> bool {
        let mut attached = self.attached();
        attached.places.remove(workspace);
        let known = attached.owners.remove(workspace).is_some();
        if known {
            self.persist(&attached);
        }
        known
    }

    fn route_new(&self, executor: Option<&str>) -> Result<Route, RouteError> {
        let mut open = self.open_executors();
        match executor {
            Some(id) => open.into_iter().find(|(open_id, _)| open_id == id).map(|(executor, channel)| Route { executor, channel }).ok_or_else(|| RouteError::UnknownExecutor(id.to_string())),
            None if open.len() > 1 => Err(RouteError::AmbiguousExecutor(open.into_iter().map(|(id, _)| id).collect())),
            None => open.pop().map(|(executor, channel)| Route { executor, channel }).ok_or(RouteError::NoExecutor),
        }
    }

    fn route_worktree(&self, workspace: &str) -> Result<Route, RouteError> {
        let executor = self.owner(workspace).ok_or_else(|| RouteError::UnknownWorkspace(workspace.to_string()))?;
        self.route_new(Some(&executor)).map_err(|e| match e {
            RouteError::UnknownExecutor(_) => RouteError::NoExecutor,
            other => other,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::Channel;
    use crate::testing::RecordingSink;

    #[tokio::test]
    async fn a_second_open_for_a_workspace_closes_the_first_with_4001_and_the_first_cannot_unregister_the_second() {
        let (a, b) = (Arc::new(RecordingSink::default()), Arc::new(RecordingSink::default()));
        let (first, second): (Arc<dyn ExecutorChannel>, Arc<dyn ExecutorChannel>) = (Channel::new(a.clone()), Channel::new(b.clone()));
        let registry = Registry::default();
        registry.open("w", first.clone()).unwrap();
        registry.open("w", second.clone()).unwrap();
        registry.release("w", &first);
        assert_eq!((a.closes(), b.closes(), first.is_closed()), (vec![REPLACED], vec![], true));
        assert!(registry.get("w").is_some_and(|c| Arc::ptr_eq(&c, &second)));
        assert!(registry.get("other").is_none());
    }

    #[test]
    fn given_a_place_under_a_prefix_when_paths_are_resolved_then_they_stay_below_it_and_the_root_leaves_confinement_to_the_executor() {
        // Given a worktree place, and the root
        let wt = Place { prefix: Some("/base/wt".into()), worktree: Some("wt".into()) };
        let root = Place::default();
        // When relative paths are resolved
        // Then they are joined below the prefix, "." and "" are the prefix, and a leading ./ is dropped
        assert_eq!(wt.path("src/a.rs").unwrap(), "/base/wt/src/a.rs");
        assert_eq!((wt.path(".").unwrap(), wt.path("").unwrap(), wt.path("./a").unwrap()), ("/base/wt".to_string(), "/base/wt".to_string(), "/base/wt/a".to_string()));
        // And every spelling that leaves the prefix is refused here, before any executor is asked
        for bad in ["../x", "a/../../x", "/etc/passwd", "a/.."] {
            assert!(wt.path(bad).is_err(), "{bad}");
        }
        // And a directory place is a relative path that names something below the root, and nothing else
        assert_eq!(Place::directory("sub/./dir").unwrap().prefix.as_deref(), Some("sub/dir"));
        for bad in [".", "", "../x", "/abs", "a/../.."] {
            assert!(Place::directory(bad).is_err(), "{bad}");
        }
        assert_eq!(Place::directory("sub").unwrap().path("a.txt").unwrap(), "sub/a.txt");
        // And the root passes the path on as it came
        assert_eq!((root.path("../x").unwrap(), root.path("/abs").unwrap()), ("../x".to_string(), "/abs".to_string()));
    }

    #[tokio::test]
    async fn given_a_workspace_added_at_a_place_when_asked_then_the_place_is_kept_until_the_workspace_is_removed_and_a_taken_name_changes_nothing() {
        // Given two attached executors and a worktree workspace added for the first
        let registry = Registry::default();
        for id in ["e1", "e2"] {
            registry.open_executor(id, id, Channel::new(Arc::new(RecordingSink::default()))).unwrap();
        }
        let place = Place { prefix: Some("/base/wt".into()), worktree: Some("wt".into()) };
        registry.add_workspace_at("e1", "wt", place.clone()).unwrap();
        // Then the place is the one given, a workspace never placed is at the root, and the owner is the executor it was added for
        assert_eq!((registry.place("wt"), registry.place("e1"), registry.owner("wt")), (place.clone(), Place::default(), Some("e1".into())));
        // And the other executor cannot take the name, nor change its place
        assert!(matches!(registry.add_workspace_at("e2", "wt", Place::default()), Err(WorkspaceError::Taken { .. })));
        assert_eq!(registry.place("wt"), place);
        // And removing the workspace forgets its place
        assert!(registry.remove_workspace("wt"));
        assert_eq!((registry.place("wt"), registry.owner("wt")), (Place::default(), None));
    }

    #[tokio::test]
    async fn given_a_thread_bound_to_a_worktree_when_its_channel_is_used_then_the_executor_is_asked_below_the_worktree() {
        // Given an executor with a root workspace and a worktree workspace, and a thread bound to each
        let executor = crate::testing::ScriptedChannel::new(|_, _| Ok(crate::remote::ActionOutput { result: serde_json::json!({}), output: String::new() }));
        let registry = Registry::default();
        registry.open_executor("e", "root", executor.clone()).unwrap();
        registry.add_workspace_at("e", "wt", Place { prefix: Some("/base/wt".into()), worktree: Some("wt".into()) }).unwrap();
        registry.bind("at-root", "root");
        registry.bind("in-wt", "wt");
        // When each thread's tool reads the same relative path
        registry.for_thread("at-root").unwrap().call("read_file", serde_json::json!({"path": "a.txt"})).await.unwrap();
        registry.for_thread("in-wt").unwrap().call("read_file", serde_json::json!({"path": "a.txt"})).await.unwrap();
        // Then the first read the root's file and the second the worktree's
        let seen: Vec<serde_json::Value> = executor.calls().into_iter().map(|(_, args)| args).collect();
        assert_eq!(seen, [serde_json::json!({"path": "a.txt"}), serde_json::json!({"path": "/base/wt/a.txt"})]);
    }

    #[tokio::test]
    async fn a_thread_reaches_the_channel_of_its_workspace_only_while_bound() {
        let channel: Arc<dyn ExecutorChannel> = Channel::new(Arc::new(RecordingSink::default()));
        let registry = Registry::default();
        registry.open("w", channel.clone()).unwrap();
        assert!(registry.for_thread("t").is_none());
        registry.bind("t", "w");
        registry.bind("lost", "elsewhere");
        assert!(registry.for_thread("t").is_some_and(|c| Arc::ptr_eq(&c, &channel)));
        assert!(registry.for_thread("lost").is_none());
        registry.unbind("t");
        assert!(registry.for_thread("t").is_none());
    }

    #[tokio::test]
    async fn the_executors_port_lists_the_workspaces_with_an_open_channel_sorted_and_hands_out_only_open_ones() {
        // Given a registry with channels for two workspaces opened out of order, one of which then closes
        let registry = Registry::default();
        let (b, a): (Arc<dyn ExecutorChannel>, Arc<dyn ExecutorChannel>) = (Channel::new(Arc::new(RecordingSink::default())), Channel::new(Arc::new(RecordingSink::default())));
        registry.open("b", b.clone()).unwrap();
        registry.open("a", a).unwrap();
        let port: &dyn Executors = &registry;
        // When the port is asked, Then both are listed in order, then only the one still open once the other closes
        assert_eq!(port.workspaces(), ["a", "b"]);
        b.close(1000);
        assert_eq!((port.workspaces(), port.get("b").is_none(), port.get("a").is_some()), (vec!["a".to_string()], true, true));
    }

    fn attach(registry: &Registry, executor: &str, root: &str) -> (Arc<dyn ExecutorChannel>, Arc<RecordingSink>) {
        let sink = Arc::new(RecordingSink::default());
        let channel: Arc<dyn ExecutorChannel> = Channel::new(sink.clone());
        registry.open_executor(executor, root, channel.clone()).unwrap();
        (channel, sink)
    }

    fn same(route: Result<Route, RouteError>, channel: &Arc<dyn ExecutorChannel>) -> bool {
        route.is_ok_and(|r| Arc::ptr_eq(&r.channel, channel))
    }

    #[tokio::test]
    async fn a_workspace_is_served_by_the_executor_that_owns_it_and_a_new_worktree_workspace_follows_its_repository_to_that_executor() {
        // Given two executors with a root workspace each, and a worktree workspace made for the first one's repository
        let registry = Registry::default();
        let ((a, _), (b, _)) = (attach(&registry, "box-a", "repo-a"), attach(&registry, "box-b", "repo-b"));
        registry.add_workspace("box-a", "repo-a/feature").unwrap();
        // When each workspace is looked up, and a worktree is routed from the first repository and from the worktree itself
        let found = |w: &str| registry.get(w);
        // Then each reaches its owner's channel, never the other's
        assert!(found("repo-a").is_some_and(|c| Arc::ptr_eq(&c, &a)) && found("repo-a/feature").is_some_and(|c| Arc::ptr_eq(&c, &a)));
        assert!(found("repo-b").is_some_and(|c| Arc::ptr_eq(&c, &b)));
        assert!(same(registry.route_worktree("repo-a"), &a) && same(registry.route_worktree("repo-a/feature"), &a) && same(registry.route_worktree("repo-b"), &b));
        // And the owner and the workspaces of an executor are listed
        assert_eq!((registry.owner("repo-a/feature"), registry.workspaces_of("box-a"), registry.executors()), (Some("box-a".to_string()), vec!["repo-a".to_string(), "repo-a/feature".to_string()], vec!["box-a".to_string(), "box-b".to_string()]));
        // And a workspace nobody owns is unknown, not routed anywhere
        assert_eq!(registry.route_worktree("nowhere").err(), Some(RouteError::UnknownWorkspace("nowhere".into())));
    }

    #[tokio::test]
    async fn a_new_workspace_goes_to_the_named_executor_or_the_only_one_and_never_to_a_guess_between_several() {
        // Given no executor, then one, then two
        let registry = Registry::default();
        assert_eq!(registry.route_new(None).err(), Some(RouteError::NoExecutor));
        let (a, _) = attach(&registry, "box-a", "repo-a");
        // Then with one attached it is chosen with or without a name, and an unknown name is refused
        assert!(same(registry.route_new(None), &a) && same(registry.route_new(Some("box-a")), &a));
        assert_eq!(registry.route_new(Some("box-z")).err(), Some(RouteError::UnknownExecutor("box-z".into())));
        // When a second attaches, Then no name is ambiguous and lists both, and a name still picks one
        let (b, _) = attach(&registry, "box-b", "repo-b");
        assert_eq!(registry.route_new(None).err(), Some(RouteError::AmbiguousExecutor(vec!["box-a".into(), "box-b".into()])));
        assert!(same(registry.route_new(Some("box-b")), &b));
    }

    #[tokio::test]
    async fn a_workspace_another_executor_owns_is_refused_and_its_owner_keeps_its_channel() {
        // Given an executor that owns "repo"
        let registry = Registry::default();
        let (a, a_sink) = attach(&registry, "box-a", "repo");
        // When another executor attaches claiming it, and gives itself a workspace the first owns
        let second = Arc::new(RecordingSink::default());
        let claim = registry.open_executor("box-b", "repo", Channel::new(second.clone()) as Arc<dyn ExecutorChannel>);
        attach(&registry, "box-b", "own");
        let steal = registry.add_workspace("box-b", "repo");
        // Then both are Taken by box-a, nothing was closed, and a workspace for an executor that is not attached is refused
        let taken = Err(WorkspaceError::Taken { workspace: "repo".into(), owner: "box-a".into() });
        assert_eq!((claim, steal), (taken.clone(), taken));
        assert_eq!((a_sink.closes(), second.closes()), (vec![], vec![]));
        assert!(registry.get("repo").is_some_and(|c| Arc::ptr_eq(&c, &a)));
        assert_eq!(registry.add_workspace("ghost", "w"), Err(WorkspaceError::UnknownExecutor("ghost".into())));
        // And giving the owner its own workspace again is a no-op
        assert_eq!(registry.add_workspace("box-a", "repo"), Ok(()));
    }

    #[tokio::test]
    async fn the_workspaces_of_an_executor_survive_its_disconnect_and_its_replacement_and_route_again_when_it_is_back() {
        // Given an executor with a root and a made workspace
        let registry = Registry::default();
        let (first, first_sink) = attach(&registry, "box-a", "repo");
        registry.add_workspace("box-a", "repo/wt").unwrap();
        // When its channel drops
        registry.release("box-a", &first);
        // Then nothing is served and nothing can be routed, but the workspaces are still its own
        assert!((registry.get("repo/wt").is_none(), registry.workspaces(), registry.executors()) == (true, Vec::<String>::new(), Vec::<String>::new()));
        assert_eq!(registry.route_worktree("repo/wt").err(), Some(RouteError::NoExecutor));
        assert_eq!(registry.workspaces_of("box-a"), ["repo", "repo/wt"]);
        // When it reattaches, then a newer channel replaces it
        let (second, _) = attach(&registry, "box-a", "repo");
        let (third, _) = attach(&registry, "box-a", "repo");
        // Then both workspaces route to the newest channel, the replaced one was closed with 4001, and the one released earlier was not closed again
        assert!(registry.get("repo/wt").is_some_and(|c| Arc::ptr_eq(&c, &third)) && second.is_closed() && !third.is_closed());
        assert_eq!((first_sink.closes(), registry.workspaces()), (vec![], vec!["repo".to_string(), "repo/wt".to_string()]));
        // And a removed workspace is no longer anyone's
        assert!(registry.remove_workspace("repo/wt") && !registry.remove_workspace("repo/wt"));
        assert!(registry.get("repo/wt").is_none() && registry.owner("repo/wt").is_none());
    }

    #[derive(Default)]
    struct MemoryLedger(Mutex<Vec<(String, String, Place)>>);

    impl WorkspaceLedger for MemoryLedger {
        fn load(&self) -> Vec<(String, String, Place)> {
            self.0.lock().unwrap().clone()
        }

        fn save(&self, owners: &HashMap<String, String>, places: &HashMap<String, Place>) {
            let mut rows: Vec<_> = owners.iter().map(|(w, e)| (w.clone(), e.clone(), places.get(w).cloned().unwrap_or_default())).collect();
            rows.sort_by(|a, b| a.0.cmp(&b.0));
            *self.0.lock().unwrap() = rows;
        }
    }

    #[tokio::test]
    async fn given_a_ledger_when_the_registry_is_built_again_then_workspaces_are_known_but_route_only_once_their_executor_attaches_and_a_drop_leaves_the_ledger() {
        // Given a registry over a ledger, an executor with a root and a worktree workspace made for it
        let ledger = Arc::new(MemoryLedger::default());
        let first = Registry::with_ledger(ledger.clone());
        attach(&first, "box", "repo");
        let place = Place { prefix: Some("/base/wt".into()), worktree: Some("wt".into()) };
        first.add_workspace_at("box", "wt", place.clone()).unwrap();
        first.add_workspace_at("box", "gone", Place::default()).unwrap();
        // When a new registry is built over the same ledger
        let second = Registry::with_ledger(ledger.clone());
        // Then the workspaces, their owner and the place are known, and none routes yet
        assert_eq!((second.known_workspaces(), second.owner("wt"), second.place("wt")), (vec!["gone".to_string(), "repo".to_string(), "wt".to_string()], Some("box".to_string()), place));
        assert!(second.get("wt").is_none() && second.workspaces().is_empty());
        assert_eq!(second.route_worktree("wt").err(), Some(RouteError::NoExecutor));
        // And they route the moment the same executor id attaches
        let (channel, _) = attach(&second, "box", "repo");
        assert!(second.get("wt").is_some_and(|c| Arc::ptr_eq(&c, &channel)) && same(second.route_worktree("wt"), &channel));
        // And dropping a workspace removes it from the ledger
        assert!(second.remove_workspace("gone"));
        assert_eq!(ledger.load().into_iter().map(|row| row.0).collect::<Vec<_>>(), ["repo", "wt"]);
    }

    #[tokio::test]
    async fn given_a_ledger_owner_that_has_not_attached_when_another_executor_claims_its_root_then_it_takes_over_but_a_live_owner_is_never_displaced() {
        // Given a registry restored from a ledger that names executor `old` as the owner of root `repo`
        let ledger = Arc::new(MemoryLedger::default());
        attach(&Registry::with_ledger(ledger.clone()), "old", "repo");
        let restored = Registry::with_ledger(ledger.clone());
        // When another executor opens the same root before `old` has attached since the boot
        let (channel, _) = attach(&restored, "new", "repo");
        // Then it owns the root and routes, and the ledger says so
        assert!(restored.owner("repo").as_deref() == Some("new") && restored.get("repo").is_some_and(|c| Arc::ptr_eq(&c, &channel)));
        assert_eq!(ledger.load().into_iter().map(|row| (row.0, row.1)).collect::<Vec<_>>(), [("repo".to_string(), "new".to_string())]);
        // And an executor that is attached now is never displaced by a third one
        assert!(matches!(restored.open_executor("third", "repo", Channel::new(Arc::new(RecordingSink::default()))), Err(WorkspaceError::Taken { .. })));
    }
}
