//! The CI callers (`ci-hosted.yml`, `cold-ci.yml`) share one pipeline: they hold their triggers, name, concurrency group, permissions and
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
fn every_input_and_secret_a_caller_passes_is_declared_by_the_pipeline() {
    // Given the callers' `with:` and `secrets:` and the pipeline's declared inputs and secrets
    let declared_inputs = keys_under(&workflow("pipeline.yml"), "    inputs:");
    let declared_secrets = keys_under(&workflow("pipeline.yml"), "    secrets:");

    for caller in ["ci-hosted.yml", "cold-ci.yml"] {
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
