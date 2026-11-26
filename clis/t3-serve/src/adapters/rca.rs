//! `AgentApi` over rca's agent API (`GET /agent/v1/socket`, a device bearer), through `agent-api-client`. A turn is `thread.create`,
//! `turn.start` and `thread.subscribe` on `agent.threads`, followed as the thread's events until its `turn.finished`; a thread's past is
//! `thread.history`; project files are `file.list` and `file.read` in one rca workspace, whose executor works where the files are. History pages can share an idle socket.
//! Turn control and subscriptions use independent connections.
use agent_api_agent::lifecycle::{self, Cursor, Shelf, SettleState};
use agent_api_agent::queue::{self, TURN_STEER};
use agent_api_agent::requests::{self, Decision};
use agent_api_agent::{code as thread_code, CreateParams, FinishReason, HistoryParams, HistoryResult, InterruptParams, Role, StartParams, StartResult, SubscribeParams, ThreadEvent, ThreadView, THREAD_CREATE, THREAD_HISTORY, THREAD_SUBSCRIBE, TURN_INTERRUPT, TURN_START};
use agent_api_client::{Client, Config, Error, Subscribe};
use agent_api_proto::{thread_topic, Event, GroupName, Method, PROTOCOL};
use agent_api_service::activity::ClientActivityParams;
use agent_api_service::panes::{PaneState, PaneView};
use agent_api_service::{StatusResult, CLIENT_ACTIVITY, STATUS};
use agent_api_accounts as accounts;
use agent_api_catalog as catalog;
use agent_api_git as git;
use agent_api_preview as preview;
use agent_api_schedules as schedules;
use agent_api_attachments as attachments;
use agent_api_hosted as hosted;
use agent_api_terminal as terminal;
use agent_api_workspace::{code as workspace_code, edit, worktrees, FILE_LIST, FILE_READ, REPO_LIST, WORKSPACE, WORKSPACE_CREATE, WORKSPACE_REMOVE};
use exec_fs::{EntryKind, ListResult, ReadResult};
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use t3_core::ports::{AgentApi, AgentError, AgentEvent, AgentEventKind, AgentThread, AgentTurn, CatalogModel, CatalogProvider, DirEntry, Finish, Follow, History, Op, RuntimeRequestEvent, RuntimeResponse, SetupBatch, SetupEvent, TerminalBatch, TerminalEvent, TerminalSummary};

pub struct RcaAgent {
    authority: String,
    /// A device bearer, which is the one credential this mapper holds for rca (`wf rca bearers mint`).
    bearer: String,
    /// The rca workspace whose executor works where the projects' files are: every turn and file call acts there.
    workspace: String,
    providers: OnceLock<Vec<CatalogProvider>>,
    idle_history: Mutex<Option<Client>>,
}

/// Every agent method this mapper calls. The start check below and the calls both read this one list, and a flow asserts that the methods
/// the adapter really sends are exactly these, so the groups a mapper needs are the groups it uses.
pub const USES: [Method; 158] = [
    STATUS,
    preview::rendering::FRAME, preview::rendering::INPUT, preview::rendering::GOTO, preview::rendering::HISTORY,
    THREAD_CREATE,
    agent_api_agent::THREAD_LIST,
    agent_api_agent::THREAD_GET,
    THREAD_SUBSCRIBE,
    THREAD_HISTORY,
    TURN_START,
    TURN_INTERRUPT,
    FILE_LIST,
    FILE_READ,
    requests::APPROVAL_ANSWER,
    requests::INPUT_ANSWER,
    requests::INPUT_DISMISS,
    queue::QUEUE_ADD,
    queue::QUEUE_EDIT,
    queue::QUEUE_MOVE,
    queue::QUEUE_DELETE,
    queue::QUEUE_RESUME,
    TURN_STEER,
    lifecycle::THREAD_ARCHIVE,
    lifecycle::THREAD_DELETE,
    lifecycle::THREAD_FORK,
    lifecycle::THREAD_SETTLE,
    lifecycle::THREAD_SEARCH,
    lifecycle::THREAD_FAMILY,
    lifecycle::THREAD_FORCE_STOP,
    lifecycle::ITEM_GET,
    lifecycle::WORKFLOW_STOP,
    lifecycle::THREAD_MERGE_BACK,
    lifecycle::THREAD_ROLLBACK,
    lifecycle::THREAD_DETACH,
    lifecycle::TURN_RETRY,
    lifecycle::THREAD_SHELF,
    agent_api_state::GET,
    agent_api_state::watch::SUBSCRIBE,
    agent_api_service::panes::LIST,
    agent_api_service::panes::MARK,
    agent_api_state::SET,
    agent_api_state::COMPARE_AND_SET,
    CLIENT_ACTIVITY,
    edit::FILE_CREATE,
    edit::ARTIFACT_WRITE,
    edit::FILE_STAT,
    edit::FILE_READ_CHUNK,
    edit::FILE_WRITE,
    edit::FILE_BROWSE,
    edit::FILE_MKDIR,
    edit::FILE_SEARCH,
    edit::TURN_DIFF,
    edit::THREAD_DIFF,
    worktrees::WORKTREE_CREATE,
    worktrees::WORKTREE_REMOVE,
    worktrees::WORKTREE_LIST,
    REPO_LIST,
    worktrees::SETUP_SUBSCRIBE,
    worktrees::SETUP_CANCEL,
    terminal::OPEN,
    terminal::WRITE,
    terminal::RESIZE,
    terminal::CLOSE,
    terminal::CLEAR,
    terminal::LIST,
    terminal::ATTACH,
    git::STATUS,
    git::UPSTREAM_STATUS,
    git::DIFF,
    git::REFS,
    git::HISTORY,
    git::CHECKOUT,
    git::SWITCH_WORKTREE_BRANCH,
    git::CREATE_REF,
    git::FETCH,
    git::COMPARE,
    git::PULL,
    git::BLOB,
    git::STAGE,
    git::COMMIT,
    git::PUSH,
    git::INIT,
    git::IGNORED,
    git::SUBSCRIBE,
    WORKSPACE_CREATE,
    WORKSPACE_REMOVE,
    accounts::PROVIDER_LIST,
    accounts::PROVIDER_REFRESH,
    accounts::PROVIDER_SELECT,
    accounts::AUTH_START,
    accounts::AUTH_RESPOND,
    accounts::AUTH_COMPLETE,
    accounts::AUTH_CANCEL,
    accounts::AUTH_SUBSCRIBE,
    accounts::PROVIDER_LOGOUT,
    accounts::USAGE_SUMMARY,
    accounts::RESET_CREDIT,
    hosted::pr::LIST,
    hosted::pr::VIEW,
    hosted::pr::DIFF,
    hosted::pr::CHECKS,
    hosted::pr::COMMENTS,
    hosted::pr::REVIEWS,
    hosted::pr::THREADS,
    hosted::pr::FILES,
    hosted::pr::EDIT,
    hosted::pr::READY,
    hosted::pr::COMMENT,
    hosted::pr::REPLY,
    hosted::pr::REVIEW,
    hosted::pr::REQUEST_REVIEWERS,
    hosted::pr::SET_LABELS,
    hosted::pr::REACT,
    hosted::pr::STACK,
    hosted::pr::EDIT_COMMENT,
    hosted::pr::RESOLVE_THREAD,
    hosted::pr::UPDATE_BRANCH,
    hosted::pr::MERGE,
    hosted::pr::LABEL_CANDIDATES,
    hosted::pr::REVIEWER_CANDIDATES,
    hosted::auth::DISCOVER,
    hosted::auth::APP_ACCESS,
    hosted::auth::STARRED,
    hosted::auth::STAR,
    hosted::auth::LOOKUP_REPOSITORY,
    hosted::auth::PUBLISH_REPOSITORY,
    hosted::auth::SIGN_IN_START,
    hosted::auth::SIGN_IN_WAIT,
    hosted::auth::SIGN_IN_CANCEL,
    hosted::clone::START,
    hosted::clone::STATUS,
    hosted::clone::CANCEL,
    hosted::clone::SUBSCRIBE,
    schedules::LIST,
    schedules::UPSERT,
    schedules::SET_ENABLED,
    schedules::DELETE,
    schedules::RUN_NOW,
    schedules::ROTATE_WEBHOOK_TOKEN,
    schedules::watch::SUBSCRIBE,
    attachments::UPLOAD_PUT,
    attachments::GET,
    attachments::DELETE,
    preview::sessions::OPEN,
    preview::sessions::REPORT,
    preview::sessions::RESIZE,
    preview::sessions::ADJUST,
    preview::sessions::CLOSE,
    preview::sessions::LIST,
    preview::watch::SUBSCRIBE,
    catalog::SPEECH_STATUS,
    catalog::SPEECH_TRANSCRIBE,
    catalog::SESSIONS_LIST,
    catalog::AGENTS_DETECT,
    catalog::MODELS_LIST,
    catalog::COMMANDS_LIST,
    agent_api_service::LOGS,
];

/// The groups the mapper calls that `service.status` does not list: `workspace` is served, but its owner methods `workspace.activate` and
/// `repo.hooks` are not, and a group with a method pending is not listed.
pub const UNLISTED: [GroupName; 1] = [WORKSPACE];

/// The groups `service.status` must list for this mapper to start.
pub fn needed_groups() -> Vec<GroupName> {
    let mut groups: Vec<GroupName> = Vec::new();
    for method in USES {
        if !groups.contains(&method.group()) && !UNLISTED.contains(&method.group()) {
            groups.push(method.group());
        }
    }
    groups
}

/// How long a followed thread waits for its next event before it looks again; a silent CLI is not an error.
const QUIET: Duration = Duration::from_secs(1);

struct MobilePlacement {
    missing: bool,
    path: String,
    branch: Option<String>,
    main: bool,
    owner: Option<String>,
}

impl MobilePlacement {
    fn metadata_key(&self, repo: &str) -> String { format!("orca.worktree.{repo}::{}", self.path) }

