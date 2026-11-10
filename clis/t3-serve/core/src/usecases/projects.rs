//! The project rows of M12. A project the phone makes (`projects.mutate`) is a folder the executor serves: `project.create` checks the folder
//! with the agent API (a directory below the executor's root) and makes it a workspace there (`workspace.create`, named for the project),
//! `project.update` keeps what the project says in the agent API's `state` and moves the workspace when the folder changed, and `project.delete`
//! forgets the workspace. The three RPCs that make a folder (`projects.ensureScratch`, `projects.createNew`, `projects.createFromThread`) are
//! t3-local over the agent API: the folder is made by the executor (`file.mkdir`, `git.init`), never by this server, below `.rca` of the
//! executor's root, where a `.gitignore` of `*` keeps the repository around it from seeing it.
use super::engine::Failure;
use super::queries::{field, QueryFailure};
use super::service::Service;
use crate::domain::auth::uuid_v4;
use crate::domain::command::{Command, Family};
use crate::ports::{agent_codes, AgentError, Op};
use serde_json::{json, Value};
use t3_contracts::time::iso;

/// The folders the server makes for projects, below the executor's root.
pub const SCRATCH: &str = ".rca/scratch";
const NAMED: &str = ".rca/projects";
const IGNORE: &str = ".rca/.gitignore";
/// How many names a new project's folder tries (`name`, `name-2`, ..) before it gives up.
const NAME_ATTEMPTS: u32 = 100;
const NAME_MAX: usize = 200;

/// The project mutations that are asked of the agent API.
pub const FORWARDED: [&str; 3] = ["project.create", "project.update", "project.delete"];

/// What a project says that other devices read: its row less the ids and times.
const SETTINGS: [&str; 9] = ["title", "workspaceRoot", "defaultModelSelection", "defaultThreadEnvMode", "autoPull", "projectIcon", "scripts", "pinnedAt", "archivedAt"];

/// The scratch project's icon, set when it is made (a person's own choice is never overwritten).
fn scratch_icon() -> Value {
    json!({"kind": "lucide", "name": "message-square-dashed", "color": "gray"})
}

fn dispatch_failed(message: impl Into<String>) -> QueryFailure {
    QueryFailure::Error(json!({"_tag": "OrchestrationDispatchCommandError", "message": message.into()}))
}

fn failure_text(failure: Failure) -> String {
    match failure {
        Failure::Defect(message) | Failure::Refused { message, .. } => message,
        Failure::ProjectNotEmpty { project_id, .. } => format!("Project {project_id} is not empty."),
    }
}

/// The name the agent API gives the workspace a project's folder is.
pub fn workspace_name(project: &str) -> String {
    format!("project-{project}")
}

/// `dir` relative to `root` (empty for `root` itself), or none when `dir` is not below it.
fn relative(root: &str, dir: &str) -> Option<String> {
    let (root, dir) = (root.trim_end_matches('/'), dir.trim_end_matches('/'));
    if dir == root {
        return Some(String::new());
    }
    dir.strip_prefix(root).and_then(|rest| rest.strip_prefix('/')).map(str::to_string)
}

/// Latin letters with an accent stand for their base letter in a folder name (`é` is `e`).
fn fold(c: char) -> char {
    const FROM: &str = "àáâãäåāăąçćčďèéêëēėęěìíîïīįłñńňòóôõöøōőŕřśšşťùúûüūůűųýÿźżž";
    const TO: &str = "aaaaaaaaacccdeeeeeeeeiiiiiilnnnooooooooorrsssstuuuuuuuuyyzzz";
    FROM.chars().position(|f| f == c).and_then(|at| TO.chars().nth(at)).unwrap_or(c)
}

