//! The rules that keep the fixed cost before the first test down: what a run does not read, it does not fetch.
use crate::support::repo_root;

fn read(path: &str) -> String {
    std::fs::read_to_string(repo_root().join(path)).unwrap_or_else(|e| panic!("{path} is readable: {e}"))
}

#[test]
fn a_core_or_cli_run_plans_no_legs_in_a_trivial_step_and_the_other_stages_still_ask_the_vault() {
    // Given the pipeline's plan job
    let text = read(".github/workflows/pipeline.yml");

    // When the step that writes the legs and the step that asks for them are read
    let none = text.find("id: nolegs").expect("a step writes the empty legs");
    let none_block = &text[none..none + 400];
    let ask = text.find("id: legs\n").expect("a step asks which services changed");
    let ask_block = &text[ask..ask + 200];

    // Then the empty step runs for core and cli only, writes the values the deploy and e2e jobs test for, and the real one runs for every other stage
    assert!(none_block.contains("inputs.stages == 'core' || inputs.stages == 'cli'"), "{none_block}");
    assert!(none_block.contains("legs=[]") && none_block.contains("eas_update=false"), "{none_block}");
    assert!(ask_block.contains("inputs.stages != 'core' && inputs.stages != 'cli'"), "{ask_block}");
    assert!(text.contains("legs: ${{ steps.legs.outputs.legs || steps.nolegs.outputs.legs }}"));
    assert!(text.contains("eas_update: ${{ steps.legs.outputs.eas_update || steps.nolegs.outputs.eas_update }}"));
}

#[test]
fn chromium_and_its_system_packages_come_from_a_cache_keyed_by_version_and_image_with_the_mirror_as_the_fallback() {
    // Given the install-test-deps action
    let text = read(".github/actions/install-test-deps/action.yml");

    // When its Playwright steps are read
    let key = text.find("id: pw-key").expect("a step keys the cache");
    let install = text.find("name: Chromium for Playwright").expect("the install step");
    let save = text.find("name: Save the Playwright cache").expect("the save step");

    // Then the key names the Playwright version and the runner image, the restore comes before the install, the .deb files install with dpkg before any apt
    // call, the mirror path stays behind them, and the saved tree carries no marker
    assert!(key < install && install < save);
    assert!(text.contains("playwright-$version-$ImageVersion-"));
    assert!(text.contains("[ -n \"${ImageVersion:-}\" ] || exit 0"));
    assert!(text.contains("dpkg -i \"$debs\"/*.deb"));
    assert!(text.find("dpkg -i \"$debs\"").unwrap() < text.find("install --with-deps chromium").unwrap());
    assert!(text.contains("rm -f \"$HOME\"/.cache/ms-playwright/.installed-*"));
    assert!(text.contains("steps.playwright.outcome == 'success'"));
    assert!(text.contains("steps.pw-prep.outputs.save == 'true'"));
}

fn job_block(text: &str, job: &str) -> String {
    let head = format!("\n  {job}:\n");
    let start = text.find(&head).unwrap_or_else(|| panic!("no job {job}")) + 1;
    let rest = &text[start + head.len() - 1..];
    let end = rest.lines().skip(1).scan(0usize, |at, l| { *at += l.len() + 1; Some((*at, l)) }).find(|(_, l)| l.starts_with("  ") && !l.starts_with("   ") && l.trim_end().ends_with(':') && !l.trim_start().starts_with('#')).map(|(at, l)| at - l.len() - 1);
    rest[..end.unwrap_or(rest.len())].to_string()
}

#[test]
fn the_browser_job_installs_chromium_beside_the_web_export_and_joins_it_before_its_tests() {
    // Given the e2e workflow
    let text = read(".github/workflows/e2e-tests.yml");

    // When the steps that start the install, build the export and join the install are located
    let start = text.find("name: Start the Chromium install").expect("a step starts the install");
    let export = text.find("name: Export the web app").expect("the export step");
    let join = text.find("- name: Chromium for the browser harness").expect("a step joins the install");
    let run = text.find("name: Run the e2e tests").expect("the run step");
    let apt = text.find("name: Install ffmpeg").expect("the apt step");

    // Then the install starts after the apt steps (the dpkg lock) and detaches before the export, and the join comes after the export and before the tests
    assert!(apt < start && start < export && export < join && join < run);
    let start_block = &text[start..export];
    assert!(start_block.contains("setsid nohup") && start_block.contains("chromium.rc") && start_block.contains("env -u RUNNER_TRACKING_ID"));
    let join_block = &text[join..run];
    assert!(join_block.contains("until [ -f \"$RUNNER_TEMP/chromium.rc\" ]") && !join_block.contains("pnpm --filter @lib/browser-client exec playwright install"));
    assert!(join_block.contains("@tooling/browser-harness run build") && join_block.contains("wf browser stop"));
    // And the old step that ran the install before the export, ahead of every other step, is gone
    assert_eq!(text.matches("playwright install --with-deps chromium").count(), 1);
}

