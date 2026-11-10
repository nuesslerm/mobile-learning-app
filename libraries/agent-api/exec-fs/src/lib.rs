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
pub const EDIT_FILE: &str = "edit_file";
pub const LIST_DIR: &str = "list_dir";
pub const SEARCH: &str = "search";
pub const STAT: &str = "stat";
pub const READ_CHUNK: &str = "read_chunk";
pub const CREATE_FILE: &str = "create_file";
pub const BROWSE: &str = "browse";
pub const SEARCH_ENTRIES: &str = "search_entries";
pub const PUT_BLOB: &str = "put_blob";
pub const MAKE_DIR: &str = "make_dir";
pub const SKILL_SCAN: &str = "skill_scan";

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

/// The most bytes one `read_chunk` returns; a longer `length` is cut to it.
pub const CHUNK_MAX: u64 = 1 << 20;
/// The most entries one `search_entries` returns; a larger `limit` is cut to it.
pub const ENTRIES_MAX: usize = 500;
const ENTRIES_DEFAULT: usize = 100;
/// The most bytes one `put_blob` writes (decoded); a longer `data` is refused.
pub const BLOB_MAX: u64 = 10 << 20;

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
    Action {
        name: EDIT_FILE,
        about: "Replace text in a file in the workspace.",
        properties: &[PATH, Property { name: "old", kind: "string", about: "Text to replace" }, Property { name: "new", kind: "string", about: "Replacement text" }, Property { name: "replace_all", kind: "boolean", about: "Replace every occurrence" }],
        required: &["path", "old", "new"],
    },
    Action { name: LIST_DIR, about: "List a directory in the workspace.", properties: &[PATH, Property { name: "depth", kind: "integer", about: "How many levels to list (default 1)" }], required: &["path"] },
    Action {
        name: SEARCH,
        about: "Search file contents or file names in the workspace.",
        properties: &[
            Property { name: "pattern", kind: "string", about: "Pattern to look for" },
            Property { name: "path", kind: "string", about: "Directory to search in" },
            Property { name: "glob", kind: "string", about: "Only files matching this glob" },
            Property { name: "kind", kind: "string", about: "\"content\" or \"files\"" },
        ],
        required: &["pattern", "kind"],
    },
    Action { name: STAT, about: "Kind, size and modification time of a path in the workspace.", properties: &[PATH], required: &["path"] },
    Action {
        name: READ_CHUNK,
        about: "Read a byte range of a file in the workspace, base64 encoded (binary safe).",
        properties: &[PATH, Property { name: "offset", kind: "integer", about: "First byte to return (default 0)" }, Property { name: "length", kind: "integer", about: "Number of bytes to return (at most 1 MiB, the default)" }],
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
        name: MAKE_DIR,
        about: "Make a directory in the workspace, held to the root like every path. Without `parents` the parent must exist and a path that exists is refused (a claim of the name); with it missing parents are made and a directory that exists is accepted.",
        properties: &[PATH, Property { name: "parents", kind: "boolean", about: "Make missing parent directories and accept a directory that exists" }],
        required: &["path"],
    },
    Action {
        name: SKILL_SCAN,
        about: "List the skills (a SKILL.md in a directory below .claude/skills, .agents/skills or .codex/skills) of a directory of the workspace and, when asked, of the executor's own home: the name and description of each front matter.",
        properties: &[
            Property { name: "root", kind: "string", about: "Directory relative to the workspace root whose skill directories are listed (\".\" is the root)" },
            Property { name: "home", kind: "boolean", about: "Also list the skill directories of the executor's own home" },
        ],
        required: &["root"],
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
pub struct EditFile {
    pub path: String,
    pub old: String,
    pub new: String,
    pub replace_all: bool,
}

impl EditFile {
    pub fn parse(args: &Value) -> ExecResult<EditFile> {
        Ok(EditFile { path: string(args, "path")?, old: string(args, "old")?, new: string(args, "new")?, replace_all: flag(args, "replace_all") })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditResult {
    pub replacements: usize,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchKind {
    Content,
    Files,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Search {
    pub pattern: String,
    pub path: String,
    pub glob: Option<String>,
    pub kind: SearchKind,
}

impl Search {
    pub fn parse(args: &Value) -> ExecResult<Search> {
        let kind = match args["kind"].as_str() {
            Some("content") => SearchKind::Content,
            Some("files") => SearchKind::Files,
            _ => return Err(ExecError::invalid_args("`kind` must be \"content\" or \"files\"")),
        };
        Ok(Search { pattern: string(args, "pattern")?, path: opt_string(args, "path")?.unwrap_or_else(|| ".".into()), glob: opt_string(args, "glob")?, kind })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Match {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Found {
    pub matches: Vec<Match>,
    pub truncated: bool,
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
}

impl ReadChunk {
    pub fn parse(args: &Value) -> ExecResult<ReadChunk> {
        Ok(ReadChunk { path: string(args, "path")?, offset: opt_uint(args, "offset")?.unwrap_or(0), length: opt_uint(args, "length")?.unwrap_or(CHUNK_MAX).min(CHUNK_MAX) })
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
}

impl SkillScan {
    pub fn parse(args: &Value) -> ExecResult<SkillScan> {
        Ok(SkillScan { root: string(args, "root")?, home: flag(args, "home") })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillScope {
    Workspace,
    Home,
}

/// One skill. `dir` is the one of `SKILL_DIRS` it lies in. `path` is its `SKILL.md`: relative to the executor's root for the workspace, and
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

/// The workspace's skills first, then the home's, each by directory order of `SKILL_DIRS` and then by directory name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillsFound {
    pub skills: Vec<Skill>,
    pub truncated: bool,
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
        let edit = EditFile::parse(&json!({"path": "a", "old": "x", "new": "y"})).unwrap();
        let list = ListDir::parse(&json!({"path": "."})).unwrap();
        let search = Search::parse(&json!({"pattern": "p", "kind": "files"})).unwrap();
        // Then a read starts at line 0 with no limit, a write and an edit do not create or replace all, a listing is one level, a search starts at "."
        assert_eq!((read.offset, read.limit), (0, None));
        assert_eq!((write.create_dirs, edit.replace_all), (false, false));
        assert_eq!(list.depth, 1);
        assert_eq!((search.path.as_str(), search.glob, search.kind), (".", None, SearchKind::Files));
    }

    #[test]
    fn given_the_required_arguments_of_each_action_when_one_is_missing_then_the_parse_fails_invalid_args() {
        // Given each action's table, and the minimal valid args built from its required list
        for action in ACTIONS {
            let full: serde_json::Map<String, Value> = action.required.iter().map(|k| (k.to_string(), if *k == "kind" { json!("content") } else { json!("v") })).collect();
            let parse = |args: &Value| match action.name {
                READ_FILE => ReadFile::parse(args).map(drop),
                WRITE_FILE => WriteFile::parse(args).map(drop),
                EDIT_FILE => EditFile::parse(args).map(drop),
                LIST_DIR => ListDir::parse(args).map(drop),
                SEARCH => Search::parse(args).map(drop),
                STAT => Stat::parse(args).map(drop),
                READ_CHUNK => ReadChunk::parse(args).map(drop),
                CREATE_FILE => CreateFile::parse(args).map(drop),
                BROWSE => Browse::parse(args).map(drop),
                SEARCH_ENTRIES => SearchEntries::parse(args).map(drop),
                PUT_BLOB => PutBlob::parse(args).map(drop),
                MAKE_DIR => MakeDir::parse(args).map(drop),
                SKILL_SCAN => SkillScan::parse(args).map(drop),
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
    fn given_a_search_kind_that_is_neither_content_nor_files_when_parsed_then_it_is_refused_before_the_pattern() {
        // Given a search with no pattern and a wrong kind
        let e = Search::parse(&json!({"kind": "names"})).unwrap_err();
        // Then the kind is what is refused
        assert!(e.message.contains("`kind`"), "{e:?}");
    }

    #[test]
    fn given_the_results_when_encoded_then_the_fields_keep_the_contract_names_and_order() {
        // Given one result of each shape
        // When they are encoded
        let read = to_value(&ReadResult { content: "x".into(), total_lines: 1, truncated: false }).to_string();
        let list = to_value(&ListResult { entries: vec![Entry { name: "src".into(), kind: EntryKind::Dir, size: 4096 }] }).to_string();
        let found = to_value(&Found { matches: vec![Match { path: "a".into(), line: Some(2), text: Some("t".into()) }, Match { path: "b".into(), line: None, text: None }], truncated: true }).to_string();
        // Then the keys are the contract's, a match carries only the parts it has, and the order is declaration order
        assert_eq!(read, r#"{"content":"x","total_lines":1,"truncated":false}"#);
        assert_eq!(list, r#"{"entries":[{"name":"src","kind":"dir","size":4096}]}"#);
        assert_eq!(found, r#"{"matches":[{"path":"a","line":2,"text":"t"},{"path":"b"}],"truncated":true}"#);
        assert_eq!((to_value(&WriteResult { bytes: 2 }).to_string(), to_value(&EditResult { replacements: 1 }).to_string()), (r#"{"bytes":2}"#.to_string(), r#"{"replacements":1}"#.to_string()));
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
}
