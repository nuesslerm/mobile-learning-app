# remote-chat-agent: unit tests and black-box suites replaced by flow tests (2026-10-17)

Owner decision (17 Oct): replace the service's unit tests with fewer, larger flow tests in Rust that drive the service in
process through the router `main` serves, every external dependency replaced by a double injected at construction. Why: CI
compile time (each `#[cfg(test)]` module and each `tests/*.rs` file was its own compile and link) and tests that exercise
what callers hit.

## What changed

- **Ports.** `rs/src/application/ports.rs` defines what the service needs from outside its process: `Opencode` (opencode's
  HTTP API and its event stream), `Commands` (every program: `git`, `gh`, `pnpm`, `sqlite3`, `pdftotext`, `tesseract`,
  `ffmpeg`, whisper.cpp) and `Outbound` (the speech-to-text engines, the edit-stats table, Telegram), gathered in `Ports`.
  `main.rs` fills it with the real adapters (`opencode::Http`, `proc::System`, `outbound::Reqwest`); `App::new(config, ports)`
  wires the rest, including the two orchestrators `main` used to build. The WebSocket heartbeat and the edit-stats flush
  interval are `Config` fields (`main` keeps 15 s and 5 s). External behaviour is unchanged.
- **One flow binary.** `rs/tests/flow.rs` (helpers and journeys under `rs/tests/flow/`): 18 tests. HTTP goes through
  `routes::router(app)` with `tower::ServiceExt::oneshot`; the WebSocket goes through the same router served by
  `axum::serve` over in-memory pipes (no socket), with `tokio-tungstenite` as the client. The doubles (`flow/doubles.rs`)
  are in process: a scripted opencode (sessions, prompts, aborts, questions, permissions, inventory and `/event`), a
  scripted program runner, and a recording HTTP poster. Flows set the environment the service reads under one lock, so
  they take turns.
- **Deleted.** Every `#[cfg(test)]` module in `rca-service` and `remote-chat-agent-core`, `rs/tests/flow_http.rs`,
  `core/tests/flow_orchestration.rs`, the core crate's `testing` feature and fakes (`core/src/testing.rs`), 15 TypeScript
  black-box suites plus `chatStream.ts`, `fakeServers.ts` and `vitest.blackbox.config.ts`; the `wiremock` dev-dependency
  (and its workspace entry). `tests/blackbox/{harness,fakeOpencode,wsClient,global-setup}.ts` stay: `tests/bench` imports
  them; the bench's large source fixture (it read `vcs.blackbox.test.ts`) is now `tests/bench/fixtures/large-source.ts.txt`.
- **Scripts.** `test`, `test:unit` and `test:blackbox` are gone; `test:flow` runs
  `cargo test --profile deploy --locked -p rca-service --test flow`, the profile the other Rust services test with, so the CI
  runner's shared `backend/target` holds one build of the workspace's dependencies, not a deploy and a debug one. CI's
  `flow-tests` stage picks it up through `wf test plan`. `test:e2e` and `test:e2e:deployed` are kept.
- **Layer rules.** `routes/vcs.rs` and `routes/source_control.rs` no longer name infrastructure and left the
  `api-does-not-use-infrastructure` ignore list ([rca-hexagonal plan](../plans/rca-hexagonal/plan.md)).

Counts: before, 183 Rust tests in 43 `#[cfg(test)]` modules of 42 source files plus 2 `tests/*.rs` files, and 15 TypeScript black-box suites
(219 `it` declarations, more cases through `it.each`); after, 18 Rust flow tests in one binary.

## The flows

| Flow (`rs/tests/flow/…`) | The journey |
| --- | --- |
| `front_door.rs` | startup check, health, CORS (allowed, lookalikes, preflights), 404, malformed JSON, body limit, code-assist mock, rate limit by caller |
| `chat.rs` (turns) | `/api/chat` validation and CLIs, the SSE stream, opencode call order, session reuse, plan agent, models, cwd, thinking and tool frames, session errors, concurrent turns, closed subscriptions |
| `chat.rs` (waits, stops, fails) | a question during a turn answered through `/api/question`, stop on a running turn, hard timeout, refused session |
| `control.rs` | stop, queue, steer and question replies for sessions the service never ran, validation, opencode down |
| `documents.rs` | `/api/convert` (PDF through `pdftotext`, DOCX, refusals) and every kind of chat attachment (source, text, PDF, DOCX, pictures for blind and seeing models, OCR) |
| `inventory.rs` | agent CLIs, model preferences, MCP/skills/commands/models proxies, recent models through `sqlite3`, 502s |
| `runs.rs` | `/runs` auth and validation, the ladder, verdicts, tool events, diff stat through `git`, run log, cold pools |
| `source_control.rs` (git) | `/api/vcs` status parsing, diff and truncation, commit, push, every refusal |
| `source_control.rs` (gh) | `/api/source-control` list, view, create, gh failures |
| `worktree.rs` | `/api/worktree` auth, validation, clone, fetch, add, conflicts, token redaction, removal |
| `viewers.rs` (diff) | `/api/diff` register, store file, page rendering, expansion and containment, highlighting, annotations to Telegram, restart |
| `viewers.rs` (files and plans) | `/api/file` markdown and source rendering, containment, `/api/plan` redirects, annotations to Telegram |
| `voice.rs` (plan) | `/api/voice-plan` end to end, alternative validation, retries, session reuse and limits, timeouts, latency rows, client `/api/edit-stats` rows |
| `voice.rs` (recording) | `/api/voice-transcribe` and `/api/voice-edit` through the sidecar, Groq and whisper.cpp, latency rows |
| `voice.rs` (grammar golden) | the voice-edit grammar against the TypeScript library's generated answers (`tests/golden/dsl.json`), through the public `domain::voice_dsl` API: 900 validation cases are too many to plan one request each |
| `tamil.rs` | `/api/tamil/check` validation, the rasterised picture, the question, verdict tidying, failures |
| `websocket.rs` (requests and commands) | upgrade auth, hello, ping, envelope errors, methods, every command's validation, thread lifecycle, models and probe |
| `websocket.rs` (subscriptions, turns, restart) | shell and thread subscriptions, resume, replace, unsubscribe, delete, a full turn with activities, approvals and questions, interrupts, queueing, session stop, plan mode, failure modes, heartbeat, restart from disk |

