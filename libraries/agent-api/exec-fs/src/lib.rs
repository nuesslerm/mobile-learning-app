//! The file actions of the executor contract (`docs/plans/rca-client/protocol.md`): each action's arguments as the executor reads them from
//! a request's `args`, and its result as it goes on the wire. Paths are relative to the executor's root; confining them is the executor's.
use exec_proto::args::{flag, opt_string, opt_uint, string};
use exec_proto::{Action, ExecError, ExecResult, Property};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(feature = "testing")]
pub mod testing;

pub const READ_FILE: &str = "read_file";
pub const WRITE_FILE: &str = "write_file";
pub const LIST_DIR: &str = "list_dir";
pub const STAT: &str = "stat";
pub const READ_CHUNK: &str = "read_chunk";
pub const CREATE_FILE: &str = "create_file";
pub const BROWSE: &str = "browse";
pub const SEARCH_ENTRIES: &str = "search_entries";
pub const PUT_BLOB: &str = "put_blob";
pub const PUT_TEMP: &str = "put_temp";
pub const MAKE_DIR: &str = "make_dir";
pub const SECRET_FILE_PUT: &str = "secret_file_put";
pub const SKILL_SCAN: &str = "skill_scan";
pub const SESSION_SCAN: &str = "session_scan";

/// Where `skill_scan` looks for skills, below the scanned directory and below the executor's home: one directory per skill, holding `SKILL.md`.
pub const SKILL_DIRS: [&str; 3] = [".claude/skills", ".agents/skills", ".codex/skills"];
/// The file that makes a directory a skill.
pub const SKILL_FILE: &str = "SKILL.md";
/// The most skills one `skill_scan` returns.
pub const SKILLS_MAX: usize = 200;
/// How much of a `SKILL.md` is read: its front matter is at the top.
pub const SKILL_READ_MAX: u64 = 64 << 10;
/// The longest name and description kept.
pub const SKILL_NAME_MAX: usize = 200;
pub const SKILL_DESCRIPTION_MAX: usize = 1000;

/// The agent CLIs whose session files `session_scan` reads, below the executor's home: `~/.claude/projects/<project>/<id>.jsonl` and
/// `~/.codex/sessions/<year>/<month>/<day>/rollout-*.jsonl`; the line formats come from the CLIs' own files, unpinned.
pub const SESSION_AGENTS: [&str; 2] = ["claude", "codex"];
/// The most sessions one `session_scan` returns; a larger `limit` is cut to it.
pub const SESSIONS_MAX: usize = 500;
const SESSIONS_DEFAULT: usize = 100;
/// How much of one session file is read; a longer file counts the messages of this much.
pub const SESSION_READ_MAX: u64 = 4 << 20;
/// The longest prompt and title kept.
pub const PROMPT_MAX: usize = 300;

/// The most bytes one `read_chunk` returns; a longer `length` is cut to it.
pub const CHUNK_MAX: u64 = 1 << 20;
/// The most entries one `search_entries` returns; a larger `limit` is cut to it.
pub const ENTRIES_MAX: usize = 500;
const ENTRIES_DEFAULT: usize = 100;
/// The most bytes one `put_blob` writes (decoded); a longer `data` is refused.
pub const BLOB_MAX: u64 = 10 << 20;
/// The most bytes one `secret_file_put` writes: a key, a token or a certificate, never a blob.
pub const SECRET_MAX: usize = 16 << 10;

const PATH: Property = Property { name: "path", kind: "string", about: "Path relative to the workspace root" };

