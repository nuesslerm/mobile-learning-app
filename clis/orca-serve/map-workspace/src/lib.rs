//! Orca's workspace and file rows over rca: `files.*` (list, search, read, preview, chunk, directory, stat, create), `folderWorkspace.list` and
//! `mobileRelay.projectSections`. An Orca repo or worktree is an rca workspace (`repo.list`), a file is read through the executor of that
//! workspace (`file.*`), and nothing here touches a disk. The rows that name a desktop editor tab are not offered.
mod agent;
mod catalog;
mod files;
mod repos;
mod worktrees;
pub mod natural;
mod target;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use files::{image_mime, previewable_limit};
pub use target::{resolve, targets, Target};

use agent_api_workspace::edit::{FILE_CREATE, FILE_READ_CHUNK, FILE_SEARCH, FILE_STAT};
use agent_api_proto::Method as AgentMethod;
use agent_api_git::{FETCH, REFS};
use agent_api_state::SET;
use agent_api_workspace::worktrees::{WORKTREE_CREATE, WORKTREE_LIST, WORKTREE_REMOVE};
use agent_api_workspace::{FILE_LIST, REPO_LIST};
use orca_core::wire::{self, Failure};
use orca_core::{Area, Ctx, Kind, Method, NotOffered};
use serde_json::Value;

pub const FILES_LIST: &str = "files.list";
pub const FILES_SEARCH_PATHS: &str = "files.searchPaths";
pub const FILES_READ: &str = "files.read";
pub const FILES_READ_PREVIEW: &str = "files.readPreview";
pub const FILES_READ_CHUNK: &str = "files.readChunk";
pub const FILES_READ_DIR: &str = "files.readDir";
pub const FILES_STAT: &str = "files.stat";
pub const FILES_CREATE_FILE: &str = "files.createFile";
pub const FILES_OPEN: &str = "files.open";
pub const FILES_OPEN_DIFF: &str = "files.openDiff";
pub const MARKDOWN_READ_TAB: &str = "markdown.readTab";
pub const MARKDOWN_SAVE_TAB: &str = "markdown.saveTab";
pub const FOLDER_WORKSPACE_LIST: &str = "folderWorkspace.list";
pub const PROJECT_SECTIONS: &str = "mobileRelay.projectSections";
pub const REPO_LIST_ROW: &str = "repo.list";
pub const REPO_UPDATE: &str = "repo.update";
pub const REPO_HOOKS: &str = "repo.hooks";
pub const REPO_SPARSE_PRESETS: &str = "repo.sparsePresets";
pub const REPO_SAVE_SPARSE_PRESET: &str = "repo.saveSparsePreset";
pub const REPO_BASE_REF_DEFAULT: &str = "repo.baseRefDefault";
pub const REPO_SEARCH_REFS: &str = "repo.searchRefs";
pub const PROJECT_LIST: &str = "project.list";
pub const PROJECT_GROUP_LIST: &str = "projectGroup.list";
pub const PROJECT_HOST_SETUP_LIST: &str = "projectHostSetup.list";
pub const WORKTREE_LIST_ROW: &str = "worktree.list";
pub const WORKTREE_SHOW: &str = "worktree.show";
pub const WORKTREE_CREATE_ROW: &str = "worktree.create";
pub const WORKTREE_RM: &str = "worktree.rm";
pub const WORKTREE_SET: &str = "worktree.set";
pub const WORKTREE_LIST_RETIRED_NAMES: &str = "worktree.listRetiredNames";
pub const WORKTREE_PREFETCH_CREATE_BASE: &str = "worktree.prefetchCreateBase";

/// The agent methods this crate sends: its calls read this list, and a flow test holds it to the methods really sent.
pub const USES: &[AgentMethod] = &[REPO_LIST, FILE_LIST, FILE_READ_CHUNK, FILE_STAT, FILE_CREATE, FILE_SEARCH, SET, REFS, FETCH, WORKTREE_CREATE, WORKTREE_LIST, WORKTREE_REMOVE];

