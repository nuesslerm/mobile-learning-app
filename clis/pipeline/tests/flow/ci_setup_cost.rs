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
    // Given the Playwright cache action, its install script, and the two places that install Chromium
    let action = read(".github/actions/playwright-cache/action.yml");
    let script = read(".github/actions/playwright-cache/install.sh");
    let deps = read(".github/actions/install-test-deps/action.yml");

    // When the action's key, restore and save and the script's two paths are read
    let key = action.find("id: key").expect("a step keys the cache");
    let restore = action.find("name: Restore Chromium and the system packages").expect("the restore step");
    let save = action.find("name: Save the Playwright cache").expect("the save step");

    // Then the key names the Playwright version and the runner image, and a runner with no image version gets no key
    assert!(key < restore && restore < save);
    assert!(action.contains("playwright-$version-$ImageVersion-"));
    assert!(action.contains("[ -n \"${ImageVersion:-}\" ] || exit 0"));
    // And the script installs the .deb files with dpkg before any apt call, with the bounded mirror path behind them
    assert!(script.contains("dpkg -i \"$debs\"/*.deb"));
    assert!(script.find("dpkg -i \"$debs\"").unwrap() < script.find("install --with-deps chromium").unwrap());
    assert!(script.contains("timeout -k 10 150") && script.contains("Keep-Downloaded-Packages"));
    // And a save happens only after a miss that left the packages and the browser, and the saved tree carries no marker
    assert!(action.contains("[ \"$HIT\" != true ] || exit 0"));
    assert!(action.contains("rm -f \"$HOME\"/.cache/ms-playwright/.installed-*"));
    assert!(action.contains("steps.prep.outputs.save == 'true'"));
    // And install-test-deps restores, installs through the script and saves, with no copy of the install of its own
    let (r, i, s) = (deps.find("mode: restore").expect("restore"), deps.find("playwright-cache/install.sh").expect("install"), deps.find("mode: save").expect("save"));
    assert!(r < i && i < s);
    assert!(!deps.contains("install --with-deps"));
}

fn job_block(text: &str, job: &str) -> String {
    let head = format!("\n  {job}:\n");
    let start = text.find(&head).unwrap_or_else(|| panic!("no job {job}")) + 1;
    let rest = &text[start + head.len() - 1..];
    // each line's start offset, counted from the first line: skipping the first line must not drop its length from the offsets
    let end = rest.split_inclusive('\n').scan(0usize, |at, l| { let start = *at; *at += l.len(); Some((start, l.trim_end_matches('\n'))) }).skip(1).find(|(_, l)| l.starts_with("  ") && !l.starts_with("   ") && l.trim_end().ends_with(':') && !l.trim_start().starts_with('#')).map(|(start, _)| start);
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
    // And the install is the cached one: the cache restores before the detached start, the browser harness job saves after the join (shard 1 only), and no copy of the install is left
    let restore = text.find("name: Start the Chromium install").and_then(|at| text[..at].rfind("playwright-cache")).expect("a restore before the start");
    assert!(restore < start && start_block.contains("playwright-cache/install.sh"));
    let saved = &text[join..run];
    assert!(saved.contains("mode: save\n          save: ${{ inputs.shard == 1 }}"), "{saved}");
    assert_eq!(text.matches("playwright install --with-deps chromium").count(), 0);
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
        // Then each shard saves the entry of its own shard (its key names the shard, so the writer of a key is one job)
        assert!(save.contains("shard: ${{ matrix.shard }}") && !save.contains("save: false"), "{job}: {save}");
    }
    assert!(e2e.contains("stage: ${{ inputs.platform }}-e2e-tests\n          shard: ${{ inputs.shard }}"));
    let sccache = read(".github/actions/sccache-cache/action.yml");
    assert!(sccache.matches("inputs.shard && format('-s{0}', inputs.shard)").count() >= 3, "restore key, restore-keys and the remembered key all name the shard");
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
    assert!(text.contains("deployment-verification-tests, codex-session-proof, eas-update, eas-promote, eas-update-e2e"));
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

