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
/// The changes of the workspace since `base`, the revision a turn started from (the caller records it).
pub const TURN_DIFF: Method = Method::new(GROUP, "turn.diff");
/// The changes of the workspace since `base`, the revision its thread's work started from (the index when absent).
pub const THREAD_DIFF: Method = Method::new(GROUP, "thread.diff");

pub const METHODS: &[Method] = &[FILE_READ_CHUNK, FILE_WRITE, FILE_CREATE, FILE_STAT, FILE_SEARCH, FILE_BROWSE, FILE_MKDIR, TURN_DIFF, THREAD_DIFF];
