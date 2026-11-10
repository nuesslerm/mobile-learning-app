# CLI flow tests: wf (2026-10-17)

Owner decision (17 Oct): wf's unit tests and its black-box crate are replaced by as few and as meaningful flow tests as possible.
Input enters through wf's main entry point (argv into the top-level dispatch of the built binary), every external dependency is a
double, and wf's external behaviour is unchanged. The one test binary is `clis/wf/tests/flow.rs` with one module per command family
under `clis/wf/tests/flow/`, and the harness (stub programs, mock HTTP/HTTPS servers, a SOCKS5 tailnet, a vm-ops-api double that
checks Ed25519 signatures, a browser daemon double) under `clis/wf/tests/flow/support/`. `wf test plan` plans it like every other
crate's flow binary (`cargo test --locked -p wf --test flow --manifest-path clis/Cargo.toml`).

## Counts and timings

| | before | after |
| --- | --- | --- |
| tests | 1,160: 201 in `#[cfg(test)]` modules of 39 source files, `tests/dispatch.rs` (3), `tests/bench_footprint.rs` (1), 955 in 86 files of `clis/wf-blackbox` | 91 journeys in `tests/flow.rs` (16 modules) |
| test binaries | 89: wf's unit harness, 2 wf integration targets, 86 wf-blackbox targets | 1 |
| clean `cargo build --release -p wf` | 462 s | 256 s |
| clean build of the tests | 218 s (`cargo test -p wf --no-run`) plus 119 s for the 86 blackbox targets, built in 7 batches for disk | 142 s (`cargo test -p wf --test flow --no-run`, after the release build) |
| run | 1 s (unit) plus 216 s (blackbox, 7 batches) | 90 s (92 s on a second run) |
| incremental (touch `src/main.rs`) release / test build | 130 s / 5 s (unit only; each blackbox batch relinked separately) | 122 s / 4 s |
| target dir | 3.8 GB | 2.7 GB |

All builds with `-j 2` on one target dir. The before numbers were taken on the same machine at `473f809ff` (origin/claude/device-lab);
the machine is shared, so a difference under about 20 % is noise. The baseline itself was not green: 1,104 passed and 15 failed, and
`cli_vm_recover` aborted (see "Found along the way").

## In process or through the binary

Every family drives the built binary (`env!("CARGO_BIN_EXE_wf")`, or `WF_BIN`). wf is a binary crate without a library target, and
what it does depends on process-global state an in-process call would share between parallel tests: the environment (the secrets
backend, the vault tokens, `WF_*` switches, `PATH` lookups of every external tool), the working directory and the repository root
it finds from it, the slot registry and lease files on disk, process groups and signals (`SIGTERM` to a run, `SIGINT` to a tunnel),
and the exit code and the split between stdout and stderr, which are wf's interface to its callers (agents, hooks, CI). One spawn
per step costs milliseconds; the binary is the one `main` the owner asked for. Each run is isolated by `support::run::isolate()`:
it strips `BWS_*`, `AWS_*`, `CLOUDSDK_*`, `GH_TOKEN`, `GITHUB_TOKEN` and every registry key from the inherited environment and
points the tailnet at a closed port, so a step only reaches the doubles it was given.

