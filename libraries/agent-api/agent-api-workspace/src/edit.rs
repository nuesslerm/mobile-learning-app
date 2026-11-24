//! The `files.edit` group: the methods that read bytes, write, create, stat, search and browse files of a workspace, make a directory, and the two diffs
//! against a base revision. Each is one executor action (`exec-fs`, `exec-git`), its params read by `FileParams`.
use agent_api_proto::{GroupName, Method};

pub const GROUP: GroupName = GroupName::new("files.edit");

pub const FILE_READ_CHUNK: Method = Method::new(GROUP, "file.readChunk");
pub const FILE_WRITE: Method = Method::new(GROUP, "file.write");
pub const FILE_CREATE: Method = Method::new(GROUP, "file.create");
pub const FILE_STAT: Method = Method::new(GROUP, "file.stat");
pub const FILE_SEARCH: Method = Method::new(GROUP, "file.search");
pub const FILE_BROWSE: Method = Method::new(GROUP, "file.browse");
/// A directory made in the workspace (`parents` makes the missing ones and accepts one that exists; without it the name is claimed).
pub const FILE_MKDIR: Method = Method::new(GROUP, "file.mkdir");
/// A secret a person typed, written to a new file of the workspace (mode 0600, never replacing a path): the answer is the path and the size, never the value.
pub const FILE_PUT_SECRET: Method = Method::new(GROUP, "file.putSecret");
/// The processes below the executor that owns the workspace, sampled once (pid, parent, name, memory, CPU time, with totals).
pub const PROCESS_SAMPLE: Method = Method::new(GROUP, "process.sample");
/// One request (`tools/list`, `tools/call` or `resources/read`) to a stdio MCP server the workspace's `.mcp.json` declares, started for it and stopped after.
pub const MCP_REQUEST: Method = Method::new(GROUP, "mcp.request");
/// A file a terminal printed, which may lie outside the workspace: described by its canonical path, identity and fingerprint. Served only inside the places the workspace's executor was told to allow for artifacts (none by default), else refused as outside the root.
pub const ARTIFACT_STAT: Method = Method::new(GROUP, "artifact.stat");
/// A byte range of such a file; refused with `CHANGED` when the identity or fingerprint given are no longer the file's.
pub const ARTIFACT_READ: Method = Method::new(GROUP, "artifact.read");
/// The content of such a file replaced atomically under the same check; it never creates a file.
pub const ARTIFACT_WRITE: Method = Method::new(GROUP, "artifact.write");
/// The changes of the workspace since `base`, the revision a turn started from (the caller records it).
pub const TURN_DIFF: Method = Method::new(GROUP, "turn.diff");
/// The changes of the workspace since `base`, the revision its thread's work started from (the index when absent).
pub const THREAD_DIFF: Method = Method::new(GROUP, "thread.diff");

pub const METHODS: &[Method] = &[FILE_READ_CHUNK, FILE_WRITE, FILE_CREATE, FILE_STAT, FILE_SEARCH, FILE_BROWSE, FILE_MKDIR, FILE_PUT_SECRET, PROCESS_SAMPLE, MCP_REQUEST, ARTIFACT_STAT, ARTIFACT_READ, ARTIFACT_WRITE, TURN_DIFF, THREAD_DIFF];
