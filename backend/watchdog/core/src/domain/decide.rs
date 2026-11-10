use crate::domain::snapshot::Snapshot;
use std::collections::{HashMap, HashSet};

/// The daemon's own container; it never acts on itself.
pub const SELF_NAME: &str = "watchdog";
pub const LOOP_WINDOW_MS: i64 = 5 * 60_000;
pub const LOOP_MAX_RESTARTS: usize = 3;
pub const RECREATE_COOLDOWN_MS: i64 = 10 * 60_000;
/// The one compose project whose stopped containers the watchdog starts again: everything else on the VM is stopped on purpose
/// (an app mid-redeploy, a retired project), but the control plane is the only way in, so a stopped one is a failure.
pub const CONTROL_PLANE_PROJECT: &str = "vm-ops-api";
/// A swap or a flip removes the old container within seconds, so a container that has been stopped (or created and never started)
/// this long is nobody's step.
pub const EXITED_AFTER_MS: i64 = 2 * 60_000;

/// The services each project's compose file declares today (`docker compose config --services`), for the projects asked about.
pub type Declared = HashMap<String, HashSet<String>>;

/// What the watchdog remembers about one container between sweeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub last_count: i64,
    pub restarts: Vec<i64>,
    pub last_recreate_at: Option<i64>,
    /// The network owner the previous sweep found gone: the same one on the next sweep is not a compose recreate in flight.
    pub orphaned_target: Option<String>,
    /// The container id the previous sweep found unhealthy: a compose stop (a redeploy) reads unhealthy for a moment, and the
    /// recreate this would start races the redeploy's own (CI, 24 Oct: "container name already in use"), so only the same container
    /// unhealthy on two sweeps in a row is sick.
    pub unhealthy_id: Option<String>,
}

pub type History = HashMap<String, Entry>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Unhealthy,
    RestartLoop,
    /// It shares the network of a container that is gone, so it has no network left (18 Oct: the CI runner lost GitHub).
    OrphanedNetwork,
    /// It stopped and nothing started it again, and its service is still in the project's compose file.
    Exited,
    /// Every container of its project was created and none ever started (a stack never brought up, or a redeploy that died half way).
    Created,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Unhealthy => "unhealthy",
            Reason::RestartLoop => "restart_loop",
            Reason::OrphanedNetwork => "orphaned_network",
            Reason::Exited => "exited",
            Reason::Created => "created",
        }
    }
}

/// One `docker compose up --force-recreate --no-deps`: which container, why, and the compose labels that say how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub container: String,
    pub reason: Reason,
    pub project: String,
    pub service: String,
    pub working_dir: Option<String>,
    pub config_files: Vec<String>,
}

impl Action {
    /// argv for `docker compose ... up -d --force-recreate --no-deps <service>`. Without `--no-deps` compose also recreates the
    /// network sidecar the service depends on, which strands every sibling sharing that sidecar's network. A created stack is
    /// brought up whole, with a plain `up -d`.
    pub fn recreate_args(&self) -> Vec<String> {
        let tail: &[&str] = if self.reason == Reason::Created { &["up", "-d"] } else { &["up", "-d", "--force-recreate", "--no-deps", &self.service] };
        compose_args(&self.project, self.working_dir.as_deref(), &self.config_files, tail)
    }
}

/// `docker compose -p <project> [--project-directory <dir>] [-f <file>]... <tail>`: the project as its labels say it was started.
pub fn compose_args(project: &str, working_dir: Option<&str>, config_files: &[String], tail: &[&str]) -> Vec<String> {
    let mut args: Vec<String> = ["compose", "-p", project].map(String::from).to_vec();
    if let Some(dir) = working_dir {
        args.extend(["--project-directory".to_string(), dir.to_string()]);
    }
    for file in config_files {
        args.extend(["-f".to_string(), file.clone()]);
    }
    args.extend(tail.iter().map(|word| word.to_string()));
    args
}

/// The control plane's containers that have been stopped for more than two minutes: the ones whose compose file is worth asking.
pub fn exited(snapshots: &[Snapshot], now: i64) -> Vec<&Snapshot> {
    let stopped = |s: &&Snapshot| s.name != SELF_NAME && s.project.as_deref() == Some(CONTROL_PLANE_PROJECT) && s.exited_at.is_some_and(|at| now - at > EXITED_AFTER_MS);
    snapshots.iter().filter(stopped).collect()
}

