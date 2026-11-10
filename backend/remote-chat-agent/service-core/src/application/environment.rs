//! What the service reads of the machine it runs on: its environment variables, working directory, home and temporary directories. The
//! port is read at each call, so a variable changed after start shows at once; `rca-service`'s `ProcessEnv` is the adapter and a test
//! can answer from a map.

use super::settings::Settings;
use crate::domain::util::{resolve_from, resolve_in_from};
use std::path::PathBuf;

pub trait Environment: Send + Sync {
    /// The variable as set; an empty value counts as set.
    fn var(&self, name: &str) -> Option<String>;
    fn current_dir(&self) -> Option<PathBuf>;
    /// The user's home, `/root` when there is none.
    fn home_dir(&self) -> PathBuf;
    fn temp_dir(&self) -> PathBuf;

    /// Every setting of the service, read now.
    fn settings(&self) -> Settings {
        Settings::from_lookup(&|name| self.var(name))
    }

    /// The directory opencode turns of the service run in: `OPENCODE_CWD` when it is set and not empty, else the working directory.
    fn opencode_cwd(&self) -> String {
        self.settings().opencode_cwd.filter(|v| !v.is_empty()).unwrap_or_else(|| self.current_dir_or("/"))
    }

    /// The working directory as text, `fallback` when it cannot be read.
    fn current_dir_or(&self, fallback: &str) -> String {
        self.current_dir().map_or_else(|| fallback.to_string(), |dir| dir.to_string_lossy().into_owned())
    }

    /// Node's `path.resolve(path)` against the working directory (`/` when it cannot be read).
    fn resolve(&self, path: &str) -> PathBuf {
        resolve_from(&self.current_dir().unwrap_or_else(|| PathBuf::from("/")), path)
    }

    /// Node's `path.resolve(base, rel)`.
    fn resolve_in(&self, base: &str, rel: &str) -> PathBuf {
        resolve_in_from(&self.current_dir().unwrap_or_else(|| PathBuf::from("/")), base, rel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Mapped {
        vars: HashMap<String, String>,
        cwd: Option<PathBuf>,
    }

    impl Environment for Mapped {
        fn var(&self, name: &str) -> Option<String> {
            self.vars.get(name).cloned()
        }
        fn current_dir(&self) -> Option<PathBuf> {
            self.cwd.clone()
        }
        fn home_dir(&self) -> PathBuf {
            PathBuf::from("/home/someone")
        }
        fn temp_dir(&self) -> PathBuf {
            PathBuf::from("/tmp")
        }
    }

    fn at(cwd: Option<&str>, vars: &[(&str, &str)]) -> Mapped {
        Mapped { vars: vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), cwd: cwd.map(PathBuf::from) }
    }

    #[test]
    fn given_a_working_directory_when_a_relative_path_is_resolved_then_it_is_folded_against_it_and_an_absolute_one_is_not() {
        let env = at(Some("/srv/work"), &[]);
        assert_eq!(env.resolve("a/../b/./c/"), PathBuf::from("/srv/work/b/c"));
        assert_eq!(env.resolve("/etc/../opt"), PathBuf::from("/opt"));
        assert_eq!(env.resolve_in("repo", "/abs/file"), PathBuf::from("/abs/file"));
        assert_eq!(env.resolve_in("repo", "x/../y"), PathBuf::from("/srv/work/repo/y"));
    }

    #[test]
    fn given_no_readable_working_directory_when_a_path_is_resolved_then_the_root_stands_in() {
        let env = at(None, &[]);
        assert_eq!((env.resolve("a"), env.current_dir_or("/"), env.current_dir_or("")), (PathBuf::from("/a"), "/".to_string(), String::new()));
    }

    #[test]
    fn given_an_opencode_directory_when_the_turn_directory_is_asked_then_a_set_one_wins_and_an_empty_or_missing_one_falls_back_to_the_working_directory() {
        let cwd = Some("/srv/work");
        assert_eq!(at(cwd, &[("OPENCODE_CWD", "/elsewhere")]).opencode_cwd(), "/elsewhere");
        assert_eq!(at(cwd, &[("OPENCODE_CWD", "")]).opencode_cwd(), "/srv/work");
        assert_eq!(at(cwd, &[]).opencode_cwd(), "/srv/work");
        assert_eq!(at(None, &[]).opencode_cwd(), "/");
    }

    #[test]
    fn given_variables_when_the_settings_are_read_then_they_come_through_the_port_each_time() {
        let env = at(None, &[("PORT", "9100"), ("OPENCODE_CWD", "/work")]);
        let settings = env.settings();
        assert_eq!((settings.port, settings.opencode_cwd), (9100, Some("/work".to_string())));
    }
}