#[test]
fn every_cache_key_has_one_writer_among_the_shards_that_restore_it() {
    // Given the pipeline, the e2e workflow and the metro and sccache actions
    let pipeline = read(".github/workflows/pipeline.yml");
    let e2e = read(".github/workflows/e2e-tests.yml");
    let metro = read(".github/actions/metro-cache/action.yml");

    // When the sccache saves of every sharded job and the metro saves are read
    for job in ["core-tests", "integration-tests", "cli-e2e-tests"] {
        let block = job_block(&pipeline, job);
        let save = block.find("mode: save\n          stage: ").and_then(|_| block.rfind("sccache-cache")).map(|at| &block[at..]).unwrap_or_else(|| panic!("{job} saves sccache"));
        // Then only shard 1 of a stage saves its sccache directory
        assert!(save.contains("save: ${{ matrix.shard == 1 }}"), "{job}: {save}");
    }
    assert!(e2e.contains("stage: ${{ inputs.platform }}-e2e-tests\n          save: ${{ inputs.shard == 1 }}"));
    // And the browser export keeps its own metro key (the web transforms), saved by shard 1, apart from the one eas-update saves
    assert!(e2e.contains("scope: export\n          suffix: web\n          save: ${{ inputs.shard == 1 }}"));
    assert!(metro.contains("inputs.save == 'true'"));
    let eas = job_block(&pipeline, "eas-update");
    assert!(eas.contains("mode: save\n          scope: export") && !eas.contains("suffix:"));
}

#[test]
fn the_production_update_is_published_beside_the_tests_and_its_pointer_moves_only_after_them() {
    // Given the pipeline's eas-update and eas-promote jobs
    let text = read(".github/workflows/pipeline.yml");
    let eas = job_block(&text, "eas-update");
    let promote = job_block(&text, "eas-promote");
    let line = |block: &str, key: &str| block.lines().find(|l| l.starts_with(&format!("    {key}:"))).unwrap_or_else(|| panic!("no {key}")).to_string();

    // When what each job waits for and reads is compared
    let (needs, cond) = (line(&eas, "needs"), line(&eas, "if"));
    let (p_needs, p_cond) = (line(&promote, "needs"), line(&promote, "if"));

    // Then the publish needs the plan and the wf build only, and no test stage nor the release of wf
    assert_eq!(needs, "    needs: [plan, wf-build]");
    for t in ["core-tests", "integration-tests", "cli-e2e-tests", "clis"] {
        assert!(!needs.contains(t) && !cond.contains(t), "{t}");
        assert!(p_needs.contains(t), "eas-promote needs {t}");
    }
    // And it no longer writes the state ref, which is the promote job's step, run only for the head of dev after every test stage passed or was skipped
    assert!(!eas.contains("wf bucket state write") && eas.contains("name: eas-pointer-production"));
    assert!(promote.contains("wf bucket state write") && promote.contains("name: eas-pointer-production"));
    assert!(p_cond.contains("needs.eas-update.result == 'success'") && p_cond.contains("needs.plan.outputs.is_head == 'true'"));
    for t in ["core-tests", "integration-tests", "cli-e2e-tests"] {
        assert!(p_cond.contains(&format!("contains(fromJSON('[\"success\",\"skipped\"]'), needs.{t}.result)")), "{p_cond}");
    }
    assert!(text.contains("deployment-verification-tests, eas-update, eas-promote, eas-update-e2e"));
}

#[test]
fn a_job_that_only_serves_the_head_of_dev_is_skipped_for_a_superseded_commit() {
    // Given the pipeline
    let text = read(".github/workflows/pipeline.yml");

    // When the conditions of the head-only jobs are read
    for job in ["deploy-build", "deploy", "eas-update", "eas-promote", "eas-update-e2e", "deployment-verification-tests", "android-e2e-tests"] {
        let cond = job_block(&text, job).lines().find(|l| l.starts_with("    if:")).unwrap_or_else(|| panic!("no if for {job}")).to_string();
        // Then each asks the plan whether this commit is the head
        assert!(cond.contains("needs.plan.outputs.is_head == 'true'"), "{job}: {cond}");
    }
    // And the test stages are not head-only: a red superseded commit still reports
    for job in ["core-tests", "integration-tests", "cli-e2e-tests", "browser-e2e-tests"] {
        let cond = job_block(&text, job).lines().find(|l| l.starts_with("    if:")).unwrap().to_string();
        assert!(!cond.contains("is_head"), "{job}: {cond}");
    }
}

#[test]
fn an_integration_or_cli_e2e_shard_a_job_already_proved_is_left_out_of_the_matrix_and_a_passing_shard_saves_its_marker() {
    // Given the pipeline
    let text = read(".github/workflows/pipeline.yml");
    let plan = job_block(&text, "plan");

    // When the plan job's proven step and the two jobs' matrices are read
    assert!(plan.contains("wf ci proven --stage \"$stage\"") && plan.contains("continue-on-error: true"));
    for (job, shards, keys) in [("integration-tests", "integration_run_shards", "integration_proof_keys"), ("cli-e2e-tests", "cli_e2e_run_shards", "cli_e2e_proof_keys")] {
        let block = job_block(&text, job);
        // Then the matrix is the run list, an empty list skips the job, the marker is a cache entry saved only after a pass on a run that is not fresh, and never a state-ref push
        assert!(block.contains(&format!("shard: ${{{{ fromJSON(needs.plan.outputs.{shards}) }}}}")), "{job}");
        assert!(block.contains(&format!("needs.plan.outputs.{shards} != '[]'")), "{job}");
        assert!(block.contains(&format!("fromJSON(needs.plan.outputs.{keys})[matrix.shard]")) && block.contains("success() && needs.plan.outputs.fresh != 'true'"), "{job}");
        assert!(block.contains("actions/cache/save@") && !block.contains("state write"), "{job}");
    }
    // And core-tests keeps every shard: its shards run packages, which a fingerprint cannot prove
    assert!(job_block(&text, "core-tests").contains("shard: ${{ fromJSON(needs.plan.outputs.shards) }}"));
}