#[test]
fn every_job_that_runs_a_weighted_stage_hands_its_database_rows_to_the_report_job() {
    // Given the pipeline and the e2e workflow, whose jobs run the weighted stages
    let pipeline = read(".github/workflows/pipeline.yml");
    let e2e = read(".github/workflows/e2e-tests.yml");

    // When the last steps of each stage's job are read
    for job in ["core-tests", "integration-tests", "cli-e2e-tests"] {
        let block = job_block(&pipeline, job);
        // Then each uploads the rows (the Nx tasks and unit spans its wf commands kept) under its own name
        assert!(block.contains("./.github/actions/hosted-stats\n        if: ${{ always() }}\n        with:\n          mode: upload"), "{job}");
    }
    // And so does the e2e workflow, once per platform and shard: its tests never reached CI_UNITS or NX_TASKS before this step existed
    assert!(e2e.contains("./.github/actions/hosted-stats\n        if: ${{ always() }}\n        with:\n          mode: upload\n          job: ${{ inputs.platform }}-e2e-tests-${{ inputs.shard }}"));
    // And the name matches the one the job's sampler started under (setup-job), so the samples and the rows pair up
    let name = "job: ${{ inputs.platform }}-e2e-tests-${{ inputs.shard }}";
    let setup = e2e.find("./.github/actions/setup-job").expect("the job's setup step");
    let setup_step = e2e[setup..].split("\n      - ").next().unwrap();
    assert!(setup_step.contains(name), "{setup_step}");
    // And the report job downloads every ci-db artifact by pattern
    assert!(pipeline.contains("pattern: ci-db-*"));
}

struct Install {
    home: std::path::PathBuf,
    stubs: std::path::PathBuf,
}

impl Install {
    fn new(with_deb: bool, with_marker: bool) -> Self {
        let root = tempfile::tempdir().expect("a scratch dir").keep();
        let (home, stubs) = (root.join("home"), root.join("stubs"));
        std::fs::create_dir_all(&stubs).unwrap();
        std::fs::create_dir_all(home.join("playwright-debs")).unwrap();
        std::fs::create_dir_all(home.join(".cache/ms-playwright")).unwrap();
        if with_deb {
            std::fs::write(home.join("playwright-debs/libx.deb"), "").unwrap();
        }
        if with_marker {
            std::fs::write(home.join(".cache/ms-playwright/.installed-_ersion_1.2.3"), "").unwrap();
        }
        // authored doubles of the tools the script calls: each logs its call, and pnpm/dpkg fail on request; none touches the machine's apt or sources
        let log = "echo \"$(basename \"$0\") $*\" >> \"$HOME/calls\"\n".to_string();
        let stubs_text = [
            ("pnpm", format!("{log}case \"$*\" in *--version*) echo 'Version 1.2.3';; *--with-deps*) n=$(cat \"$HOME/n\" 2>/dev/null || echo 0); echo $((n+1)) > \"$HOME/n\"; [ \"$n\" -ge \"${{FAIL_WITH_DEPS:-0}}\" ];; esac\n")),
            ("dpkg", format!("{log}[ \"${{DPKG_FAILS:-0}}\" != 1 ] || [ \"$1\" != -i ]\n")),
            ("timeout", "shift 3; exec \"$@\"\n".to_string()),
            ("sudo", "exec \"$@\"\n".to_string()),
            ("tee", format!("{log}cat > /dev/null\n")),
            ("sed", log.clone()),
            ("pkill", log),
        ];
        for (name, body) in stubs_text {
            let path = stubs.join(name);
            std::fs::write(&path, format!("#!/bin/bash\n{body}")).unwrap();
            std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        }
        Self { home, stubs }
    }

    fn run(&self, env: &[(&str, &str)]) -> (bool, Vec<String>, bool) {
        let script = repo_root().join(".github/actions/playwright-cache/install.sh");
        let path = format!("{}:/usr/bin:/bin", self.stubs.display());
        let ok = std::process::Command::new("/bin/bash").arg(script).env_clear().env("PATH", path).env("HOME", &self.home).envs(env.iter().copied())
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().expect("bash runs").success();
        let calls = std::fs::read_to_string(self.home.join("calls")).unwrap_or_default().lines().map(str::to_string).collect();
        (ok, calls, self.home.join(".cache/ms-playwright/.installed-_ersion_1.2.3").exists())
    }
}

fn count(calls: &[String], needle: &str) -> usize {
    calls.iter().filter(|c| c.contains(needle)).count()
}