    fn row(&self, repo: &str, meta: Option<&Value>) -> Value {
        let path = self.path.as_str();
        let title = self.branch.as_deref().unwrap_or_else(|| path.trim_end_matches('/').rsplit('/').next().unwrap_or(path));
        let name = meta.and_then(|m| m["displayName"].as_str()).filter(|s| !s.is_empty()).unwrap_or(title);
        json!({"id":format!("{repo}::{path}"),"name":name,"automaticName":title,
            "missing":self.missing,"pinned":meta.is_some_and(|m| m["isPinned"] == true),"branch":self.branch,"main":self.main,
            "workspace":self.owner.as_deref().unwrap_or(repo),"path":path,"workspaceStatus":meta.and_then(|m| m["workspaceStatus"].as_str()),"unread":meta.is_some_and(|m| m["isUnread"] == true)})
    }
}

struct MobileInventory {
    checkouts: Vec<(Value, Option<String>)>,
    panes: Vec<PaneView>,
}

fn checkout_missing() -> AgentError {
    AgentError::Refused { code: "CHECKOUT_NOT_FOUND".into(), message: "Checkout not found".into() }
}

fn failed(e: Error) -> AgentError {
    if e.code == thread_code::THREAD_NOT_FOUND {
        return AgentError::NoThread;
    }
    AgentError::Failed(if e.from_service() { e.message } else { e.to_string() })
}

/// Like `failed`, but a refusal of the service keeps its code, for the requests whose refusals the app tells apart.
fn refused(e: Error) -> AgentError {
    if e.from_service() {
        return AgentError::Refused { code: e.code, message: e.message };
    }
    failed(e)
}

/// Where in the executor a project's `path` is: the project `root` (the executor's own when empty) and the path under it.
fn under(root: &str, path: &str) -> String {
    let relative = path.trim_start_matches('/');
    match (root.is_empty(), relative.is_empty()) {
        (true, true) => ".".into(),
        (true, false) => relative.into(),
        (false, true) => root.into(),
        (false, false) => format!("{}/{relative}", root.trim_end_matches('/')),
    }
}

/// What a wire event is to t3, or why it is none: a kind the contract does not have or a field it does not read.
pub fn agent_event(event: &Event) -> Result<AgentEvent, AgentError> {
    if requests::events::KINDS.contains(&event.kind.as_str()) {
        use requests::events::RequestEvent;
        let read = RequestEvent::from_event(event).map_err(|e| AgentError::Failed(format!("rca sent a request event this t3 serve cannot read: {}", e.0)))?;
        let (id, response) = match read {
            RequestEvent::ApprovalAnswered { approval, decision } => {
                let decision = match decision {
                    Decision::Accept => "accept",
                    Decision::AcceptForSession => "acceptForSession",
                    Decision::Decline => "decline",
                    Decision::Cancel => "cancel",
                };
                (approval, RuntimeResponse::Approval(decision))
            }
            RequestEvent::InputAnswered { request, answer } => (request, RuntimeResponse::Answer(answer)),
            RequestEvent::InputDismissed { request } => (request, RuntimeResponse::Dismissed),
        };
        return Ok(AgentEvent { seq: event.seq, turn: event.turn.clone(), kind: AgentEventKind::Request(RuntimeRequestEvent::Closed { id, response }) });
    }
    if queue::events::KINDS.contains(&event.kind.as_str()) || lifecycle::events::KINDS.contains(&event.kind.as_str()) {
        return Ok(AgentEvent { seq: event.seq, turn: event.turn.clone(), kind: AgentEventKind::Other });
    }
    let read = ThreadEvent::from_event(event).map_err(|e| AgentError::Failed(format!("rca sent an event this t3 serve cannot read: {}", e.0)))?;
    let kind = match read {
        ThreadEvent::TurnStarted {} => AgentEventKind::TurnStarted,
        ThreadEvent::MessageDelta { role: Role::User, text, attachments } => AgentEventKind::UserMessage { text, attachments: attachments.into_iter().map(|a| (a.id, a.name)).collect() },
        ThreadEvent::MessageDelta { role: Role::Assistant, text, .. } => AgentEventKind::Text(text),
        ThreadEvent::ReasoningDelta { text } => AgentEventKind::Reasoning(text),
        ThreadEvent::ToolStarted { call, tool, args } => AgentEventKind::ToolStarted { id: call, name: tool, input: args },
        ThreadEvent::ToolOutput { call, data, .. } => AgentEventKind::ToolOutput { id: call, data },
        ThreadEvent::ToolFinished { call, ok, .. } => AgentEventKind::ToolFinished { id: call, ok },
        ThreadEvent::Error { message, .. } => AgentEventKind::Failure(message),
        ThreadEvent::TurnFinished { reason, .. } => AgentEventKind::TurnFinished(match reason {
            FinishReason::Completed => Finish::Completed,
            FinishReason::Interrupted => Finish::Interrupted,
            FinishReason::Failed => Finish::Failed,
        }),
        ThreadEvent::ApprovalRequested { approval, tool, args } => AgentEventKind::Request(RuntimeRequestEvent::Approval { id: approval, tool, args }),
        ThreadEvent::InputRequested { request, prompt, options } => AgentEventKind::Request(RuntimeRequestEvent::Question { id: request, prompt, options }),
        ThreadEvent::FileChanged { .. } | ThreadEvent::PlanUpdated { .. } | ThreadEvent::Usage(_) => AgentEventKind::Other,
    };
    Ok(AgentEvent { seq: event.seq, turn: event.turn.clone(), kind })
}

impl RcaAgent {
    /// `url` is `http://host:port`; anything else is refused at startup.
    pub fn new(url: &str, bearer: &str, workspace: &str) -> Result<RcaAgent, String> {
        let authority = url.strip_prefix("http://").map(|r| r.trim_end_matches('/').to_string()).filter(|a| !a.is_empty() && !a.contains('/'));
        authority.map(|authority| RcaAgent { authority, bearer: bearer.to_string(), workspace: workspace.to_string(), providers: OnceLock::new(), idle_history: Mutex::default() }).ok_or_else(|| format!("rca url {url} must be http://host:port"))
    }

    fn open(&self) -> Result<Client, Error> {
        Client::connect(Config::new(format!("ws://{}", self.authority), self.bearer.clone()))
    }

    fn connect(&self) -> Result<Client, AgentError> {
        self.open().map_err(failed)
    }

    /// Asks the service for `service.status` and refuses a service that speaks another protocol or lacks a group this mapper needs, so
    /// `t3 serve` stops at startup instead of failing on the first request.
    pub fn check_agent_api(&self) -> Result<(), String> {
        let unreachable = |e: &dyn std::fmt::Display| format!("rca at {} did not answer its agent API: {e}", self.authority);
        let mut client = self.open().map_err(|e| unreachable(&e))?;
        match client.call(STATUS, json!({})) {
            Ok(result) => compatible(&result),
            Err(e) if e.from_service() => Err(format!("rca refused service.status ({}: {}): update one of them", e.code, e.message)),
            Err(e) => Err(unreachable(&e)),
        }
    }

    fn call(client: &mut Client, method: Method, params: Value) -> Result<Value, AgentError> {
        client.call(method, params).map_err(failed)
    }

