//! The two CI callers share one pipeline (docs/solutions/ci/one-pipeline-two-flavors.md): `deploy.yml` (self-hosted) and `ci-hosted.yml`
//! (GitHub-hosted) may differ in their triggers, name, concurrency group, permissions and `with:`, and in nothing that is job logic.
use crate::support::repo_root;
use std::collections::BTreeSet;

fn workflow(name: &str) -> String {
    std::fs::read_to_string(repo_root().join(".github/workflows").join(name)).unwrap_or_else(|e| panic!("{name} is readable: {e}"))
}

/// The keys one level below `header` (a whole line, at its own indent), read as text: no YAML crate is in the workspace.
fn keys_under(text: &str, header: &str) -> BTreeSet<String> {
    let indent = header.len() - header.trim_start().len();
    let mut lines = text.lines().skip_while(|l| *l != header);
    lines.next();
    let child = indent + 2;
    lines
        .take_while(|l| l.trim().is_empty() || l.len() - l.trim_start().len() >= child)
        .filter(|l| l.len() - l.trim_start().len() == child && !l.trim_start().starts_with('#'))
        .filter_map(|l| l.trim_start().split(':').next().map(str::to_string))
        .collect()
}

fn jobs(text: &str) -> BTreeSet<String> {
    keys_under(text, "jobs:")
}

/// The text of one job, from its key to the next job (or the end).
fn job_block<'a>(text: &'a str, job: &str) -> &'a str {
    let start = text.find(&format!("\n  {job}:\n")).unwrap_or_else(|| panic!("job {job} exists")) + 1;
    let rest = &text[start..];
    let mut offset = rest.find('\n').map_or(rest.len(), |n| n + 1);
    for line in rest[offset..].split_inclusive('\n') {
        if !line.trim().is_empty() && !line.trim_start().starts_with('#') && line.len() - line.trim_start().len() <= 2 {
            break;
        }
        offset += line.len();
    }
    &rest[..offset]
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn the_self_hosted_caller_has_one_job_that_only_calls_the_shared_pipeline() {
    // Given the self-hosted caller
    let vm = workflow("deploy.yml");

    // When its jobs and their content are read
    let names = jobs(&vm);

    // Then it holds the one `run` job, a call of pipeline.yml with no runner of its own and no steps
    assert_eq!(names, set(&["run"]), "deploy.yml jobs");
    assert!(vm.contains("    uses: ./.github/workflows/pipeline.yml\n"), "deploy.yml calls pipeline.yml");
    assert!(!vm.contains("runs-on:") && !vm.contains("steps:"), "deploy.yml holds job logic");
}

#[test]
fn the_hosted_caller_adds_only_a_trigger_gate_and_calls_the_same_pipeline() {
    // Given the hosted caller
    let hosted = workflow("ci-hosted.yml");

    // When its jobs and the `run` job's content are read
    let names = jobs(&hosted);
    let run = job_block(&hosted, "run");

    // Then it holds the commit gate and the `run` job, which calls pipeline.yml with no runner of its own and no steps
    assert_eq!(names, set(&["trigger", "run"]), "ci-hosted.yml jobs");
    assert!(run.contains("    uses: ./.github/workflows/pipeline.yml\n"), "ci-hosted.yml calls pipeline.yml");
    assert!(!run.contains("runs-on:") && !run.contains("steps:"), "the hosted run job holds job logic");
}

#[test]
fn the_two_callers_pass_the_same_inputs_except_the_comment_label_and_every_input_is_declared() {
    // Given both callers' `with:` and the pipeline's declared inputs
    let vm = keys_under(job_block(&workflow("deploy.yml"), "run"), "    with:");
    let hosted = keys_under(job_block(&workflow("ci-hosted.yml"), "run"), "    with:");
    let declared = keys_under(&workflow("pipeline.yml"), "    inputs:");

    // When they are compared
    let only_hosted: BTreeSet<String> = hosted.difference(&vm).cloned().collect();
    let only_vm: BTreeSet<String> = vm.difference(&hosted).cloned().collect();
    let undeclared: Vec<&String> = vm.union(&hosted).filter(|k| !declared.contains(*k)).collect();

    // Then the hosted caller adds only `label`, the self-hosted one lacks nothing, and the pipeline declares every input passed
    assert_eq!(only_hosted, set(&["label"]), "inputs only the hosted caller passes");
    assert!(only_vm.is_empty(), "inputs only the self-hosted caller passes: {only_vm:?}");
    assert!(undeclared.is_empty(), "inputs passed but not declared by pipeline.yml: {undeclared:?}");
}

#[test]
fn the_self_hosted_caller_passes_no_secret_the_hosted_one_does_not() {
    // Given both callers' `secrets:`
    let vm = keys_under(job_block(&workflow("deploy.yml"), "run"), "    secrets:");
    let hosted = keys_under(job_block(&workflow("ci-hosted.yml"), "run"), "    secrets:");

    // When the self-hosted names are checked against the hosted ones
    let missing: Vec<&String> = vm.difference(&hosted).collect();

    // Then the hosted caller passes every secret the self-hosted one does, and its own tailnet-join pair on top
    assert!(missing.is_empty(), "secrets only the self-hosted caller passes: {missing:?}");
    assert_eq!(hosted.difference(&vm).cloned().collect::<BTreeSet<_>>(), set(&["TS_HOSTED_OAUTH_CLIENT_ID", "TS_HOSTED_OAUTH_CLIENT_SECRET"]));
}