#[test]
fn an_installed_chromium_is_left_alone_and_a_restored_cache_installs_by_dpkg_without_the_mirror() {
    // Given a machine whose marker says the install is done
    let done = Install::new(true, true);
    // When the script runs
    let (ok, calls, _) = done.run(&[]);
    // Then it does nothing but ask the Playwright version
    assert!(ok && count(&calls, "dpkg") == 0 && count(&calls, "--with-deps") == 0, "{calls:?}");

    // Given a restored cache with .deb files and no marker
    let hit = Install::new(true, false);
    // When the script runs
    let (ok, calls, marker) = hit.run(&[]);
    // Then dpkg installs the files, only the browser is fetched (no --with-deps, no apt configuration), and the marker is written
    assert!(ok && marker, "{calls:?}");
    assert_eq!(count(&calls, "dpkg -i"), 1);
    assert_eq!(count(&calls, "--with-deps"), 0);
    assert_eq!(count(&calls, "tee"), 0);
}

#[test]
fn a_cached_install_that_fails_falls_back_to_the_mirror_and_a_stalled_mirror_is_retried_on_another_one_up_to_three_times() {
    // Given cached .deb files that dpkg refuses
    let broken = Install::new(true, false);
    // When the script runs
    let (ok, calls, marker) = broken.run(&[("DPKG_FAILS", "1")]);
    // Then the mirror path installs once, keeps the downloads (the apt configuration is written) and writes the marker
    assert!(ok && marker, "{calls:?}");
    assert_eq!(count(&calls, "--with-deps"), 1);
    assert_eq!(count(&calls, "tee"), 1);

    // Given a cold machine whose first two mirror tries fail
    let cold = Install::new(false, false);
    // When the script runs
    let (ok, calls, marker) = cold.run(&[("FAIL_WITH_DEPS", "2")]);
    // Then the third try installs, and the sources moved to a new mirror after each failure
    assert!(ok && marker, "{calls:?}");
    assert_eq!(count(&calls, "--with-deps"), 3);
    assert_eq!(count(&calls, "pkill"), 4, "each failure kills the stuck apt and dpkg");
    assert!(count(&calls, "sed") > 0, "the sources were switched to another mirror");

    // Given a cold machine whose mirror never answers
    let dead = Install::new(false, false);
    // When the script runs
    let (ok, calls, marker) = dead.run(&[("FAIL_WITH_DEPS", "9")]);
    // Then it gives up after three tries and leaves no marker
    assert!(!ok && !marker, "{calls:?}");
    assert_eq!(count(&calls, "--with-deps"), 3);
    assert_eq!(count(&calls, "pkill"), 6);
}

#[test]
fn every_bun_install_names_an_exact_version_so_no_run_lists_tags_through_the_github_api() {
    // Given every workflow and composite action in the repository
    let root = repo_root().join(".github");
    let mut files = vec![];
    let mut dirs = vec![root];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).expect("a readable directory").flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|e| e == "yml") {
                files.push(path);
            }
        }
    }

    // When each setup-bun step is read with the lines that follow it
    let mut steps = 0;
    for file in files {
        let text = std::fs::read_to_string(&file).expect("a readable file");
        let lines: Vec<&str> = text.lines().collect();
        for (at, _) in lines.iter().enumerate().filter(|(_, l)| l.contains("uses: oven-sh/setup-bun@")) {
            steps += 1;
            let step = lines[at + 1..].iter().take_while(|l| !l.trim_start().starts_with("- ")).copied().collect::<Vec<_>>().join("\n");

            // Then it pins an exact x.y.z version (a range or latest resolves through the tag listing)
            let version = step.lines().find_map(|l| l.trim().strip_prefix("bun-version:")).map(str::trim).unwrap_or("");
            let exact = version.split('.').count() == 3 && version.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
            assert!(exact, "{} line {} pins bun-version {version:?}", file.display(), at + 1);
        }
    }
    assert!(steps >= 9, "the setup-bun steps were found: {steps}");
}

#[test]
fn a_hung_core_tests_shard_is_ended_within_the_longest_honest_run_with_headroom() {
    // Given the core-tests job, whose hung shards ran to 2700 s while an honest shard's test step took at most 707 s
    let text = read(".github/workflows/pipeline.yml");
    let start = text.find("\n  core-tests:\n").expect("the core-tests job");
    let end = start + text[start..].find("\n  clis:\n").expect("the job after it");
    let job = &text[start..end];

    // When the job's and the test step's limits are read
    let minutes = |from: usize| -> u32 {
        let rest = &job[from..];
        let at = rest.find("timeout-minutes: ").expect("a limit") + "timeout-minutes: ".len();
        rest[at..].split_whitespace().next().unwrap().parse().unwrap()
    };
    let job_limit = minutes(0);
    let step_limit = minutes(job.find("name: Flow, typecheck, lint and unit tests").expect("the test step"));

    // Then the step may run twice the longest honest step (707 s is 12 min) and the job ends well under the 40 minutes the hangs took
    assert!((15..=20).contains(&step_limit), "step limit {step_limit} min");
    assert!(step_limit < job_limit && job_limit <= 30, "job limit {job_limit} min");
}