## Old Rust tests

| Old test (file :: name) | Now covered by |
| --- | --- |
| `core/src/application/engine.rs` :: `given_two_threads_when_commands_interleave_then_sequences_are_unique_and_rising` | websocket (requests and commands) (sequences rise across threads) |
| `core/src/application/engine.rs` :: `given_a_subscriber_when_a_turn_starts_then_it_gets_the_snapshot_first_then_synchronized_then_each_event` | websocket (subscriptions, turns, restart) |
| `core/src/application/engine.rs` :: `given_a_replay_request_when_subscribed_after_a_sequence_then_only_later_events_replay` | websocket (subscriptions, turns, restart) |
| `core/src/application/engine.rs` :: `given_a_dropped_receiver_when_events_follow_then_the_listener_is_forgotten` | websocket (subscriptions, turns, restart) |
| `core/src/application/engine.rs` :: `given_the_hook_when_events_commit_then_it_sees_each_batch` | dropped: the engine-to-reactor hook is internal wiring; every flow whose turn reaches opencode depends on it |
| `core/src/application/engine.rs` :: `given_a_log_with_a_running_turn_when_a_new_engine_starts_over_it_then_it_lists_the_thread_and_marks_the_turn_interrupted` | websocket (subscriptions, turns, restart) |
| `core/src/application/engine.rs` :: `given_a_thread_in_the_log_when_a_new_engine_dispatches_to_it_then_it_hydrates_and_continues_the_sequence` | websocket (subscriptions, turns, restart) |
| `core/src/application/store.rs` :: `given_the_in_memory_store_when_the_store_contract_runs_then_it_holds` | chat (turns), websocket (requests and commands) (both run on the in-memory log) |
| `core/src/domain/accumulator.rs` :: `given_increments_then_snapshots_when_reconciled_then_each_character_is_handed_out_once` | chat (turns), websocket (subscriptions, turns, restart) |
| `core/src/domain/compat.rs` :: `given_a_growing_assistant_message_when_framed_then_only_the_increment_is_sent_and_a_user_message_is_ignored` | chat (turns) |
| `core/src/domain/compat.rs` :: `given_a_tool_that_runs_twice_then_finishes_when_framed_then_running_is_sent_once_per_activity_and_done_follows` | chat (turns) |
| `core/src/domain/compat.rs` :: `given_the_session_settling_when_framed_then_done_error_or_interrupted_is_sent_once` | chat (turns), chat (waits, stops, fails) |
| `core/src/domain/compat.rs` :: `given_the_model_in_a_meta_update_when_framed_then_model_used_is_sent_once_and_a_question_once_per_request` | chat (turns), chat (waits, stops, fails) |
| `core/src/domain/decider.rs` :: `given_no_thread_when_a_turn_starts_without_bootstrap_then_it_is_rejected_and_with_bootstrap_it_creates_the_thread` | websocket (subscriptions, turns, restart) |
| `core/src/domain/decider.rs` :: `given_a_running_turn_when_another_starts_then_it_is_queued_and_an_interrupt_is_accepted` | websocket (subscriptions, turns, restart) |
| `core/src/domain/decider.rs` :: `given_no_running_turn_when_an_interrupt_arrives_then_it_is_rejected` | websocket (subscriptions, turns, restart) |
| `core/src/domain/decider.rs` :: `given_an_open_user_input_request_when_a_turn_starts_then_it_is_refused_until_the_request_resolves` | websocket (subscriptions, turns, restart) |
| `core/src/domain/decider.rs` :: `given_assistant_deltas_when_appended_to_one_message_then_the_text_accumulates_and_the_creation_time_stays` | websocket (subscriptions, turns, restart) |
| `core/src/domain/ids.rs` :: `given_the_clock_when_formatted_then_ids_and_timestamps_have_the_javascript_shapes` | chat (turns) (ses_ id), viewers (diff) (createdAt), voice (plan) (row ts) |
| `core/src/domain/ingest.rs` :: `given_a_turn_start_and_a_completion_when_ingested_then_the_session_goes_running_then_ready` | websocket (subscriptions, turns, restart) |
| `core/src/domain/ingest.rs` :: `given_assistant_text_when_ingested_then_it_appends_to_the_message_of_its_part_and_reasoning_becomes_an_activity` | websocket (subscriptions, turns, restart) |
| `core/src/domain/ingest.rs` :: `given_tool_items_when_ingested_then_the_activity_kind_follows_the_phase_and_the_status_decides_the_tone` | websocket (subscriptions, turns, restart) |
| `core/src/domain/ingest.rs` :: `given_tool_output_in_several_shapes_when_projected_then_it_is_summarised_to_one_line` | websocket (subscriptions, turns, restart) (tool.completed payload); the other output shapes come from CLIs this build does not serve |
| `core/src/domain/ingest.rs` :: `given_approval_requests_when_ingested_then_the_kind_is_derived_and_tool_user_input_is_left_to_the_other_event` | websocket (subscriptions, turns, restart) |
| `core/src/domain/ingest.rs` :: `given_proposed_plan_deltas_when_ingested_against_a_thread_then_they_merge_into_one_plan` | websocket (subscriptions, turns, restart) (the proposed plan of a plan-mode turn); the delta merge itself is internal |
| `core/src/domain/ingest.rs` :: `given_a_provider_title_when_the_thread_still_has_the_placeholder_then_it_renames_it_and_otherwise_leaves_it` | dropped: no opencode event is translated into a provider title, so nothing a caller can send reaches it |
| `core/src/domain/projector.rs` :: `given_a_created_thread_when_a_running_session_settles_then_the_latest_turn_completes` | websocket (subscriptions, turns, restart) |
| `core/src/domain/projector.rs` :: `given_a_message_sent_twice_when_projected_then_it_is_replaced_not_duplicated` | websocket (subscriptions, turns, restart) |
| `core/src/domain/projector.rs` :: `given_activities_out_of_order_when_appended_then_they_stay_sorted_and_an_id_is_never_kept_twice` | dropped: projector internal; ingestion appends activities in order and with fresh ids |
| `core/src/domain/projector.rs` :: `given_events_for_a_thread_that_does_not_exist_when_projected_then_only_the_sequence_moves` | dropped: projector internal; no route produces such an event |
| `core/src/domain/translate.rs` :: `given_increments_then_a_snapshot_of_the_same_part_when_translated_then_each_character_is_handed_out_once` | chat (turns), websocket (subscriptions, turns, restart) |
| `core/src/domain/translate.rs` :: `given_the_users_own_prompt_echoed_back_when_translated_then_it_is_dropped` | websocket (subscriptions, turns, restart) |
| `core/src/domain/translate.rs` :: `given_a_tool_part_that_runs_then_completes_when_translated_then_it_starts_once_and_completes_once` | chat (turns), websocket (subscriptions, turns, restart) |
| `core/src/domain/translate.rs` :: `given_the_thread_view_when_a_tool_part_arrives_then_the_input_and_output_travel_with_it` | chat (turns), websocket (subscriptions, turns, restart) |
| `core/src/domain/translate.rs` :: `given_a_completion_then_an_idle_when_translated_then_the_turn_is_settled_once_and_a_bare_idle_settles_it_too` | chat (turns) (exactly one done) |
| `core/src/domain/translate.rs` :: `given_a_question_when_translated_then_each_gets_its_header_as_id_and_multi_select_defaults_to_false` | chat (waits, stops, fails), websocket (subscriptions, turns, restart) |
| `core/src/domain/translate.rs` :: `given_a_session_error_when_translated_then_it_is_a_runtime_error_with_its_message_and_an_abort` | chat (turns), websocket (subscriptions, turns, restart) |
| `core/tests/flow_orchestration.rs` :: `given_an_agent_that_answers_when_a_turn_is_dispatched_then_the_thread_holds_the_prompt_the_answer_and_a_settled_session` | websocket (subscriptions, turns, restart) |
| `core/tests/flow_orchestration.rs` :: `given_an_agent_that_cannot_start_a_session_when_a_turn_is_dispatched_then_the_session_ends_in_error_with_the_reason` | websocket (subscriptions, turns, restart) |
| `core/tests/flow_orchestration.rs` :: `given_a_turn_the_agent_refuses_when_it_is_sent_then_the_session_ends_in_error_and_no_stream_stays_open` | websocket (subscriptions, turns, restart) |
| `core/tests/flow_orchestration.rs` :: `given_a_running_turn_when_the_agents_event_stream_breaks_off_then_the_session_ends_in_error` | websocket (subscriptions, turns, restart) |
| `core/tests/flow_orchestration.rs` :: `given_a_running_turn_when_an_interrupt_is_dispatched_then_the_agent_is_told_to_stop_the_turn` | websocket (subscriptions, turns, restart) |
| `core/tests/flow_orchestration.rs` :: `given_changes_in_the_working_tree_when_a_turn_completes_then_its_diff_is_recorded_for_the_turn` | websocket (subscriptions, turns, restart) |
| `core/tests/flow_orchestration.rs` :: `given_two_turns_on_one_thread_when_the_second_follows_the_first_then_both_answers_land_in_order` | chat (turns) (the next turn on the same session) |
| `rs/src/api/cors.rs` :: `given_the_environment_list_when_parsed_then_trailing_slashes_and_blanks_go` | front_door |
| `rs/src/api/cors.rs` :: `given_origins_when_judged_then_the_listed_localhost_and_metro_runner_pass_and_lookalikes_do_not` | front_door |
| `rs/src/api/routes/caller.rs` :: `given_a_loopback_peer_when_a_caller_is_forwarded_then_the_forwarded_address_is_the_caller` | front_door |
| `rs/src/api/routes/caller.rs` :: `given_a_peer_that_is_not_loopback_when_it_forwards_then_it_is_still_the_caller` | front_door |
| `rs/src/api/routes/chat.rs` :: `given_request_bodies_when_parsed_then_only_the_documented_shapes_are_accepted` | chat (turns) |
| `rs/src/api/routes/chat.rs` :: `given_a_timeout_when_its_seconds_are_written_then_they_read_like_javascript` | chat (turns) |
| `rs/src/api/routes/code_assist.rs` :: `given_the_shipped_mocks_when_a_transcript_matches_then_its_frames_come_back_and_case_does_not_matter` | front_door |
| `rs/src/api/routes/diff.rs` :: `given_a_unified_diff_when_parsed_then_files_hunks_and_line_numbers_follow_the_headers` | viewers (diff) |
| `rs/src/api/routes/diff.rs` :: `given_hunk_headers_when_read_then_counts_are_optional` | viewers (diff) |
| `rs/src/api/routes/diff.rs` :: `given_a_store_file_when_loaded_then_only_fresh_entries_remain_and_a_bad_file_is_empty` | viewers (diff) |
| `rs/src/api/routes/file_preview.rs` :: `given_paths_when_resolved_under_the_fork_dir_then_a_climb_is_refused` | viewers (files and plans) |
| `rs/src/api/routes/inventory.rs` :: `given_the_directory_query_when_read_then_only_one_plain_non_empty_value_counts` | inventory |
| `rs/src/api/routes/inventory.rs` :: `given_nothing_configured_when_the_clis_are_listed_then_opencode_alone_is_available_and_selected` | inventory |
| `rs/src/api/routes/runs.rs` :: `given_a_valid_body_when_read_then_it_is_a_request` | runs |
| `rs/src/api/routes/runs.rs` :: `given_bad_fields_when_read_then_the_first_is_reported_with_its_path` | runs |
| `rs/src/api/routes/source_control.rs` :: `given_path_segments_when_read_as_javascript_numbers_then_the_same_values_result` | source_control (gh) |
| `rs/src/api/routes/source_control.rs` :: `given_gh_output_when_summarised_then_extras_go_and_a_missing_state_is_open` | source_control (gh) |
| `rs/src/api/routes/vcs.rs` :: `given_a_status_header_when_parsed_then_branch_upstream_and_counts_are_read` | source_control (git) |
| `rs/src/api/routes/vcs.rs` :: `given_no_upstream_or_a_detached_head_when_parsed_then_they_are_null` | source_control (git) |
| `rs/src/api/routes/vcs.rs` :: `given_a_long_diff_when_truncated_then_it_is_cut_at_the_unit_limit` | source_control (git) |
| `rs/src/api/routes/voice.rs` :: `given_clip_numbers_as_form_text_when_read_then_only_whole_numbers_from_0_to_10000_count` | voice (recording) |
| `rs/src/api/routes/voice.rs` :: `given_no_usable_turn_id_when_an_identity_is_made_then_the_server_names_the_turn` | voice (recording) |
| `rs/src/api/routes/voice.rs` :: `given_a_millisecond_clock_when_written_in_base_36_then_it_matches_javascript` | voice (recording) (server-made edit id shape); the exact base-36 digits of a fixed clock are not reachable from a request |
| `rs/src/api/routes/voice.rs` :: `given_a_name_that_climbs_when_the_recording_is_stored_then_only_its_last_segment_is_used` | voice (recording) |
| `rs/src/api/routes/worktree.rs` :: `given_repo_and_branch_shapes_when_checked_then_only_the_documented_characters_pass` | worktree |
| `rs/src/api/routes/worktree.rs` :: `given_names_when_slugified_then_they_are_lowercase_path_safe_and_trimmed` | worktree |
| `rs/src/api/routes/worktree.rs` :: `given_bodies_when_read_then_the_contracts_sentences_are_the_errors` | worktree |
| `rs/src/api/wire.rs` :: `given_a_well_formed_create_when_parsed_then_it_becomes_the_command` | websocket (requests and commands) |
| `rs/src/api/wire.rs` :: `given_a_create_that_breaks_rules_when_parsed_then_the_first_is_reported_with_its_path` | websocket (requests and commands) |
| `rs/src/api/wire.rs` :: `given_something_that_is_not_a_command_when_parsed_then_serde_says_what_was_expected` | websocket (requests and commands) |
| `rs/src/api/wire.rs` :: `given_a_turn_start_with_bad_nested_fields_when_parsed_then_the_path_reaches_the_field` | websocket (requests and commands) |
| `rs/src/api/wire.rs` :: `given_the_other_command_types_when_parsed_then_their_fields_are_checked` | websocket (requests and commands) |
| `rs/src/api/ws/envelope.rs` :: `given_well_formed_envelopes_when_parsed_then_each_kind_is_read` | websocket (requests and commands) |
| `rs/src/api/ws/envelope.rs` :: `given_a_request_that_lacks_something_when_parsed_then_the_first_missing_field_is_named` | websocket (requests and commands) |
| `rs/src/api/ws/envelope.rs` :: `given_a_ping_that_is_not_a_whole_number_when_parsed_then_serde_says_so` | websocket (requests and commands) |
| `rs/src/api/ws/envelope.rs` :: `given_something_that_is_not_an_envelope_when_parsed_then_serde_says_what_was_expected` | websocket (requests and commands) |
| `rs/src/application/attachments.rs` :: `given_a_source_file_when_processed_then_it_is_fenced_by_its_extension` | documents |
| `rs/src/application/attachments.rs` :: `given_a_long_file_when_processed_then_it_is_cut_at_eight_thousand_characters_and_says_so` | documents |
| `rs/src/application/attachments.rs` :: `given_a_type_it_does_not_read_when_processed_then_it_is_skipped` | documents |
| `rs/src/application/attachments.rs` :: `given_a_pdf_or_document_that_is_not_one_when_processed_then_the_model_is_told_no_text_could_be_had` | documents |
| `rs/src/application/edit_stats.rs` :: `given_a_valid_client_turn_when_converted_then_it_is_a_full_row_with_flags_as_0_and_1` | voice (plan) |
| `rs/src/application/edit_stats.rs` :: `given_an_error_outcome_when_converted_then_ok_is_zero` | voice (plan) |
| `rs/src/application/edit_stats.rs` :: `given_bad_fields_when_validated_then_the_first_is_reported_with_its_path` | voice (plan) |
| `rs/src/application/edit_stats.rs` :: `given_bounds_when_checked_then_range_and_integer_rules_apply` | voice (plan) |
| `rs/src/application/edit_stats.rs` :: `given_placeholders_when_read_then_they_count_as_unset` | voice (plan) |
| `rs/src/application/edit_stats.rs` :: `given_the_clock_when_formatted_then_it_is_iso_8601_with_milliseconds` | voice (plan) (row ts) |
| `rs/src/application/runs.rs` :: `given_snapshots_of_one_call_when_folded_then_the_last_state_wins_and_the_arguments_of_the_snapshot_that_had_them_stay` | runs |
| `rs/src/application/runs.rs` :: `given_calls_with_neither_an_id_nor_arguments_when_folded_then_they_are_left_out` | runs |
| `rs/src/application/runs.rs` :: `given_a_file_change_with_several_paths_when_folded_then_each_path_is_a_patch_event` | runs |
| `rs/src/application/runs.rs` :: `given_a_model_and_the_requests_cli_when_the_cli_is_chosen_then_a_pin_wins_and_an_anthropic_model_goes_to_claude` | runs |
| `rs/src/application/runs.rs` :: `given_a_path_inside_a_checkout_when_the_root_is_looked_for_then_the_directory_holding_dot_git_is_found` | runs (default run log under the checkout root) |
| `rs/src/application/startup.rs` :: `given_no_cli_or_opencode_when_checked_then_opencode_is_served` | front_door |
| `rs/src/application/startup.rs` :: `given_a_typo_when_checked_then_the_message_lists_what_is_supported_and_what_is_available` | front_door |
| `rs/src/application/startup.rs` :: `given_a_backend_this_build_does_not_serve_when_checked_then_it_refuses_and_says_why` | front_door |
| `rs/src/application/voice_planner.rs` :: `given_replies_wrapped_in_a_fence_or_prose_when_parsed_then_the_json_is_found` | voice (plan) |
| `rs/src/application/voice_planner.rs` :: `given_text_that_is_not_json_or_has_no_alternatives_when_parsed_then_the_codes_tell_them_apart` | voice (plan) |
| `rs/src/application/voice_planner.rs` :: `given_one_alternative_the_schema_refuses_when_parsed_then_the_others_are_kept_and_with_none_left_it_fails` | voice (plan) |
| `rs/src/application/voice_planner.rs` :: `given_a_model_setting_when_read_then_the_provider_is_split_at_the_first_slash_and_a_bad_one_falls_back` | voice (plan) |
| `rs/src/domain/clis.rs` :: `given_the_ids_when_checked_then_opencode_is_always_available_and_unknown_ids_are_not_clis` | inventory |
| `rs/src/domain/md.rs` :: `given_headings_emphasis_and_paragraphs_when_converted_then_they_become_html` | viewers (files and plans) |
| `rs/src/domain/md.rs` :: `given_code_the_markup_inside_it_is_escaped_and_left_alone` | viewers (files and plans) |
| `rs/src/domain/md.rs` :: `given_a_table_a_list_a_link_and_a_quote_when_converted_then_each_is_rendered` | viewers (files and plans) |
| `rs/src/domain/md.rs` :: `given_raw_html_when_converted_then_it_is_shown_as_text_and_a_script_link_has_no_target` | viewers (files and plans) |
| `rs/src/domain/models.rs` :: `given_the_cli_when_the_provider_is_checked_then_anthropic_belongs_to_claude_alone` | chat (turns) (a model of another CLI), runs |
| `rs/src/domain/schema.rs` :: `given_a_field_that_breaks_a_rule_when_parsed_then_the_message_names_the_path_and_the_rule` | runs, worktree, websocket (requests and commands) (serde paths in 400s) |
| `rs/src/domain/schema.rs` :: `given_a_well_formed_value_when_parsed_then_it_is_accepted_and_unknown_fields_are_dropped` | runs, worktree, websocket (requests and commands) (serde paths in 400s) |
| `rs/src/domain/util.rs` :: `given_json_values_when_stringified_then_they_read_as_javascript_would` | chat (turns), inventory, viewers (diff), viewers (files and plans) |
| `rs/src/domain/util.rs` :: `given_text_when_uri_encoded_then_only_the_unreserved_marks_survive` | chat (turns), inventory, viewers (diff), viewers (files and plans) |
| `rs/src/domain/util.rs` :: `given_a_location_with_spaces_and_escapes_when_encoded_then_spaces_are_escaped_and_escapes_stay` | chat (turns), inventory, viewers (diff), viewers (files and plans) |
| `rs/src/domain/util.rs` :: `given_the_clock_when_formatted_then_ids_and_timestamps_have_the_javascript_shapes` | chat (turns), inventory, viewers (diff), viewers (files and plans) |
| `rs/src/domain/util.rs` :: `given_a_title_with_a_closing_tag_when_embedded_then_it_cannot_end_the_script` | chat (turns), inventory, viewers (diff), viewers (files and plans) |
| `rs/src/domain/util.rs` :: `given_text_when_parsed_as_an_integer_then_only_the_leading_digits_count` | chat (turns), inventory, viewers (diff), viewers (files and plans) |
| `rs/src/domain/util.rs` :: `given_an_absolute_second_path_when_resolved_in_a_base_then_it_wins` | chat (turns), inventory, viewers (diff), viewers (files and plans) |
| `rs/src/domain/voice_dsl.rs` :: `given_valid_commands_when_validated_then_they_pass_and_a_start_after_an_end_is_refused` | voice (grammar golden), voice (plan) |
| `rs/src/domain/voice_dsl.rs` :: `given_an_end_past_the_last_character_when_validated_then_it_means_to_the_end` | voice (grammar golden), voice (plan) |
| `rs/src/domain/voice_dsl.rs` :: `given_two_commands_over_the_same_characters_when_validated_then_the_second_is_reported` | voice (grammar golden), voice (plan) |
| `rs/src/domain/voice_dsl.rs` :: `given_sequential_commands_in_bounds_when_validated_then_they_pass` | voice (grammar golden), voice (plan) |
| `rs/src/domain/voice_dsl.rs` :: `given_a_whole_function_in_one_command_when_validated_then_it_is_refused_with_advice_and_one_step_lands` | voice (grammar golden), voice (plan) |
| `rs/src/domain/voice_dsl.rs` :: `given_whole_solution_asks_when_the_limits_are_read_then_each_is_the_size_of_a_first_step` | voice (grammar golden), voice (plan) |
| `rs/src/domain/voice_dsl.rs` :: `given_line_commands_when_validated_then_out_of_range_lines_clamp_and_a_block_counts_its_leading_newline` | voice (grammar golden), voice (plan) |
| `rs/src/domain/voice_dsl.rs` :: `given_a_rename_off_any_identifier_when_validated_then_it_snaps_to_one_on_its_line_or_is_refused` | voice (grammar golden), voice (plan) |
| `rs/src/domain/voice_dsl.rs` :: `given_offsets_in_utf16_units_when_validated_then_a_character_outside_the_basic_plane_counts_twice` | voice (grammar golden), voice (plan) |
| `rs/src/domain/voice_dsl.rs` :: `given_raw_line_breaks_inside_strings_when_repaired_then_only_those_are_escaped_and_valid_json_is_untouched` | voice (grammar golden), voice (plan) |
| `rs/src/domain/voice_dsl.rs` :: `given_field_name_drift_when_normalized_then_each_command_gets_its_schema_field_and_a_conforming_document_is_equal` | voice (grammar golden), voice (plan) |
| `rs/src/domain/voice_dsl.rs` :: `given_python_and_a_header_with_nothing_under_it_when_completed_then_a_pass_body_goes_one_level_in` | voice (grammar golden), voice (plan) |
| `rs/src/domain/voice_dsl.rs` :: `given_a_header_with_its_own_body_or_a_deeper_line_below_or_another_language_when_completed_then_nothing_is_added` | voice (grammar golden), voice (plan) |
| `rs/src/domain/voice_dsl.rs` :: `given_headers_in_the_forms_python_has_when_checked_then_only_real_block_openers_count` | voice (grammar golden), voice (plan) |
| `rs/src/domain/voice_dsl.rs` :: `given_the_schemas_when_values_are_parsed_then_known_fields_survive_and_unknown_ones_go` | voice (grammar golden), voice (plan) |
| `rs/src/domain/voice_dsl.rs` :: `given_a_response_that_breaks_rules_when_parsed_then_serde_names_the_path_it_can_reach` | voice (grammar golden), voice (plan) |
| `rs/src/domain/voice_dsl.rs` :: `given_the_two_prompt_variants_when_built_then_v2_numbers_the_lines_and_v1_fences_the_code` | voice (grammar golden), voice (plan) |
| `rs/src/domain/voice_dsl.rs` :: `given_generated_codes_and_commands_when_validated_then_every_verdict_and_reason_matches_the_typescript` | voice (grammar golden), voice (plan) |
| `rs/src/domain/voice_dsl.rs` :: `given_generated_python_blocks_when_completed_then_the_commands_match_the_typescript` | voice (grammar golden), voice (plan) |
| `rs/src/domain/voice_dsl.rs` :: `given_strings_with_raw_control_characters_when_repaired_then_they_match_the_typescript` | voice (grammar golden), voice (plan) |
| `rs/src/domain/voice_dsl.rs` :: `given_transcripts_when_the_limits_are_read_then_they_match_the_typescript` | voice (grammar golden), voice (plan) |
| `rs/src/domain/voice_dsl.rs` :: `given_responses_with_drifted_fields_when_normalized_then_they_match_the_typescript` | voice (grammar golden), voice (plan) |
| `rs/src/infrastructure/docs.rs` :: `given_a_word_document_when_read_then_its_paragraphs_come_back_in_order_each_with_a_blank_line` | documents |
| `rs/src/infrastructure/docs.rs` :: `given_bytes_that_are_not_a_zip_or_have_no_main_part_when_read_then_the_error_says_so` | documents |
| `rs/src/infrastructure/docs.rs` :: `given_pdftotext_output_with_form_feeds_when_read_then_pages_are_joined_by_a_blank_line_and_a_failure_names_its_cause` | documents |
| `rs/src/infrastructure/docs.rs` :: `given_a_picture_when_it_is_too_large_or_unreadable_then_ocr_gives_nothing_and_otherwise_the_trimmed_words` | documents |
| `rs/src/infrastructure/file_store.rs` :: `given_appended_events_when_read_after_a_sequence_then_only_the_later_ones_come_back_in_order` | websocket (subscriptions, turns, restart) |
| `rs/src/infrastructure/file_store.rs` :: `given_a_thread_id_that_names_another_place_when_used_then_the_store_refuses_it` | websocket (subscriptions, turns, restart) |
| `rs/src/infrastructure/file_store.rs` :: `given_a_log_whose_last_line_was_cut_off_when_read_then_the_earlier_lines_load_and_a_corrupt_middle_line_is_an_error` | websocket (subscriptions, turns, restart) |
| `rs/src/infrastructure/file_store.rs` :: `given_the_file_store_when_the_store_contract_runs_then_it_holds` | websocket (subscriptions, turns, restart) (state directory, restart) |
| `rs/src/infrastructure/file_store.rs` :: `given_a_log_on_disk_with_a_running_turn_when_the_engine_starts_then_it_lists_the_thread_and_marks_the_turn_interrupted` | websocket (subscriptions, turns, restart) |
| `rs/src/infrastructure/file_store.rs` :: `given_a_thread_on_disk_when_a_new_engine_dispatches_to_it_then_it_hydrates_and_continues_the_sequence` | websocket (subscriptions, turns, restart) |
| `rs/src/infrastructure/highlight.rs` :: `given_typescript_when_highlighted_then_each_line_comes_back_with_keywords_and_strings_coloured` | viewers (diff) |
| `rs/src/infrastructure/highlight.rs` :: `given_text_or_an_unknown_language_when_highlighted_then_the_lines_are_only_escaped` | viewers (diff) |
| `rs/src/infrastructure/highlight.rs` :: `given_a_multiline_comment_when_highlighted_then_every_line_carries_its_own_span` | viewers (diff) |
| `rs/src/infrastructure/highlight.rs` :: `given_filenames_when_mapped_then_the_viewer_languages_result` | viewers (diff) |
| `rs/src/infrastructure/opencode.rs` :: `given_a_credential_when_a_call_is_made_then_it_goes_out_as_a_basic_header_and_the_json_comes_back` | dropped: real opencode HTTP transport (opencode::Http), outside the flow boundary; exercised by test:e2e:deployed |
| `rs/src/infrastructure/opencode.rs` :: `given_an_error_status_when_json_is_fetched_then_the_message_names_the_path_and_the_status_the_way_the_gateway_does` | inventory (502 "opencode <path> responded with 500" comes from Client::fetch_json over the double) |
| `rs/src/infrastructure/opencode.rs` :: `given_nobody_listening_when_json_is_fetched_then_it_reads_as_a_failed_fetch` | control (opencode down); the reqwest error text itself is the real transport's |
| `rs/src/infrastructure/opencode.rs` :: `given_an_event_stream_when_it_is_opened_then_each_data_payload_arrives_in_order_and_the_end_is_the_end` | dropped: SSE framing of the real transport; the end of a stream is asserted in websocket (subscriptions, turns, restart) (opencode ended its event stream) |
| `rs/src/infrastructure/opencode.rs` :: `given_a_refusing_server_when_a_stream_is_opened_then_the_error_names_the_path_and_status` | dropped: real transport; the message is produced in opencode::Http |
| `rs/src/infrastructure/opencode_backend.rs` :: `given_an_idle_session_when_mapped_then_a_turn_completion_comes_first_and_a_failure_before_it_marks_it_failed` | chat (turns), websocket (subscriptions, turns, restart) |
| `rs/src/infrastructure/opencode_backend.rs` :: `given_a_plan_mode_session_when_it_goes_idle_then_its_last_text_is_proposed_as_the_plan` | chat (turns), websocket (subscriptions, turns, restart) |
| `rs/src/infrastructure/opencode_backend.rs` :: `given_an_assistant_message_with_model_and_tokens_when_mapped_then_each_is_reported_once` | chat (turns), websocket (subscriptions, turns, restart) |
| `rs/src/infrastructure/opencode_backend.rs` :: `given_a_permission_request_when_mapped_then_the_kind_title_and_detail_follow_the_permission_and_its_patterns` | chat (turns), websocket (subscriptions, turns, restart) |
| `rs/src/infrastructure/opencode_backend.rs` :: `given_a_tool_part_without_a_call_id_when_mapped_then_the_call_id_is_an_empty_string_and_only_the_session_and_part_remain` | dropped: event-mapper internal; the activity a tool part becomes is asserted in websocket (subscriptions, turns, restart) |
| `rs/src/infrastructure/opencode_backend.rs` :: `given_decisions_when_mapped_to_opencode_replies_then_only_accept_variants_pass` | chat (turns), websocket (subscriptions, turns, restart) |
| `rs/src/infrastructure/proc.rs` :: `given_a_command_that_succeeds_when_run_then_stdout_comes_back` | dropped: real process runner (proc::System), outside the flow boundary; the failure wording it uses is the shared ExecError constructor the double uses too |
| `rs/src/infrastructure/proc.rs` :: `given_a_command_that_fails_when_run_then_the_error_carries_the_command_line_stdout_and_stderr` | dropped: real process runner; its error wording is ExecError::failed, which the Commands double uses (asserted in inventory, source_control) |
| `rs/src/infrastructure/proc.rs` :: `given_a_slow_command_when_the_timeout_passes_then_it_is_killed` | dropped: real process runner mechanics (kill on timeout) |
| `rs/src/infrastructure/proc.rs` :: `given_a_missing_program_when_run_then_the_error_says_it_could_not_start` | dropped: real process runner; ExecError::not_started is what the double answers for an unknown program |
| `rs/src/infrastructure/stt.rs` :: `given_an_engine_name_in_the_environment_when_resolved_then_the_sidecar_wins_over_groq` | voice (recording) |
| `rs/src/infrastructure/stt.rs` :: `given_a_missing_recording_when_transcribed_then_the_error_is_a_whisper_error` | dropped: unreachable; the route writes the upload before it transcribes |
| `rs/src/infrastructure/tamil.rs` :: `given_ink_and_a_letter_when_rasterised_then_one_png_two_panels_wide` | tamil |
| `rs/src/infrastructure/tamil.rs` :: `given_a_drawing_that_is_not_svg_when_rasterised_then_the_error_says_why` | tamil |
| `rs/src/infrastructure/tamil.rs` :: `given_a_word_or_a_letter_when_asked_about_then_the_question_names_which` | tamil |
| `rs/src/infrastructure/tamil.rs` :: `given_model_replies_when_tidied_then_the_verdict_is_the_one_the_node_service_gives` | tamil |
| `rs/src/infrastructure/tamil.rs` :: `given_a_reply_with_no_object_when_tidied_then_the_error_quotes_the_start_of_it` | tamil |
| `rs/tests/flow_http.rs` :: `given_the_service_when_health_is_asked_then_it_answers_ok_with_the_commit` | front_door, chat (turns), chat (waits, stops, fails), worktree |
| `rs/tests/flow_http.rs` :: `given_a_preflight_from_an_allowed_origin_when_it_arrives_then_the_layer_answers_with_the_allowed_methods` | front_door, chat (turns), chat (waits, stops, fails), worktree |
| `rs/tests/flow_http.rs` :: `given_a_path_the_service_does_not_have_when_it_is_asked_then_it_is_a_json_404` | front_door, chat (turns), chat (waits, stops, fails), worktree |
| `rs/tests/flow_http.rs` :: `given_malformed_json_when_it_is_posted_then_it_is_a_400_before_any_route_looks_at_it` | front_door, chat (turns), chat (waits, stops, fails), worktree |
| `rs/tests/flow_http.rs` :: `given_the_wrong_secret_when_a_worktree_is_requested_then_it_is_401_and_with_the_right_one_an_invalid_body_is_400_with_the_reason` | front_door, chat (turns), chat (waits, stops, fails), worktree |
| `rs/tests/flow_http.rs` :: `given_an_agent_that_answers_when_a_chat_turn_is_posted_then_the_stream_carries_the_session_the_text_and_the_end` | front_door, chat (turns), chat (waits, stops, fails), worktree |
| `rs/tests/flow_http.rs` :: `given_a_chat_body_without_a_prompt_when_it_is_posted_then_it_is_refused_before_any_stream_starts` | front_door, chat (turns), chat (waits, stops, fails), worktree |
| `rs/tests/flow_http.rs` :: `given_a_running_thread_when_a_stop_is_posted_then_the_agent_is_told_to_interrupt_it` | chat (waits, stops, fails) |