| family (module) | journeys | entry | doubles |
| --- | --- | --- | --- |
| surface | 4 | `wf --version`, `--help` of every command, refusals, program options, the `bin/wf` launcher | none; the launcher runs with a built binary, without one, and with no `cargo` on `PATH` |
| secrets | 7 | `wf secret ...`, the exposure guards | `bws` and `cargo` stubs, Better Stack as mock HTTPS, the hook-runtime deny list and guard script as they ship |
| tailnet | 3 | `wf vm net ...`, `wf tailnet ...` | the Tailscale API as mock HTTP, `tailscale`, `tailscaled` and `bws` stubs |
| vm | 5 | `wf vm ops/exec/logs/recover/creds` | vm-ops-api double behind a SOCKS5 tailnet (signature checked), Better Stack as mock HTTPS, `oci` (a node stub) and `terraform` stubs, a node double of vm-ops-api for `recover` |
| ship | 6 | `wf ship ...` | the vm-ops-api double, the deploy scripts compared against goldens, a stub EAS publish script |
| android | 7 | `wf android ...` | `adb`, `emulator`, `avdmanager`, `lsof` and `kill` stubs, the vm-ops-api double for the lab, a uiautomator2 JSON-RPC double, the lease clock (`WF_LEASE_CLOCK_OFFSET_MS`) |
| browser | 13 | `wf browser ...`, `wf visual` | browser daemon double, mock tailnet; real Chromium and ffmpeg where the browser is the point |
| harness | 6 | `wf android run`, `wf browser run`, `wf flow run` | `adb`, `pnpm` and browser-client stubs, the stub server's backend as mock HTTP |
| pipeline | 4 | `wf test plan/run/publish/results` | `pnpm`, `gh`, `curl` stubs, a fake harness, the bucket, the ORDS table and the dev Metro's bridge as mock HTTP |
| check | 4 | `wf check --staged/--budget`, `wf verify` | `pnpm` and `gh` stubs, the deploy status and the bucket as mock HTTP behind the tailnet; real Chromium for tier 3 |
| ask | 7 | `wf offload ask ...` | `opencode` as a node stub answering per model, the opencode-go preflight and OpenRouter as mock HTTP, the vm-ops-api double for the ledger |
| worker | 2 | `wf offload plan ...`, `wf work run` | a bare git origin and a clone, `pnpm` and `opencode` stubs |
| agents | 7 | `wf agent ...`, `wf caveat`, the caveat guard and liveness hooks | the judge as the repository's mock opencode (`libraries/hook-runtime/mock-opencode-bin`), `claude`, `adb`, apt, npm and mise stubs, the vm-ops-api double, an opencode session database |
| report | 4 | `wf log emit`, `wf report health`, `wf knowledge` | Better Stack, vm-ops-api and Gemini as mock HTTP(S), `gcloud`, `pnpm` and `npx` stubs, the mock opencode |
| auth | 6 | `wf auth ...`, `wf secret login listener` (the VNC auth listener) | `claude`, `codex`, `gcloud`, `bws`, `tailscale` and `s6-setuidgid` stubs, the agent-shell relay as mock HTTP; real Chromium for the desktop login |
| bench | 6 | `wf bench ...`, `pnpm --filter mla-tamagui run build:editor-bundle` | a subject binary, a stub `expo export`, the lab device through the vm-ops-api double; real Chromium for the web suites |

Deviation, inherited from the old suites: real Chromium (auth, bench, browser, check), ffmpeg (browser recordings), node (the
runtime of several doubles and of the launcher), python3 (one agents step) and git (sandbox repositories) run where they are the
double's runtime or where a real browser is what is being tested. A step that needs Chromium skips with a printed reason when none
is installed, as before. The browser journeys also need `tooling/browser-harness/dist` (the device-frame page), which the old
suites silently skipped without.

Source changes for testability: `android_lease::now_ms` reads `WF_LEASE_CLOCK_OFFSET_MS` so a journey can age a lease past its
TTL without waiting it out. `[[bin]] wf` has `test = false` (no empty unit harness). Dev-dependencies are now `rcgen`, `tiny_http`
and `ureq` (for the doubles); `tower` and `http-body-util` went with the unit tests.

## Where each old test went

Blackbox files (`clis/wf-blackbox/tests/`, all deleted with the crate; its helpers became `tests/flow/support/`):