/// The projects none of whose containers (the daemon's own aside) ever started and all created over two minutes ago, each with the
/// one container that speaks for it (the first by name), so a stack is brought up once per sweep and once per cooldown.
pub fn created_stacks(snapshots: &[Snapshot], now: i64) -> HashMap<&str, &str> {
    let mut stacks: HashMap<&str, (bool, &str)> = HashMap::new();
    for snapshot in snapshots.iter().filter(|s| s.name != SELF_NAME) {
        let Some(project) = snapshot.project.as_deref() else { continue };
        let stuck = snapshot.created_at.is_some_and(|at| now - at > EXITED_AFTER_MS);
        let (all, speaker) = stacks.entry(project).or_insert((true, &snapshot.name));
        *all &= stuck;
        *speaker = (*speaker).min(snapshot.name.as_str());
    }
    stacks.into_iter().filter(|(_, (all, _))| *all).map(|(project, (_, speaker))| (project, speaker)).collect()
}

/// Snapshots, per-container history, the services the control plane's compose file declares and the time in, recreate actions and
/// the next history out. A container is recreated when it is unhealthy on two sweeps in a row, restarted more than three times in five minutes, running in
/// the network of a container no longer running on two sweeps in a row, or (the control plane only) stopped for more than two
/// minutes while its service is still declared, or it is the one that speaks for a project whose containers were all created over
/// two minutes ago and never started; at most once per ten minutes, and only when compose labels say how.
pub fn decide(snapshots: &[Snapshot], history: &History, declared: &Declared, now: i64) -> (Vec<Action>, History) {
    let mut next = History::new();
    let mut actions = Vec::new();
    let stopped = exited(snapshots, now);
    let created = created_stacks(snapshots, now);
    let alive = |target: &str| snapshots.iter().any(|s| s.running && !s.id.is_empty() && (s.id.starts_with(target) || target.starts_with(&s.id) || s.name == target));
    for snapshot in snapshots {
        if snapshot.name == SELF_NAME {
            continue;
        }
        let previous = history.get(&snapshot.name);
        let mut restarts: Vec<i64> = previous.map(|p| p.restarts.iter().copied().filter(|t| now - t <= LOOP_WINDOW_MS).collect()).unwrap_or_default();
        // A recreated container starts back at 0, so a drop is a new baseline, not a restart.
        let delta = previous.map(|p| snapshot.restart_count - p.last_count).unwrap_or(0);
        restarts.extend((0..delta.max(0)).map(|_| now));
        let orphaned = snapshot.network_of.as_deref().filter(|target| snapshot.running && !alive(target));
        let confirmed = orphaned.is_some() && orphaned == previous.and_then(|p| p.orphaned_target.as_deref());
        let unhealthy = snapshot.health.as_deref() == Some("unhealthy");
        let sick = unhealthy && previous.and_then(|p| p.unhealthy_id.as_deref()) == Some(snapshot.id.as_str());
        let mut entry = Entry {
            last_count: snapshot.restart_count,
            restarts,
            last_recreate_at: previous.and_then(|p| p.last_recreate_at),
            orphaned_target: orphaned.map(str::to_string),
            unhealthy_id: unhealthy.then(|| snapshot.id.clone()),
        };

        let reason = if sick {
            Some(Reason::Unhealthy)
        } else if entry.restarts.len() > LOOP_MAX_RESTARTS {
            Some(Reason::RestartLoop)
        } else if confirmed {
            Some(Reason::OrphanedNetwork)
        } else if stopped.iter().any(|s| s.name == snapshot.name) && snapshot.project.as_ref().zip(snapshot.service.as_ref()).is_some_and(|(project, service)| declared.get(project).is_some_and(|services| services.contains(service))) {
            Some(Reason::Exited)
        } else if snapshot.project.as_deref().and_then(|project| created.get(project)).is_some_and(|speaker| *speaker == snapshot.name) {
            Some(Reason::Created)
        } else {
            None
        };
        if let (Some(reason), Some(project), Some(service)) = (reason, &snapshot.project, &snapshot.service) {
            let cooling = entry.last_recreate_at.is_some_and(|at| now - at < RECREATE_COOLDOWN_MS);
            if !cooling {
                entry.last_recreate_at = Some(now);
                entry.restarts.clear();
                actions.push(Action {
                    container: snapshot.name.clone(),
                    reason,
                    project: project.clone(),
                    service: service.clone(),
                    working_dir: snapshot.working_dir.clone(),
                    config_files: snapshot.config_files.clone(),
                });
            }
        }
        next.insert(snapshot.name.clone(), entry);
    }
    (actions, next)
}