## Old TypeScript black-box suites

| Old suite | Now covered by |
| --- | --- |
| `http.blackbox.test.ts` | `front_door.rs`; the worktree secret and body checks in `worktree.rs` |
| `startup.blackbox.test.ts` | `front_door.rs` through `startup::check`, the function `main` calls before it binds; the process exit itself is not run |
| `chat.blackbox.test.ts` | `chat.rs` (both flows) |
| `control.blackbox.test.ts` | `control.rs`, `chat.rs` (waits, stops, fails) |
| `attachments.blackbox.test.ts` | `documents.rs` |
| `convert.blackbox.test.ts` | `documents.rs`; PDF text now comes from a `pdftotext` double (its arguments are asserted), DOCX from the real fixture |
| `inventory.blackbox.test.ts` | `inventory.rs`; the ORDS rows in `voice.rs` (plan) |
| `routes.blackbox.test.ts` | plan: `viewers.rs` (files and plans); runs: `runs.rs`; edit stats: `voice.rs` (plan); code assist: `front_door.rs`; conversion: `documents.rs` |
| `runs.blackbox.test.ts` | `runs.rs`; the diff stat comes from a `git` double instead of a real repository |
| `tamil.blackbox.test.ts` | `tamil.rs` |
| `vcs.blackbox.test.ts` | `source_control.rs` (both flows); git's porcelain and refusals are now scripted outputs of a `git` double, so the service's parsing and mapping are pinned, not git itself |
| `viewers.blackbox.test.ts` | `viewers.rs` (both flows) |
| `voice.blackbox.test.ts` | `voice.rs` (plan, recording) |
| `websocket.blackbox.test.ts` | `websocket.rs` (both flows); the heartbeat is checked with a 100 ms interval instead of waiting 15 s |
| `worktree.blackbox.test.ts` | `worktree.rs`; cloning and worktree creation are scripted `git` outputs |
| helpers `chatStream.ts`, `fakeServers.ts`, `vitest.blackbox.config.ts` | deleted; their Rust counterparts are `flow/harness.rs` and `flow/doubles.rs` |
| helpers `harness.ts`, `fakeOpencode.ts`, `wsClient.ts`, `global-setup.ts` | kept for `tests/bench` |

