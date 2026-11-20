//! The desktop's source control rows through the real router over the agent double: the commits of a branch a review lists, the ignored files of a
//! project, and the star of a repository. The double answers what the agent API's `git` contract recorded; what each request becomes (the `Op`
//! asked) and what the phone is told are both read here.
use super::support::*;
use t3_core::ports::{AgentError, Op};

const ROOT: &str = "/workspace/demo";

fn recorded(method: &str, name: &str) -> Value {
    let fixtures = agent_api_git::testing::fixtures();
    let found = fixtures.iter().flat_map(|f| &f.exchanges).find(|e| e.request["method"] == method && e.name.contains(name) && e.succeeds());
    found.unwrap_or_else(|| panic!("no recorded {method} exchange named like {name}")).response["result"].clone()
}

fn started() -> (Harness, Rpc) {
    let h = Harness::start();
    let all = PairingRequest { scopes: t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect(), ..PairingRequest::default() };
    let rpc = connect_with(&h, all);
    (h, rpc)
}

fn reply(h: &Harness, method: &'static str, answer: Result<Value, AgentError>) {
    h.agent.replies.lock().unwrap().insert(method, answer);
}

fn ops(h: &Harness) -> Vec<Op> {
    h.agent.ops.lock().unwrap().clone()
}

fn on_topic() -> Value {
    let mut upstream = recorded("git.upstreamStatus", "no upstream");
    upstream["branch"] = json!("topic");
    upstream
}

#[test]
fn a_branch_with_commits_beyond_main_lists_them_newest_first_with_the_subject_and_the_time_the_executor_gave() {
    // Given a repository on a topic branch whose comparison with main holds the commit the contract recorded
    let (h, mut rpc) = started();
    reply(&h, "git.upstreamStatus", Ok(on_topic()));
    let compared = recorded("git.compare", "compares a branch with main");
    reply(&h, "git.compare", Ok(compared.clone()));
    // When the phone asks for the branch's commits
    let exit = rpc.call("review.getBranchCommits", json!({"cwd": ROOT}));
    // Then each commit is its sha, subject and the time as an ISO string, and nothing is cut
    let result = success(&exit);
    let first = &compared["commits"][0];
    assert_eq!(result["truncated"], json!(false));
    assert_eq!((result["commits"][0]["sha"].clone(), result["commits"][0]["subject"].clone()), (first["sha"].clone(), first["subject"].clone()));
    assert!(result["commits"][0]["committedAt"].as_str().is_some_and(|t| t.starts_with("20") && t.ends_with('Z')), "{result}");
    // And the agent API was asked for the branch and then for the comparison of main with HEAD
    assert_eq!(ops(&h), [Op::GitUpstream { root: ROOT.into() }, Op::GitCompare { root: ROOT.into(), base: "main".into(), head: "HEAD".into(), limit: 101 }]);
}

#[test]
fn a_base_the_phone_names_is_the_one_compared_and_one_the_executor_refuses_is_the_declared_detection_error() {
    // Given an executor that finds no such base
    let (h, mut rpc) = started();
    reply(&h, "git.upstreamStatus", Ok(on_topic()));
    reply(&h, "git.compare", Err(AgentError::Refused { code: "NOT_FOUND".into(), message: "git compare: nope is not a commit".into() }));
    // When the phone names it
    let exit = rpc.call("review.getBranchCommits", json!({"cwd": ROOT, "baseRef": "nope"}));
    // Then the failure is a VcsRepositoryDetectionError with the executor's words, and only that base was tried
    let error = failure(&exit);
    assert_eq!((error["_tag"].as_str(), error["detail"].as_str(), error["operation"].as_str()), (Some("VcsRepositoryDetectionError"), Some("git compare: nope is not a commit"), Some("ReviewService.getBranchCommits")));
    assert_eq!(ops(&h).iter().filter(|o| matches!(o, Op::GitCompare { .. })).count(), 1);
}

#[test]
fn a_branch_with_no_base_to_compare_with_and_a_directory_that_is_no_repository_list_no_commits() {
    // Given an executor that finds neither main nor master, and then one that says the directory is no repository
    let (h, mut rpc) = started();
    reply(&h, "git.upstreamStatus", Ok(on_topic()));
    reply(&h, "git.compare", Err(AgentError::Refused { code: "NOT_FOUND".into(), message: "git compare: main is not a commit".into() }));
    // When the phone asks for the commits
    let none = rpc.call("review.getBranchCommits", json!({"cwd": ROOT}));
    reply(&h, "git.upstreamStatus", Err(AgentError::Refused { code: "IO_ERROR".into(), message: "git upstream: fatal: not a git repository".into() }));
    let outside = rpc.call("review.getBranchCommits", json!({"cwd": ROOT}));
    // Then both are an empty list, the first after trying main and then master
    assert_eq!((success(&none).clone(), success(&outside).clone()), (json!({"commits": [], "truncated": false}), json!({"commits": [], "truncated": false})));
    assert_eq!(ops(&h).iter().filter_map(|o| if let Op::GitCompare { base, .. } = o { Some(base.as_str()) } else { None }).collect::<Vec<_>>(), ["main", "master"]);
}