    fn checkout_path<'a>(&self, checkout: &'a str) -> Result<&'a str, AgentError> {
        checkout.split_once("::")
            .filter(|(repo, path)| *repo == self.workspace && !path.is_empty())
            .map(|(_, path)| path)
            .ok_or_else(checkout_missing)
    }

    fn selected_checkout(&self, client: &mut Client, checkout: &str) -> Result<exec_git::WorktreeInfo, AgentError> {
        let path = self.checkout_path(checkout)?;
        let catalog = Self::call(client, worktrees::WORKTREE_LIST, json!({"workspace":self.workspace}))?;
        let catalog: exec_git::WorktreeListed =
            serde_json::from_value(catalog).map_err(|_| AgentError::Failed("Invalid RCA worktree list".into()))?;
        let row = catalog.worktrees.into_iter().find(|row| row.path == path).ok_or_else(checkout_missing)?;
        let head = &row.head;
        if !head.is_empty() && (!matches!(head.len(), 40 | 64) || !head.bytes().all(|b| b.is_ascii_hexdigit())) {
            return Err(AgentError::Failed("Invalid checkout HEAD".into()));
        }
        Ok(row)
    }

    fn checkout_note(&self, client: &mut Client, checkout: &str, action: &t3_core::ports::NoteAction) -> Result<Value, AgentError> {
        use t3_core::ports::NoteAction;
        let row = self.selected_checkout(client, checkout)?;
        if row.missing { return Err(checkout_missing()); }
        let root = if row.is_main { "" } else { row.path.as_str() };
        let (workspace, directory) = self.place(client, root, "")?;
        let location = |name: &str| if directory == "." { name.to_owned() } else { under(&directory, name) };
        match action {
            NoteAction::Create => {
                for index in 1..=100 {
                    let name = if index == 1 { "untitled.md".into() } else { format!("untitled-{index}.md") };
                    match client.call(edit::FILE_CREATE, json!({"workspace":workspace,"path":location(&name),"content":""})) {
                        Ok(_) => return Ok(json!({"path":name,"content":""})),
                        Err(e) if e.code == "ALREADY_EXISTS" => continue,
                        Err(e) => return Err(refused(e)),
                    }
                }
                Err(AgentError::Refused { code:"CHANGED".into(), message:"All untitled note names are in use".into() })
            }
            NoteAction::Read { path } => {
                let result = client.call(edit::FILE_READ_CHUNK, json!({"workspace":workspace,"path":location(path),"length":256*1024})).map_err(refused)?;
                let chunk: exec_fs::ChunkResult = serde_json::from_value(result).map_err(unreadable("file.readChunk"))?;
                let text = terminal::b64::decode(&chunk.data).and_then(|b| String::from_utf8(b).ok());
                let content = text.filter(|s| chunk.eof && !s.contains('\0') && s.len() as u64 == chunk.total_bytes)
                    .ok_or_else(|| AgentError::Refused { code:"INVALID_ARGS".into(), message:"This note exceeds the editor limit or is not UTF-8 text".into() })?;
                Ok(json!({"path":path,"content":content}))
            }
            NoteAction::Save { path, previous, content } => {
                client.call(edit::ARTIFACT_WRITE, json!({"workspace":workspace,"path":location(path),"fingerprint":edit::artifact_fingerprint(previous.as_bytes()),"content":content})).map_err(refused)?;
                Ok(json!({"path":path,"content":content}))
            }
        }
    }

    fn checkout_base(&self, client: &mut Client, checkout: &str) -> Result<Option<String>, AgentError> {
        for (key, field) in [
            (format!("orca.worktree.{checkout}"), "baseRef"),
            (format!("orca.repo.{}", self.workspace), "worktreeBaseRef"),
        ] {
            let meta = Self::call(client, agent_api_state::GET, json!({"scope":"orca","key":key}))?;
            if let Some(base) = meta["value"][field].as_str().filter(|s| !s.is_empty()) {
                return Ok(Some(base.to_owned()));
            }
        }
        let value = Self::call(client, git::REFS, json!({"workspace":self.workspace}))?;
        let refs: exec_git::vcs::RefsResult = serde_json::from_value(value).map_err(|_| AgentError::Failed("Invalid Git refs".into()))?;
        let default = ["origin/main", "origin/master", "main", "master"]
            .into_iter()
            .find(|name| refs.refs.iter().any(|r| r.name == *name && r.kind != "tag"));
        Ok(default.map(str::to_owned).or(refs.current))
    }

    fn checkout_pull_request(&self, client: &mut Client, checkout: &str, link: Option<u64>) -> Result<Value, AgentError> {
        let row = self.selected_checkout(client, checkout)?;
        let meta = Self::call(client, agent_api_state::GET, json!({"scope":"orca","key":format!("orca.worktree.{checkout}")}))?;
        let metadata = &meta["value"];
        let number = if let Some(linked) = link.or_else(|| metadata["linkedPR"].as_u64().filter(|n| *n > 0)) {
            Some(linked)
        } else if let Some(branch) = row.branch.as_deref().filter(|s| !s.is_empty()) {
            let listed = Self::call(client, hosted::pr::LIST, json!({"workspace":self.workspace,"provider":"github","head":branch,"state":"all","limit":20}))?;
            let rows = listed["prs"].as_array().ok_or_else(|| AgentError::Failed("Missing pull request list.".into()))?;
            let eligible = || rows.iter().filter(|p| p["head_ref"] == branch && p["number"] != metadata["suppressedGitHubPR"]);
            eligible().find(|p| p["state"] == "open").or_else(|| eligible().next())
                .map(|p| p["number"].as_u64().filter(|n| *n > 0).ok_or_else(|| AgentError::Failed("Invalid pull request number.".into()))).transpose()?
        } else {
            None
        };
        let summary = number.map(|number| -> Result<Value, AgentError> {
            let params = json!({"workspace":self.workspace,"provider":"github","number":number});
            let view = Self::call(client, hosted::pr::VIEW, params.clone())?;
            let p = &view["pr"];
            let malformed = || AgentError::Failed("Invalid pull request summary.".into());
            if p["number"].as_u64() != Some(number) { return Err(malformed()); }
            let title = p["title"].as_str().ok_or_else(malformed)?;
            let state = p["state"].as_str().filter(|s| matches!(*s,"open"|"closed"|"merged")).ok_or_else(malformed)?;
            let checks = if link.is_none() {
                match client.call(hosted::pr::CHECKS, params).map_err(refused) {
                    Ok(result) => Some(json!({"passing":result["pass"],"failing":result["fail"],"pending":result["pending"]})),
                    Err(AgentError::Refused { .. }) => None,
                    Err(other) => return Err(other),
                }
            } else { None };
            Ok(json!({"number":number,"title":title,"state":state,"draft":p["draft"]==true,
                "base":p["base_ref"],"head":p["head_ref"],"author":p["author"],"url":p["url"],
                "description":p["body"],"mergeable":p["mergeable"],"reviewDecision":p["review_decision"],
                "files":p["changed_files"],"additions":p["additions"],"deletions":p["deletions"],"checks":checks}))
        }).transpose()?;
        if let Some(number) = link {
            let mut value = match metadata {
                Value::Object(obj) => obj.clone(),
                Value::Null => serde_json::Map::new(),
                _ => return Err(AgentError::Failed("Invalid checkout metadata.".into())),
            };
            value.insert("linkedPR".into(), json!(number));
            value.remove("suppressedGitHubPR");
            Self::call(client, agent_api_state::SET, json!({"scope":"orca","key":format!("orca.worktree.{checkout}"),"value":value}))?;
        }
        Ok(json!({"branch":row.branch,"pullRequest":summary}))
    }

    fn checkout_changes(&self, client: &mut Client, checkout: &str) -> Result<Value, AgentError> {
        let row = self.selected_checkout(client, checkout)?;
        if row.head.is_empty() || row.head.bytes().all(|b| b == b'0') {
            return Ok(json!({"head":null,"branch":row.branch,"comparison":null}));
        }
        let base = self.checkout_base(client, checkout)?;
        let comparison = match base {
            Some(base) => {
                let mut diff = self.compare_checkout(client, &base, &row.head)?;
                let base_oid = match diff.merge_base.clone() {
                    Some(oid) => oid,
                    None => {
                        // Unrelated histories compare directly to the base commit.
                        let log = Self::call(client, git::HISTORY, json!({"workspace":self.workspace,"rev":base,"limit":1}))?;
                        let pinned = log["commits"][0]["sha"].as_str().ok_or_else(|| AgentError::Failed("Missing comparison base".into()))?;
                        diff = self.compare_checkout(client, pinned, &row.head)?;
                        diff.merge_base.clone().unwrap_or_else(|| pinned.to_owned())
                    }
                };
                let limited = diff.files.len() >= exec_git::vcs::STATUS_FILES_CAP;
                json!({"base":base,"baseOid":base_oid,"ahead":diff.ahead,"files":diff.files,"limited":limited})
            }
            None => Value::Null,
        };
        Ok(json!({"head":row.head,"branch":row.branch,"comparison":comparison}))
    }

    fn compare_checkout(&self, client: &mut Client, base: &str, head: &str) -> Result<exec_git::vcs::CompareResult, AgentError> {
        let value = Self::call(
            client,
            git::COMPARE,
            json!({"workspace":self.workspace,"base":base,"head":head,"limit":1}),
        )?;
        serde_json::from_value(value).map_err(|_| AgentError::Failed("Invalid Git comparison".into()))
    }

    fn checkout_file(
        &self,
        client: &mut Client,
        checkout: &str,
        head: &str,
        base: Option<&str>,
        path: &str,
        old_path: Option<&str>,
    ) -> Result<Value, AgentError> {
        self.selected_checkout(client, checkout)?;
        let mut read = |revision: &str, path: &str| -> Result<Option<exec_git::vcs::BlobResult>, AgentError> {
            let params = json!({"workspace":self.workspace,"rev":revision,"path":path,"max_bytes":8 << 20});
            match client.call(git::BLOB, params) {
                Ok(value) => serde_json::from_value(value)
                    .map(Some)
                    .map_err(|_| AgentError::Failed("Invalid Git blob".into())),
                Err(e) if e.from_service() && e.code == "NOT_FOUND" => Ok(None),
                Err(e) => Err(refused(e)),
            }
        };
        let original = match base {
            Some(base) => read(base, old_path.unwrap_or(path))?,
            None => None,
        };
        let modified = read(head, path)?;
        if original.is_none() && modified.is_none() {
            return Err(AgentError::Refused {
                code: "FILE_NOT_FOUND".into(),
                message: "File absent in both revisions".into(),
            });
        }
        let sides = [original.as_ref(), modified.as_ref()];
        if sides.iter().flatten().any(|blob| blob.truncated) || sides.iter().flatten().map(|blob| blob.size).sum::<u64>() > 4 * 1024 * 1024 {
            return Ok(json!({"kind":"tooLarge"}));
        }
        if sides.iter().flatten().any(|blob| blob.binary) {
            return Ok(json!({"kind":"binary"}));
        }
        let text = |side: Option<exec_git::vcs::BlobResult>| -> Result<Option<String>, AgentError> {
            side.map(|blob| {
                let bytes = terminal::b64::decode(&blob.data).ok_or_else(|| AgentError::Failed("Invalid Git blob encoding".into()))?;
                Ok(String::from_utf8_lossy(&bytes).into_owned())
            })
            .transpose()
        };
        Ok(json!({"kind":"text","original":text(original)?,"modified":text(modified)?}))
    }

    fn checkout_commit(&self, client: &mut Client, checkout: &str, sha: &str) -> Result<Value, AgentError> {
        self.selected_checkout(client, checkout)?;
        // Linked checkouts share this repository's immutable commits and their first parents.
        let log = client
            .call(git::HISTORY, json!({"workspace":self.workspace, "rev":sha, "limit":1}))
            .map_err(|error| {
                if error.from_service() && (error.code == "NOT_FOUND" || error.message.ends_with(&format!("bad object {sha}"))) {
                    return AgentError::Refused {
                        code: "COMMIT_NOT_FOUND".into(),
                        message: "Commit not found".into(),
                    };
                }
                refused(error)
            })?;
        let log: exec_git::vcs::LogResult = serde_json::from_value(log).map_err(|_| AgentError::Failed("Invalid Git history".into()))?;
        let commit = log
            .commits
            .into_iter()
            .next()
            .filter(|c| c.sha == sha)
            .ok_or_else(|| AgentError::Refused {
                code: "COMMIT_NOT_FOUND".into(),
                message: "Commit not found".into(),
            })?;
        let Some(parent) = commit.parents.first() else {
            return Ok(json!({"body":commit.body, "initial":true, "files":[], "limited":false}));
        };
        let diff = self.compare_checkout(client, parent, sha)?;
        let limited = diff.files.len() >= exec_git::vcs::STATUS_FILES_CAP;
        Ok(json!({"body":commit.body, "initial":false, "files":diff.files, "limited":limited}))
    }

    fn checkout_history(
        &self,
        client: &mut Client,
        checkout: &str,
        expected_head: Option<&str>,
        base: Option<&str>,
        offset: u32,
    ) -> Result<Value, AgentError> {
        const PAGE_SIZE: u32 = 50;
        let row = self.selected_checkout(client, checkout)?;
        let head = &row.head;
        let repo = &self.workspace;
        if expected_head.is_some_and(|expected| head != expected) {
            return Err(AgentError::Refused {
                code: "CHECKOUT_HEAD_CHANGED".into(),
                message: "Checkout HEAD changed. Reload its history.".into(),
            });
        }
        let branch = row.branch.filter(|name| !name.is_empty());
        if head.is_empty() || head.bytes().all(|b| b == b'0') {
            return Ok(json!({"branch": branch, "head": null, "commits": [], "nextOffset": null}));
        }
        // Linked worktrees share an object database. A detached checkout is addressable by its own HEAD.
        let rev = base.map_or_else(|| head.clone(), |base| format!("{base}..{head}"));
        let log = Self::call(
            client,
            git::HISTORY,
            json!({"workspace": repo, "rev": rev, "skip": offset, "limit": PAGE_SIZE}),
        )?;
        let log: exec_git::vcs::LogResult = serde_json::from_value(log).map_err(|_| AgentError::Failed("Invalid Git history".into()))?;
        let count = log.commits.len();
        if count > PAGE_SIZE as usize || (log.truncated && count != PAGE_SIZE as usize) {
            return Err(AgentError::Failed("Invalid Git history page size".into()));
        }
        let next = if log.truncated {
            Some(
                offset
                    .checked_add(PAGE_SIZE)
                    .ok_or_else(|| AgentError::Failed("Git history offset exceeded".into()))?,
            )
        } else {
            None
        };
        let items: Vec<_> = log
            .commits
            .into_iter()
            .map(|commit| {
                json!({
                    "sha": commit.sha, "shortSha": commit.short, "subject": commit.subject,
                    "author": commit.author_name, "authoredAtMs": commit.authored_ms, "parent": commit.parents.first()
                })
            })
            .collect();
        Ok(json!({"branch": branch, "head": head, "commits": items, "nextOffset": next}))
    }

    fn mobile_placements(&self, client: &mut Client) -> Result<Vec<MobilePlacement>, AgentError> {
        let listed = Self::call(client, worktrees::WORKTREE_LIST, json!({"workspace": self.workspace}))?;
        let registrations = Self::call(client, REPO_LIST, json!({}))?;
        let rows = listed["worktrees"].as_array().ok_or_else(|| AgentError::Failed("Missing RCA worktrees.".into()))?;
        let repos = registrations["repos"].as_array().ok_or_else(|| AgentError::Failed("Missing RCA repositories.".into()))?;
        let executor = repos.iter().find(|r| r["workspace"] == self.workspace).and_then(|r| r["executor"].as_str());
        let by_path: std::collections::HashMap<_, _> = repos.iter()
            .filter(|r| r["kind"] == "worktree" && executor.is_some_and(|owner| r["executor"] == owner))
            .filter_map(|r| Some((r["path"].as_str()?, r["workspace"].as_str()?)))
            .collect();
        rows.iter().map(|row| {
            let path = row["path"].as_str().ok_or_else(|| AgentError::Failed("Missing checkout path.".into()))?;
            let main = row["is_main"] == true;
            let owner = if main { Some(self.workspace.as_str()) } else { by_path.get(path).copied() };
            Ok(MobilePlacement {
                missing: row["missing"] == true,
                path: path.into(), main, owner: owner.map(str::to_owned),
                branch: row["branch"].as_str().filter(|b| !b.is_empty()).map(str::to_owned),
            })
        }).collect()
    }

    fn mobile_inventory(&self, client: &mut Client) -> Result<MobileInventory, AgentError> {
        let placements = self.mobile_placements(client)?;
        let snapshot = Self::call(client, agent_api_state::watch::SUBSCRIBE, json!({"scope":"orca"}))?;
        let metadata = snapshot["values"].as_object().ok_or_else(|| AgentError::Failed("Missing RCA workspace metadata.".into()))?;
        let pane_list = Self::call(client, agent_api_service::panes::LIST, json!({}))?;
        let panes: Vec<PaneView> = serde_json::from_value(pane_list["panes"].clone()).map_err(|_| AgentError::Failed("Invalid RCA pane list.".into()))?;
        let checkouts = placements.into_iter().map(|p| {
            let meta = metadata.get(&p.metadata_key(&self.workspace));
            (p.row(&self.workspace, meta), p.owner)
        }).collect();
        Ok(MobileInventory { checkouts, panes })
    }

    fn thread_checkout(&self, client: &mut Client, thread: &str, path: Option<&str>, selected: Option<&str>) -> Result<Value, AgentError> {
        let selected_path = selected.map(|id| self.checkout_path(id)).transpose()?;
        if selected_path.zip(path).is_some_and(|(selected, projected)| selected != projected) { return Err(checkout_missing()); }
        let found = Self::call(client, agent_api_agent::THREAD_GET, json!({"thread":thread}))?;
        let workspace = found["thread"]["workspace"].as_str().ok_or_else(checkout_missing)?;
        let exact = path.or(selected_path);
        let row = self.mobile_placements(client)?.into_iter().find(|p| {
            if p.missing { return false; }
            if let Some(exact) = exact {
                p.path == exact && (p.owner.as_deref().unwrap_or(&self.workspace) == workspace ||
                    (path.is_some() && workspace == self.workspace))
            } else {
                p.owner.as_deref() == Some(workspace)
            }
        }).ok_or_else(checkout_missing)?;
        let meta = Self::call(client, agent_api_state::GET, json!({"scope":"orca","key":row.metadata_key(&self.workspace)}))?;
        Ok(row.row(&self.workspace, meta.get("value")))
    }

    fn mobile_worktrees(&self, client: &mut Client) -> Result<Value, AgentError> {
        let inventory = self.mobile_inventory(client)?;
        let mut unread = std::collections::HashMap::<String, Vec<String>>::new();
        for pane in inventory.panes.into_iter().filter(|p| p.unread && p.listed && p.state != PaneState::Idle) {
            if let Some(workspace) = pane.workspace { unread.entry(workspace).or_default().push(pane.pane); }
        }
        let checkouts = inventory.checkouts.into_iter().map(|(mut row, owner)| {
            let alerts = owner.as_ref().and_then(|key| unread.get(key)).map(Vec::as_slice).unwrap_or_default();
            row["unread"] = json!(!alerts.is_empty() || row["unread"] == true);
            row["unreadPanes"] = json!(alerts);
            row
        }).collect::<Vec<_>>();
        Ok(json!({"repo": self.workspace, "checkouts": checkouts}))
    }

    fn mobile_agents(&self, client: &mut Client) -> Result<Value, AgentError> {
        let MobileInventory { checkouts, mut panes } = self.mobile_inventory(client)?;
        let owned: std::collections::HashMap<_, _> = checkouts.into_iter()
            .filter_map(|(checkout, owner)| Some((owner?, checkout))).collect();
        panes.sort_by(|a, b| b.sort_at_ms.cmp(&a.sort_at_ms).then_with(|| a.thread.cmp(&b.thread)));
        let mut places = serde_json::Map::new();
        let mut agents = Vec::new();
        for pane in panes.into_iter().filter(|p| p.listed && p.state != PaneState::Idle) {
            let Some(workspace) = pane.workspace else { continue };
            let Some(checkout) = owned.get(&workspace) else { continue };
            places.entry(workspace.clone()).or_insert_with(|| json!({"id":checkout["id"],"name":checkout["name"]}));
            agents.push(json!({"threadId":pane.thread,"title":pane.title,"status":pane.state,
                "updatedAt":pane.sort_at_ms,"unread":pane.unread,"clearable":pane.clearable,"workspace":workspace}));
        }
        Ok(json!({"repo": self.workspace, "checkouts":places, "agents":agents}))
    }

    /// Use a separate workspace only when RCA registered the Git worktree.
    fn place(&self, client: &mut Client, root: &str, path: &str) -> Result<(String, String), AgentError> {
        let own = |path: &str| (self.workspace.clone(), under(root, path));
        if root.is_empty() {
            return Ok(own(path));
        }
        let listed = client.call(worktrees::WORKTREE_LIST, json!({"workspace": self.workspace})).unwrap_or_default();
        let found = listed["worktrees"].as_array().into_iter().flatten().filter(|w| w["is_main"] != true).filter_map(|w| w["path"].as_str()).filter(|p| root == *p || root.strip_prefix(p).is_some_and(|rest| rest.starts_with('/'))).max_by_key(|p| p.len());
        let Some(dir) = found else { return Ok(own(path)) };
        let registered = Self::call(client, REPO_LIST, json!({}))?;
        let workspace = registered["repos"].as_array().into_iter().flatten()
            .find(|row| row["kind"] == "worktree" && row["path"] == dir)
            .and_then(|row| row["workspace"].as_str());
        Ok(workspace.map_or_else(|| own(path), |workspace| {
            (workspace.into(), under(root[dir.len()..].trim_start_matches('/'), path))
        }))
    }

    /// The workspace a turn runs in: the one the thread's worktree is, else the executor's own (a worktree the agent API does not list is no workspace
    /// of its own).
    fn turn_workspace(&self, client: &mut Client, worktree: Option<&str>) -> String {
        match worktree {
            Some(path) => self.place(client, path, "").map(|(workspace, _)| workspace).unwrap_or_else(|_| self.workspace.clone()),
            None => self.workspace.clone(),
        }
    }

    /// A `file.*` call on `root`/`path` of the workspace.
    fn file(&self, method: Method, root: &str, path: &str) -> Result<Value, AgentError> {
        let mut client = self.connect()?;
        let (workspace, path) = self.place(&mut client, root, path)?;
        Self::call(&mut client, method, json!({"workspace": workspace, "path": path}))
    }

    /// The method and params of the requests that act in a directory of the project: placed in the workspace the directory is in.
    fn placed(&self, client: &mut Client, op: &Op) -> Result<Option<(Method, Value)>, AgentError> {
        // An absolute path with no project is the executor's own, which it confines (browsing, metadata)
        let at = |client: &mut Client, root: &str, path: &str| {
            if root.is_empty() && path.starts_with('/') {
                Ok((self.workspace.clone(), path.to_string()))
            } else {
                self.place(client, root, path)
            }
        };
        // A path to give the executor only when the directory is below the workspace's own
        let below = |path: String| Some(path).filter(|p| p != ".");
        Ok(Some(match op {
            Op::OpenServerPreview {root, params} => {
                let mut params = params.clone();
                params["workspace"] = json!(self.place(client, root, "")?.0);
                (preview::sessions::OPEN, params)
            }
            Op::ProviderCommands { root, driver } => {
                let (workspace, path) = self.place(client, root, "")?;
                let mut params = json!({"workspace": workspace, "driver": driver});
                if let Some(path) = below(path) {
                    params["path"] = json!(path);
                }
                (catalog::COMMANDS_LIST, params)
            }
            Op::FileStat { root, path } => {
                let (workspace, path) = at(client, root, path)?;
                (edit::FILE_STAT, json!({"workspace": workspace, "path": path}))
            }
            Op::FileWrite { root, path, content } => {
                let (workspace, path) = at(client, root, path)?;
                (edit::FILE_WRITE, json!({"workspace": workspace, "path": path, "content": content, "create_dirs": true}))
            }
            Op::FileBrowse { root, path, show_hidden } => {
                let (workspace, path) = at(client, root, path)?;
                (edit::FILE_BROWSE, json!({"workspace": workspace, "path": path, "show_hidden": show_hidden}))
            }
            Op::WorkflowScript { path, length } => (edit::FILE_READ_CHUNK, json!({"workspace": self.workspace, "path": path, "length": length, "scope": "claudeProjects"})),
            Op::FileMkdir { path, parents } => (edit::FILE_MKDIR, json!({"workspace": self.workspace, "path": path, "parents": parents})),
            Op::GitInit { path, initial_branch, files, message } => {
                let mut params = json!({"workspace": self.workspace, "path": path});
                if let Some(branch) = initial_branch {
                    params["initial_branch"] = json!(branch);
                }
                if !files.is_empty() {
                    params["files"] = files.iter().map(|(path, content)| json!({"path": path, "content": content})).collect::<Vec<_>>().into();
                }
                if let Some(message) = message {
                    params["message"] = json!(message);
                }
                (git::INIT, params)
            }
            Op::FileSearch { root, query, limit } => {
                let (workspace, path) = self.place(client, root, "")?;
                let mut params = json!({"workspace": workspace, "query": query, "limit": limit});
                if let Some(path) = below(path) {
                    params["path"] = json!(path);
                }
                (edit::FILE_SEARCH, params)
            }
            Op::TurnDiff { root, base } => {
                let (workspace, path) = self.place(client, root, "")?;
                let mut params = json!({"workspace": workspace, "base": base});
                if let Some(path) = below(path) {
                    params["path"] = json!(path);
                }
                (edit::TURN_DIFF, params)
            }
            Op::ThreadDiff { root } => {
                let (workspace, path) = self.place(client, root, "")?;
                let mut params = json!({"workspace": workspace});
                if let Some(path) = below(path) {
                    params["path"] = json!(path);
                }
                (edit::THREAD_DIFF, params)
            }
            Op::WorktreeCreate { root, worktree, branch, base } => {
                let (workspace, _) = self.place(client, root, "")?;
                let mut params = json!({"workspace": workspace, "worktree": worktree, "branch": branch});
                if let Some(base) = base {
                    params["base"] = json!(base);
                }
                (worktrees::WORKTREE_CREATE, params)
            }
            Op::WorktreeRemove { root } => {
                let (workspace, _) = self.place(client, root, "")?;
                (worktrees::WORKTREE_REMOVE, json!({"workspace": workspace, "force": true}))
            }
            Op::WorktreeSetupCancel { root } => {
                let (workspace, _) = self.place(client, root, "")?;
                (worktrees::SETUP_CANCEL, json!({"workspace": workspace}))
            }
            Op::TerminalOpen { terminal, cwd, cols, rows, env, .. } => {
                let (workspace, path) = self.place(client, cwd, "")?;
                let mut params = json!({"workspace": workspace, "terminal": terminal, "cwd": path});
                for (key, size) in [("cols", cols), ("rows", rows)] {
                    if let Some(size) = size {
                        params[key] = json!(size);
                    }
                }
                if !env.is_empty() {
                    params["env"] = env.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<serde_json::Map<_, _>>().into();
                }
                (terminal::OPEN, params)
            }
            Op::TerminalWrite { root, terminal: name, data } => (terminal::WRITE, json!({"workspace": self.place(client, root, "")?.0, "terminal": name, "data": data})),
            Op::TerminalResize { root, terminal: name, cols, rows } => (terminal::RESIZE, json!({"workspace": self.place(client, root, "")?.0, "terminal": name, "cols": cols, "rows": rows})),
            Op::TerminalClear { root, terminal: name } => (terminal::CLEAR, json!({"workspace": self.place(client, root, "")?.0, "terminal": name})),
            Op::TerminalClose { root, terminal: name } => (terminal::CLOSE, json!({"workspace": self.place(client, root, "")?.0, "terminal": name})),
            Op::GitStatus { root } => (git::STATUS, json!({"workspace": self.place(client, root, "")?.0})),
            Op::GitUpstream { root } => (git::UPSTREAM_STATUS, json!({"workspace": self.place(client, root, "")?.0})),
            Op::GitDiff { root, base, head, path } => {
                let mut params = json!({"workspace": self.place(client, root, "")?.0});
                for (key, value) in [("base", base), ("head", head), ("path", path)] {
                    if let Some(value) = value {
                        params[key] = json!(value);
                    }
                }
                (git::DIFF, params)
            }
            Op::GitRefs { root, kind, limit } => {
                let mut params = json!({"workspace": self.place(client, root, "")?.0, "limit": limit});
                if let Some(kind) = kind {
                    params["kind"] = json!(kind);
                }
                (git::REFS, params)
            }
            Op::GitHistory { root, rev, limit } => {
                let mut params = json!({"workspace": self.place(client, root, "")?.0, "limit": limit});
                if let Some(rev) = rev {
                    params["rev"] = json!(rev);
                }
                (git::HISTORY, params)
            }
            Op::GitCheckout { root, target, track } => (git::CHECKOUT, json!({"workspace": self.place(client, root, "")?.0, "ref": target, "track": track})),
            Op::GitCreateRef { root, name, from, checkout } => (git::CREATE_REF, strip_nulls(json!({"workspace": self.place(client, root, "")?.0, "name": name, "from": from, "checkout": checkout}))),
            Op::GitFetch { root, refspecs } => (git::FETCH, json!({"workspace": self.place(client, root, "")?.0, "refspecs": refspecs})),
            Op::GitCompare { root, base, head, limit } => (git::COMPARE, json!({"workspace": self.place(client, root, "")?.0, "base": base, "head": head, "limit": limit})),
            Op::WorktreeList { root } => (worktrees::WORKTREE_LIST, json!({"workspace": self.place(client, root, "")?.0})),
            Op::GitPull { root } => (git::PULL, json!({"workspace": self.place(client, root, "")?.0})),
            Op::GitBlob { root, rev, path } => (git::BLOB, json!({"workspace": self.place(client, root, "")?.0, "rev": rev, "path": path})),
            Op::GitStage { root, paths } => (git::STAGE, json!({"workspace": self.place(client, root, "")?.0, "paths": paths})),
            Op::GitCommit { root, message } => (git::COMMIT, json!({"workspace": self.place(client, root, "")?.0, "message": message})),
            Op::GitIgnored { root } => (git::IGNORED, json!({"workspace": self.place(client, root, "")?.0})),
            Op::GitPush { root, set_upstream } => (git::PUSH, json!({"workspace": self.place(client, root, "")?.0, "set_upstream": set_upstream})),
            Op::Hosted { method, root, params } => {
                let found = hosted::all_methods().into_iter().find(|m| m.name() == *method).ok_or_else(|| AgentError::Failed(format!("{method} is no hosted method of the agent API")))?;
                let mut params = params.clone();
                params["workspace"] = json!(self.place(client, root, "")?.0);
                (found, params)
            }
            Op::Schedule { method, root: Some(root), params } => {
                let found = schedules::METHODS.iter().find(|m| m.name() == *method).copied().ok_or_else(|| AgentError::Failed(format!("{method} is no schedules method of the agent API")))?;
                let mut params = params.clone();
                params["workspace"] = json!(self.place(client, root, "")?.0);
                (found, params)
            }
            _ => return Ok(None),
        }))
    }
}