/// A project's folder name: its name in lower-case letters and digits, one dash between the runs of anything else, at most 64 characters, and
/// `project` for a name with none (a name Windows reserves gets `-project`, so the folder can be copied there).
pub fn project_folder_name(name: &str) -> String {
    let mut slug = String::new();
    for c in name.chars().flat_map(char::to_lowercase).map(fold) {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.chars().take(64).collect::<String>();
    let slug = slug.trim_end_matches('-');
    let reserved = matches!(slug, "con" | "prn" | "aux" | "nul") || (slug.len() == 4 && (slug.starts_with("com") || slug.starts_with("lpt")) && slug.ends_with(|c: char| ('1'..='9').contains(&c)));
    match (slug.is_empty(), reserved) {
        (true, _) => "project".into(),
        (false, true) => format!("{slug}-project"),
        _ => slug.into(),
    }
}

/// Up to five words of what the thread was started with, for the name of its folder.
fn folder_words(text: &str) -> String {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit())).filter(|w| !w.is_empty()).take(5).collect();
    words.join("-").chars().take(48).collect::<String>().trim_end_matches('-').to_string()
}

const ICON_COLORS: [&str; 15] = ["#dc2626", "#ea580c", "#d97706", "#16a34a", "#059669", "#0d9488", "#0891b2", "#0284c7", "#2563eb", "#4f46e5", "#7c3aed", "#9333ea", "#c026d3", "#db2777", "#e11d48"];

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The first two letters of the name's first two words on a colour its characters pick, as the icon of a new project.
fn project_icon_svg(name: &str) -> String {
    let words: Vec<&str> = name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let mut initials: String = words.iter().take(2).filter_map(|w| w.chars().next()).flat_map(char::to_uppercase).collect();
    if initials.is_empty() {
        initials = name.trim().chars().next().map_or("?".into(), |c| c.to_uppercase().collect());
    }
    let hash = name.chars().fold(0u32, |hash, c| hash.wrapping_mul(31).wrapping_add(c as u32));
    let background = ICON_COLORS[hash as usize % ICON_COLORS.len()];
    let size = if initials.chars().count() > 1 { 26 } else { 32 };
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 64 64\">\n  <rect width=\"64\" height=\"64\" rx=\"14\" fill=\"{background}\"/>\n  <text x=\"32\" y=\"32\" dy=\"0.35em\" text-anchor=\"middle\" font-family=\"ui-sans-serif, system-ui, -apple-system, sans-serif\" font-size=\"{size}\" font-weight=\"600\" fill=\"#ffffff\">{}</text>\n</svg>\n",
        escape_xml(&initials)
    )
}

/// The first files of a new project: a README that shows the icon, and the icon.
fn scaffold(name: &str) -> Vec<(String, String)> {
    let readme = format!("<img src=\"assets/icon.svg\" width=\"64\" height=\"64\" alt=\"\">\n\n# {name}\n\nCreated in [T3 Code](https://t3.codes).\n");
    vec![("README.md".into(), readme), ("assets/icon.svg".into(), project_icon_svg(name))]
}

/// What a refusal of the executor to show a folder says in words.
fn describe(dir: &str, e: &AgentError) -> String {
    match e {
        AgentError::Refused { code, .. } if code == agent_codes::PATH_OUTSIDE_ROOT => format!("{dir} is outside the folder this server serves."),
        AgentError::Refused { code, .. } if code == agent_codes::NOT_FOUND => format!("{dir} does not exist."),
        other => other.message(),
    }
}

impl Service {
    /// `path` as the executor names it (links resolved) when it is a folder the executor may show, the executor's root for `.`.
    fn served_path(&self, path: &str) -> Result<String, AgentError> {
        let listed = self.agent().run(&Op::FileBrowse { root: String::new(), path: path.into(), show_hidden: true })?;
        listed["path"].as_str().map(str::to_string).ok_or_else(|| AgentError::Failed("the agent API did not say where the folder is".into()))
    }

    /// The executor's root and the folder `dir` below it, or why `dir` is no folder of the executor's tree.
    fn below_root(&self, dir: &str) -> Result<(String, String), String> {
        let root = self.served_path(".").map_err(|e| e.message())?;
        let canonical = self.served_path(dir).map_err(|e| describe(dir, &e))?;
        let relative = relative(&root, &canonical).ok_or_else(|| format!("{dir} is outside the folder this server serves ({root})."))?;
        Ok((root, relative))
    }