#[test]
fn both_eas_jobs_that_push_a_pointer_write_the_phases_of_the_push_into_the_job_summary() {
    // Given the two jobs that push the EAS pointers to the state ref
    let text = read(".github/workflows/pipeline.yml");

    // When each push step is read
    let steps: Vec<&str> = text.match_indices("name: Push the EAS pointer").map(|(at, _)| &text[at..at + 1100]).collect();

    // Then each keeps the verb's output in the log, fails on a failed push, and appends its attempts, fetch, commit and push seconds to the summary
    assert_eq!(steps.len(), 2);
    for step in steps {
        assert!(step.contains("wf bucket state write") && step.contains("|| { printf '%s\\n' \"$out\"; exit 1; }"), "{step}");
        assert!(step.contains(".timing.fetchS") && step.contains(".timing.pushS") && step.contains(".timing.attempts"), "{step}");
        assert!(step.contains(">> \"$GITHUB_STEP_SUMMARY\""), "{step}");
    }
}

#[test]
fn the_core_tests_step_kills_only_the_processes_that_hold_its_own_output_pipe_open_and_names_them() {
    // Given the core-tests job
    let job = job_block(&read(".github/workflows/pipeline.yml"), "core-tests");

    // When its test step is read
    let step = job.split("- name: Flow, typecheck, lint and unit tests").nth(1).expect("the test step is there");
    let step = step.split("- uses: ./.github/actions/nx-cache").next().unwrap();

    // Then after the stages are waited for, a process is picked only because its stdout or stderr is the pipe of the step's own, the runner's processes are skipped by name,
    // each is named in a warning, and only the picked ones are killed (a session-wide kill took the runner down)
    let waited = step.find("wait \"${pids[$i]}\"").expect("it waits for the stages");
    let after = &step[waited..];
    assert!(after.contains("readlink /proc/$$/fd/1"), "{after}");
    assert!(after.contains("readlink \"$p/fd/1\""), "{after}");
    assert!(after.contains("Runner*|*runsvc*|*provjobd*|*actions-runner*"), "{after}");
    assert!(after.contains("title=left behind"), "{after}");
    assert!(after.contains("kill -KILL \"${holders[@]}\""), "{after}");
    assert!(!after.contains("pgrep -s") && !after.contains("ps -eo"), "{after}");
}

#[test]
fn every_cache_step_of_the_pipeline_is_bounded_and_cannot_fail_the_job() {
    // Given the pipeline
    let text = read(".github/workflows/pipeline.yml");
    let lines: Vec<&str> = text.lines().collect();

    // When each step that restores or saves the Nx or sccache cache is read
    let steps: Vec<usize> = (0..lines.len()).filter(|i| lines[*i].trim_start().starts_with("- uses: ./.github/actions/nx-cache") || lines[*i].trim_start().starts_with("- uses: ./.github/actions/sccache-cache")).collect();
    assert!(steps.len() >= 8, "the pipeline restores and saves both caches: {}", steps.len());

    // Then it has its own timeout and continues on error: a cache is an optimisation, and a stalled upload must not hold a job until the job timeout
    for i in steps {
        let head: String = lines[i..(i + 6).min(lines.len())].iter().take_while(|l| !l.trim_start().starts_with("with:")).map(|l| format!("{l}\n")).collect();
        assert!(head.contains("timeout-minutes: 5"), "line {}: {head}", i + 1);
        assert!(head.contains("continue-on-error: true"), "line {}: {head}", i + 1);
    }
}

#[test]
fn the_core_tests_job_does_not_join_the_tailnet_because_its_tests_reach_nothing_deployed() {
    // Given the core-tests job of the pipeline
    let job = job_block(&read(".github/workflows/pipeline.yml"), "core-tests");

    // When its setup-job call is read
    let setup = job.split("./.github/actions/setup-job").nth(1).expect("it sets the job up");
    let setup = setup.split("- uses:").next().unwrap();

    // Then it turns the tailnet join off
    assert!(setup.contains("tailnet: 'false'"), "{setup}");
}