/// The method an `Op` is and its params, built from the contract's own types.
fn request(op: &Op) -> Result<(Method, Value), AgentError> {
    let same = |s: &str| s.to_string();
    Ok(match op {
        Op::ApprovalAnswer { thread, approval, decision } => {
            let decision: Decision = serde_json::from_value(json!(decision)).map_err(|e| AgentError::Failed(format!("{decision} is no decision of the agent API: {e}")))?;
            (requests::APPROVAL_ANSWER, requests::ApprovalAnswerParams { thread: same(thread), approval: same(approval), decision }.to_value())
        }
        Op::InputAnswer { thread, request, answer } => (requests::INPUT_ANSWER, requests::InputAnswerParams { thread: same(thread), request: same(request), answer: same(answer) }.to_value()),
        Op::InputDismiss { thread, request } => (requests::INPUT_DISMISS, requests::InputDismissParams { thread: same(thread), request: same(request) }.to_value()),
        Op::QueueAdd { thread, item, prompt, model } => (queue::QUEUE_ADD, queue::AddParams { thread: same(thread), item: Some(same(item)), prompt: same(prompt), model: model.clone(), ..Default::default() }.to_value()),
        Op::QueueEdit { thread, item, prompt } => (queue::QUEUE_EDIT, queue::EditParams { thread: same(thread), item: same(item), prompt: same(prompt) }.to_value()),
        Op::QueueMove { thread, item, before } => (queue::QUEUE_MOVE, queue::MoveParams { thread: same(thread), item: same(item), before: before.clone() }.to_value()),
        Op::QueueDelete { thread, item } => (queue::QUEUE_DELETE, queue::DeleteParams { thread: same(thread), item: same(item) }.to_value()),
        Op::QueueResume { thread, item } => (queue::QUEUE_RESUME, queue::ResumeParams { thread: same(thread), item: item.clone() }.to_value()),
        Op::QueueList { thread } => (queue::QUEUE_LIST, queue::ListParams { thread: same(thread) }.to_value()),
        Op::TurnSteer { thread, turn, item, prompt } => (TURN_STEER, queue::SteerParams { thread: same(thread), turn: turn.clone(), prompt: prompt.clone(), item: item.clone() }.to_value()),
        Op::ThreadArchive { thread, archived } => (lifecycle::THREAD_ARCHIVE, lifecycle::ArchiveParams { thread: same(thread), archived: Some(*archived) }.to_value()),
        Op::ThreadDelete { thread, only_if_unused } => (lifecycle::THREAD_DELETE, lifecycle::DeleteParams { thread: same(thread), only_if_unused: Some(*only_if_unused) }.to_value()),
        Op::ThreadFork { thread, fork, turn, title } => (lifecycle::THREAD_FORK, lifecycle::ForkParams { thread: same(thread), fork: Some(same(fork)), turn: turn.clone(), title: title.clone() }.to_value()),
        Op::ThreadSettle { thread, state, until_ms, at_ms } => {
            let state: SettleState = serde_json::from_value(json!(state)).map_err(|e| AgentError::Failed(format!("{state} is no settle state of the agent API: {e}")))?;
            (lifecycle::THREAD_SETTLE, lifecycle::SettleParams { thread: same(thread), state, until_ms: *until_ms, at_ms: *at_ms }.to_value())
        }
        Op::ThreadSearch { query, limit } => (lifecycle::THREAD_SEARCH, lifecycle::SearchParams { query: same(query), limit: Some(*limit) }.to_value()),
        Op::ThreadList => (agent_api_agent::THREAD_LIST, agent_api_agent::ListParams::default().to_value()),
        Op::ThreadFamily { thread } => (lifecycle::THREAD_FAMILY, lifecycle::FamilyParams { thread: same(thread) }.to_value()),
        Op::ThreadForceStop { thread, turn } => (lifecycle::THREAD_FORCE_STOP, lifecycle::ForceStopParams { thread: same(thread), turn: turn.clone() }.to_value()),
        Op::ItemGet { thread, seq } => (lifecycle::ITEM_GET, lifecycle::ItemGetParams { thread: same(thread), seq: *seq }.to_value()),
        Op::WorkflowStop { thread, turn, workflow } => (lifecycle::WORKFLOW_STOP, lifecycle::WorkflowStopParams { thread: same(thread), turn: same(turn), workflow: workflow.clone() }.to_value()),
        Op::ThreadMergeBack { thread, into, turn } => (lifecycle::THREAD_MERGE_BACK, lifecycle::MergeBackParams { thread: same(thread), into: same(into), turn: turn.clone() }.to_value()),
        Op::ThreadRollback { thread, turn, restore_files } => (lifecycle::THREAD_ROLLBACK, lifecycle::RollbackParams { thread: same(thread), turn: same(turn), restore_files: Some(*restore_files) }.to_value()),
        Op::ThreadDetach { thread, session } => (lifecycle::THREAD_DETACH, lifecycle::DetachParams { thread: same(thread), session: same(session), reason: None }.to_value()),
        Op::TurnRetry { thread, turn } => (lifecycle::TURN_RETRY, lifecycle::RetryParams { thread: same(thread), turn: same(turn) }.to_value()),
        Op::ThreadShelf { shelf, before, limit } => {
            let shelf: Shelf = serde_json::from_value(json!(shelf)).map_err(|e| AgentError::Failed(format!("{shelf} is no shelf of the agent API: {e}")))?;
            let before = before.as_ref().map(|(at_ms, thread)| Cursor { at_ms: *at_ms, thread: thread.clone() });
            (lifecycle::THREAD_SHELF, lifecycle::ShelfParams { shelf, before, limit: Some(*limit) }.to_value())
        }
        Op::ClientActivity(a) => {
            let raw = json!({"client": a.client, "kind": a.kind, "visible": a.visible, "focused": a.focused, "recently_interacted": a.recently_interacted, "app_state": a.app_state, "low_power_mode": a.low_power_mode, "scopes": a.scopes, "ttl_ms": a.ttl_ms, "observed_at_ms": a.observed_at_ms});
            let params = ClientActivityParams::parse(&strip_nulls(raw)).map_err(|e| AgentError::Failed(format!("not a client activity of the agent API: {}", e.0)))?;
            (CLIENT_ACTIVITY, params.to_value())
        }
        Op::AccountsList => (accounts::PROVIDER_LIST, json!({})),
        Op::AccountsRefresh { provider } => (accounts::PROVIDER_REFRESH, strip_nulls(json!({"provider": provider}))),
        Op::AccountsSelect { provider } => (accounts::PROVIDER_SELECT, json!({"provider": provider})),
        Op::AuthStart { provider, method } => (accounts::AUTH_START, strip_nulls(json!({"provider": provider, "method": method}))),
        Op::AuthRespond { auth, text } => (accounts::AUTH_RESPOND, json!({"auth": auth, "text": text})),
        Op::AuthComplete { auth, timeout_ms } => (accounts::AUTH_COMPLETE, json!({"auth": auth, "timeout_ms": timeout_ms})),
        Op::AuthCancel { auth } => (accounts::AUTH_CANCEL, json!({"auth": auth})),
        Op::AccountsLogout { provider } => (accounts::PROVIDER_LOGOUT, json!({"provider": provider})),
        Op::UsageSummary => (accounts::USAGE_SUMMARY, json!({})),
        Op::ResetCredit { provider } => (accounts::RESET_CREDIT, json!({"provider": provider})),
        Op::ServiceLogs { limit, min_level } => (agent_api_service::LOGS, json!({"limit": limit, "min_level": min_level})),
        Op::Schedule { method, root: None, params } => {
            let found = schedules::METHODS.iter().find(|m| m.name() == *method).copied().ok_or_else(|| AgentError::Failed(format!("{method} is no schedules method of the agent API")))?;
            (found, params.clone())
        }
        Op::PreviewPage {method, params} => {
            let found = preview::rendering::METHODS.iter().find(|m| m.name() == *method).copied().ok_or_else(|| AgentError::Failed(format!("Unknown preview page operation {method}")))?;
            (found, params.clone())
        }
        Op::Preview { method, params } => {
            let found = preview::sessions::METHODS.iter().chain(preview::watch::METHODS).find(|m| m.name() == *method).copied().ok_or_else(|| AgentError::Failed(format!("{method} is no preview.sessions method of the agent API")))?;
            (found, params.clone())
        }
        Op::Attachment { method, params } => {
            let found = attachments::METHODS.iter().find(|m| m.name() == *method).copied().ok_or_else(|| AgentError::Failed(format!("{method} is no attachments method of the agent API")))?;
            (found, params.clone())
        }
        Op::Speech { method, params } => {
            let found = [catalog::SPEECH_STATUS, catalog::SPEECH_TRANSCRIBE].into_iter().find(|m| m.name() == *method).ok_or_else(|| AgentError::Failed(format!("{method} is no speech method of the agent API")))?;
            (found, params.clone())
        }
        Op::Catalog { method, params } => {
            let found = [catalog::SESSIONS_LIST].into_iter().find(|m| m.name() == *method).ok_or_else(|| AgentError::Failed(format!("{method} is no host-reading method of the agent API")))?;
            (found, params.clone())
        }
        Op::WorkspaceCreate { workspace, path } => (WORKSPACE_CREATE, json!({"workspace": workspace, "path": path})),
        Op::WorkspaceRemove { workspace } => (WORKSPACE_REMOVE, json!({"workspace": workspace})),
        Op::StateGet { scope, key } => (agent_api_state::GET, json!({"scope": scope, "key": key})),
        Op::StateCompareAndSet { scope, key, expected, value } => (agent_api_state::COMPARE_AND_SET, json!({"scope": scope, "key": key, "expected": expected, "value": value})),
        Op::StateSet { scope, key, value } => (agent_api_state::SET, json!({"scope": scope, "key": key, "value": value})),
        Op::CheckoutNote { .. } | Op::OpenServerPreview { .. } | Op::CheckoutPullRequest { .. } | Op::CheckoutBranches { .. } | Op::SwitchCheckoutBranch { .. } | Op::CheckoutFile { .. } | Op::CheckoutChanges { .. } | Op::CheckoutCommit { .. } | Op::CheckoutHistory { .. } | Op::ReadPanes { .. } | Op::MobileWorktrees | Op::MobileAgents | Op::MobileThreadCheckout { .. } | Op::ProviderCommands { .. } | Op::FileStat { .. } | Op::WorkflowScript { .. } | Op::FileWrite { .. } | Op::FileBrowse { .. } | Op::FileMkdir { .. } | Op::GitInit { .. } | Op::FileSearch { .. } | Op::TurnDiff { .. } | Op::ThreadDiff { .. } | Op::WorktreeCreate { .. } | Op::WorktreeRemove { .. } | Op::WorktreeSetupCancel { .. } | Op::TerminalOpen { .. } | Op::TerminalWrite { .. } | Op::TerminalResize { .. } | Op::TerminalClear { .. } | Op::TerminalClose { .. } | Op::GitStatus { .. } | Op::GitUpstream { .. } | Op::GitDiff { .. } | Op::GitRefs { .. } | Op::GitHistory { .. } | Op::GitCheckout { .. } | Op::GitCreateRef { .. } | Op::GitFetch { .. } | Op::GitCompare { .. } | Op::WorktreeList { .. } | Op::GitPull { .. } | Op::GitBlob { .. } | Op::GitStage { .. } | Op::GitCommit { .. } | Op::GitPush { .. } | Op::GitIgnored { .. } | Op::Hosted { .. } | Op::Schedule { root: Some(_), .. } => {
            return Err(AgentError::Failed(format!("{} is placed in a workspace before it is sent", op.method())));
        }
    })
}