#[test]
fn a_relative_directory_is_the_review_error_the_pinned_server_gives_for_a_directory_beyond_the_workspace() {
    // Given a connected phone
    let (h, mut rpc) = started();
    // When it asks for the commits of a directory that is no absolute path
    let exit = rpc.call("review.getBranchCommits", json!({"cwd": "a"}));
    // Then the failure is the one the pin recorded, and no executor was asked
    let error = failure(&exit);
    assert_eq!((error["_tag"].as_str(), error["detail"].as_str(), error["cwd"].as_str()), (Some("VcsRepositoryDetectionError"), Some("Review commit list cwd must stay within the configured workspace root."), Some("a")));
    assert!(ops(&h).is_empty());
}

#[test]
fn ignored_files_are_the_executors_with_their_sizes_as_byte_lengths() {
    // Given a repository whose ignored files the contract recorded
    let (h, mut rpc) = started();
    let found = recorded("git.ignored", "lists the ignored files");
    reply(&h, "git.ignored", Ok(found));
    // When the phone lists them
    let exit = rpc.call("projects.listIgnoredFiles", json!({"cwd": ROOT}));
    // Then the file is its path and byte length, and the list is whole
    assert_eq!(success(&exit), &json!({"files": [{"path": "debug.log", "byteLength": 4}], "truncated": false}));
    assert!(ops(&h).contains(&Op::GitIgnored { root: ROOT.into() }));
}

#[test]
fn a_failure_of_git_lists_no_ignored_files_and_a_missing_workspace_is_the_declared_list_error() {
    // Given an executor whose git fails
    let (h, mut rpc) = started();
    reply(&h, "git.ignored", Err(AgentError::Refused { code: "IO_ERROR".into(), message: "git ls-files: fatal: not a git repository".into() }));
    // When the phone lists the ignored files
    let none = rpc.call("projects.listIgnoredFiles", json!({"cwd": ROOT}));
    // And then the executor cannot list the folder at all
    *h.agent.dir_error.lock().unwrap() = Some(AgentError::Refused { code: "NOT_FOUND".into(), message: "no such directory".into() });
    let missing = rpc.call("projects.listIgnoredFiles", json!({"cwd": "a"}));
    // Then the first is an empty list, and the second the ProjectListEntriesError the pin recorded
    assert_eq!(success(&none), &json!({"files": [], "truncated": false}));
    let error = failure(&missing);
    assert_eq!((error["_tag"].as_str(), error["failure"].as_str(), error["message"].as_str(), error["cwd"].as_str()), (Some("ProjectListEntriesError"), Some("workspace_root_not_found"), Some("Failed to list workspace entries in 'a'."), Some("a")));
}

#[test]
fn a_github_repository_is_starred_as_the_executors_account_says_and_any_other_provider_or_refusal_is_false() {
    // Given an account that has starred the repository
    let (h, mut rpc) = started();
    reply(&h, "sourceControl.starred", Ok(json!({"starred": true})));
    reply(&h, "sourceControl.star", Ok(json!({"starred": true})));
    let payload = json!({"provider": "github", "host": "github.com", "repository": "octo/hello"});
    // When the phone asks whether it is starred and stars it
    let starred = rpc.call("sourceControl.starred", payload.clone());
    let star = rpc.call("sourceControl.star", payload.clone());
    // And asks about a provider that has no star, and about one whose executor refuses
    let other = rpc.call("sourceControl.starred", json!({"provider": "gitlab", "host": "gitlab.com", "repository": "grp/proj"}));
    reply(&h, "sourceControl.star", Err(AgentError::Refused { code: "CLI_NOT_INSTALLED".into(), message: "gh is not installed".into() }));
    let refused = rpc.call("sourceControl.star", payload);
    // Then the first two are true, the others false
    assert_eq!([success(&starred), success(&star), success(&other), success(&refused)], [&json!(true), &json!(true), &json!(false), &json!(false)]);
    // And the executor was asked with the repository and no host for github.com, and never for gitlab
    let hosted: Vec<Op> = ops(&h).into_iter().filter(|o| matches!(o, Op::Hosted { .. })).collect();
    assert_eq!(hosted.len(), 3);
    assert_eq!(hosted[0], Op::Hosted { method: "sourceControl.starred", root: String::new(), params: json!({"provider": "github", "repo": "octo/hello"}) });
}