    /// Checks the project's folder with the agent API and, below the root, makes it the workspace named for the project (none for the root itself).
    fn register_project(&self, id: &str, dir: &str, moved: bool) -> Result<(), String> {
        let (_, relative) = self.below_root(dir)?;
        let workspace = workspace_name(id);
        if relative.is_empty() {
            // The executor's own root is a workspace already; a project that moved there lets its own go
            if moved {
                self.forget_workspace(id);
            }
            return Ok(());
        }
        let create = || self.agent().run(&Op::WorkspaceCreate { workspace: workspace.clone(), path: relative.clone() });
        match create() {
            Err(AgentError::Refused { code, .. }) if code == agent_codes::WORKSPACE_EXISTS => {
                // The project's own workspace from before (a retry, or the folder changed): pointed at this folder
                self.agent().run(&Op::WorkspaceRemove { workspace: workspace.clone() }).map_err(|e| e.message())?;
                create().map(drop).map_err(|e| e.message())
            }
            other => other.map(drop).map_err(|e| e.message()),
        }
    }

    fn forget_workspace(&self, id: &str) {
        match self.agent().run(&Op::WorkspaceRemove { workspace: workspace_name(id) }) {
            Ok(_) => {}
            // A project at the executor's root has no workspace of its own
            Err(AgentError::Refused { code, .. }) if code == agent_codes::UNKNOWN_WORKSPACE => {}
            Err(e) => self.log("project.workspace.forget.failed", &[("project", id.to_string()), ("error", e.message())]),
        }
    }

    /// What the agent API is asked before the engine decides a project command: the folder of a new project, or of one that moved, is checked and
    /// made a workspace. An `Err` is the refusal. A command the engine will refuse itself (an id that is taken or not there) is left to it.
    pub(super) fn project_before(&self, cmd: &Command) -> Result<(), String> {
        if !self.agent().configured() {
            return Ok(());
        }
        let id = cmd.require("projectId")?;
        let known = self.engine().read(|state| state.projects.contains_key(id));
        match cmd.kind.as_str() {
            "project.create" if !known => self.register_project(id, cmd.require("workspaceRoot")?, false),
            "project.update" if known => match cmd.str("workspaceRoot") {
                Some(root) if self.engine().project_root(id).as_deref() != Some(root) => self.register_project(id, root, true),
                _ => Ok(()),
            },
            _ => Ok(()),
        }
    }

    /// What is kept in the agent API once the engine accepted a project command: its settings in `state` (null once it is deleted), and a deleted
    /// project's workspace forgotten. A failure is logged: the rows already say it.
    pub(super) fn project_after(&self, cmd: &Command) {
        let Some(id) = cmd.str("projectId").filter(|_| self.agent().configured()) else { return };
        let value = if cmd.kind == "project.delete" {
            // A project on the executor's root has no workspace of its own to forget
            let at_root = self.engine().project_root(id).zip(self.root().ok()).is_some_and(|(folder, root)| folder.trim_end_matches('/') == root);
            if !at_root {
                self.forget_workspace(id);
            }
            Value::Null
        } else {
            let Some(row) = self.engine().read(|state| state.projects.get(id).cloned()) else { return };
            Value::Object(SETTINGS.iter().map(|key| (key.to_string(), row.get(*key).cloned().unwrap_or(Value::Null))).collect())
        };
        if let Err(e) = self.agent().run(&Op::StateSet { scope: "t3".into(), key: format!("project.settings:{id}"), value }) {
            self.log("state.mirror.failed", &[("key", "project.settings".to_string()), ("project", id.to_string()), ("error", e.message())]);
        }
    }

    /// The id of the project that is at `folder`, when one is not deleted.
    fn project_at(&self, folder: &str) -> Option<String> {
        self.engine().read(|state| state.projects.iter().find(|(id, p)| state.project_exists(id) && p.get("workspaceRoot").and_then(Value::as_str) == Some(folder)).map(|(id, _)| id.clone()))
    }