fn is_accounts(op: &Op) -> bool {
    accounts::METHODS.iter().any(|m| m.name() == op.method())
}

/// The requests whose refusals keep the service's own codes (`CLI_NOT_INSTALLED`, `PROVIDER_NOT_SIGNED_IN`, the executor's).
fn is_hosted(op: &Op) -> bool {
    matches!(op, Op::PreviewPage { .. } | Op::OpenServerPreview { .. } | Op::Hosted { .. } | Op::Schedule { .. } | Op::Attachment { .. } | Op::Preview { .. } | Op::Speech { .. } | Op::Catalog { .. })
}

/// The requests about directories and workspaces whose refusals keep the codes the app tells apart (`ALREADY_EXISTS`, `PATH_OUTSIDE_ROOT`,
/// `WORKSPACE_EXISTS`).
fn keeps_code(op: &Op) -> bool {
    matches!(op, Op::FileMkdir { .. } | Op::WorkflowScript { .. } | Op::FileBrowse { .. } | Op::WorkspaceCreate { .. } | Op::WorkspaceRemove { .. })
}

fn strip_nulls(mut value: Value) -> Value {
    if let Some(map) = value.as_object_mut() {
        map.retain(|_, v| !v.is_null());
    }
    value
}

fn unreadable(method: &str) -> impl Fn(serde_json::Error) -> AgentError + '_ {
    move |e| AgentError::Failed(format!("rca answered {method} with {e}"))
}

