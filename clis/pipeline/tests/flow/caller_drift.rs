//! The CI callers (`ci-hosted-backend-core.yml`, `cold-ci.yml`) share one pipeline: they hold their triggers, name, concurrency group, permissions and
//! `with:`, and no job logic.
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
fn the_hosted_caller_adds_only_a_trigger_gate_and_calls_the_same_pipeline() {
    // Given the hosted caller
    let hosted = workflow("ci-hosted-backend-core.yml");

    // When its jobs and the `run` job's content are read
    let names = jobs(&hosted);
    let run = job_block(&hosted, "run");

    // Then it holds the commit gate and the `run` job, which calls pipeline.yml with no runner of its own and no steps
    assert_eq!(names, set(&["trigger", "run"]), "ci-hosted-backend-core.yml jobs");
    assert!(run.contains("    uses: ./.github/workflows/pipeline.yml\n"), "ci-hosted-backend-core.yml calls pipeline.yml");
    assert!(!run.contains("runs-on:") && !run.contains("steps:"), "the hosted run job holds job logic");
}

#[test]
fn the_cli_deploy_and_e2e_callers_are_runs_of_their_own_of_the_same_pipeline_each_for_one_stage_with_a_label_and_a_group_of_its_own() {
    for (caller, stage, group) in [
        ("ci-hosted-cli-publish.yml", "cli", "ci-hosted-cli-publish-"),
        ("ci-hosted-backend-deploy.yml", "deploy", "ci-hosted-backend-deploy-"),
        ("ci-hosted-mobile-publish.yml", "e2e", "ci-hosted-mobile-publish-"),
        ("ci-hosted-mobile-core.yml", "core", "ci-hosted-mobile-core-"),
        ("ci-hosted-cli-core.yml", "core", "ci-hosted-cli-core-"),
    ] {
        // Given a stage caller
        let text = workflow(caller);

        // When its jobs, its `run` job and its concurrency group are read
        let run = job_block(&text, "run");

        // Then it holds the commit gate and a `run` that calls the pipeline for its one stage, under its own label and its own group
        assert_eq!(jobs(&text), set(&["trigger", "run"]), "{caller} jobs");
        assert!(run.contains("    uses: ./.github/workflows/pipeline.yml\n") && !run.contains("runs-on:") && !run.contains("steps:"), "{caller} holds job logic");
        assert!(run.contains(&format!("      stages: {stage}\n")), "{caller} runs the {stage} stage");
        assert!(run.lines().any(|l| l.starts_with("      label: ") && l.len() > "      label: ".len()), "{caller} has a label of its own, so its pull request comment is its own");
        assert!(text.starts_with(&format!("name: {}\n", run.lines().find_map(|l| l.strip_prefix("      label: ")).unwrap_or_default())), "{caller} is named by its label");
        assert!(text.contains(&format!("  group: {group}${{{{ github.sha }}}}\n")), "{caller} has a concurrency group of its own");
    }
    let backend = job_block(&workflow("ci-hosted-backend-core.yml"), "run").to_string();
    assert!(backend.contains("      stages: core\n") && backend.contains("      area: backend\n"), "ci-hosted-backend-core.yml runs the core stage of the backend services");
}

#[test]
fn every_input_and_secret_a_caller_passes_is_declared_by_the_pipeline() {
    // Given the callers' `with:` and `secrets:` and the pipeline's declared inputs and secrets
    let declared_inputs = keys_under(&workflow("pipeline.yml"), "    inputs:");
    let declared_secrets = keys_under(&workflow("pipeline.yml"), "    secrets:");

    for caller in ["ci-hosted-backend-core.yml", "ci-hosted-cli-publish.yml", "ci-hosted-backend-deploy.yml", "ci-hosted-mobile-publish.yml", "ci-hosted-mobile-core.yml", "ci-hosted-cli-core.yml", "cold-ci.yml"] {
        // When what each passes is compared with what the pipeline declares
        let run = job_block(&workflow(caller), "run").to_string();
        let inputs = keys_under(&run, "    with:");
        let secrets = keys_under(&run, "    secrets:");
        let undeclared: Vec<&String> = inputs.iter().filter(|k| !declared_inputs.contains(*k)).collect();
        let unknown: Vec<&String> = secrets.iter().filter(|k| !declared_secrets.contains(*k)).collect();

        // Then the pipeline declares all of them
        assert!(undeclared.is_empty(), "{caller} passes inputs pipeline.yml does not declare: {undeclared:?}");
        assert!(unknown.is_empty(), "{caller} passes secrets pipeline.yml does not declare: {unknown:?}");
    }
}