    fn new_project_id(&self) -> String {
        uuid_v4(&self.0.ids.random_bytes(16))
    }

    /// A project on `folder`, through the same command the phone sends, so the folder is checked and made a workspace like any other.
    fn create_project(&self, id: &str, title: &str, folder: &str, extra: Value) -> Result<(), QueryFailure> {
        let mut command = json!({"type": "project.create", "commandId": format!("project-{id}"), "projectId": id, "title": title, "workspaceRoot": folder});
        if let (Some(into), Some(from)) = (command.as_object_mut(), extra.as_object()) {
            into.extend(from.clone());
        }
        self.dispatch(Family::Project, &command).map(drop).map_err(|failure| dispatch_failed(failure_text(failure)))
    }

    /// The folder `parent` made if it was not there, with the file that keeps the repository around it from seeing what is in `.rca`.
    fn prepare_managed(&self, parent: &str) -> Result<(), String> {
        self.agent().run(&Op::FileMkdir { path: parent.into(), parents: true }).map_err(|e| e.message())?;
        self.agent().run(&Op::FileWrite { root: String::new(), path: IGNORE.into(), content: "*\n".into() }).map(drop).map_err(|e| e.message())
    }

    /// Claims a folder of its own below `parent`: `candidates` in order, the first the executor makes (a name that is taken is skipped).
    fn claim_folder(&self, parent: &str, candidates: impl Iterator<Item = String>) -> Result<String, String> {
        for name in candidates {
            let path = format!("{parent}/{name}");
            match self.agent().run(&Op::FileMkdir { path: path.clone(), parents: false }) {
                Ok(_) => return Ok(path),
                Err(AgentError::Refused { code, .. }) if code == agent_codes::ALREADY_EXISTS => {}
                Err(e) => return Err(e.message()),
            }
        }
        Err(format!("Every folder name below {parent} is taken."))
    }

    fn lock_projects(&self) -> std::sync::MutexGuard<'_, ()> {
        self.2.projects.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `projects.ensureScratch`: the project for threads that have none, on a folder of its own below the executor's root; the same one every time.
    pub fn ensure_scratch(&self) -> Result<Value, QueryFailure> {
        let _one = self.lock_projects();
        let folder = format!("{}/{SCRATCH}", self.root().map_err(dispatch_failed)?);
        if let Some(id) = self.project_at(&folder) {
            return Ok(json!({"projectId": id}));
        }
        self.prepare_managed(SCRATCH).map_err(dispatch_failed)?;
        let id = self.new_project_id();
        self.create_project(&id, "No project", &folder, json!({"projectIcon": scratch_icon()}))?;
        Ok(json!({"projectId": id}))
    }

    /// `projects.createNew`: a folder of the project's name below the executor's root, made a repository with a README and an icon committed, and a
    /// project on it. A commit that could not be made is `commitError`; the project and its files are there either way.
    pub fn create_new(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let name = field(payload, "name")?.trim();
        if name.is_empty() || name.chars().count() > NAME_MAX {
            return Err(QueryFailure::Defect(format!("Expected a non-empty string of at most {NAME_MAX} characters\n  at [\"name\"]")));
        }
        let _one = self.lock_projects();
        self.prepare_managed(NAMED).map_err(dispatch_failed)?;
        let slug = project_folder_name(name);
        let folder = self.claim_folder(NAMED, (1..=NAME_ATTEMPTS).map(|n| if n == 1 { slug.clone() } else { format!("{slug}-{n}") })).map_err(dispatch_failed)?;
        let made = self.agent().run(&Op::GitInit { path: folder.clone(), initial_branch: None, files: scaffold(name), message: Some("Initial commit".into()) }).map_err(|e| dispatch_failed(e.message()))?;
        let workspace_root = format!("{}/{folder}", self.root().map_err(dispatch_failed)?);
        let id = self.new_project_id();
        self.create_project(&id, name, &workspace_root, json!({}))?;
        let mut result = json!({"projectId": id, "workspaceRoot": workspace_root});
        if let Some(error) = made["commit_error"].as_str().filter(|e| !e.is_empty()) {
            result["commitError"] = json!(error);
        }
        Ok(result)
    }