impl AgentApi for RcaAgent {
    fn catalog_providers(&self) -> Result<Vec<CatalogProvider>, AgentError> {
        if let Some(providers) = self.providers.get() {
            return Ok(providers.clone());
        }
        let mut client = self.connect()?;
        let detected = Self::call(&mut client, catalog::AGENTS_DETECT, json!({"executor": self.workspace}))?;
        let mut providers = Vec::new();
        for row in detected["agents"].as_array().into_iter().flatten() {
            let Some(driver) = row["driver"].as_str().filter(|d| catalog::DRIVERS.contains(d)) else { continue };
            if row["installed"] != true { continue; }
            let listing = Self::call(&mut client, catalog::MODELS_LIST, json!({"driver": driver}))?;
            let models = listing["models"].as_array().into_iter().flatten().filter_map(|model| Some(CatalogModel {
                slug: model["id"].as_str()?.into(),
                name: model["label"].as_str()?.into(),
            })).collect();
            providers.push(CatalogProvider { driver: driver.into(), signed_in: row["signed_in"].as_bool(), models });
        }
        if !providers.is_empty() {
            let _ = self.providers.set(providers.clone());
        }
        Ok(providers)
    }

    fn run(&self, op: &Op) -> Result<Value, AgentError> {
        let mut client = self.connect()?;
        if let Op::CheckoutNote { checkout, action } = op {
            return self.checkout_note(&mut client, checkout, action);
        }
        if let Op::CheckoutPullRequest { checkout, link } = op {
            return self.checkout_pull_request(&mut client, checkout, *link);
        }
        if let Op::CheckoutBranches { checkout } = op {
            let row = self.selected_checkout(&mut client, checkout)?;
            let value = Self::call(&mut client, git::REFS, json!({"workspace":self.workspace,"kind":"branch","limit":exec_git::vcs::REFS_MAX}))?;
            let refs: exec_git::vcs::RefsResult = serde_json::from_value(value).map_err(|_| AgentError::Failed("Invalid Git refs".into()))?;
            return Ok(json!({"current":row.branch,"branches":refs.refs.into_iter().map(|item|item.name).collect::<Vec<_>>(), "truncated":refs.truncated}));
        }
        if let Op::SwitchCheckoutBranch { checkout, branch } = op {
            let path = self.checkout_path(checkout)?;
            return client.call(git::SWITCH_WORKTREE_BRANCH, json!({"workspace":self.workspace,"path":path,"branch":branch})).map_err(refused);
        }
        if let Op::CheckoutChanges { checkout } = op {
            return self.checkout_changes(&mut client, checkout);
        }
        if let Op::CheckoutFile {checkout, head, base, path, old_path} = op {
            return self.checkout_file(&mut client, checkout, head, base.as_deref(), path, old_path.as_deref());
        }
        if let Op::CheckoutCommit { checkout, sha } = op {
            return self.checkout_commit(&mut client, checkout, sha);
        }
        if let Op::CheckoutHistory { checkout, expected_head, base, offset } = op {
            return self.checkout_history(&mut client, checkout, expected_head.as_deref(), base.as_deref(), *offset);
        }
        if let Op::MobileThreadCheckout { thread, path, selected } = op {
            return self.thread_checkout(&mut client, thread, path.as_deref(), selected.as_deref());
        }
        if matches!(op, Op::MobileAgents) { return self.mobile_agents(&mut client); }
        if matches!(op, Op::MobileWorktrees) { return self.mobile_worktrees(&mut client); }
        if let Op::ReadPanes { panes } = op {
            for keys in panes.chunks(agent_api_service::panes::MAX_MARKED) {
                Self::call(&mut client, agent_api_service::panes::MARK, json!({"panes":keys,"mark":"read"}))?;
            }
            return Ok(Value::Null);
        }
        let (method, params) = match self.placed(&mut client, op)? {
            Some(placed) => placed,
            None => request(op)?,
        };
        if op.method().starts_with("terminal.") || op.method().starts_with("git.") || is_accounts(op) || is_hosted(op) || keeps_code(op) {
            let mut answer = client.call(method, params).map_err(refused)?;
            if matches!(op, Op::GitBlob { .. }) {
                let bytes = terminal::b64::decode(answer["data"].as_str().unwrap_or_default()).ok_or_else(|| AgentError::Failed("rca answered git.blob with data that is no base64".into()))?;
                answer["text"] = json!(String::from_utf8_lossy(&bytes));
            }
            return Ok(answer);
        }
        Self::call(&mut client, method, params)
    }

