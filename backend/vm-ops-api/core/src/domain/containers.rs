//! A container as the Engine lists it, which of the listed ones belong to an app, and which of those is the app's own.
use serde::Deserialize;

/// The fields of one entry of `GET /containers/json` the service reads; `Names` carry the leading slash the Engine adds.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct DockerContainer {
    #[serde(rename = "Id")]
    pub id: String,
    #[serde(rename = "Names")]
    pub names: Option<Vec<String>>,
    #[serde(rename = "State")]
    pub state: Option<String>,
    #[serde(rename = "Status")]
    pub status: Option<String>,
    #[serde(rename = "Image")]
    pub image: Option<String>,
}

impl DockerContainer {
    fn names(&self) -> &[String] {
        self.names.as_deref().unwrap_or_default()
    }
}

/// The listed containers that carry one of the app's exact names, in the Engine's order and whatever their state: the Engine's
/// own `name` filter is not anchored, so it would also return `remote-chat-agent-old`.
pub fn of_app(listed: Vec<DockerContainer>, app_container_names: &[&str]) -> Vec<DockerContainer> {
    let is_the_apps = |name: &String| name.strip_prefix('/').is_some_and(|bare| app_container_names.contains(&bare));
    listed.into_iter().filter(|c| c.names().iter().any(is_the_apps)).collect()
}

/// The app's own running container: the one named exactly `name` (its compose `container_name`). Its role siblings (sessions, dind) and the
/// tailscale sidecar never stand in for it: a stopped app is not running, not a sibling that happens to be up. One whose State is not `running`
/// is never picked.
pub fn pick_app_container<'a>(containers: &'a [DockerContainer], name: &str) -> Option<&'a DockerContainer> {
    containers.iter().find(|c| c.state.as_deref() == Some("running") && c.names().iter().any(|n| n.strip_prefix('/').unwrap_or(n) == name))
}

/// The container's name without the Engine's leading slash, or `fallback` when it has none (or an empty first one).
pub fn container_name_or<'a>(c: &'a DockerContainer, fallback: &'a str) -> &'a str {
    let name = c.names().first().map(String::as_str).filter(|name| !name.is_empty()).unwrap_or(fallback);
    name.strip_prefix('/').unwrap_or(name)
}

pub fn container_name(c: &DockerContainer) -> &str {
    container_name_or(c, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn container(name: &str) -> DockerContainer {
        DockerContainer { id: name.to_string(), names: Some(vec![format!("/{name}")]), state: Some("running".to_string()), ..Default::default() }
    }

    fn with_names(id: &str, names: &[&str]) -> DockerContainer {
        DockerContainer { id: id.to_string(), names: Some(names.iter().map(|n| n.to_string()).collect()), state: Some("running".to_string()), ..Default::default() }
    }

    fn picked(containers: &[DockerContainer], name: &str) -> Option<String> {
        pick_app_container(containers, name).map(|c| c.id.clone())
    }

    #[test]
    fn given_a_split_app_when_its_own_name_is_asked_then_that_container_is_picked_wherever_it_is_listed() {
        let own = container("agent-shell");
        let siblings = [container("agent-shell-sessions"), container("agent-shell-dind"), container("agent-shell-tailscale")];
        let listed: Vec<DockerContainer> = siblings.iter().cloned().chain([own.clone()]).collect();
        assert_eq!(picked(&listed, "agent-shell"), Some("agent-shell".to_string()));
        let listed: Vec<DockerContainer> = [own].into_iter().chain(siblings).collect();
        assert_eq!(picked(&listed, "agent-shell"), Some("agent-shell".to_string()));
    }

    #[test]
    fn given_the_app_container_is_down_when_its_siblings_run_then_nothing_is_picked() {
        let listed = [container("agent-shell-sessions"), container("agent-shell-dind"), container("agent-shell-tailscale")];
        assert_eq!(picked(&listed, "agent-shell"), None);
    }

    #[test]
    fn given_an_empty_list_when_picking_then_nothing_is_picked() {
        assert_eq!(picked(&[], "agent-shell"), None);
    }

    #[test]
    fn given_a_container_that_is_not_running_when_picking_then_it_is_skipped() {
        let stopped = |state: Option<&str>| DockerContainer { state: state.map(str::to_string), ..container("agent-shell") };
        assert_eq!(picked(&[stopped(Some("exited")), container("agent-shell-dind")], "agent-shell"), None);
        assert_eq!(picked(&[stopped(Some("created")), stopped(None)], "agent-shell"), None);
        assert_eq!(picked(&[stopped(Some("exited")), container("agent-shell")], "agent-shell"), Some("agent-shell".to_string()));
    }

    #[test]
    fn given_names_that_only_start_or_end_like_the_app_when_picking_then_only_the_exact_name_matches() {
        assert_eq!(picked(&[container("agent-shell-x"), container("my-agent-shell"), container("agent-shell2")], "agent-shell"), None);
    }

    #[test]
    fn given_a_container_with_several_names_when_one_is_exact_then_it_is_picked() {
        assert_eq!(picked(&[with_names("x", &["/other", "/agent-shell"])], "agent-shell"), Some("x".to_string()));
        assert_eq!(picked(&[with_names("x", &["agent-shell"])], "agent-shell"), Some("x".to_string()));
    }

    #[test]
    fn given_a_container_with_no_names_when_picking_then_it_is_never_the_app() {
        let nameless = DockerContainer { id: "bare".to_string(), state: Some("running".to_string()), ..Default::default() };
        assert_eq!(picked(&[nameless], "agent-shell"), None);
    }
}