    /// Whether `project` is the scratch project: its folder is the scratch folder of the executor's root.
    fn is_scratch_project(&self, project: &str, scratch: &str) -> bool {
        self.engine().project_root(project).is_some_and(|root| root.trim_end_matches('/') == scratch)
    }

    /// The executor's root, without a trailing slash.
    fn root(&self) -> Result<String, String> {
        Ok(self.served_path(".").map_err(|e| e.message())?.trim_end_matches('/').to_string())
    }

    /// The folder of a thread started in the scratch project: its own, made below the scratch folder and named for the day, the first words of
    /// what it was started with and the end of its id. None for a thread of any other project.
    pub(super) fn scratch_folder(&self, project: &str, thread: &str, text: &str) -> Result<Option<String>, String> {
        // A project whose folder does not end in the scratch folder is not asked about
        if !self.engine().project_root(project).is_some_and(|root| root.trim_end_matches('/').ends_with(SCRATCH)) {
            return Ok(None);
        }
        let root = self.root()?;
        if !self.is_scratch_project(project, &format!("{root}/{SCRATCH}")) {
            return Ok(None);
        }
        let (date, words) = (iso(self.now_ms())[..10].to_string(), folder_words(text));
        let id: String = thread.to_lowercase().chars().filter(char::is_ascii_alphanumeric).collect();
        let tail: String = id.chars().rev().take(8).collect::<Vec<_>>().into_iter().rev().collect();
        let random = hex::encode(self.0.ids.random_bytes(4));
        let named = |part: &str| [date.as_str(), words.as_str(), part].into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
        let candidates = vec![named(&tail), named(&id), named(&format!("{id}-{random}"))];
        let folder = self.claim_folder(SCRATCH, candidates.into_iter())?;
        Ok(Some(format!("{root}/{folder}")))
    }