    fn ensure_thread(&self, thread: &AgentThread) -> Result<u64, AgentError> {
        let mut client = self.connect()?;
        let workspace = self.turn_workspace(&mut client, thread.worktree.as_deref());
        let params = CreateParams { thread: Some(thread.thread_id.clone()), title: None, workspace: Some(workspace), driver: Some(thread.instance_id.clone()), model: thread.model.clone() };
        let created = Self::call(&mut client, THREAD_CREATE, params.to_value())?;
        Ok(ThreadView::from_result(&created).map_err(|e| AgentError::Failed(format!("rca answered thread.create with {}", e.0)))?.last_seq)
    }

    fn start_turn(&self, turn: &AgentTurn) -> Result<String, AgentError> {
        let mut client = self.connect()?;
        let modes = |mode: &str| Some(mode.to_string()).filter(|m| !m.is_empty());
        let start = StartParams {
            thread: turn.thread_id.clone(),
            workspace: self.turn_workspace(&mut client, turn.worktree.as_deref()),
            driver: turn.instance_id.clone(),
            model: turn.model.clone(),
            prompt: turn.prompt.clone(),
            attachments: Some(turn.attachments.clone()).filter(|ids| !ids.is_empty()),
            runtime_mode: modes(&turn.runtime_mode),
            interaction_mode: modes(&turn.interaction_mode),
        };
        Ok(serde_json::from_value::<StartResult>(Self::call(&mut client, TURN_START, start.to_value())?).map_err(unreadable("turn.start"))?.turn)
    }

    fn interrupt(&self, thread_id: &str) -> Result<(), AgentError> {
        let mut client = self.connect()?;
        match client.call(TURN_INTERRUPT, InterruptParams { thread: thread_id.to_string(), turn: None }.to_value()) {
            // A thread that runs nothing, or that rca never heard of, has nothing to stop
            Err(e) if e.code == thread_code::NO_TURN || e.code == thread_code::THREAD_NOT_FOUND => Ok(()),
            other => other.map(drop).map_err(failed),
        }
    }

    fn follow_until(&self, thread_id: &str, after_seq: u64, stop: &AtomicBool, sink: &mut dyn FnMut(AgentEvent) -> Follow) -> Result<(), AgentError> {
        let mut client = self.connect()?;
        let params = SubscribeParams { thread: thread_id.to_string(), after_seq };
        client.subscribe(Subscribe::new(thread_topic(thread_id), THREAD_SUBSCRIBE, params.to_value(), "after_seq")).map_err(failed)?;
        while !stop.load(Ordering::SeqCst) {
            let Some(event) = client.next_event(QUIET).map_err(failed)? else { continue };
            if sink(agent_event(&event)?) == Follow::Stop {
                return Ok(());
            }
        }
        Ok(())
    }

    fn history(&self, thread_id: &str, after_seq: u64, limit: u32) -> Result<History, AgentError> {
        let params = HistoryParams { thread: thread_id.to_string(), after_seq, limit: Some(limit) }.to_value();
        // Take the idle connection before doing I/O so simultaneous readers never wait on each other.
        let cached = self.idle_history.lock().unwrap_or_else(|e| e.into_inner()).take();
        let mut client = match cached {
            Some(client) => client,
            None => self.connect()?,
        };
        let response = match client.call(THREAD_HISTORY, params.clone()) {
            Err(error) if error.code == agent_api_client::code::DISCONNECTED => {
                client = self.connect()?;
                Self::call(&mut client, THREAD_HISTORY, params)?
            }
            answer => answer.map_err(failed)?,
        };
        *self.idle_history.lock().unwrap_or_else(|e| e.into_inner()) = Some(client);
        let page: HistoryResult = serde_json::from_value(response).map_err(unreadable("thread.history"))?;
        let events = page.events().map_err(|e| AgentError::Failed(format!("rca answered thread.history with {}", e.0)))?;
        Ok(History { events: events.iter().map(agent_event).collect::<Result<_, _>>()?, last_seq: page.last_seq, more: page.more })
    }

    fn list_dir(&self, root: &str, path: &str) -> Result<Vec<DirEntry>, AgentError> {
        let listed: ListResult = serde_json::from_value(self.file(FILE_LIST, root, path)?).map_err(unreadable("file.list"))?;
        let mut entries: Vec<DirEntry> = listed.entries.into_iter().map(|e| DirEntry { name: e.name, directory: e.kind == EntryKind::Dir, size: e.size }).collect();
        entries.sort_by(|a, b| b.directory.cmp(&a.directory).then_with(|| a.name.cmp(&b.name)));
        Ok(entries)
    }

    fn file_preview(&self, root: &str, path: &str) -> Result<t3_core::ports::FilePreview, AgentError> {
        use t3_core::ports::{FilePreview, FILE_PREVIEW_LIMIT};
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let mut client = self.connect()?;
        let (workspace, path) = self.place(&mut client, root, path)?;
        let result = client.call(edit::FILE_READ_CHUNK, json!({"workspace": workspace, "path": path, "length": FILE_PREVIEW_LIMIT})).map_err(refused)?;
        let chunk: exec_fs::ChunkResult = serde_json::from_value(result).map_err(unreadable("file.readChunk"))?;
        let bytes = STANDARD.decode(&chunk.data).map_err(|_| AgentError::Failed("Invalid file preview encoding".into()))?;
        if chunk.offset != 0 || bytes.len() as u64 != chunk.bytes || bytes.len() > FILE_PREVIEW_LIMIT {
            return Err(AgentError::Failed("Invalid file preview range".into()));
        }
        Ok(FilePreview { bytes, total_bytes: chunk.total_bytes })
    }

    fn read_file(&self, root: &str, path: &str) -> Result<String, AgentError> {
        let read: ReadResult = serde_json::from_value(self.file(FILE_READ, root, path)?).map_err(unreadable("file.read"))?;
        if read.truncated {
            return Err(AgentError::Failed(format!("{path} is longer than one file.read returns ({} lines); a chunked read arrives with the files.edit group", read.total_lines)));
        }
        Ok(read.content)
    }

