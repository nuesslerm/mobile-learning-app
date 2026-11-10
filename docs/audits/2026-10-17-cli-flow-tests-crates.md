# CLI crates: unit tests replaced by flow tests (2026-10-17)

Owner decision of 17 Oct: as few, as meaningful flow tests as possible. Each crate below now has one test binary,
`tests/flow.rs` (helpers and data under `tests/flow/`), run with `cargo test --locked -p <crate> --test flow`. Input
enters through the crate's public entry; every external dependency is a double that the flow injects.

| Crate | Before | After | Entry | Doubles |
| --- | --- | --- | --- | --- |
| `agent-config-sync` | 12 (3 unit, 9 parity) | 3 | `cli::run_in` (new: `run` with the checkout root injected instead of read from `git rev-parse`) | throwaway checkout in the temp dir, root handed in, stdin as bytes |
| `cdp` | 26 (4 unit, 17 + 5 against a real Chromium) | 5 | `launch::launch`, `Attached::attach` and its commands, `Cdp`, `json_endpoint` | Chromium double on loopback (DevTools HTTP + WebSocket, scripted CDP answers, recorded commands, pushed events); fake browser executable; upstream HTTP server |
| `cli-http` | 7 (unit) | 2 | `Request`, `answers`, `fetch_local`, `download` | loopback HTTP service double (routes, recorded requests, a stalled answer released by the test) |
| `rca-core` | 20 (5 unit, 15 golden) | 4 | the public API `wf offload ask` uses: `run_once` with a `RunHost`, `FileLedgerStore`/`FileRunLog`, ladder, ledger, stats, savings, opencode, time | `RunHost` double (scripted clock and opencode attempts), ledger and run log in real files under a temp dir; golden data moved to `tests/flow/golden/rca-core.json` |

Totals: 65 tests in 10 binaries before, 14 tests in 4 binaries after. No crate has a `package.json`, and no workspace
script runs these crates, so there is no `test:flow` script to add; lefthook's post-land `cargo test --workspace` runs the
flow binaries.

Timings (4 vCPU; before at the default `-j`, after at `-j 2` because RAM was short):