| old file (tests) | flow journey |
| --- | --- |
| `auth_login.rs` (6), `cli_auth_login.rs` (6), `cli_auth_login_modes.rs` (16), `cli_auth.rs` (17) | `auth::a_person_logs_each_tool_in_here_and_wf_confirms_only_what_left_a_credential`, `auth::a_person_logs_a_tool_in_through_the_shared_desktop_and_drives_its_capture` |
| `cli_auth_listener.rs` (28) | `auth::the_listener_admits_only_the_owner_...`, `the_listener_refuses_to_start_half_configured_...`, `the_listener_relays_a_file_credential_...`, `the_listener_drives_a_real_desktop_browser_...` |
| `cli_agent_deps.rs` (12), `infra_agent_deps_cli.rs` (2) | `agents::the_vm_deps_manifest_records_what_an_agent_installed_and_replay_brings_it_back` |
| `cli_agent_deps_install.rs` (7) | `agents::vm_deps_install_installs_on_the_vm_and_records_what_it_installed` |
| `cli_agent_proof.rs` (4) | `agents::agent_proof_asks_an_agent_for_a_device_script_runs_it_recorded_and_reports_the_arm` |
| `cli_agents_caveat.rs` (8) | `agents::caveat_switches_the_judge_per_repository_and_the_mcp_guard_trusts_untrusted_connectors_only_to_read` (reads `clis/agent-config-sync/tests/flow/golden-guard-deny.json`) |
| `cli_caveat_guard.rs` (31) | `agents::the_caveat_guard_judges_each_finished_turn_and_blocks_unfinished_work_until_the_block_is_disclosed`, `report::log_emit_...` (the OTLP telemetry line) |
| `cli_caveat_guard_opencode.rs` (5) | `agents::the_caveat_guard_judges_an_opencode_session_read_from_its_database` |
| `cli_guard_eval.rs` (4), `cli_guard_liveness.rs` (9) | `agents::guard_liveness_reports_a_stop_hook_that_did_not_run_and_guard_eval_scores_the_judge_against_its_cases` |
| `cli_android.rs` (2), `cli_android_adb.rs` (11) | `android::an_agent_claims_an_emulator_drives_it_through_adb_and_releases_it` |
| `cli_android_start.rs` (11), `cli_android_teardown.rs` (6) | `android::android_start_reuses_what_the_registry_holds_and_fails_cleanly_when_the_emulator_does_not_come_up` |
| `cli_android_input.rs` (33) | `android::a_runbook_finds_taps_strokes_types_and_records_through_the_uiautomator_dump`, `android::a_runbook_drives_the_screen_through_the_on_device_ui_server` |
| `cli_android_lab.rs` (16) | `android::the_lab_scripts_boot_a_slim_redroid_safely_and_report_its_features` |
| `cli_android_lease.rs` (6) | `android::the_device_lease_hands_the_device_to_one_sandbox_at_a_time_in_order` |
| `cli_app_bench.rs` (7) | `bench::repo_bench_lists_and_compares_the_committed_suites_against_their_baselines` |
| `cli_app_bench_run.rs` (10) | `bench::repo_bench_runs_the_build_suite_...`, `bench::repo_bench_measures_memory_runtime_and_startup_...`, `bench::repo_bench_cold_starts_the_lab_apk_...` |
| `cli_bench.rs` (5) | `bench::repo_bench_cli_times_every_startup_target_of_a_binary_and_counts_the_repository_s_footprint` |
| `cli_cm6.rs` (5) | `bench::repo_bundle_rebuilds_the_editor_s_codemirror_bundle_only_when_it_is_stale` |
| `cli_binary_smoke.rs` (15), `cli_help.rs` (4), `cli_surface.rs` (8), `cli_wf_version.rs` (1), `cli_option_errors.rs` (8), `command_mapping.rs` (2) | `surface::a_new_caller_learns_the_commands_...`, `surface::a_caller_with_a_retired_or_malformed_command_line_is_refused_with_the_reason`, `surface::the_program_options_mean_the_same_wherever_the_caller_puts_them` |
| `cli_wf_launcher.rs` (2) | `surface::the_launcher_runs_the_built_binary_and_without_one_or_cargo_says_so` |
| `cli_browser.rs` (1), `cli_browser_client.rs` (13) | `browser::the_browser_client_posts_each_command_to_the_registered_daemon_and_prints_its_answer` |
| `cli_browser_tunnel.rs` (5) | `browser::the_tunnel_publishes_the_fleet_on_localhost_through_the_tailnet_under_the_names_it_resolves` |
| `cli_browser_tailnet_forward.rs` (2) | `browser::a_tailnet_forward_reaches_a_tailnet_host_from_a_local_port_and_is_reused` |
| `cli_browser_video_check.rs` (3) | `browser::video_check_finds_the_tap_indicator_in_a_recording_for_each_logged_tap` |
| `cli_browser_launch.rs` (8), `cli_browser_indicate.rs` (3) | `browser::a_phone_framed_session_looks_and_behaves_like_the_device_it_was_opened_as` |
| `cli_browser_state.rs` (13) | `browser::a_phone_framed_session_exposes_the_page_state_an_agent_inspects` |
| `cli_browser_observe.rs` (6) | `browser::a_phone_framed_session_measures_memory_and_coverage_for_a_benchmark` |
| `cli_browser_debug.rs` (6) | `browser::an_agent_debugs_a_page_by_its_source_mapped_lines_without_touching_the_code` |
| `cli_browser_attach.rs` (45) | `browser::an_attached_session_reads_pages_...`, `..._taps_types_drags_strokes_and_swipes_like_a_user`, `..._proxies_stubs_and_forwards_network_and_feeds_fake_media`, `..._traces_a_lesson_pad_and_serves_the_device_frame` |
| `cli_visual.rs` (13) | `browser::the_visual_tier_screenshots_changed_routes_and_judges_them_against_baselines` |
| `cli_deploy.rs` (21), `cli_deploy_run.rs` (20) | `ship::the_ship_plan_...`, `ship::ship_status_...`, `ship::shipping_the_metro_server_...`, `ship::a_release_is_built_on_the_vm_...`, `ship::a_redeploy_of_a_tag_is_sent_once_...` |
| `cli_ship_eas.rs` (6) | `ship::ship_eas_publishes_the_app_as_an_eas_update_through_its_publish_script` |
| `cli_dispatch_run.rs` (28) | `ask::an_ask_runs_one_prompt_down_the_cheap_model_ladder_and_logs_the_run`, `ask::the_preflight_probe_marks_a_capped_pool_cold_...` |
| `cli_dispatch_batch.rs` (7) | `ask::a_batch_asks_once_per_matched_file_with_a_shared_template_a_bounded_number_at_a_time` |
| `cli_dispatch_runbook.rs` (10) | `ask::a_runbook_is_driven_through_the_ladder_and_judged_by_the_steps_the_agent_reports` |
| `cli_dispatch_stats.rs` (13) | `ask::ask_stats_summarize_the_run_log_here_or_on_the_vm_ledger` |
| `cli_dispatch_savings.rs` (10) | `ask::ask_savings_prices_what_delegating_avoided_from_the_run_log_and_the_delegation_report` |
| `cli_dispatch_cooldown.rs` (7), `cli_dispatch_pricing_sync.rs` (6) | `ask::the_ladder_tools_clear_a_pool_s_cooldown_and_refresh_model_prices_from_openrouter` |
| `cli_infra.rs` (17) | `vm::the_oracle_credentials_are_synced_from_the_vault_and_terraform_keeps_the_vm_within_the_free_tier` |
| `cli_vm_ops.rs` (17) | `vm::vm_ops_api_is_reached_through_the_tailnet_with_requests_signed_by_the_vault_key` |
| `cli_vm_exec.rs` (17) | `vm::vm_exec_runs_a_script_on_the_vm_streams_its_output_and_brings_back_its_artifacts` |
| `cli_vm_logs.rs` (13) | `vm::vm_logs_queries_betterstack_over_https_and_prints_one_line_per_row` |
| `cli_vm_recover.rs` (15) | `vm::vm_recover_brings_an_unreachable_vm_and_its_unhealthy_apps_back` |
| `cli_knowledge.rs` (22) | `report::repo_knowledge_ranks_the_gaps_and_learnings_a_file_touches_indexes_the_gaps_and_finds_the_stale_ones` |
| `cli_knowledge_gemini.rs` (8) | `report::repo_knowledge_packs_the_repository_into_gemini_docs_one_category_each_and_keeps_them_synced` |
| `cli_log_emit.rs` (10) | `report::log_emit_sends_one_scrubbed_otlp_line_to_betterstack_and_always_exits_0` |
| `cli_report_health.rs` (13), `cli_report_health_targets.rs` (3) | `report::the_health_report_renders_every_section_from_the_vm_and_betterstack_and_degrades_instead_of_failing` |
| `cli_run.rs` (59) | `harness::an_android_script_reads_the_screen_...`, `harness::an_android_run_is_recorded_timed_and_reported_pass_or_fail`, `harness::a_run_told_to_stop_ends_at_once_...`, `harness::a_browser_script_drives_the_daemon_...`, `harness::goldens_are_kept_recorded_compared_and_shown_in_the_report` |
| `cli_secrets.rs` (35) | `secrets::a_developer_reads_secrets_from_the_vault_...`, `secrets::every_app_group_gets_its_required_then_its_optional_keys_in_order`, `secrets::the_sealed_local_copy_saves_vault_calls_...` |
| `cli_secrets_guard_coverage.rs` (4) | `secrets::the_value_revealing_subcommands_are_refused_to_agents_by_both_guards_and_no_default_file_holds_a_vault_key` |
| `cli_secrets_registry_sync.rs` (3) | `secrets::the_doctor_reports_drift_between_the_registry_and_the_vault_and_registry_sync_rebuilds_wf_first` |
| `cli_secrets_telemetry.rs` (4) | `secrets::every_lookup_is_reported_to_betterstack_without_the_value_unless_switched_off` |
| `cli_secrets_writers.rs` (32) | `secrets::the_writers_preview_by_default_and_apply_through_the_edit_token_keeping_the_old_value` |
| `cli_tailscale.rs` (21) | `tailnet::the_tailnet_s_device_records_are_listed_with_a_verdict_each_and_stale_ones_retired` |
| `cli_tailscale_rotate.rs` (6) | `tailnet::the_vm_auth_key_s_expiry_is_watched_and_a_rotation_mints_a_new_key_into_the_vault_without_showing_it` |
| `cli_tailscale_up.rs` (7) | `tailnet::net_up_starts_the_userspace_daemon_and_joins_this_machine_with_the_vault_key_only_when_it_must` |
| `cli_test_plan.rs` (5) | `pipeline::ci_plans_the_stages_a_change_reaches_and_runs_each_planned_stage` (now also cargo crates and `--base last-green`) |
| `cli_test_run.rs` (10) | `pipeline::focused_e2e_tests_run_unit_by_unit_through_their_harness` |
| `cli_test_publish.rs` (2) | `pipeline::a_run_is_published_as_a_bundle_and_a_metrics_row_and_read_back_by_query` |
| `cli_test_results.rs` (3) | `pipeline::published_runs_are_found_by_a_query_over_the_index_and_the_table` |
| `cli_verify.rs` (4), `cli_verify_staged.rs` (10) | `check::the_pre_commit_gate_lints_what_is_staged_then_typechecks_and_tests_every_package_it_reaches` |
| `cli_verify_budget.rs` (10) | `check::the_simplicity_budget_measures_the_repo_and_fails_only_on_gated_limits_unless_strict` |
| `cli_verify_dry_run.rs` (12) | `check::a_deployed_commit_s_verification_plan_is_printed_from_the_paths_it_changed_without_the_network` |
| `cli_verify_tier3.rs` (8) | `check::a_deployed_commit_is_verified_tier_by_tier_reported_to_the_bucket_and_the_pr_and_a_failure_leaves_a_fix_unit` |
| `cli_work_plan.rs` (9) | `worker::a_plan_is_checked_before_it_runs_and_its_state_is_read_off_the_remote` |
| `cli_work_run.rs` (14) | `worker::a_worker_runs_a_plan_once_landing_what_passes_its_gate_and_releasing_what_cannot_run` |
| `cli_helpers_smoke.rs` (4) | dropped: it tested the test helpers, not wf |
| `cli_headless_chromium_init.rs` (5) | dropped: it tests the VNC desktop image's init script (`clis/vnc-auth-capture`), not wf; it needs a home in that package's own flow test |
| `fixtures/` | moved to `tests/flow/fixtures/` (deploy-script goldens regenerated with the device gate, `app-key-groups.json` synced); `golden/` dropped with the caveat-guard golden replay below |