    fn setup_events(&self, root: &str, after_seq: u64, wait: Duration) -> Result<SetupBatch, AgentError> {
        let mut client = self.connect()?;
        let (workspace, _) = self.place(&mut client, root, "")?;
        let opened = match client.call(worktrees::SETUP_SUBSCRIBE, json!({"workspace": workspace, "after_seq": after_seq})) {
            Ok(opened) => opened,
            Err(e) if e.code == workspace_code::NO_SETUP => return Ok(SetupBatch::default()),
            Err(e) => return Err(failed(e)),
        };
        let (last_seq, running) = (opened["last_seq"].as_u64().unwrap_or(after_seq), opened["running"].as_bool().unwrap_or(false));
        let deadline = Instant::now() + wait;
        let mut events: Vec<SetupEvent> = Vec::new();
        let finished = |events: &[SetupEvent]| events.iter().any(|e| e.kind == "setup.finished");
        while !finished(&events) && (running || events.last().map_or(after_seq, |e| e.seq) < last_seq) && Instant::now() < deadline {
            if let Some(event) = client.next_event(QUIET.min(deadline.saturating_duration_since(Instant::now()))).map_err(failed)? {
                events.push(SetupEvent { seq: event.seq, kind: event.kind, data: Value::Object(event.data) });
            }
        }
        Ok(SetupBatch { ran: true, events })
    }

    fn terminal_events(&self, root: &str, terminal_name: &str, after_offset: Option<u64>, wait: Duration) -> Result<TerminalBatch, AgentError> {
        let mut client = self.connect()?;
        let (workspace, _) = self.place(&mut client, root, "")?;
        let attach = |client: &mut Client, after: Option<u64>| {
            let mut params = json!({"workspace": workspace, "terminal": terminal_name});
            if let Some(after) = after {
                params["after_offset"] = json!(after);
            }
            client.call(terminal::ATTACH, params)
        };
        match attach(&mut client, after_offset) {
            Ok(_) => {}
            Err(e) if e.code == terminal::code::UNKNOWN_TERMINAL => return Ok(TerminalBatch::default()),
            // The service lost the stream's end (it restarted): what it has now is the stream, so the reader starts it over
            Err(e) if e.code == terminal::code::OFFSET_AHEAD => {
                client = self.connect()?;
                attach(&mut client, None).map_err(failed)?;
            }
            Err(e) => return Err(failed(e)),
        }
        let deadline = Instant::now() + wait;
        let mut events: Vec<TerminalEvent> = Vec::new();
        loop {
            let patience = if events.is_empty() { deadline.saturating_duration_since(Instant::now()) } else { Duration::from_millis(30) };
            if patience.is_zero() {
                break;
            }
            match client.next_event(patience.min(QUIET)).map_err(failed)? {
                Some(event) => events.extend(terminal_event(&event)?),
                None if !events.is_empty() => break,
                None => {}
            }
        }
        Ok(TerminalBatch { found: true, events })
    }

    fn git_follow(&self, root: &str, sink: &mut dyn FnMut(Option<Value>) -> Follow) -> Result<(), AgentError> {
        let mut client = self.connect()?;
        let (workspace, _) = self.place(&mut client, root, "")?;
        client.call(git::SUBSCRIBE, json!({"workspace": workspace})).map_err(failed)?;
        loop {
            let status = client.next_event(QUIET).map_err(failed)?.filter(|e| e.kind == git::kind::STATUS).and_then(|e| e.data.get("status").cloned());
            if sink(status) == Follow::Stop {
                return Ok(());
            }
        }
    }

    fn auth_follow(&self, auth: &str, sink: &mut dyn FnMut(Value) -> Follow) -> Result<(), AgentError> {
        let mut client = self.connect()?;
        let params = json!({"auth": auth, "after_seq": 0});
        client.subscribe(Subscribe::new(format!("provider.auth/{auth}"), accounts::AUTH_SUBSCRIBE, params, "after_seq")).map_err(refused)?;
        loop {
            let Some(event) = client.next_event(QUIET).map_err(failed)? else {
                if sink(Value::Null) == Follow::Stop {
                    return Ok(());
                }
                continue;
            };
            let view = event.data.get("auth").cloned().unwrap_or(Value::Null);
            let ended = view["status"].as_str().is_some_and(|s| s != "pending");
            if sink(view) == Follow::Stop || ended {
                return Ok(());
            }
        }
    }

    fn clone_follow(&self, root: &str, sink: &mut dyn FnMut(Option<Value>) -> Follow) -> Result<(), AgentError> {
        let mut client = self.connect()?;
        let (workspace, _) = self.place(&mut client, root, "")?;
        let params = json!({"workspace": workspace, "after_seq": 0});
        client.subscribe(Subscribe::new(format!("hosted/{workspace}/clones"), hosted::clone::SUBSCRIBE, params, "after_seq")).map_err(refused)?;
        loop {
            let state = client.next_event(QUIET).map_err(failed)?.filter(|e| e.kind == hosted::kind::CLONE_STATE).map(|e| {
                let mut data = e.data;
                data.insert("seq".into(), json!(e.seq));
                Value::Object(data)
            });
            if sink(state) == Follow::Stop {
                return Ok(());
            }
        }
    }

    fn preview_follow(&self, epoch: &str, after_seq: u64, sink: &mut dyn FnMut(Option<Value>) -> Follow) -> Result<(), AgentError> {
        let mut client = self.connect()?;
        let params = json!({"epoch": epoch, "after_seq": after_seq});
        client.subscribe(Subscribe::new(preview::watch::TOPIC, preview::watch::SUBSCRIBE, params, "after_seq")).map_err(refused)?;
        loop {
            let event = client.next_event(QUIET).map_err(failed)?.map(|e| {
                let mut data = e.data;
                data.insert("seq".into(), json!(e.seq));
                data.insert("type".into(), json!(e.kind));
                Value::Object(data)
            });
            if sink(event) == Follow::Stop {
                return Ok(());
            }
        }
    }

    fn schedule_follow(&self, sink: &mut dyn FnMut(Option<Value>) -> Follow) -> Result<(), AgentError> {
        let mut client = self.connect()?;
        client.subscribe(Subscribe::new(schedules::watch::TOPIC, schedules::watch::SUBSCRIBE, json!({}), "after_seq")).map_err(refused)?;
        loop {
            let event = client.next_event(QUIET).map_err(failed)?.map(|e| {
                let mut data = e.data;
                data.insert("seq".into(), json!(e.seq));
                data.insert("type".into(), json!(e.kind));
                Value::Object(data)
            });
            if sink(event) == Follow::Stop {
                return Ok(());
            }
        }
    }

    fn terminals(&self) -> Result<Vec<TerminalSummary>, AgentError> {
        let mut client = self.connect()?;
        let mut places: Vec<(String, Option<String>)> = vec![(self.workspace.clone(), None)];
        let listed = client.call(worktrees::WORKTREE_LIST, json!({"workspace": self.workspace})).unwrap_or_default();
        for path in listed["worktrees"].as_array().into_iter().flatten().filter(|w| w["is_main"] != true).filter_map(|w| w["path"].as_str()) {
            places.push((path.rsplit('/').next().unwrap_or_default().to_string(), Some(path.to_string())));
        }
        let mut all = Vec::new();
        for (index, (workspace, worktree_path)) in places.into_iter().enumerate() {
            let listed = match client.call(terminal::LIST, json!({"workspace": workspace})) {
                Ok(listed) => listed,
                Err(e) if index > 0 && e.from_service() => continue,
                Err(e) => return Err(failed(e)),
            };
            let listed: terminal::ListResult = serde_json::from_value(listed).map_err(unreadable("terminal.list"))?;
            all.extend(listed.terminals.into_iter().map(|t| TerminalSummary {
                name: t.terminal,
                cwd: t.cwd,
                worktree_path: worktree_path.clone(),
                title: t.title,
                running: t.status == terminal::Status::Running,
                exit_code: t.exit_code,
                pid: t.pid,
                updated_at_ms: t.updated_at,
            }));
        }
        Ok(all)
    }
}

fn terminal_event(event: &Event) -> Result<Option<TerminalEvent>, AgentError> {
    let bytes = |key: &str| terminal::b64::decode(event.data.get(key).and_then(Value::as_str).unwrap_or_default()).ok_or_else(|| AgentError::Failed(format!("rca sent {} with data that is no base64", event.kind)));
    let number = |key: &str| event.data.get(key).and_then(Value::as_u64).unwrap_or_default();
    Ok(Some(match event.kind.as_str() {
        terminal::kind::SNAPSHOT => TerminalEvent::Snapshot {
            start: number("start_offset"),
            offset: number("offset"),
            data: bytes("data")?,
            reset: event.data.get("reset").and_then(Value::as_bool).unwrap_or(false),
            cols: number("cols") as u16,
            rows: number("rows") as u16,
            running: event.data.get("status").and_then(Value::as_str) == Some("running"),
            exit_code: event.data.get("exit_code").and_then(Value::as_i64).map(|c| c as i32),
        },
        terminal::kind::OUTPUT => TerminalEvent::Output { offset: number("offset"), end: event.seq, data: bytes("data")? },
        terminal::kind::EXITED => TerminalEvent::Exited { exit_code: event.data.get("exit_code").and_then(Value::as_i64).map(|c| c as i32) },
        terminal::kind::CLOSED => TerminalEvent::Closed,
        terminal::kind::CLEARED => TerminalEvent::Cleared,
        _ => return Ok(None),
    }))
}

/// Whether a `service.status` result is one this mapper can work with; the error names what is missing.
pub(crate) fn compatible(result: &Value) -> Result<(), String> {
    let status: StatusResult = serde_json::from_value(result.clone()).map_err(|e| format!("rca answered a service.status this t3 serve cannot read ({e}): update one of them"))?;
    if status.protocol != PROTOCOL {
        return Err(format!("rca speaks agent API protocol {}, this t3 serve speaks {PROTOCOL}: update one of them", status.protocol));
    }
    let missing: Vec<&str> = needed_groups().iter().map(|g| g.as_str()).filter(|g| !status.groups.iter().any(|listed| listed == g)).collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("rca does not serve the agent API group(s) {} that t3 serve needs", missing.join(", ")))
    }
}