| | Before | After |
| --- | --- | --- |
| Clean build and run | 146 s | 134 s |
| Incremental (each crate's `lib.rs` touched) | 21.8 s | 7.3 s |
| Test execution | about 21 s (17 s of it real Chromium) | about 2.9 s |

## Flows

- **ACS-1** `given_the_fixture_when_it_is_synced_checked_edited_and_tampered_with_then_every_output_matches_the_golden_tree_and_drift_is_named_and_repaired`
- **ACS-2** `given_settings_claude_code_wrote_or_a_broken_command_when_synced_then_foreign_keys_survive_and_the_broken_source_fails_loud`
- **ACS-3** `given_the_mcp_guard_when_tools_of_each_tier_are_called_then_the_decision_matches_the_policy_and_a_broken_contract_fails_closed`
- **CDP-1** `given_a_launched_browser_when_its_page_is_attached_and_driven_then_each_command_reaches_the_page_and_the_answers_come_back`
- **CDP-2** `given_a_page_when_screenshots_are_taken_then_viewport_and_full_page_pngs_come_back_and_a_tiled_capture_is_taken_again`
- **CDP-3** `given_a_device_frame_page_with_nested_frames_when_frames_are_listed_searched_and_evaluated_then_each_frame_answers_in_its_own_document`
- **CDP-4** `given_proxied_urls_when_the_page_requests_them_then_this_process_fetches_them_and_hands_back_the_answer_or_a_502`
- **CDP-5** `given_devtools_endpoints_when_the_raw_client_connects_and_calls_then_results_errors_and_events_reach_the_caller`
- **HTTP-1** `given_a_service_when_a_caller_posts_reads_follows_and_downloads_then_each_exchange_arrives_and_answers_as_sent`
- **HTTP-2** `given_nothing_listening_or_a_stalled_service_when_a_caller_sends_then_it_is_an_error_and_the_probe_says_no`
- **RCA-1** `given_the_ladder_and_a_cooldown_ledger_on_disk_when_dispatches_run_then_rungs_rotate_and_escalate_and_the_ledger_and_run_log_persist_as_the_typescript_did`
- **RCA-2** `given_a_run_log_when_the_stats_report_is_built_then_filters_groups_costs_and_both_renderings_match_the_typescript_byte_for_byte`
- **RCA-3** `given_a_run_log_and_a_delegation_report_when_savings_are_computed_then_each_ratio_variant_renders_as_the_typescript_did`
- **RCA-4** `given_an_opencode_dispatch_when_its_command_environment_and_event_stream_are_handled_then_args_keys_tool_lines_and_summaries_match`

## Old test -> covering flow

### agent-config-sync

| Old test | Covered by |
| --- | --- |
| `cli.rs` `given_the_three_shapes_when_parsed_then_the_mode_is_read` | ACS-1 (`[]`, `--check`, `sync --check` through the real dispatch), ACS-3 (`mcp-guard`) |
| `cli.rs` `given_wf_s_own_options_when_parsed_then_they_are_read_and_dropped_wherever_they_stand` | ACS-1 (`--app x sync --json`, `sync --check --app=other`, `--json --check --app x`, `--app` alone exits 2), ACS-3 (`mcp-guard --json --app x`) |
| `cli.rs` `given_something_else_when_parsed_then_clap_names_it` | ACS-1 (`sync --bogus` exits 2 with clap's text, `nope` exits 2) |
| `parity.rs` `..._every_file_matches_the_typescript_output_byte_for_byte` | ACS-1 |
| `parity.rs` `..._check_runs_then_it_exits_0_silently_and_a_second_sync_changes_nothing` | ACS-1 |
| `parity.rs` `..._edited_instruction_source_when_check_runs_then_it_names_both_generated_files_and_exits_1` | ACS-1 |
| `parity.rs` `..._stale_file_in_an_owned_directory_and_a_legacy_dir_..._both_are_removed` | ACS-1 |
| `parity.rs` `..._symlinked_output_when_sync_runs_then_the_link_is_replaced_by_a_real_file` | ACS-1 |
| `parity.rs` `..._keys_claude_code_wrote_itself_..._survive_in_place_...` | ACS-2 |
| `parity.rs` `..._command_without_a_description_..._exits_1_naming_the_file` | ACS-2 |
| `parity.rs` `given_the_mcp_guard_when_tools_of_each_tier_are_called_...` | ACS-3 (plus the two newer first-party servers, `ccd_session` and `Claude_Docs`) |
| `parity.rs` `given_a_broken_mcps_file_when_the_guard_runs_then_it_fails_loud_with_exit_1` | ACS-3 |

### cdp

The real-Chromium tests become protocol-level flows against the double: each asserts the commands the client sends
(method, params, session) and what it makes of the browser's answers. **Dropped: the proof that a real Chromium
honours them** (that the page really reports the emulated user agent, that a real capture is 3000 px tall, that a real
server's HttpOnly cookie is set). That is the browser's behaviour, an external dependency here; the `wf browser`
harness and the browser e2e tier run a real Chromium.

| Old test | Covered by |
| --- | --- |
| `tiled.rs` `given_a_band_repeated_down_the_image_when_checked_then_it_is_tiled` (both images) | CDP-2 (each capture is taken again) |
| `tiled.rs` `given_a_faint_band_repeated_down_the_image_when_checked_then_it_is_tiled` | CDP-2 |
| `tiled.rs` `given_a_smooth_vertical_gradient_when_checked_then_it_is_not_tiled` | CDP-2 (returned as captured) |
| `tiled.rs` `given_a_blank_page_or_a_page_with_content_when_checked_then_neither_is_tiled` | CDP-2 |
| `attached.rs` `given_a_browser_endpoint_when_attaching_then_the_first_page_is_driven` | CDP-1 (handshake, `document.title`, awaited promise) |
| `attached.rs` `given_a_script_that_throws_when_evaluating_then_the_error_text_is_returned` | CDP-1 (thrown error and a syntax error by text) |
| `attached.rs` `given_an_unreachable_url_when_navigating_then_the_net_error_is_reported` | CDP-1 |
| `attached.rs` `given_an_http_only_cookie_when_reading_cookies_then_it_is_listed_with_its_value` | CDP-1 (all, scoped and none; browser vs page session) |
| `attached.rs` `given_a_page_when_capturing_then_the_png_decodes_to_a_png` | CDP-2 |
| `attached.rs` `given_mobile_emulation_when_applied_then_the_page_reports_the_user_agent_and_touch_points` | CDP-1 (viewport plus gutter pinned, user agent, five touch points) |
| `attached.rs` `given_a_window_when_asking_for_fullscreen_then_it_answers_without_failing` | CDP-1 (normal then fullscreen, size read back; a missing window answers `(false, 0, 0)`) |
| `attached.rs` `given_base64_text_when_decoding_then_the_bytes_come_back` | CDP-2 (padded and unpadded captures decode; data that is not base64 fails the capture) |
| `attached.rs` `given_a_tall_page_when_capturing_full_page_then_the_png_is_taller_than_the_viewport` | CDP-2 (clip from `cssContentSize`, `captureBeyondViewport`, 500x3000 back) |
| `attached.rs` `given_nested_iframes_when_listing_frames_then_each_has_its_parent_and_url` | CDP-3 |
| `attached.rs` `given_nested_iframes_when_evaluating_in_each_frame_then_each_sees_its_own_document` | CDP-3 |
| `attached.rs` `given_a_frame_named_device_frame_app_when_finding_it_then_it_is_returned_and_an_absent_one_times_out` | CDP-3 (found after it appears; absent one times out) |
| `attached.rs` `given_nested_frames_when_searching_for_an_editable_one_then_the_first_with_a_field_is_found` | CDP-3 |
| `attached.rs` `given_a_target_and_preset_when_building_the_device_frame_url_then_it_matches_url_search_params` | CDP-3 |
| `attached.rs` `given_a_binding_and_an_init_script_when_the_page_calls_it_then_the_call_arrives_without_a_command_in_flight` | CDP-1 |
| `attached.rs` `given_a_proxied_url_when_the_page_fetches_it_then_this_process_makes_the_request_and_the_page_gets_the_answer` | CDP-4 |
| `attached.rs` `given_a_proxied_url_that_cannot_be_reached_when_the_page_navigates_to_it_then_it_shows_the_502_error` | CDP-4 |
| `chromium.rs` `given_a_running_chromium_when_asking_the_browser_version_then_it_names_chrome` | CDP-5 (`json_endpoint` + `Cdp::connect` + `call`), CDP-1 (`browser_version`) |
| `chromium.rs` `given_a_page_when_navigating_and_evaluating_then_the_script_sees_the_document` | CDP-1 (through `Attached`) |
| `chromium.rs` `given_a_page_when_capturing_a_screenshot_then_a_png_comes_back` | CDP-2 |
| `chromium.rs` `given_an_unknown_method_when_called_then_the_browser_error_is_returned` | CDP-5 |
| `chromium.rs` `given_a_closed_port_when_connecting_then_it_reports_the_unreachable_endpoint` | CDP-5 |

New coverage: `launch::launch` (arguments, environment, endpoint read from stderr, a browser that dies first) and
`Launched::close` (profile removed); `json_endpoint` errors (HTTP 404, not JSON, closed port); the event queue of the raw
client; attaching from a `ws://` URL and opening a page when the browser has none; `set_target`; execution contexts
destroyed and cleared; a request that is not proxied continuing untouched; `Fetch.disable` once no route is left.
Still uncovered, as before: `record.rs` (ffmpeg), stubs, HTTP credentials, input and coverage commands.

### cli-http

| Old test | Covered by |
| --- | --- |
| `given_a_server_when_a_json_body_is_posted_then_it_arrives_with_its_headers_and_the_reply_comes_back` | HTTP-1 |
| `given_an_error_status_when_a_request_is_sent_then_it_is_a_response_not_an_error` | HTTP-1 |
| `given_basic_credentials_when_a_request_is_sent_then_the_authorization_header_carries_them` | HTTP-1 |
| `given_a_redirect_when_it_is_not_followed_then_the_3xx_comes_back_and_when_followed_the_target_answers` | HTTP-1 |
| `given_nothing_listening_when_a_request_is_sent_then_it_is_an_error_and_the_probe_says_no` | HTTP-2 |
| `given_a_slow_server_when_the_timeout_passes_then_the_request_fails` | HTTP-2 (the service holds its answer until the test releases it, instead of sleeping) |
| `given_a_download_when_it_succeeds_then_the_file_holds_the_body_and_a_failure_leaves_none` | HTTP-1 |

New coverage: `fetch_local`, `answers` on a live endpoint, `encode_uri_component` on the wire, `decode_uri_component`,
`form_encode`.

### rca-core

| Old test | Covered by |
| --- | --- |
| `golden.rs` `classify_failure_matches`, `should_escalate_matches`, `resolve_ladder_matches`, `next_rung_matches`, `next_rung_index_matches` | RCA-1 |
| `golden.rs` `ledger_scripts_match`, `next_ledger_matches`, `cold_window_matches` | RCA-1 |
| `golden.rs` `runner_scenarios_match` | RCA-1 (the host now keeps the ledger and run log in real files, and the flow checks both files) |
| `file_store.rs` `ledger_store_round_trips_and_reads_corrupt_as_empty` | RCA-1 (per scenario: missing reads empty, seed written, final ledger read back, no `.tmp` left, corrupt reads empty) |
| `file_store.rs` `run_log_appends_one_row_per_line_and_skips_torn_lines` | RCA-1 |
| `golden.rs` `run_log_parse_matches`, `stats_and_renderings_match`, `shape_stats_filters_and_since_match`, `percentiles_and_notional_cost_match` | RCA-2 |
| `time.rs` `round_trips_and_matches_known_values` | RCA-2 |
| `stats.rs` `to_fixed_rounds_ties_up_like_javascript` | RCA-2 (each value as a caller's cost in the rendered `--stats` table, and through the public `to_fixed`) |
| `golden.rs` `savings_match` | RCA-3 |
| `golden.rs` `opencode_events_and_formatting_match` | RCA-4 |
| `opencode.rs` `env_remaps_the_opencode_key_and_copies_the_others` | RCA-4 |

New coverage: `next_utc_midnight`.

## Found on the way

- The agent-config-sync parity test was red on `dev`: `c7be79433` added `ccd_session` and `Claude_Docs` to
  `FIRST_PARTY_TRUSTED_SERVERS` without updating the golden `.claude/settings.json`. The golden now lists both.
- The golden tree never held `.mcp.json` and `opencode.json`: the root `.gitignore` ignores both names, so they were
  never committed, and the test could pass only where an earlier run had left them on disk. Both were regenerated from
  the current Rust output for the fixture and force-added. They are not TypeScript-verified (the TypeScript is gone).
- `clis/wf-blackbox/tests/cli_agents_caveat.rs` reads the guard's golden deny line; its path now points to
  `clis/agent-config-sync/tests/flow/golden-guard-deny.json`.