const METHODS: &[Method] = &[
    Method { name: FILES_LIST, kind: Kind::Unary },
    Method { name: FILES_SEARCH_PATHS, kind: Kind::Unary },
    Method { name: FILES_READ, kind: Kind::Unary },
    Method { name: FILES_READ_PREVIEW, kind: Kind::Unary },
    Method { name: FILES_READ_CHUNK, kind: Kind::Unary },
    Method { name: FILES_READ_DIR, kind: Kind::Unary },
    Method { name: FILES_STAT, kind: Kind::Unary },
    Method { name: FILES_CREATE_FILE, kind: Kind::Unary },
    Method { name: FOLDER_WORKSPACE_LIST, kind: Kind::Unary },
    Method { name: PROJECT_SECTIONS, kind: Kind::Unary },
    Method { name: REPO_LIST_ROW, kind: Kind::Unary },
    Method { name: REPO_UPDATE, kind: Kind::Unary },
    Method { name: REPO_HOOKS, kind: Kind::Unary },
    Method { name: REPO_SPARSE_PRESETS, kind: Kind::Unary },
    Method { name: REPO_SAVE_SPARSE_PRESET, kind: Kind::Unary },
    Method { name: REPO_BASE_REF_DEFAULT, kind: Kind::Unary },
    Method { name: REPO_SEARCH_REFS, kind: Kind::Unary },
    Method { name: PROJECT_LIST, kind: Kind::Unary },
    Method { name: PROJECT_GROUP_LIST, kind: Kind::Unary },
    Method { name: PROJECT_HOST_SETUP_LIST, kind: Kind::Unary },
    Method { name: WORKTREE_LIST_ROW, kind: Kind::Unary },
    Method { name: WORKTREE_SHOW, kind: Kind::Unary },
    Method { name: WORKTREE_CREATE_ROW, kind: Kind::Unary },
    Method { name: WORKTREE_RM, kind: Kind::Unary },
    Method { name: WORKTREE_SET, kind: Kind::Unary },
    Method { name: WORKTREE_LIST_RETIRED_NAMES, kind: Kind::Unary },
    Method { name: WORKTREE_PREFETCH_CREATE_BASE, kind: Kind::Unary },
];

/// Each reason equals the one in `docs/plans/rca-parity/coverage/orca-decisions.json`; a flow test compares them.
pub const NOT_OFFERED: &[NotOffered] = &[
    NotOffered { name: FILES_OPEN, reason: "proposed default awaiting owner confirmation: it opens an editor tab in the desktop's own window and this host has no desktop; the phone reads a file with files.read and files.readPreview", decided_by: "M6" },
    NotOffered { name: FILES_OPEN_DIFF, reason: "proposed default awaiting owner confirmation: it opens a diff tab in the desktop's own window and this host has no desktop", decided_by: "M6" },
    NotOffered { name: MARKDOWN_READ_TAB, reason: "proposed default awaiting owner confirmation: it reads a markdown editor tab of the desktop by its tab id and this host has no tab; the phone's own fallback reads the file with files.read", decided_by: "M6" },
    NotOffered { name: MARKDOWN_SAVE_TAB, reason: "proposed default awaiting owner confirmation: it saves a markdown editor tab of the desktop by its tab id and this host has no tab to save", decided_by: "M6" },
];

pub struct WorkspaceArea;

impl Area for WorkspaceArea {
    fn name(&self) -> &'static str {
        "workspace"
    }

    fn methods(&self) -> &'static [Method] {
        METHODS
    }

    fn not_offered(&self) -> &'static [NotOffered] {
        NOT_OFFERED
    }

    fn call(&self, ctx: &mut Ctx, method: &str, params: &Value) -> Result<Value, Failure> {
        let agent = ctx.agent;
        match method {
            FILES_LIST => files::list(agent, params),
            FILES_SEARCH_PATHS => files::search_paths(agent, params),
            FILES_READ => files::read(agent, params),
            FILES_READ_PREVIEW => files::read_preview(agent, params),
            FILES_READ_CHUNK => files::read_chunk(agent, params),
            FILES_READ_DIR => files::read_dir(agent, params),
            FILES_STAT => files::stat(agent, params),
            FILES_CREATE_FILE => files::create_file(agent, params),
            FOLDER_WORKSPACE_LIST => catalog::folder_workspaces(agent),
            PROJECT_SECTIONS => catalog::project_sections(agent),
            REPO_LIST_ROW => repos::list(agent),
            REPO_UPDATE => repos::update(agent, params),
            REPO_HOOKS => repos::hooks(agent, params),
            REPO_SPARSE_PRESETS => repos::sparse_presets(agent, params),
            REPO_SAVE_SPARSE_PRESET => repos::save_sparse_preset(agent, params),
            REPO_BASE_REF_DEFAULT => repos::base_ref_default(agent, params),
            REPO_SEARCH_REFS => repos::search_refs(agent, params),
            PROJECT_LIST => repos::projects(agent),
            PROJECT_GROUP_LIST => repos::groups(agent),
            PROJECT_HOST_SETUP_LIST => repos::host_setups(agent),
            WORKTREE_LIST_ROW => worktrees::list(agent, params),
            WORKTREE_SHOW => worktrees::show(agent, params),
            WORKTREE_CREATE_ROW => worktrees::create(agent, params),
            WORKTREE_RM => worktrees::remove(agent, params),
            WORKTREE_SET => worktrees::set(agent, params),
            WORKTREE_LIST_RETIRED_NAMES => worktrees::retired_names(agent, params),
            WORKTREE_PREFETCH_CREATE_BASE => worktrees::prefetch_create_base(agent, params),
            other => Err(Failure::new(wire::METHOD_NOT_FOUND, format!("Unknown method: {other}"))),
        }
    }
}
