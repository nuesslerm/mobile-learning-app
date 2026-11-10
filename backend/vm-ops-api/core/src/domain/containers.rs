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

/// The app's own running container: role siblings and the tailscale sidecar carry a suffix after its name, so it is the shortest
/// first name that is not a sidecar's. A tie keeps the earlier container; one whose State is not `running` is never picked.
pub fn pick_app_container(containers: &[DockerContainer]) -> Option<&DockerContainer> {
    let mut candidates = containers.iter().filter(|c| c.state.as_deref() == Some("running") && !c.names().iter().any(|n| n.ends_with("-tailscale")));
    let first = candidates.next()?;
    Some(candidates.fold(first, |best, c| if primary_length(c) < primary_length(best) { c } else { best }))
}

/// The length JavaScript's `String#length` gives the container's first name: UTF-16 code units, 0 when it has none.
fn primary_length(c: &DockerContainer) -> usize {
    c.names().first().map_or(0, |name| name.encode_utf16().count())
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

    fn picked_id(containers: &[DockerContainer]) -> Option<&str> {
        pick_app_container(containers).map(|c| c.id.as_str())
    }

    fn picked_names(containers: &[DockerContainer]) -> Option<Vec<String>> {
        pick_app_container(containers).map(|c| c.names.clone().expect("a named container"))
    }

    #[test]
    fn given_a_split_app_whose_containers_include_role_siblings_when_picking_then_the_role_siblings_are_never_chosen() {
        let listed = [
            container("agent-shell-sessions"),
            container("agent-shell-dind"),
            container("agent-shell"),
            container("agent-shell-tailscale"),
        ];
        assert_eq!(picked_names(&listed), Some(vec!["/agent-shell".to_string()]));
    }

    #[test]
    fn given_a_role_sibling_listed_first_when_picking_then_the_app_container_still_wins() {
        let listed = [container("remote-chat-agent-opencode"), container("remote-chat-agent"), container("remote-chat-agent-tailscale")];
        assert_eq!(picked_names(&listed), Some(vec!["/remote-chat-agent".to_string()]));
    }

    #[test]
    fn given_an_unsplit_app_when_picking_then_the_single_non_tailscale_container_is_returned() {
        let listed = [container("reporting-service-tailscale"), container("reporting-service")];
        assert_eq!(picked_names(&listed), Some(vec!["/reporting-service".to_string()]));
    }

    #[test]
    fn given_only_the_tailscale_sidecar_when_picking_then_nothing_is_picked() {
        assert_eq!(picked_id(&[container("leetcode-backend-tailscale")]), None);
    }

    #[test]
    fn given_an_empty_list_when_picking_then_nothing_is_picked() {
        assert_eq!(picked_id(&[]), None);
    }

    #[test]
    fn given_two_names_of_one_length_when_picking_then_the_earlier_container_wins() {
        assert_eq!(picked_id(&[with_names("a", &["/abc"]), with_names("b", &["/xyz"])]), Some("a"));
        assert_eq!(picked_id(&[with_names("b", &["/xyz"]), with_names("a", &["/abc"])]), Some("b"));
    }

    #[test]
    fn given_three_candidates_when_picking_then_the_shortest_wins_wherever_it_is_and_a_later_tie_loses() {
        assert_eq!(picked_id(&[with_names("l", &["/aaaa"]), with_names("m", &["/aa"]), with_names("s", &["/aaa"])]), Some("m"));
        assert_eq!(picked_id(&[with_names("l", &["/aaaa"]), with_names("m", &["/aa"]), with_names("s", &["/bb"])]), Some("m"));
    }

    #[test]
    fn given_a_container_with_a_sidecar_name_second_when_picking_then_it_is_a_sidecar_whatever_its_first_name() {
        let listed = [with_names("x", &["/a", "/b-tailscale"]), with_names("y", &["/long-name"])];
        assert_eq!(picked_id(&listed), Some("y"));
    }

    #[test]
    fn given_containers_with_several_names_when_picking_then_only_the_first_name_is_measured() {
        let listed = [with_names("x", &["/long-long-name", "/s"]), with_names("y", &["/mid-name"])];
        assert_eq!(picked_id(&listed), Some("y"));
    }

    #[test]
    fn given_a_container_with_no_names_when_picking_then_its_empty_name_is_the_shortest() {
        let nameless = DockerContainer { id: "bare".to_string(), state: Some("running".to_string()), ..Default::default() };
        assert_eq!(picked_id(&[with_names("named", &["/agent"]), nameless]), Some("bare"));
        assert_eq!(picked_id(&[with_names("named", &["/agent"]), with_names("empty", &[])]), Some("empty"));
    }

    #[test]
    fn given_names_with_astral_characters_when_picking_then_lengths_count_utf16_units_as_javascript_does() {
        // U+1F600 is 4 UTF-8 bytes, 2 UTF-16 units, 1 character: "/\u{1F600}" is 3 units where "/abcd" is 5 and "/ab" is 3
        assert_eq!(picked_id(&[with_names("ascii", &["/abcd"]), with_names("astral", &["/\u{1F600}"])]), Some("astral"));
        assert_eq!(picked_id(&[with_names("ab", &["/ab"]), with_names("astral", &["/\u{1F600}"])]), Some("ab"));
        assert_eq!(picked_id(&[with_names("bmp", &["/\u{e9}\u{e9}"]), with_names("ascii", &["/abc"])]), Some("bmp"));
    }

    #[test]
    fn given_a_container_that_is_not_running_when_picking_then_it_is_skipped_and_a_running_sibling_of_the_app_wins() {
        let stopped = |state: Option<&str>| DockerContainer { state: state.map(str::to_string), ..container("agent-shell") };
        assert_eq!(picked_id(&[stopped(Some("exited")), container("agent-shell-tailscale")]), None);
        assert_eq!(picked_id(&[stopped(Some("created")), stopped(None)]), None);
        assert_eq!(picked_id(&[stopped(Some("exited")), container("agent-shell-opencode")]), Some("agent-shell-opencode"));
    }

    #[test]
    fn given_names_that_only_look_like_a_sidecar_when_picking_then_only_a_suffix_makes_one() {
        assert_eq!(picked_id(&[with_names("x", &["/tailscale-agent"])]), Some("x"));
        assert_eq!(picked_id(&[with_names("x", &["-tailscale"])]), None);
    }

    #[test]
    fn given_the_engines_list_when_filtered_to_an_app_then_exact_names_stay_in_the_engines_order_in_any_state() {
        let state = |c: &str, s: &str| DockerContainer { state: Some(s.to_string()), ..container(c) };
        let listed = vec![
            state("remote-chat-agent", "running"),
            state("remote-chat-agent-old", "running"),
            state("remote-chat-agent-tailscale", "exited"),
            state("remote-chat-agent-opencode", "created"),
            state("leetcode-backend", "running"),
        ];
        let names = ["remote-chat-agent", "remote-chat-agent-tailscale", "remote-chat-agent-opencode"];
        let kept: Vec<_> = of_app(listed, &names).into_iter().map(|c| c.id).collect();
        assert_eq!(kept, ["remote-chat-agent", "remote-chat-agent-tailscale", "remote-chat-agent-opencode"]);
    }

    #[test]
    fn given_containers_without_the_slash_or_without_names_when_filtered_then_only_a_slashed_exact_name_matches() {
        let listed = vec![
            with_names("a", &["leetcode-backend"]),
            with_names("b", &["//leetcode-backend"]),
            with_names("c", &["/other", "/leetcode-backend"]),
            DockerContainer { id: "d".to_string(), ..Default::default() },
            with_names("e", &[]),
        ];
        let kept: Vec<_> = of_app(listed, &["leetcode-backend"]).into_iter().map(|c| c.id).collect();
        assert_eq!(kept, ["c"]);
    }

    #[test]
    fn given_an_app_with_no_containers_listed_when_filtered_then_nothing_is_left() {
        assert!(of_app(vec![container("agent-shell")], &[]).is_empty());
        assert!(of_app(Vec::new(), &["agent-shell"]).is_empty());
    }

    #[test]
    fn given_a_container_when_its_name_is_read_then_the_leading_slash_goes_and_a_missing_name_falls_back() {
        let named = |names: &[&str]| with_names("id", names);
        assert_eq!(container_name(&named(&["/leetcode-backend"])), "leetcode-backend");
        assert_eq!(container_name(&named(&["plain"])), "plain");
        assert_eq!(container_name(&named(&["//x"])), "/x");
        assert_eq!(container_name(&named(&["/"])), "");
        assert_eq!(container_name(&named(&["/one", "/two"])), "one");
        assert_eq!(container_name(&named(&[])), "");
        assert_eq!(container_name(&DockerContainer::default()), "");
        assert_eq!(container_name_or(&named(&["", "/second"]), "fb"), "fb");
        assert_eq!(container_name_or(&named(&[]), "z"), "z");
        assert_eq!(container_name_or(&DockerContainer::default(), "abc123"), "abc123");
        assert_eq!(container_name_or(&DockerContainer::default(), "/weird"), "weird");
    }

    #[test]
    fn given_the_engines_list_when_decoded_then_missing_and_null_fields_are_none_and_extras_are_ignored() {
        let body = r#"[
            {"Id":"r1","Names":["/remote-chat-agent"],"State":"running","Status":"Up 2 hours","Image":"ns/a:1","Labels":{"x":"y"},"Ports":[]},
            {"Id":"r4","Names":["/remote-chat-agent-opencode"],"State":"created","Image":"ns/opencode:sha-1"},
            {"Id":"n1","Names":null,"State":null}
        ]"#;
        let listed: Vec<DockerContainer> = serde_json::from_str(body).expect("the Engine's list");
        assert_eq!(listed[0].status.as_deref(), Some("Up 2 hours"));
        assert_eq!(listed[1].status, None);
        assert_eq!((listed[2].names.as_deref(), listed[2].state.as_deref()), (None, None));
        assert_eq!(container_name_or(&listed[2], &listed[2].id), "n1");
    }
}