## Dropped, and why

Nothing a caller can observe through the router was dropped. What was dropped is either an internal step whose effect a flow
asserts, or the real adapters behind the ports, which the owner's rule puts outside the flow boundary:

- The real adapters: `proc::System` (process spawning, output capture, kill on timeout), `opencode::Http` (Basic credential,
  SSE framing) and `outbound::Reqwest`. Their wording that reaches callers is shared with the doubles (the `ExecError`
  constructors) or produced above them (`Client::fetch_json`). They run for real in `test:e2e:deployed`.
- Engine and projector internals no route can reach: the commit hook, events for a thread that does not exist, out-of-order
  activities, a provider-supplied title (no opencode event is translated into one), the tool-part call-id default.

## Observations (not fixed; production behaviour unchanged)

- A `session.error` on the thread view sets the session to `error` with the message, then the turn's abort sets it to
  `interrupted` without one: a client that reads the thread afterwards finds no error text (a subscriber sees it in the
  stream). This mirrors the gateway's translation.
- After a restart, a new thread's first event can carry a sequence number another thread already used (the flow saw 61 after
  71); each thread's own sequence continues correctly, which is what `afterSequence` resumes on.
- A thread log with a corrupt line before its end makes `Engine::start` fail, and the restarted service then lists no
  threads at all, not just that one. The flow asserts only that the corrupt thread is not served.