/// The family's actions, in the contract's order.
pub const ACTIONS: &[Action] = &[
    Action {
        name: READ_FILE,
        about: "Read a file in the workspace.",
        properties: &[PATH, Property { name: "offset", kind: "integer", about: "First line to return" }, Property { name: "limit", kind: "integer", about: "Number of lines to return" }],
        required: &["path"],
    },
    Action {
        name: WRITE_FILE,
        about: "Write a file in the workspace, replacing it.",
        properties: &[PATH, Property { name: "content", kind: "string", about: "The whole new content" }, Property { name: "create_dirs", kind: "boolean", about: "Create missing parent directories" }],
        required: &["path", "content"],
    },
    Action { name: LIST_DIR, about: "List a directory in the workspace.", properties: &[PATH, Property { name: "depth", kind: "integer", about: "How many levels to list (default 1)" }], required: &["path"] },
    Action { name: STAT, about: "Kind, size and modification time of a path in the workspace.", properties: &[PATH], required: &["path"] },
    Action {
        name: READ_CHUNK,
        about: "Read a byte range of a file in the workspace, base64 encoded (binary safe).",
        properties: &[PATH, Property { name: "offset", kind: "integer", about: "First byte to return (default 0)" }, Property { name: "length", kind: "integer", about: "Number of bytes to return (at most 1 MiB, the default)" }, Property { name: "scope", kind: "string", about: "`claudeProjects` reads an absolute path below the executor's own ~/.claude/projects instead of the workspace" }],
        required: &["path"],
    },
    Action {
        name: CREATE_FILE,
        about: "Create a new file in the workspace; fails when the path exists.",
        properties: &[PATH, Property { name: "content", kind: "string", about: "The new file's content (empty when absent)" }, Property { name: "create_dirs", kind: "boolean", about: "Create missing parent directories" }],
        required: &["path"],
    },
    Action {
        name: BROWSE,
        about: "List a directory the executor was allowed to show, which may lie outside the workspace root (absolute path).",
        properties: &[Property { name: "path", kind: "string", about: "Absolute directory path" }, Property { name: "show_hidden", kind: "boolean", about: "Include names that start with a dot" }],
        required: &["path"],
    },
    Action {
        name: SEARCH_ENTRIES,
        about: "Find files and directories of the workspace whose path contains the query, best match first.",
        properties: &[
            Property { name: "query", kind: "string", about: "Text the path must contain, case-insensitive" },
            Property { name: "limit", kind: "integer", about: "Most entries to return (default 100, at most 500)" },
            Property { name: "path", kind: "string", about: "Directory to search in, the paths returned being relative to it (default the workspace root)" },
        ],
        required: &["query"],
    },
    Action {
        name: PUT_BLOB,
        about: "Write an attachment into the workspace: new file only (never replaces or follows a link), parent directories made, at most 10 MiB.",
        properties: &[PATH, Property { name: "data", kind: "string", about: "The bytes, base64 (standard alphabet, padded)" }],
        required: &["path", "data"],
    },
    Action {
        name: PUT_TEMP,
        about: "Write a pasted file into a new folder of the executor's temporary directory (never the workspace) and answer its absolute path: the name is one path segment (a separator, a control character or a leading dot is replaced), at most 10 MiB.",
        properties: &[Property { name: "name", kind: "string", about: "The file's name" }, Property { name: "data", kind: "string", about: "The bytes, base64 (standard alphabet, padded)" }],
        required: &["name", "data"],
    },
    Action {
        name: MAKE_DIR,
        about: "Make a directory in the workspace, held to the root like every path. Without `parents` the parent must exist and a path that exists is refused (a claim of the name); with it missing parents are made and a directory that exists is accepted.",
        properties: &[PATH, Property { name: "parents", kind: "boolean", about: "Make missing parent directories and accept a directory that exists" }],
        required: &["path"],
    },
    Action {
        name: SECRET_FILE_PUT,
        about: "Write a secret a person typed into a new file of the workspace, mode 0600, never replacing a path and never echoing the value (not in the result, not in an error); at most 16 KiB. The model learns the path, not the value.",
        properties: &[PATH, Property { name: "value", kind: "string", about: "The secret, as typed" }],
        required: &["path", "value"],
    },
    Action {
        name: SKILL_SCAN,
        about: "List the skills (a SKILL.md in a directory below .claude/skills, .agents/skills or .codex/skills) of a directory of the workspace and, when asked, of the executor's own home: the name and description of each front matter.",
        properties: &[
            Property { name: "root", kind: "string", about: "Directory relative to the workspace root whose skill directories are listed (\".\" is the root)" },
            Property { name: "home", kind: "boolean", about: "Also list the skill directories of the executor's own home" },
            Property { name: "dirs", kind: "array", about: "The skill directories to list below `root`, relative and without `..` (default .claude/skills, .agents/skills, .codex/skills)" },
            Property { name: "home_dirs", kind: "array", about: "The skill directories to list below the home, relative and without `..` (default the same three)" },
        ],
        required: &["root"],
    },
    Action {
        name: SESSION_SCAN,
        about: "List the sessions the agent CLIs (claude, codex) keep in the executor's own home, newest file first: id, title, directory, branch, model, times, message count, first and last prompt, and the file as ~/<path>.",
        properties: &[
            Property { name: "agents", kind: "array", about: "The CLIs to list, each claude or codex (default both)" },
            Property { name: "limit", kind: "integer", about: "The most sessions to return (default 100, at most 500)" },
        ],
        required: &[],
    },
];

#[derive(Debug, Clone, PartialEq)]
pub struct ReadFile {
    pub path: String,
    /// Lines to skip.
    pub offset: usize,
    pub limit: Option<usize>,
}