#[test]
fn the_six_callers_pin_their_file_name_workflow_name_label_and_group_together() {
    for (file, name, stage) in [
        ("ci-hosted-backend-core.yml", "backend_core+lint+pre-release-testing", "core"),
        ("ci-hosted-mobile-core.yml", "mobile_core+lint+pre-release-testing", "core"),
        ("ci-hosted-cli-core.yml", "cli_core+lint+pre-release-testing", "core"),
        ("ci-hosted-backend-deploy.yml", "backend_build+deploy+release-testing", "deploy"),
        ("ci-hosted-mobile-publish.yml", "mobile_build+publish+release-testing", "e2e"),
        ("ci-hosted-cli-publish.yml", "cli_build+publish+release-testing", "cli"),
    ] {
        // Given a caller file
        let text = workflow(file);
        let run = job_block(&text, "run");

        // When its workflow name, label, stage and concurrency group are read
        let stem = file.strip_suffix(".yml").unwrap_or(file);

        // Then the name and the label are the agreed name, and the group is its file stem plus the commit
        assert!(text.starts_with(&format!("name: {name}\n")), "{file} is named {name}");
        assert!(run.contains(&format!("      label: {name}\n")), "{file} is labelled {name}");
        assert!(run.contains(&format!("      stages: {stage}\n")), "{file} runs the {stage} stage");
        assert!(text.contains(&format!("  group: {stem}-${{{{ github.sha }}}}\n")), "{file} has the group {stem}-<sha>");
        assert!(text.contains("  cancel-in-progress: false\n"), "{file} never cancels a running run");
    }
    for old in ["ci-hosted.yml", "ci-hosted-core-mobile.yml", "ci-hosted-core-cli.yml", "ci-hosted-service-deploy.yml", "ci-hosted-mobile-e2e.yml"] {
        assert!(!repo_root().join(".github/workflows").join(old).exists(), "{old} is renamed");
    }
}

#[test]
fn the_pipeline_group_scheme_shares_shard_groups_per_workflow_and_deploy_groups_across_workflows_and_guards_nothing_else() {
    // Given the pipeline's text
    let text = workflow("pipeline.yml");

    // When every `group:` line under a `concurrency:` key is read
    let groups: Vec<&str> = text.lines().filter(|l| l.trim_start().starts_with("group:")).collect();
    let cancels: Vec<&str> = text.lines().filter(|l| l.trim_start().starts_with("cancel-in-progress:")).collect();

    // Then the sharded jobs name github.workflow, the deploy jobs name the app only, and no job is guarded by a group that ends in a run id
    assert_eq!(groups.len(), 5, "five jobs have a concurrency group: {groups:?}");
    for job in ["core-tests", "integration", "cli-e2e-tests"] {
        let prefix = format!("group: ${{{{ format('hosted-{job}-{{0}}-");
        let hits: Vec<&&str> = groups.iter().filter(|g| g.trim_start().starts_with(&prefix)).collect();
        assert_eq!(hits.len(), 1, "one group follows hosted-{job}-<workflow>-<key>: {groups:?}");
        assert!(hits[0].contains(", github.workflow, "), "{job} passes github.workflow as the first argument: {}", hits[0]);
    }
    for (job, line) in [("deploy-build", "group: ${{ format('hosted-deploy-build-{0}', matrix.app) }}"), ("deploy", "group: ${{ format('hosted-deploy-{0}', matrix.app) }}")] {
        assert!(groups.iter().any(|g| g.trim() == line), "{job} is guarded per app across workflows: {groups:?}");
    }
    assert!(groups.iter().all(|g| !g.contains("run_id")), "no group ends in a run id: {groups:?}");
    assert_eq!(cancels.len(), groups.len(), "every group states cancel-in-progress");
    assert!(cancels.iter().all(|c| c.trim() == "cancel-in-progress: false"), "no group cancels a running job: {cancels:?}");
}