- Rasterising a Tamil letter is the slowest step: in an unoptimised build it costs about 0.7 s per request and `tamil.rs`
  took ~9 s of the binary's ~15 s; in the deploy profile the whole binary runs in ~7.5 s.

## Timings

Measured with `pnpm run test:flow` on dev's head (0fc6e3080), deploy profile, `CARGO_BUILD_JOBS=2`, target on disk, on the
4-core sandbox while other agents built (load 6 to 16):

| Build | Compile | Test run | Wall |
| --- | --- | --- | --- |
| clean (empty target directory) | 9 min 56 s | 7.6 s | 10 min 6 s |
| one-line change in the service (`routes/vcs.rs`) | 66 s | 7.2 s | 75 s |
| one-line change in a flow (`flow/front_door.rs`) | 52 to 101 s | 8 to 10 s | 64 to 113 s |
| nothing changed | 0.6 s | 7.8 s | 10 s |

For comparison, the same binary in the default (debug, incremental) test profile on the earlier base, with 4 jobs and the
target on tmpfs: clean 96 s compile and 17 s run; a one-line change 5 s compile and 17 s run. The deploy profile is not
incremental, so a one-line change recompiles the service crate with optimisation; it was chosen so the CI runner keeps one
build of the shared dependencies (the other Rust services test in it too). If the pre-commit gate's ~1 min per change
matters more than that, switching `test:flow` to the default profile is a one-word change.

The deploy profile build of the service binary (`cargo build --profile deploy --locked -p rca-service`) has no warnings.