wf's own tests:

| old | flow journey |
| --- | --- |
| `tests/dispatch.rs` (3) | `surface::a_new_caller_learns_the_commands_...`, `surface::a_caller_with_a_retired_or_malformed_command_line_is_refused_with_the_reason` |
| `tests/bench_footprint.rs` (1) | `bench::repo_bench_cli_times_every_startup_target_...` (footprint step) |
| `src/caveat_guard/tests.rs` (7) | `agents::the_caveat_guard_judges_each_finished_turn_...`; the golden replay against the removed TypeScript guard (`tests/golden`) dropped: a parity check with an implementation that no longer exists |
| `src/agent_proof.rs` (7) | `agents::agent_proof_...` (fenced and bare answers, the claude JSON, a failed run, the prompt) |
| `src/android_features.rs` (6) | `android::the_lab_scripts_boot_a_slim_redroid_safely_and_report_its_features` (catalog, plan, camera, unknown feature, report) |
| `src/android_input.rs` (4), `src/android_u2.rs` (5) | `android::a_runbook_finds_taps_strokes_types_...`, `android::a_runbook_drives_the_screen_through_the_on_device_ui_server` (selectors, entities, paced strokes of at most 60 points and no segment over 30 px) |
| `src/android_lease.rs` (8) | `android::the_device_lease_hands_the_device_to_one_sandbox_at_a_time_in_order` (FIFO, TTL, heartbeat, stray release, refusal naming the holder, status) |
| `src/app_bench/domain.rs` (25), `lab.rs` (3), `android.rs` (6) | `bench::repo_bench_runs_the_build_suite_...` (export log, record modes, regressions, A/B), `bench::repo_bench_measures_..._startup_...` (executed bytes per package from source maps), `bench::repo_bench_cold_starts_the_lab_apk_...` (start stamp, meminfo, screen, missing marker). Dropped as internal: the per-case VLQ inputs (continuation digits, negative deltas, astral characters), the even-count median, and the export path resolver's traversal and twin-parameter cases |
| `src/args.rs` (28) | `surface::the_program_options_mean_the_same_wherever_the_caller_puts_them`, `surface::a_caller_with_a_retired_or_malformed_command_line_is_refused_with_the_reason`; the parser's internal probing helpers dropped as internal |
| `src/browser_debug.rs` (4), `src/hermes_debug.rs` (2) | `browser::an_agent_debugs_a_page_...` (source-mapped lines, steps); dropped as internal: Metro's `x_facebook_sources`, the Hermes target pick and the Metro host split, which need a live Expo Go to reach from outside |
| `src/build_image.rs` (3) | `ship::a_release_is_built_on_the_vm_...` (the device gate before the first build, in the golden scripts); dropped: the Dockerfile cache-mount lint (`build_cache_gaps`), a repository lint rather than wf behaviour |
| `src/deploy.rs` (5) | `ship::the_ship_plan_...` (the desktop leg's paths checked against the Dockerfile's COPY sources and the listener's `#[path]` modules); dropped: the `cargo-touch-changed.sh` mtime test, a build script, not wf |
| `src/e2e_tests.rs` (12), `src/e2e_run.rs` (3) | `pipeline::ci_plans_...`, `pipeline::focused_e2e_tests_...` (header parsing, focus and change selection, cold starts, the Metro wait by the deploy's pnpm rule, the bundle warm-up from the manifest's launch asset, the rerun of a unit a Metro move broke) |
| `harness-flow/src/flow_run.rs` (1) | `harness-flow` integration `goldens::a_flow_file_with_update_goldens_turns_its_frozen_screen_into_a_golden` |
| `src/golden.rs` (5) | `harness::goldens_are_kept_recorded_compared_and_shown_in_the_report` |
| `src/log_export.rs` (4) | `report::log_emit_...`, `agents::the_caveat_guard_...` (typed OTLP attributes, a rejecting ingest); dropped as internal: an unknown level mapping to INFO |
| `src/routes.rs` (3) | `surface::a_new_caller_learns_the_commands_...` |
| `src/run_report.rs` (9), `src/spans.rs` (1), `src/run_stop.rs` (1) | `harness::an_android_run_is_recorded_timed_and_reported_pass_or_fail`, `harness::a_run_told_to_stop_ends_at_once_and_still_leaves_its_report` |
| `src/secrets/cache.rs` (4) | `secrets::the_sealed_local_copy_saves_vault_calls_and_stands_in_only_where_it_is_safe` |
| `src/stub_server.rs` (2) | `harness::an_android_script_reads_the_screen_...` (a stubbed call and a forwarded one) |
| `src/tailnet_url.rs` (2), `src/visual.rs` (2) | `browser::an_attached_session_proxies_stubs_and_forwards_...`, `browser::the_visual_tier_...`; dropped as internal: the URL rewrite and the SOCKS argument builders |
| `src/test_metrics.rs` (8), `src/test_publish.rs` (4), `src/test_results.rs` (3), `src/test_plan.rs` (2) | `pipeline::a_run_is_published_...`, `pipeline::published_runs_are_found_...`, `pipeline::ci_plans_...` |
| `src/trace.rs` (8) | `android::the_lesson_pads_as_redroid_shows_them_are_traced_along_the_whole_guide`, `browser::an_attached_session_traces_a_lesson_pad_...` |
| `src/video_check.rs` (2) | `browser::video_check_finds_the_tap_indicator_in_a_recording_for_each_logged_tap` |
| `src/vmops.rs` (2) | `vm::vm_ops_api_is_reached_through_the_tailnet_...` (the vault is listed once per process and a failed listing is retried) |
| `src/vnc/listener.rs` (3), `view.rs` (5), `desktop.rs` (2) | `auth::the_listener_*` |

## Callers re-pointed

`.github/workflows/architecture.yml` (`--test flow surface::`), `lefthook.yml` (the `secrets` job runs
`--test flow secrets::the_value_revealing`), the comment in `scripts/guards/block-secret-exposure.mjs`, `clis/vnc-auth-capture/README.md`,
`clis/wf/assets/README.md`, `clis/wf/src/help_table.rs`, the package descriptions of `clis/workflow-cli` and `tooling/wf-blackbox`,
`docs/plans/device-test-process/plan-2026-10-14.md`, and the criteria of `docs/goals/2026-10-13-device-test-process.md` (a2, b1, b2,
c2) and `docs/goals/2026-10-14-device-lab-fast-green.md` (p0.down-is-clean, p1.timing-in-results, p2.harness-hygiene, p9.debugger,
p10.goldens). Each criterion now runs the flow journeys that hold the same assertions and checks the passed count
(`grep -q '^test result: ok. N passed'`), so a renamed journey fails the criterion instead of passing it with zero tests.

## Found along the way

- The baseline was red on the trunk: `cli_app_bench` did not know the android suite, `cli_deploy` missed the remote-chat-agent
  extras, the `cli_deploy_run` goldens lacked the device gate, and `cli_dispatch_batch` was timing-flaky.
- The old suites inherited the caller's environment: with `BWS_*` and the agent proxy set, the caveat-guard tests listed the real
  vault and dialled the real tailnet SOCKS port (4.8 s per run). The flow harness strips it.
- The browser suites skipped silently when `tooling/browser-harness/dist` was missing; `harness_dist()` looks next to the binary or in
  the working directory's checkout, so where the binary sits decides whether the device frame is found (possible bug).
- `test_publish`'s evidence pick matches `.png` and `.mp4` case-sensitively, so `SHOT.PNG` is not published (possible bug).
- Neither suite ever asserted two behaviours criterion c2 names: a startup run that executed no script fails, and a missing
  device-frame page is a clear error (`app_bench/suites.rs`, `app_bench/lab.rs`).