impl ReadFile {
    pub fn parse(args: &Value) -> ExecResult<ReadFile> {
        Ok(ReadFile { path: string(args, "path")?, offset: opt_uint(args, "offset")?.unwrap_or(0) as usize, limit: opt_uint(args, "limit")?.map(|l| l as usize) })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadResult {
    pub content: String,
    pub total_lines: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WriteFile {
    pub path: String,
    pub content: String,
    pub create_dirs: bool,
}

impl WriteFile {
    pub fn parse(args: &Value) -> ExecResult<WriteFile> {
        Ok(WriteFile { path: string(args, "path")?, content: string(args, "content")?, create_dirs: flag(args, "create_dirs") })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WriteResult {
    pub bytes: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListDir {
    pub path: String,
    pub depth: usize,
}

impl ListDir {
    pub fn parse(args: &Value) -> ExecResult<ListDir> {
        Ok(ListDir { path: string(args, "path")?, depth: opt_uint(args, "depth")?.unwrap_or(1) as usize })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    File,
    Dir,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListResult {
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stat {
    pub path: String,
}

impl Stat {
    pub fn parse(args: &Value) -> ExecResult<Stat> {
        Ok(Stat { path: string(args, "path")? })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatResult {
    pub kind: EntryKind,
    pub size: u64,
    pub modified_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReadChunk {
    pub path: String,
    pub offset: u64,
    pub length: u64,
    /// `Some(CLAUDE_PROJECTS)` reads below the executor's home `.claude/projects`, not the workspace.
    pub scope: Option<String>,
}

/// The `scope` of a `read_chunk` that reads the Claude CLI's own projects directory (workflow scripts and logs).
pub const CLAUDE_PROJECTS: &str = "claudeProjects";

impl ReadChunk {
    pub fn parse(args: &Value) -> ExecResult<ReadChunk> {
        let scope = opt_string(args, "scope")?;
        if scope.as_deref().is_some_and(|s| s != CLAUDE_PROJECTS) {
            return Err(ExecError::invalid_args(format!("`scope` must be `{CLAUDE_PROJECTS}`")));
        }
        Ok(ReadChunk { path: string(args, "path")?, offset: opt_uint(args, "offset")?.unwrap_or(0), length: opt_uint(args, "length")?.unwrap_or(CHUNK_MAX).min(CHUNK_MAX), scope })
    }
}

/// `data` is the bytes, base64 (standard alphabet, padded); `eof` is true when the range reaches the end of the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChunkResult {
    pub data: String,
    pub offset: u64,
    pub bytes: u64,
    pub total_bytes: u64,
    pub eof: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreateFile {
    pub path: String,
    pub content: String,
    pub create_dirs: bool,
}

impl CreateFile {
    pub fn parse(args: &Value) -> ExecResult<CreateFile> {
        Ok(CreateFile { path: string(args, "path")?, content: opt_string(args, "content")?.unwrap_or_default(), create_dirs: flag(args, "create_dirs") })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PutBlob {
    pub path: String,
    /// Base64 of the bytes; the executor decodes it.
    pub data: String,
}

impl PutBlob {
    pub fn parse(args: &Value) -> ExecResult<PutBlob> {
        let data = string(args, "data")?;
        if data.len() as u64 > BLOB_MAX.div_ceil(3) * 4 {
            return Err(ExecError::invalid_args(format!("`data` is longer than {BLOB_MAX} bytes")));
        }
        Ok(PutBlob { path: string(args, "path")?, data })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PutBlobResult {
    pub path: String,
    pub bytes: u64,
}

/// `secret_file_put`: one secret into one new file. Its `Debug` hides the value, so no log line or panic message can carry it.
#[derive(Clone, PartialEq)]
pub struct SecretFilePut {
    pub path: String,
    pub value: String,
}

impl std::fmt::Debug for SecretFilePut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretFilePut").field("path", &self.path).field("value", &"<hidden>").finish()
    }
}

impl SecretFilePut {
    pub fn parse(args: &Value) -> ExecResult<SecretFilePut> {
        let value = string(args, "value")?;
        if value.is_empty() || value.len() > SECRET_MAX {
            return Err(ExecError::invalid_args(format!("`value` must hold 1 to {SECRET_MAX} bytes")));
        }
        Ok(SecretFilePut { path: string(args, "path")?, value })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecretFilePutResult {
    pub path: String,
    pub bytes: u64,
}

/// `put_temp`: a file of its own in the executor's temporary directory.
#[derive(Debug, Clone, PartialEq)]
pub struct PutTemp {
    /// One path segment, made safe.
    pub name: String,
    /// Base64 of the bytes; the executor decodes it.
    pub data: String,
}

impl PutTemp {
    pub fn parse(args: &Value) -> ExecResult<PutTemp> {
        let data = string(args, "data")?;
        if data.len() as u64 > BLOB_MAX.div_ceil(3) * 4 {
            return Err(ExecError::invalid_args(format!("`data` is longer than {BLOB_MAX} bytes")));
        }
        let given = string(args, "name")?;
        let last = given.rsplit(['/', '\\']).next().unwrap_or("");
        let name: String = last.chars().map(|c| if c.is_control() || c == ':' { '_' } else { c }).collect::<String>().trim_start_matches('.').trim().chars().take(255).collect();
        if name.is_empty() {
            return Err(ExecError::invalid_args("`name` must name a file"));
        }
        Ok(PutTemp { name, data })
    }
}

/// Where `put_temp` wrote the file: an absolute path at the executor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PutTempResult {
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MakeDir {
    pub path: String,
    pub parents: bool,
}

impl MakeDir {
    pub fn parse(args: &Value) -> ExecResult<MakeDir> {
        Ok(MakeDir { path: string(args, "path")?, parents: flag(args, "parents") })
    }
}

/// `created` is false only for a directory that was there already (`parents` accepts one).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MakeDirResult {
    pub path: String,
    pub created: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SkillScan {
    pub root: String,
    pub home: bool,
    pub dirs: Vec<String>,
    pub home_dirs: Vec<String>,
}

/// The most skill directories one `skill_scan` lists below each of the root and the home.
pub const SKILL_DIRS_MAX: usize = 64;

fn skill_dirs(args: &Value, key: &str) -> ExecResult<Vec<String>> {
    let Some(given) = args.get(key).filter(|v| !v.is_null()) else { return Ok(SKILL_DIRS.iter().map(|d| d.to_string()).collect()) };
    let list = given.as_array().ok_or_else(|| ExecError::invalid_args(format!("`{key}` must be a list of directories")))?;
    if list.len() > SKILL_DIRS_MAX {
        return Err(ExecError::invalid_args(format!("`{key}` holds at most {SKILL_DIRS_MAX} directories")));
    }
    list.iter()
        .map(|d| {
            let dir = d.as_str().unwrap_or_default();
            let relative = !dir.is_empty() && dir.len() <= 200 && !dir.starts_with(['/', '~', '\\']) && dir.split('/').all(|part| !part.is_empty() && part != ".." && part != ".");
            relative.then(|| dir.to_string()).ok_or_else(|| ExecError::invalid_args(format!("`{key}`: {dir:?} is not a relative directory")))
        })
        .collect()
}

impl SkillScan {
    pub fn parse(args: &Value) -> ExecResult<SkillScan> {
        Ok(SkillScan { root: string(args, "root")?, home: flag(args, "home"), dirs: skill_dirs(args, "dirs")?, home_dirs: skill_dirs(args, "home_dirs")? })
    }
}

/// A skill directory the scan found (whether or not it held a skill): what tells a missing directory from an empty one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillDir {
    pub scope: SkillScope,
    pub dir: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillScope {
    Workspace,
    Home,
}

/// One skill. `dir` is the scanned directory it lies in. `path` is its `SKILL.md`: relative to the executor's root for the workspace, and
/// `~/<dir>/<name>/SKILL.md` for the home (the home's own location never leaves the executor). `name` is the front matter's, else the
/// directory's; `description` is absent when the front matter has none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub scope: SkillScope,
    pub dir: String,
    pub path: String,
}

/// The workspace's skills first, then the home's, each by the order of the scanned directories and then by directory name. `present` lists
/// the scanned directories that exist (an older executor answers none).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillsFound {
    pub skills: Vec<Skill>,
    pub truncated: bool,
    #[serde(default)]
    pub present: Vec<SkillDir>,
}

fn cut(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    for quote in ['"', '\''] {
        if let Some(inner) = value.strip_prefix(quote).and_then(|v| v.strip_suffix(quote)) {
            return inner.to_string();
        }
    }
    value.to_string()
}

/// The `name` and `description` of a `SKILL.md`'s front matter (the lines between a first `---` line and the next), as the skills of the
/// agent CLIs write them: `key: value`, the value plain or quoted, or a block (`>`, `|`, with `-` or `+`) or plain value continued on indented
/// lines, which join with single spaces. Anything else in the front matter is ignored; no front matter is no fields.
pub fn front_matter(text: &str) -> (Option<String>, Option<String>) {
    let mut lines = text.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return (None, None);
    }
    let (mut name, mut description): (Option<String>, Option<String>) = (None, None);
    let mut current: Option<(&str, String)> = None;
    let mut finish = |current: &mut Option<(&str, String)>| {
        if let Some((key, value)) = current.take() {
            let value = cut(value.trim(), if key == "name" { SKILL_NAME_MAX } else { SKILL_DESCRIPTION_MAX });
            match key {
                "name" => name = Some(value).filter(|v| !v.is_empty()),
                _ => description = Some(value).filter(|v| !v.is_empty()),
            }
        }
    };
    for line in lines {
        if line.trim_end() == "---" {
            break;
        }
        if line.starts_with([' ', '\t']) {
            if let Some((_, value)) = current.as_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
            continue;
        }
        finish(&mut current);
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            if key == "name" || key == "description" {
                let value = value.trim();
                let block = value.starts_with(['>', '|']) && value[1..].chars().all(|c| matches!(c, '-' | '+'));
                current = Some((if key == "name" { "name" } else { "description" }, if block { String::new() } else { unquote(value) }));
            }
        }
    }
    finish(&mut current);
    (name, description)
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionScan {
    pub agents: Vec<String>,
    pub limit: usize,
}

impl SessionScan {
    pub fn parse(args: &Value) -> ExecResult<SessionScan> {
        let limit = opt_uint(args, "limit")?.map_or(SESSIONS_DEFAULT, |l| (l as usize).clamp(1, SESSIONS_MAX));
        let agents = match args.get("agents").filter(|v| !v.is_null()) {
            None => SESSION_AGENTS.iter().map(|a| a.to_string()).collect(),
            Some(given) => {
                let list = given.as_array().filter(|l| !l.is_empty() && l.len() <= SESSION_AGENTS.len()).ok_or_else(|| ExecError::invalid_args("`agents` must be a list of one or two CLIs"))?;
                let mut agents: Vec<String> = Vec::new();
                for agent in list {
                    let name = agent.as_str().filter(|a| SESSION_AGENTS.contains(a)).ok_or_else(|| ExecError::invalid_args(format!("`agents` names claude or codex, not {agent}")))?;
                    if agents.iter().any(|a| a == name) {
                        return Err(ExecError::invalid_args(format!("`agents` names {name} twice")));
                    }
                    agents.push(name.to_string());
                }
                agents
            }
        };
        Ok(SessionScan { agents, limit })
    }
}

/// One session file of a CLI. `path` is `~/<path>` (the home's own location never leaves the executor); the times are the file's own
/// ISO strings, `modified_ms` is the file's modification time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VaultSession {
    pub agent: String,
    pub session_id: String,
    pub path: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub modified_ms: u64,
    pub message_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionsFound {
    pub sessions: Vec<VaultSession>,
    pub truncated: bool,
}

/// What one session file says about itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Digest {
    pub session_id: Option<String>,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub branch: Option<String>,
    pub model: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub message_count: u64,
    pub first_prompt: Option<String>,
    pub last_prompt: Option<String>,
}

fn text_of(content: &Value, kinds: &[&str]) -> Option<String> {
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks.iter().filter(|b| b.get("type").and_then(Value::as_str).is_some_and(|t| kinds.contains(&t))).filter_map(|b| b.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n"),
        _ => return None,
    };
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn string_at(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
}

/// The digest of a session file's text for `agent` claude or codex. A non-JSON line is skipped; a prompt is a user message that is text,
/// not a tool result or the CLI's own context block.
pub fn digest(agent: &str, text: &str) -> Digest {
    let mut d = Digest::default();
    let prompt = |d: &mut Digest, text: String| {
        let text = cut(&text, PROMPT_MAX);
        d.first_prompt.get_or_insert_with(|| text.clone());
        d.last_prompt = Some(text);
    };
    for line in text.lines() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else { continue };
        let stamp = string_at(&entry, "timestamp");
        let kind = entry.get("type").and_then(Value::as_str).unwrap_or_default();
        if agent == "codex" {
            let payload = entry.get("payload").unwrap_or(&Value::Null);
            match kind {
                "session_meta" => {
                    d.session_id = d.session_id.take().or_else(|| string_at(payload, "id"));
                    d.cwd = d.cwd.take().or_else(|| string_at(payload, "cwd"));
                    d.branch = d.branch.take().or_else(|| payload.get("git").and_then(|g| string_at(g, "branch")));
                    d.created_at = d.created_at.take().or_else(|| string_at(payload, "timestamp")).or(stamp.clone());
                }
                "turn_context" => d.model = d.model.take().or_else(|| string_at(payload, "model")),
                "response_item" if payload.get("type").and_then(Value::as_str) == Some("message") => {
                    match payload.get("role").and_then(Value::as_str) {
                        Some("user") => {
                            if let Some(t) = text_of(&payload["content"], &["input_text"]).filter(|t| !t.starts_with("<environment_context") && !t.starts_with("<user_instructions")) {
                                d.message_count += 1;
                                prompt(&mut d, t);
                            }
                        }
                        Some("assistant") => d.message_count += 1,
                        _ => {}
                    }
                }
                _ => {}
            }
        } else {
            if entry.get("isSidechain").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            match kind {
                "summary" => d.title = d.title.take().or_else(|| string_at(&entry, "summary")),
                "user" | "assistant" => {
                    d.session_id = d.session_id.take().or_else(|| string_at(&entry, "sessionId"));
                    d.cwd = d.cwd.take().or_else(|| string_at(&entry, "cwd"));
                    d.branch = d.branch.take().or_else(|| string_at(&entry, "gitBranch"));
                    d.created_at = d.created_at.take().or(stamp.clone());
                    let message = entry.get("message").unwrap_or(&Value::Null);
                    if kind == "assistant" {
                        d.model = string_at(message, "model").or(d.model.take());
                        d.message_count += 1;
                    } else if entry.get("isMeta").and_then(Value::as_bool) != Some(true) {
                        if let Some(t) = text_of(&message["content"], &["text"]) {
                            d.message_count += 1;
                            prompt(&mut d, t);
                        }
                    }
                }
                _ => {}
            }
        }
        if stamp.is_some() {
            d.updated_at = stamp;
        }
    }
    d.title = d.title.or_else(|| d.first_prompt.clone());
    d
}

#[derive(Debug, Clone, PartialEq)]
pub struct Browse {
    pub path: String,
    pub show_hidden: bool,
}

impl Browse {
    pub fn parse(args: &Value) -> ExecResult<Browse> {
        Ok(Browse { path: string(args, "path")?, show_hidden: flag(args, "show_hidden") })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowseEntry {
    pub name: String,
    pub kind: EntryKind,
    pub path: String,
}

/// `parent` is absent when the parent is not itself a place the executor may show.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowseResult {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub entries: Vec<BrowseEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchEntries {
    pub query: String,
    pub limit: usize,
    pub path: Option<String>,
}

impl SearchEntries {
    pub fn parse(args: &Value) -> ExecResult<SearchEntries> {
        let limit = opt_uint(args, "limit")?.map_or(ENTRIES_DEFAULT, |l| (l as usize).clamp(1, ENTRIES_MAX));
        Ok(SearchEntries { query: string(args, "query")?, limit, path: opt_string(args, "path")? })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FoundEntry {
    pub path: String,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntriesResult {
    pub entries: Vec<FoundEntry>,
    pub truncated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use exec_proto::{to_value, Code};
    use serde_json::json;

    #[test]
    fn given_the_arguments_of_each_action_when_parsed_then_defaults_fill_what_is_absent() {
        // Given requests that name only what is required
        // When they are parsed
        let read = ReadFile::parse(&json!({"path": "a"})).unwrap();
        let write = WriteFile::parse(&json!({"path": "a", "content": "c"})).unwrap();
        let list = ListDir::parse(&json!({"path": "."})).unwrap();
        // Then a read starts at line 0 with no limit, a write does not create, a listing is one level
        assert_eq!((read.offset, read.limit), (0, None));
        assert!(!write.create_dirs);
        assert_eq!(list.depth, 1);
    }

    #[test]
    fn given_the_required_arguments_of_each_action_when_one_is_missing_then_the_parse_fails_invalid_args() {
        // Given each action's table, and the minimal valid args built from its required list
        for action in ACTIONS {
            let full: serde_json::Map<String, Value> = action.required.iter().map(|k| (k.to_string(), if *k == "kind" { json!("content") } else { json!("v") })).collect();
            let parse = |args: &Value| match action.name {
                READ_FILE => ReadFile::parse(args).map(drop),
                WRITE_FILE => WriteFile::parse(args).map(drop),
                LIST_DIR => ListDir::parse(args).map(drop),
                STAT => Stat::parse(args).map(drop),
                READ_CHUNK => ReadChunk::parse(args).map(drop),
                CREATE_FILE => CreateFile::parse(args).map(drop),
                BROWSE => Browse::parse(args).map(drop),
                SEARCH_ENTRIES => SearchEntries::parse(args).map(drop),
                PUT_BLOB => PutBlob::parse(args).map(drop),
                PUT_TEMP => PutTemp::parse(args).map(drop),
                MAKE_DIR => MakeDir::parse(args).map(drop),
                SECRET_FILE_PUT => SecretFilePut::parse(args).map(drop),
                SKILL_SCAN => SkillScan::parse(args).map(drop),
                SESSION_SCAN => SessionScan::parse(args).map(drop),
                other => panic!("{other} is not parsed here"),
            };
            assert!(parse(&Value::Object(full.clone())).is_ok(), "{}", action.name);
            for missing in action.required {
                // When one required argument is left out
                let mut args = full.clone();
                args.remove(*missing);
                // Then the parse refuses it by INVALID_ARGS
                assert_eq!(parse(&Value::Object(args)).unwrap_err().code, Code::InvalidArgs, "{} without {missing}", action.name);
            }
        }
    }

    #[test]
    fn given_a_secret_when_its_arguments_are_read_then_an_empty_or_oversized_value_is_refused_and_no_debug_text_shows_it() {
        // Given a valid secret, an empty one and one over the cap
        let valid = SecretFilePut::parse(&json!({"path": "k", "value": "s3cr3t"})).unwrap();
        let empty = SecretFilePut::parse(&json!({"path": "k", "value": ""}));
        let long = SecretFilePut::parse(&json!({"path": "k", "value": "x".repeat(SECRET_MAX + 1)}));
        // Then the valid one reads back, the others are INVALID_ARGS, and the Debug text of the valid one hides the value
        assert_eq!((valid.path.as_str(), valid.value.as_str()), ("k", "s3cr3t"));
        assert_eq!((empty.unwrap_err().code, long.unwrap_err().code), (Code::InvalidArgs, Code::InvalidArgs));
        assert!(!format!("{valid:?}").contains("s3cr3t") && format!("{valid:?}").contains("<hidden>"));
    }

    #[test]
    fn given_the_results_when_encoded_then_the_fields_keep_the_contract_names_and_order() {
        // Given one result of each shape
        // When they are encoded
        let read = to_value(&ReadResult { content: "x".into(), total_lines: 1, truncated: false }).to_string();
        let list = to_value(&ListResult { entries: vec![Entry { name: "src".into(), kind: EntryKind::Dir, size: 4096 }] }).to_string();
        // Then the keys are the contract's and the order is declaration order
        assert_eq!(read, r#"{"content":"x","total_lines":1,"truncated":false}"#);
        assert_eq!(list, r#"{"entries":[{"name":"src","kind":"dir","size":4096}]}"#);
        assert_eq!(to_value(&WriteResult { bytes: 2 }).to_string(), r#"{"bytes":2}"#);
    }
    #[test]
    fn given_a_chunk_or_entry_request_with_limits_out_of_range_when_parsed_then_they_are_cut_to_the_contract_maxima() {
        // Given a read_chunk asking for more than the maximum and a search asking for 0 and for far more than the maximum
        let chunk = ReadChunk::parse(&json!({"path": "a", "length": CHUNK_MAX * 4})).unwrap();
        let none = SearchEntries::parse(&json!({"query": "q", "limit": 0})).unwrap();
        let many = SearchEntries::parse(&json!({"query": "q", "limit": ENTRIES_MAX * 10})).unwrap();
        let default = SearchEntries::parse(&json!({"query": "q"})).unwrap();
        // Then the length is the maximum, the limits are clamped into 1..=maximum, and no limit is the default
        assert_eq!((chunk.offset, chunk.length), (0, CHUNK_MAX));
        assert_eq!((none.limit, many.limit, default.limit), (1, ENTRIES_MAX, 100));
    }

    #[test]
    fn given_a_put_blob_whose_data_is_longer_than_the_cap_when_parsed_then_it_is_refused_and_one_at_the_cap_is_read() {
        // Given data whose base64 length is exactly the longest the cap allows, and one character more
        let at_cap = "A".repeat((BLOB_MAX.div_ceil(3) * 4) as usize);
        let over = format!("{at_cap}A");
        // When both are parsed
        let ok = PutBlob::parse(&json!({"path": "a", "data": at_cap}));
        let refused = PutBlob::parse(&json!({"path": "a", "data": over})).unwrap_err();
        // Then the cap is the line between them, and the refusal is INVALID_ARGS
        assert!(ok.is_ok());
        assert_eq!(refused.code, Code::InvalidArgs);
    }

    #[test]
    fn given_the_new_results_when_encoded_then_the_fields_keep_the_contract_names_and_a_browse_without_parent_omits_it() {
        // Given one result of each new shape
        let stat = to_value(&StatResult { kind: EntryKind::File, size: 3, modified_ms: 9 }).to_string();
        let chunk = to_value(&ChunkResult { data: "YWJj".into(), offset: 0, bytes: 3, total_bytes: 3, eof: true }).to_string();
        let browse = to_value(&BrowseResult { path: "/a".into(), parent: None, entries: vec![BrowseEntry { name: "b".into(), kind: EntryKind::Dir, path: "/a/b".into() }] }).to_string();
        let entries = to_value(&EntriesResult { entries: vec![FoundEntry { path: "src/a.rs".into(), kind: EntryKind::File }], truncated: false }).to_string();
        // Then the keys are the contract's, in declaration order
        assert_eq!(stat, r#"{"kind":"file","size":3,"modified_ms":9}"#);
        assert_eq!(chunk, r#"{"data":"YWJj","offset":0,"bytes":3,"total_bytes":3,"eof":true}"#);
        assert_eq!(browse, r#"{"path":"/a","entries":[{"name":"b","kind":"dir","path":"/a/b"}]}"#);
        assert_eq!(entries, r#"{"entries":[{"path":"src/a.rs","kind":"file"}],"truncated":false}"#);
    }

    #[test]
    fn given_a_make_dir_request_when_parsed_then_the_path_is_required_and_parents_default_to_off() {
        // Given a request that names only the path and one that asks for the parents
        let claim = MakeDir::parse(&json!({"path": ".rca/projects/a"})).unwrap();
        let tree = MakeDir::parse(&json!({"path": ".rca/projects", "parents": true})).unwrap();
        // Then the first claims one name (no parents) and the second makes the chain; and the result keeps the contract's keys
        assert_eq!((claim.parents, tree.parents), (false, true));
        assert_eq!(to_value(&MakeDirResult { path: "a".into(), created: false }).to_string(), r#"{"path":"a","created":false}"#);
    }

    #[test]
    fn given_a_skill_scan_request_when_parsed_then_the_root_is_required_and_the_home_is_off_unless_asked() {
        // Given a request for the root only and one that asks for the home too
        let workspace = SkillScan::parse(&json!({"root": "."})).unwrap();
        let both = SkillScan::parse(&json!({"root": "pkg", "home": true})).unwrap();
        // Then the home is off in the first, and the result keeps the contract's keys with a description only when there is one
        assert_eq!((workspace.home, both.home, both.root.as_str()), (false, true, "pkg"));
        let skill = |description: Option<&str>| to_value(&Skill { name: "n".into(), description: description.map(str::to_string), scope: SkillScope::Home, dir: ".claude/skills".into(), path: "~/.claude/skills/n/SKILL.md".into() }).to_string();
        assert_eq!(skill(Some("d")), r#"{"name":"n","description":"d","scope":"home","dir":".claude/skills","path":"~/.claude/skills/n/SKILL.md"}"#);
        assert_eq!(skill(None), r#"{"name":"n","scope":"home","dir":".claude/skills","path":"~/.claude/skills/n/SKILL.md"}"#);
    }

    #[test]
    fn given_front_matter_in_each_form_skills_write_when_read_then_name_and_description_come_out_joined_on_one_line() {
        // Given a plain value, quoted values, a folded block, a literal block with keep, a plain value continued on an indented line, and text with no front matter
        let plain = front_matter("---\nname: review\ndescription: Reviews a diff\n---\nbody: not read\n");
        let quoted = front_matter("---\nname: \"a b\"\ndescription: 'it is: fine'\n---\n");
        let folded = front_matter("---\nname: x\ndescription: >\n  Line one\n  line two\nother: 1\n---\n");
        let literal = front_matter("---\ndescription: |-\n  first\n  second\nname: y\n---\n");
        let continued = front_matter("---\nname: z\ndescription: starts here\n  and goes on\n---\n");
        let none = front_matter("# Title\nname: no\n");
        let unclosed = front_matter("---\nname: open\nno end");
        // Then each is read as the CLIs read it
        assert_eq!(plain, (Some("review".into()), Some("Reviews a diff".into())));
        assert_eq!(quoted, (Some("a b".into()), Some("it is: fine".into())));
        assert_eq!(folded, (Some("x".into()), Some("Line one line two".into())));
        assert_eq!(literal, (Some("y".into()), Some("first second".into())));
        assert_eq!(continued, (Some("z".into()), Some("starts here and goes on".into())));
        assert_eq!(none, (None, None));
        assert_eq!(unclosed.0.as_deref(), Some("open"));
    }

    #[test]
    fn given_a_description_longer_than_the_cap_when_read_then_it_is_cut_at_a_character_boundary() {
        // Given a description of two-byte characters past the cap
        let text = format!("---\ndescription: {}\n---\n", "é".repeat(SKILL_DESCRIPTION_MAX));
        // When read
        let (_, description) = front_matter(&text);
        // Then it is the cap's bytes and whole characters
        assert_eq!(description.map(|d| (d.len(), d.chars().all(|c| c == 'é'))), Some((SKILL_DESCRIPTION_MAX, true)));
    }

    #[test]
    fn given_session_files_of_each_cli_when_digested_then_prompts_and_facts_come_out() {
        // Given a claude file with a summary, a context-free prompt, a tool result and a reply, and a codex file with its context block
        let claude = concat!(
            r#"{"type":"summary","summary":"Fix the build"}"#, "\n",
            r#"{"type":"user","sessionId":"s1","cwd":"/w","gitBranch":"main","timestamp":"2026-11-01T10:00:00Z","message":{"role":"user","content":"fix it"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-11-01T10:00:05Z","message":{"role":"assistant","model":"claude-x","content":[{"type":"text","text":"ok"}]}}"#, "\n",
            r#"{"type":"user","timestamp":"2026-11-01T10:00:09Z","message":{"role":"user","content":[{"type":"tool_result","content":"x"}]}}"#, "\n",
            "not json\n",
        );
        let codex = concat!(
            r#"{"type":"session_meta","payload":{"id":"c1","cwd":"/w","timestamp":"2026-11-02T09:00:00Z","git":{"branch":"dev"}}}"#, "\n",
            r#"{"type":"turn_context","payload":{"model":"gpt-x"}}"#, "\n",
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>x"}]}}"#, "\n",
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"add a test"}]}}"#, "\n",
            r#"{"timestamp":"2026-11-02T09:01:00Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}]}}"#, "\n",
        );
        // When digested
        let (c, x) = (digest("claude", claude), digest("codex", codex));
        // Then each says what its file holds, and a summary is the claude title
        assert_eq!((c.session_id.as_deref(), c.title.as_deref(), c.cwd.as_deref(), c.branch.as_deref(), c.model.as_deref()), (Some("s1"), Some("Fix the build"), Some("/w"), Some("main"), Some("claude-x")));
        assert_eq!((c.message_count, c.first_prompt.as_deref(), c.updated_at.as_deref()), (2, Some("fix it"), Some("2026-11-01T10:00:09Z")));
        assert_eq!((x.session_id.as_deref(), x.title.as_deref(), x.branch.as_deref(), x.model.as_deref()), (Some("c1"), Some("add a test"), Some("dev"), Some("gpt-x")));
        assert_eq!((x.message_count, x.created_at.as_deref(), x.updated_at.as_deref()), (2, Some("2026-11-02T09:00:00Z"), Some("2026-11-02T09:01:00Z")));
    }

    #[test]
    fn given_a_session_scan_request_when_parsed_then_both_clis_and_a_clamped_limit_are_the_default() {
        // Given no arguments, a one-CLI list, and lists that name an unknown or repeated CLI
        let all = SessionScan::parse(&json!({})).unwrap();
        let one = SessionScan::parse(&json!({"agents": ["codex"], "limit": 0})).unwrap();
        // Then the defaults fill in, the limit is clamped, and a bad list is INVALID_ARGS
        assert_eq!((all.agents.len(), all.limit, one.agents, one.limit), (2, 100, vec!["codex".to_string()], 1));
        for bad in [json!({"agents": ["opencode"]}), json!({"agents": ["codex", "codex"]}), json!({"agents": []})] {
            assert_eq!(SessionScan::parse(&bad).unwrap_err().code, Code::InvalidArgs, "{bad}");
        }
    }
}