    /// `projects.createFromThread`: the folder of a thread that has none of its own project becomes one, in place, and every thread working in that
    /// folder moves into it. Asking again finishes a move that was cut short.
    pub fn create_from_thread(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let thread = field(payload, "threadId")?;
        let _one = self.lock_projects();
        let shell = self.engine().read(|state| state.threads.get(thread).cloned()).ok_or_else(|| dispatch_failed("Thread not found."))?;
        let title = shell.get("title").and_then(Value::as_str).unwrap_or("Project").to_string();
        let project = shell.get("projectId").and_then(Value::as_str).unwrap_or_default().to_string();
        let scratch = format!("{}/{SCRATCH}", self.root().map_err(dispatch_failed)?);
        let own = shell.get("worktreePath").and_then(Value::as_str).filter(|p| !p.is_empty()).map(str::to_string).or_else(|| self.engine().project_root(&project));
        let is_scratch = self.is_scratch_project(&project, &scratch);
        let already = own.as_deref().is_some_and(|folder| self.engine().project_root(&project).as_deref() == Some(folder)) && !is_scratch;
        let refused = || dispatch_failed("Only a No project thread with its own folder can become a project.");
        let folder = own.filter(|folder| folder.rsplit_once('/').is_some_and(|(parent, _)| parent == scratch) && (is_scratch || already)).ok_or_else(refused)?;
        let moving: Vec<(String, String)> = self.engine().read(|state| {
            let text = |row: &serde_json::Map<String, Value>, key: &str| row.get(key).and_then(Value::as_str).map(str::to_string);
            let in_scratch = |row: &serde_json::Map<String, Value>| text(row, "projectId").and_then(|p| state.projects.get(&p)).and_then(|r| text(r, "workspaceRoot")).is_some_and(|root| root.trim_end_matches('/') == scratch);
            state.threads.iter().filter(|(id, row)| (is_scratch && id.as_str() == thread) || (text(row, "worktreePath").as_deref() == Some(folder.as_str()) && in_scratch(row))).map(|(id, row)| (id.clone(), text(row, "title").unwrap_or_else(|| "This thread".into()))).collect()
        });
        if let Some((_, busy)) = moving.iter().find(|(id, _)| self.engine().active_run(id).is_some()) {
            return Err(dispatch_failed(format!("\"{busy}\" is still working. Finish or stop it first.")));
        }
        let id = match self.project_at(&folder) {
            Some(id) => id,
            None => {
                let id = self.new_project_id();
                self.create_project(&id, &title, &folder, json!({}))?;
                id
            }
        };
        for (moved, _) in &moving {
            self.engine().adopt_project(moved, &id, &format!("scratch-promotion:{moved}:{id}")).map_err(|e| dispatch_failed(format!("Could not move the thread {moved}: {e}")))?;
        }
        Ok(json!({"projectId": id}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_names_when_made_into_folder_names_then_each_is_lower_case_letters_and_digits_with_one_dash_between_runs() {
        // Given names with spaces, accents, symbols, edges, a long run and names that are no letters at all
        let cases = [("Pinball Stats", "pinball-stats"), ("  Café  Déjà-vu!! ", "cafe-deja-vu"), ("--a__b--", "a-b"), ("日本語", "project"), ("", "project"), ("con", "con-project"), ("LPT3", "lpt3-project"), ("com10", "com10"), ("Ünï/cødé", "uni-code")];
        for (name, folder) in cases {
            // Then the folder name is the one expected
            assert_eq!(project_folder_name(name), folder, "{name:?}");
        }
        // And a long name is cut at 64 characters with no dash left at the end
        let long = project_folder_name(&format!("{} tail", "a".repeat(63)));
        assert_eq!((long.len(), long.ends_with('-')), (63, false));
    }

    #[test]
    fn given_a_thread_text_when_its_words_are_taken_then_they_are_five_at_most_lower_case_and_short() {
        assert_eq!(folder_words("Fix the Login page, please! And more words"), "fix-the-login-page-please");
        assert_eq!(folder_words("日本語 ???"), "");
        assert!(folder_words(&"word ".repeat(40)).len() <= 48);
    }

    #[test]
    fn given_a_project_name_when_scaffolded_then_the_readme_names_it_and_the_icon_holds_its_initials_escaped_on_a_colour_its_characters_pick() {
        // Given two names, one with markup in it
        let files = scaffold("Pinball <Stats> & co");
        let plain = project_icon_svg("Pinball Stats");
        // Then the readme and the icon are the two files, the name is in the readme as written and the initials are of the first two words
        assert_eq!(files.iter().map(|(path, _)| path.as_str()).collect::<Vec<_>>(), ["README.md", "assets/icon.svg"]);
        assert!(files[0].1.contains("# Pinball <Stats> & co\n") && files[0].1.contains("assets/icon.svg"));
        assert!(plain.contains(">PS</text>") && plain.contains("font-size=\"26\""), "{plain}");
        assert!(files[1].1.contains(">PS</text>"), "{}", files[1].1);
        // And a name picks the same colour every time, a one-letter name has the larger letter, and a name with no letter shows its first character escaped
        assert_eq!(project_icon_svg("Pinball Stats"), plain);
        assert!(project_icon_svg("x").contains("font-size=\"32\""));
        assert!(project_icon_svg("<>").contains(">&lt;</text>"));
    }

    #[test]
    fn given_a_folder_and_a_root_when_made_relative_then_the_root_is_empty_a_child_is_its_path_and_a_sibling_with_the_same_prefix_is_outside() {
        assert_eq!(relative("/srv/repo", "/srv/repo"), Some(String::new()));
        assert_eq!(relative("/srv/repo/", "/srv/repo/.rca/scratch/"), Some(".rca/scratch".into()));
        assert_eq!(relative("/srv/repo", "/srv/repo-other"), None);
        assert_eq!(relative("/srv/repo", "/etc"), None);
        assert_eq!(relative("/", "/a"), Some("a".into()));
    }
}
