# remote-chat-agent: open issue fixes

Status: proposed, 2026-11-02. Scope: the 12 `docs/gaps/rca-*.md` notes other than `rca-layers-without-ports.md` (the hexagonal plan folds that one; `docs/plans/rca-hexagonal/plan.md` is not on disk yet, so its step numbers here follow `docs/plans/hexagonal-architecture/spec.md` Phase 4: R6 composition root, R7 typed settings, R8 fs behind ports, R9 clock/ids). Every claim below was re-read against the tree at `a10440d6e`; the notes were last verified 2026-10-20 and the tree moved since (the 2026-11-01/02 core/rs split, `/exec/v1`, Clock/IdSource ports shifted some line numbers; the ones below are current).

## What is still real

| # | Issue (gap note) | Still real? | Severity | Step |
| --- | --- | --- | --- | --- |
| A | `rca-diff-expand-reads-any-file` | Yes, unchanged | Security, data exposure | 1, 2 |
| B | `rca-sse-chat-consumers` (shim open without a secret, and still served) | Yes | Security (unauthenticated agent turn), then hygiene | 3, 8 |
| C | `rca-chat-turn-hangs` | Yes for `/ws` (no deadline); `/api/chat` is bounded at 600 s | Hang | 4 |
| D | `rca-chat-hangs-when-opencode-is-silent-or-down` | Mostly fixed; stop and reply on engine-owned threads still report success | False success | 5 |
| E | `rca-chat-event-stream-not-scoped-to-session` | Scoping fixed; queued-turn subscription leak real | Resource leak | 6 |
| F | `rca-control-stop-targets-thread-id` | Mostly fixed; broken after an agent-only restart | Correctness | 9 |
| G | `rca-ws-list-threads-empty-after-restart` | Clean restart fixed; one unreadable log still blanks the list | Correctness | 7 |
| H | `rca-ws-duplicate-subscription-id-leaks` | Permanent leak fixed; overlap and client double-send real | Correctness | 10, 11 |
| I | `rca-chat-native-image-attachment-dropped` | `/api/chat` fixed; ws `message.attachments` stored but never sent (dormant) | Correctness | 12 |
| J | `rca-mcp-list-shape-mismatch` | Yes (no validation, wrong toggle contract) | Correctness | 13 |
| K | `rca-source-control-gh-unverified` | `create` never run live | Unverified | 14 |
| L | `rca-runbook-and-remote-run-usage` | Usage reaches the answer; run-log row still has none; runbook-on-service is a feature | Docs/observability | 15 |

Already fixed (no step needed): the Node-era cross-session reply crossing (`core/src/application/ingestion.rs:136-139` skips other sessions' events), the `/api/chat` hard timeout (`rs/src/api/routes/chat.rs:207`), the refused `POST /session` and failed `prompt_async` becoming error turns (`reactor.rs:189-217`), 500/502 on control and question routes for sessions the engine does not own, the register-side 401 on a wrong secret, whole-component and symlink containment inside `expand` (`diff.rs:429-437`), the permanent ws id leak (`ws/subscriptions.rs:67-69`), stop on a session the service ran before a restart (`control.rs:17`), the native picture on `/api/chat` (`attachments.rs`, `chat.rs`, `reactor.rs:213`).

Groups (same root cause, one fix):
- **No effect feedback after the engine accepts a command**: D, and the `/ws` half of C. The decider accepts, the reactor does the opencode call later, and a failure only logs `reactor.handle.failed` (`reactor.rs:150-152`); a stalled opencode produces nothing at all. Steps 4 and 5 share the same core hook (a failure recorded on the thread) so they are designed together and landed apart.
- **In-memory session map not rebuilt after a restart**: F, and the tail of G. `OpencodeBackend.by_thread` (`rs/src/infrastructure/opencode_backend.rs:29,126,177,187`) is the only thread-to-opencode-session map; `Engine::start` (`core/src/application/engine.rs:242-270`) is the only restart hook; the engine already keeps `provider_session_id` on the thread (`reactor.rs:205`). Steps 7 and 9 are two landings on that one path.
- **The shim**: B, and the shim-only halves of E and I. Removing `/api/chat` deletes the `chat` orchestrator, the thread-id/session fallback in `control.rs`, `compat.rs` and the chat-thread restart problem. It does not delete the subscription leak (E) or the ws attachment gap (I): both live on the `/ws` path in `reactor.rs`/`ingestion.rs`.
- **Client resubscription**: H. One root cause (`wsRpcClient.ts` sends a queued subscription twice on open), plus a server overlap that makes it visible.

Not the same root cause: A and B (both unauthenticated reads/starts, but different routes, different rules); J and K (both "unverified upstream reply", different adapters).

## Ground rules

- One push to `dev` per step; a revert of the commit rolls the step back with no data migration.
- Each step writes the failing test first (committed in the same push, shown red once locally with `cargo test -p rca-service --test flow <name>` before the fix), then the fix.
- No step touches `backend/service-kit`, `clis/telemetry`, `libraries/rca-core` or `libraries/rca-ladder`: a 1 Nov push that did redeployed every service and took 55 min. Steps 4, 6 and 9 need a port in `remote-chat-agent-core` only; none needs a service-kit change. If a design below seems to want one, stop and re-plan.
- A rca-only push runs `plan` -> `flow-tests` (rca) -> `typecheck-lint-unit-tests` (clippy) -> `clis` -> `deploy (remote-chat-agent)` -> `integration-tests` -> `deployment-verification-tests`: about 8-10 min warm. Budgets below add to that only where a step has extra jobs.
- Rust layering after the split (post R6-R8): `core` = `remote-chat-agent-core` (domain, ports, use cases; no I/O); `rs` = `rca-service`, with `api/` (routes, wire), `application/` (runs, attachments) and `infrastructure/` (opencode client, file store, config). "Today" below is the crate and folder as they are in the tree now.
- Test language: Given/When/Then; flow tests with the opencode double in `rs/tests/flow/doubles.rs` come first; a real opencode is needed only for step 6's empirical probe (deployment verification) and step 14.
- A negative probe must prove the request arrived: every "refused" assertion also asserts a distinguishing status/body (not a connection error) and that a sibling request with correct credentials succeeds against the same service.

## Progress (2026-11-08)

Fixed and verified in the tree: 1, 3, 4, 5, 7, 9, 10, 11, 12, 13 (evidence under each step). Step 2 done by retiring the command. Step 8 done (see it for what is left over). Nothing else is open before the hexagonal split. Owner's: 6, 14. Waits for the hexagonal split: 15.

## Steps

### Step 1. Bound a diff's `cwd` to the workspace root (issue A, security)

Status: fixed: the `/api/diff` routes are retired and 404 with the secret (`rs/tests/flow/agent_workspace.rs:196-216`). Verified 2026-11-08.

Symptom: `POST /api/diff` with `cwd: "/"` then `GET /api/diff/:id/expand?file=proc/self/environ` returns the service's environment, `CHAT_AGENT_API_SECRET` included; with no secret configured the registration needs no credential at all. Production is shielded only because nothing sets `AGENT_PUBLIC_URL`, so `register` answers 503 (`rs/src/api/routes/diff.rs:381-383`).

Root cause: `register` stores `cwd` unchecked (`diff.rs:398`); `expand` bounds a read only by that same stored root (`diff.rs:425-437`); `register` uses `authorised`, which admits a request with no secret when none is configured (`rs/src/api/routes/mod.rs:252-258`).

Fix design: reuse what `/exec/v1` already has. `confine(root, requested, relative)` and `ExecService::under_root` in `core/src/exec` are the pure containment rule for a workspace root; they are the right owner. In `rs/src/api/routes/diff.rs::register`: switch to `authorised_strict` (as `/runs` and `/exec` do), and refuse a `cwd` that `confine` does not accept under the service's workspace root (the same root `ExecService::new` gets, from config), with 403 and `{error: "cwd_outside_workspace"}`. An empty `cwd` stays allowed only if diffs without file expansion are legitimate (sections-only); then `expand` answers 404 for it. Do not add a second containment function in `diff.rs`.
- Layer today: `rs/src/api/routes` (auth, status) calling `core/src/exec` (rule). After the split: the same two places, `api/` and `core`; R8 later moves the `canonicalize` and file read in `expand` behind `DiffSnapshots`/`WorkspaceFiles` and the rule stays in `core`, so doing this first is cheaper: the new rule is already a core function and R8 moves only I/O.
- Why before R6/R8, not after: it is the only open data-exposure path; the change is about 15 lines in one route; R8 will carry the tested rule with it.

Test (flow, `rs/tests/flow/viewers.rs`; failing first):
- Given a secret configured, When registering with the secret and `cwd: "/"`, Then 403 `cwd_outside_workspace`, and `GET /api/diff/<any id>` is 404 (nothing stored). Probe-arrived: the same request with `cwd` inside the workspace root returns 200 and its expand reads a file.
- Given no secret configured, When registering any diff, Then 401 (and `/runs` still 401, to show the shared rule), while a correct sibling request on a service with a secret returns 200.
- Given an allowed `cwd`, When expanding `proc/self/environ` or `../x`, Then 403/404 (existing cases at `viewers.rs:108-111` stay).
Existing `viewers.rs` and `tests/bench/service.benchmark.test.ts` register with the secret; the bench's `cwd` must be inside the workspace root (check before landing).

CI budget: 8-10 min (rca-only). Revert: restores old behaviour, no state.

### Step 2. Agent command sends the secret (issue A, follow-up)

Status: done by retiring the command (the `/api/diff` routes it posted to no longer exist): `.agents/commands/diff-viewer.md` removed and `wf agent-config sync` run.

`.agents/commands/diff-viewer.md` step 5 (lines 45-53) POSTs `$CWD` with no `X-Bot-Secret` and nothing injects `$DIFF_ENDPOINT`; after step 1 it cannot register. Decide with the owner (open question 2): send `X-Bot-Secret` from `$CHAT_AGENT_API_SECRET` and a `cwd` under the workspace root, or retire the command and the `diff-viewer` skill. Edit `.agents/commands/diff-viewer.md`, then `pnpm run sync:agent-config` (never edit generated files). No test beyond `pnpm run sync:check`; the behaviour is covered by step 1's flow test.

CI budget: no service deploy; lint/sync-check only, about 5 min. Separate push so reverting step 1 does not revert the command text.

### Step 3. Put the `/api/chat` shim behind the same secret check (issue B, security)

Status: fixed: `chat::post` refuses without the secret (`rs/src/api/routes/chat.rs:85-86`; flow `tests/flow/chat.rs:37`). Verified 2026-11-08.

Symptom: with `CHAT_AGENT_API_SECRET` set (`docker-compose.prod.yml:194`), `POST /api/chat` with no `X-Bot-Secret` still starts an agent turn; `/ws` answers 401 without a token (`routes/mod.rs:153-157`).

Root cause: `chat::post` (`rs/src/api/routes/chat.rs:84`) drops the request head, and the router adds no auth layer (`routes/mod.rs:224-230`).

Fix design: `chat::post` takes `Call` like its siblings and returns `unauthorized()` when `!authorised(&app, &req)`. It uses `authorised` (not `_strict`) so the compose e2e stack without a secret keeps working (`tests/e2e/chat.e2e.test.ts:14` sends no header). Layer: `rs/api/routes` today and after.

Test (flow, `rs/tests/flow/chat.rs`; failing first): Given a secret configured, When `POST /api/chat` has no or a wrong `X-Bot-Secret`, Then 401 and the opencode double saw no `POST /session`; When it carries the secret, Then 200 SSE and the double saw `POST /session` (the arrived probe). Existing chat and documents flow tests add the header in the harness.

CI budget: 8-10 min. Note: this step is cheap insurance; step 8 removes the route. If step 8 lands within days, merge them (open question 3).

### Step 4. A deadline on the engine (ws) turn (issue C)

Status: fixed: `Options.turn_deadline` and the `select!` arm (`core/src/application/ingestion.rs:24,110-124`; flow `websocket.rs:505-528`). Verified 2026-11-08.

Symptom: a `/ws` turn against a silent or stalled opencode stays `running` forever; the client gives up. `POST /api/chat` ends at 600 s, which is above undici's 300 s body timeout (so an HTTP client still dies first; irrelevant once step 8 removes the route).

Root cause: the hard timeout lives only in the HTTP route (`chat.rs:148-149,207-214`); the engine path (`reactor.rs` `on_turn_start`, `ingestion.rs::run`) has no timer (`ws/methods.rs:93-96` just returns the receipt).

Fix design: put the deadline where the turn lives. `Options` (already passed into `Wiring`, `ingestion.rs:36-41`) gains `turn_deadline: Duration`, filled from `Config.hard_timeout_ms` at the composition root (`rs/src/api/app.rs:57`, which R6 moves to `main.rs`; core never reads env). In `ingestion::run`, add a `tokio::time::sleep` arm to the existing `select!` (`ingestion.rs:118`); on expiry set `ended_early = "hard_timeout: turn exceeded Ns"` and call `backend.interrupt_turn` best-effort, reusing the stream-lost branch (`ingestion.rs:167-180`) that already records an `error` session with `last_error`. Check first that the core's tokio features include `time` (the 1 Nov core-purity budget trimmed workspace tokio features); if not, put the timer behind the existing `Clock` port or a small `Sleeper` port in `core/src/application/ports.rs` and implement it in `rs/infrastructure`. A hard cap (not an inactivity timer) is the chosen shape because opencode may emit no event during a long tool call; open question 1.
- Layer today: `core/src/application/ingestion.rs` + `Options`; adapter value from `rs/src/api/app.rs`. After the split: same (use case in `core`, value from `infrastructure/config.rs`).
- Rung rotation (the other half of the gap's "unblocks") is dropped: a typed fast error satisfies the note; rotation inside a chat turn is a feature, not a fix. The `rca-ws-chat.md`/`persistent-session-services.md` criterion 4 runbooks are archived; the re-run evidence is the deployment verification below.

Test (flow, `rs/tests/flow/websocket.rs`; failing first): Given `OPENCODE_HARD_TIMEOUT_MS=1000` and an opencode double that accepts `prompt_async` and then sends nothing, When a thread-view turn starts and the client subscribes, Then within ~3 s the thread's session is `error` with `last_error` starting `hard_timeout`, the double saw `POST /session/<id>/abort`, and `oc.subscribers() == 0`. Arrived probe: the double recorded the `prompt_async` before the timer fired. Deployment verification (`tests/deployed_verification.rs`): after deploy, one real `/ws` turn still completes (guards against a deadline that is too tight).

CI budget: 8-10 min; deployment verification adds about 1 min.

### Step 5. Say so when the opencode call of an accepted command fails (issue D)

Status: fixed: failed control calls recorded and reported (`core/src/application/reactor.rs:224-231`; flow `websocket.rs:563-572`). Verified 2026-11-08.

Symptom: stop on a running engine-owned thread answers `{ok:true}`, a reply to an open request answers 204, and `orchestration.dispatchCommand` returns the receipt, even when opencode is down; nothing but a `reactor.handle.failed` log line records it.

Root cause: the decider accepts (`core/src/domain/decider.rs:220-231`) and the route answers before the reactor makes the call (`rs/src/api/routes/control.rs:55-64`, `question.rs:73-90`, `ws/methods.rs:93-96`); the failure branch at `reactor.rs:150-152` only logs.

Fix design, two parts:
1. `core/src/application/reactor.rs`: on a failed `interrupt_turn`, `respond_to_request` or `respond_to_user_input`, dispatch a provider command that records the failure on the thread (an error activity `command-failed` carrying the command id and message), exactly as `fail_turn` (`reactor.rs:162-174`) does for a turn that cannot start. WebSocket clients see it on their thread subscription; no wire contract change (activities are already in the projection; confirm with `remote-chat-agent-contracts` before choosing the activity kind).
2. `rs/api/routes/control.rs` and `question.rs`: for engine-owned threads, await the reactor's outcome for that command id (a small `Outcomes` map in the core reactor: `oneshot` keyed by command id, resolved at the end of `handle`) with a short bound (5 s) and answer 500 (control) or 502 (question) on `Err`, the same codes used for non-owned sessions. The ws `dispatchCommand` stays an accepted-receipt (contract) plus the thread activity from part 1.
Layer today: core use case plus rs routes; after the split the same. This step touches only the control path, so it is independent of R6 and R8.

Test (flow, `rs/tests/flow/control.rs` and `websocket.rs`; failing first): Given a thread the engine owns with a running turn and `oc.go_down()`, When `POST /api/control/stop` is called, Then 500 (not `{ok:true}`); When a reply is posted to its open request, Then 502; When `thread.turn.interrupt` goes over `/ws`, Then the thread shows a `command-failed` activity. Arrived probe: the same calls with opencode up return 200/204 and the double records the abort/reply; with it down the double's refused-request counter increased by one (the call was attempted).

CI budget: 8-10 min.

### Step 6. Close the event subscription when a queued turn folds into the running one (issue E) [done, uncommitted: the live probe on the deployed opencode returned `{"idle_events":1,"user_messages":2,"last_status":"ready"}`, so the counter is dropped; `Ingestions::settle` ends the loop on idle or error when the engine thread has no running session, flow test `given_a_turn_running_and_an_opencode_that_sends_one_idle_for_two_queued_prompts...` failed before and passes after]

Status: the probe exists (`rs/tests/deployed_verification.rs`, `given_two_turns_started_back_to_back_on_one_thread_...`), not yet run live. It drives `/ws` (thread view, service secret as token), starts two turns back to back and prints `{"idle_events", "turns_settled", "subscribers_after"}`: `idle_events` counts the session going ready (one per opencode `session.idle`), `turns_settled` the assistant messages, `subscribers_after` is null (see `ingestion.started`/`ingestion.ended` in BetterStack for the printed thread). Run: `WF_DEPLOYED=1 cargo test -p rca-service --test deployed_verification given_two_turns -- --ignored --nocapture` (env `CHAT_AGENT_API_SECRET`, `REMOTE_CHAT_AGENT_BASE_URL`, optional `RCA_VERIFY_CWD`). Record the result in the PR.

Symptom: queue a second turn on a thread with a turn running; the opencode `/event` subscription stays open until stop, timeout or client drop (a connection per such thread), and with step 4 it would be cut only at the deadline.

Root cause: `Ingestions::begin` adds 1 to `pending` for a turn queued on a live loop (`core/src/application/ingestion.rs:49-51`) and only `session.idle`/`session.error` decrement it (`:152-158`); opencode emits one idle for prompts folded into one run, so `pending` never reaches 0.

Fix design: do not guess opencode's behaviour; probe it, then choose.
1. Deployment verification first (read-only probe, a real opencode, `tests/deployed_verification.rs`): send two `prompt_async` back to back to one session and count `session.idle` events. Record the result in the PR.
2. If one idle (the note's claim): drop the counter. A loop ends on idle when the engine thread has no turn started-and-unsettled besides the one that just completed (`engine.with_thread(..)`, already used at `:141`), and a later `begin` after the loop ended opens a new subscription (the existing `closed` flag and lock already make that race safe, `:72-84`). If two idles: no fix, close the note as not reproducible and keep the flow test as a regression pin.
Layer: `core/src/application/ingestion.rs` today and after (use case; the stream is the `Backend::open_events` port).

Test (flow, `rs/tests/flow/websocket.rs`; failing first): Given a thread-view turn running and the double configured to send one idle for two queued prompts, When a second turn is queued and the run settles, Then both messages' turns are settled and `oc.subscribers() == 0`; with the double configured for two idles (sequential), the same assertion holds (both modes pinned). Arrived probe: the double recorded two `prompt_async` calls before the idle.

CI budget: 8-10 min, plus about 1 min for the probe.

### Step 7. A thread log that cannot be read no longer blanks the list (issue G)

Status: fixed: `Engine::start` skips a bad log (`engine.rs:351-357`) and `file_store.rs` `heal_tail` newline-terminates a cut tail. Verified 2026-11-08.

Symptom: after a restart where any one `threads/<id>/events.ndjson` fails to parse, `orchestration.listThreads` and the shell snapshot answer `[]` for every thread and the running-turn sweep is skipped.

Root cause: `Engine::start` reads each log with `?` (`core/src/application/engine.rs:~255`, the `read_events(&id, None)?` in the loop at `:246-258`), so the first failure returns `Err` before any summary is stored and `App::start` only logs `engine.start_failed` (`rs/src/api/app.rs:97-101`). The store makes such a log itself: a cut last line is dropped only while last (`rs/src/infrastructure/file_store.rs:56-60`) and `append` writes onto the cut tail without a newline (`:68-77`), so one append later the cut line is mid-file and rejected as `bad event line`.

Fix design: (a) `Engine::start` logs `engine.start.thread_skipped` with the thread id and error, and continues; (b) `FileStore::append` first ensures the file ends in `\n` (truncating a partial trailing JSON line is not allowed; newline-terminate it so the next event stays on its own line and the bad line is rejected consistently, then `parse_events` skips a single bad last-but-terminated line only if it is the log's last, see open question 4 for whether to quarantine instead). Layers: (a) `core/application` (policy), (b) `rs/infrastructure/file_store.rs` (adapter, the I/O). Independent of R8 (R8 moves diff/worktree fs, not the file store). Land (a) and (b) as two pushes if the diff is not trivial.

Test (flow, `rs/tests/flow/websocket.rs` after `:495-502`; failing first): Given a state dir with two good threads and one whose log is `not json`, When the service restarts, Then `listThreads` lists the two good ones, a running turn on one is marked interrupted, and `getThread` on the bad one fails (the arrived probe: the existing assertion). Given a log that ends mid-line, When one event is appended and the service restarts, Then the thread still lists.

CI budget: 8-10 min each.

### Step 8. Retire the `/api/chat` SSE shim (issue B, hygiene; absorbs the shim halves of E, F, I)

Status: done 2026-11-08. Removed `routes/chat.rs`, its route, `domain/compat.rs`, the chat orchestrator (`App.chat`, `orchestrators` now returns the one thread-view orchestrator), the `control.rs` fallback, `App.attachments`, the `chat.rs` flow test, the attach half of the `documents.rs` flow test (convert half stays), `chat.e2e.test.ts`, the bench `chat turn` scenario, the chat contracts and their tests. A flow assertion in `front_door.rs` pins `POST /api/chat` to the JSON 404 with no opencode call. Left for the hexagonal split or a follow-up: `Orchestrator::attach_files`/`discard_files` and `SendTurn.files` in `reactor.rs`, and the now-unused `Processor` in `rca-service-core` (`application/attachments.rs`).

Symptom: a second engine, an in-memory chat orchestrator, a legacy wire format and a route nobody calls (no client in `frontend/`, `clis/` or `libraries/` reaches it; only a doc comment in `frontend/libraries/app-config/src/appConfig.ts:29-30` names it).

Root cause/closure: the gap's own `verify` (`! git grep -q "api/chat" -- backend/remote-chat-agent/rs/src backend/remote-chat-agent/core/src`) exits 1 on four hits (`core/src/domain/compat.rs:1`, `rs/src/api/app.rs:70`, `rs/src/api/routes/chat.rs:1`, `rs/src/api/routes/mod.rs:226`).

Fix design: delete `routes/chat.rs`, its route at `mod.rs:226`, `core/src/domain/compat.rs`, the `chat` orchestrator and its `MemoryStore` in `app.rs` `orchestrators()`, the `app.chat` lookup in `control.rs:17` (the session map then comes only from the orch backend), `tests/e2e/chat.e2e.test.ts`, the shim's cases in `tests/bench/service.benchmark.test.ts`, the flow tests `chat.rs` and the chat parts of `documents.rs` (the document-conversion and picture tests move to `/ws` if still wanted: check whether `POST /api/convert` covers the document cases), and the chat schemas in `backend/remote-chat-agent-contracts/src/contracts/chat.ts` with their tests. One commit; contracts is a TS package that only rca consumes (confirm with `rg "remote-chat-agent-contracts"` before landing).
- Order: before R6, deliberately. R6 moves adapter construction out of `app.rs`; if the shim is gone first, R6 moves one orchestrator, not two, and the `control.rs` fallback disappears. Steps 4-7 do not depend on it (they live in `core` and in ws/control routes), so the order only needs 8 before 9 (restart handling then has no chat threads to cover) and before R6.
- Test: the existing `/ws` flow tests are the net; add one flow test that `POST /api/chat` answers 404 (Given the service, When posting, Then 404 and `/ws` still upgrades); a deployment-verification check that `/health` and a `/ws` turn still work.

CI budget: 10-12 min (flow, contracts unit, bench typecheck).

### Step 9. After an agent-only restart, stop/queue/steer use the session opencode really runs (issue F)

Status: fixed: `Backend::adopt_session` (`ports.rs:42`, `reactor.rs:125`, `opencode_backend.rs:192`). Verified 2026-11-08.

Symptom: when `agent` restarts alone (a crash, or a redeploy keeping opencode's image tag) the thread-to-session map is empty, so stop posts `/session/<thread id>/abort`, and queue/steer start a new turn in a fresh opencode session (`reactor.rs:188-194`) leaving the old turn running; `thread.turn.interrupt` is refused `no_active_turn` because `Engine::start` marked the turn interrupted (`engine.rs:242-270`).

Root cause: the only map is in memory (`opencode_backend.rs:29`); the engine's own `provider_session_id` (`reactor.rs:205`) is never read back.

Fix design: add `adopt_session(thread_id, session_id)` to the core `Backend` port (`core/src/application/ports.rs`; the opencode adapter inserts into `by_thread`). In `Reactor`, before `on_turn_start`'s `start_session` check and before backend control calls, if `backend.session_for` is None and the thread has `session.provider_session_id`, adopt it. In `Engine::start`'s crash sweep, after marking the turn interrupted, the reactor aborts that opencode session (best-effort; the turn is stale in the engine so the abort makes opencode agree). After step 8 the chat engine is gone, so only engine threads remain and `control.rs:17` simply uses the backend's map. Layer: port and use case in `core`, map in `rs/infrastructure`.

Test (flow, `rs/tests/flow/websocket.rs` after `:479-491`; failing first): Given a thread with a running turn in session `oc-ses-1` and a service restart over the same state dir with the same opencode double (map empty), When `thread.turn.start` queues a new turn, Then its `prompt_async` goes to `oc-ses-1`, not a new `POST /session`; and Given the restart sweep, Then the double saw `POST /session/oc-ses-1/abort` (arrived probe). Real opencode: a deployment-verification run restarts only `agent` (via a flow-style double is not enough to know opencode keeps the turn; open question below / unverified list).

CI budget: 8-10 min.

### Step 10. Server: register the new subscription only after the old one is gone (issue H, server half)

Status: fixed: `biased;` cancel-first loops and replace in `rs/src/api/ws/subscriptions.rs:103,131`. Verified 2026-11-08.

Symptom: replacing a held subscription id lets the old one deliver a few more events under the same id (5 of 60 events in 4 of 40 judge trials).

Root cause: `subscribe` registers the new sender with the engine (`rs/src/api/ws/subscriptions.rs:79`, `:104`) before cancelling the displaced one (`:67-69`), and the `tokio::select!` loops (`:84-90`, `:109-115`) are not `biased;`, so a cancelled task can still take a queued item; the old sender stays in the engine's listener list until a send fails (`core/src/application/engine.rs:95,97`).

Fix design: cancel the previous subscription first; add `biased;` with the cancel arm first in both loops; stamp each delivery with its generation and drop a frame whose generation is not the current one for that id. Layer: `rs/api/ws` today; after the split the same (transport adapter), R6-R8 do not touch it.

Test (flow, `rs/tests/flow/websocket.rs:260-273`; failing first): Given thread subscription `x`, When `x` is re-subscribed and 60 events are committed on the old thread in the same batch, Then no frame reaches `x` from the old thread after the re-subscribe frame (repeat 40 times in the test, or assert deterministically through the generation stamp). Arrived probe: the new subscription's own snapshot and later events do arrive.

CI budget: 8-10 min.

### Step 11. Client: send a queued subscription once (issue H, root cause)

Status: fixed: `resubscribeAll` drops queued subscribe envelopes whose id is in the subscription table (`wsRpcClient.ts:436-444`), the fake server ends the old listener on a held id (`fakeWsAgentServer.mjs` `handleSubscribe`), pinned by `src/__tests__/wsSubscribeOnce.flow.test.ts` (landed in `fb38357f6`). Verified by reading 2026-11-08.

Root cause: `subscribe` stores the entry and queues the envelope while the socket is closed (`frontend/mla-tamagui/features/coding-agent/src/data/wsRpcClient.ts:148-151,397-398`); on open both `resubscribeAll` and `flushQueue` send it (`:213-214`, `:350-354`, `:154-162`); `useShellSubscription` (`integration/useShellSubscription.ts:29`) subscribes before the socket opens, so the shell snapshot arrives twice on about 28 of 40 connections. The double `testing/node/fakeWsAgentServer.mjs:331-341` overwrites a held id without ending the old listener, so no frontend test could see it.

Fix design: on open, send each stored subscription once (drop queued subscribe envelopes whose id is in the subscription table before flushing, or flush first and have `resubscribeAll` skip ids just sent). Make the fake server end the old listener on a held id like the service. Layer: the feature's `data/` adapter (frontend). This is a frontend push, so it runs the app's flow tests and publishes an EAS Update; it is separate from every rca step. Verify with a flow test of the client (`*.flow.test.tsx` or a `wsRpcClient` flow test with the fake server): Given a subscription made before the socket opens, When the socket opens, Then the server sees exactly one subscribe for that id (assert on the fake server's received frames) and the UI shows one shell snapshot. Mobile UI definition of done: an EAS Update link via CI, and a `wf browser` check against a local export is not needed for a data-layer change with no visual output (say so in the PR).

CI budget: frontend push, about 15-25 min (flow tests, typecheck, EAS Update job); unverified estimate, see below.

### Step 12. Reject (or send) `message.attachments` on ws `thread.turn.start` (issue I)

Status: fixed: `attachments_unsupported` at `rs/src/api/ws/methods.rs:98` (flow `websocket.rs:808`). Verified 2026-11-08.

Symptom: a ws turn with an `image/png` in `message.attachments` is accepted and the thread shows the picture, but the one `prompt_async` carries only text; the model answers from the words. Dormant: no client in `frontend/`, `clis/` or `libraries/` sends the field.

Root cause: the contract accepts `attachments` (`backend/remote-chat-agent-contracts/src/contracts/orchestrationCommands.ts:99`); `wire.rs:109,190` parses it, `decider.rs:195` stores it on the message, but the reactor sends only what `attach_files` filed (`reactor.rs:63-64,179`) and only `/api/chat` called `attach_files` (gone after step 8).

Fix design (recommended, open question): reject at the transport. `rs/src/api/wire.rs` turns a non-empty `attachments` into a `Failure` (`attachments_unsupported`) before the command reaches the decider. Mapping is more work (a ws attachment carries a client reference, so the `Processor` conversion of `attachments.rs` has to run, with its OCR/vision fallbacks) and no client needs it; implement it when one does. Layer: `rs/api/wire.rs` (transport validation). After step 8 the only `attach_files` caller is gone, so also delete `attach_files`, `discard_files` and the `files` table from the reactor if nothing else uses them (then `SendTurn.files` is dead too; keep it if step 8's documents flow moved to ws).

Test (flow, `rs/tests/flow/websocket.rs`; failing first): Given a ws `thread.turn.start` with an `image/png` attachment and an image-capable model, When dispatched, Then the response is a typed `attachments_unsupported` failure, no turn starts and the double got no `prompt_async` (arrived probe: the same turn without attachments starts and sends one).

CI budget: 8-10 min.

### Step 13. Validate opencode's `/mcp` replies at the fetch boundary (issue J)

Status: fixed: `parse_mcp_map`/`parse_toggle` in the route (`routes/inventory.rs:72-82`; flow `inventory.rs:103-119`). Verified 2026-11-08.

Symptom: `GET /api/mcp` answers 200 with whatever opencode returned (array, `null`, a status the contract does not know), where `commands_of` answers 502 to a non-array; connect/disconnect pass through opencode's bare `boolean` while `McpActionResponseSchema` (`remote-chat-agent-contracts/src/contracts/mcp.ts:29-32`) expects `{ok, status?}`; the flow double returns `{status}`, matching neither.

Root cause: `mcp_list` (`rs/src/api/routes/inventory.rs:68-71`) and `mcp_toggle` (`:86-88`) answer `respond(200, &json)` straight from `fetch_json` (`rs/src/infrastructure/opencode.rs:59-65`). The "unknown shape" blocker is gone: opencode 1.18.34 (pinned in `opencode-server/Dockerfile:31`) declares `/mcp` as a map of name to `McpStatus` (`failed`/`needs_client_registration` require `error`; `needs_auth` has no `url`) and connect/disconnect as `boolean` (SDK `types.gen.d.ts`, read from the npm tarball by the note's author; not re-fetched here).

Fix design: a pure validator in `core/src/domain` (`McpStatus` enum with the five opencode statuses; `parse_mcp_map(Value) -> Result<Map, String>`; `parse_toggle(Value) -> bool`), called from the route, which answers 502 `{error: "unexpected_mcp_reply"}` on drift and shapes the toggle reply to the contract (`{ok: bool}`). Update `mcp.ts` to the real shapes (`error` required on `failed`, no `url` on `needs_auth`, toggle `{ok}`) and `contracts.test.ts:143-151`. Layer: validator in `core/domain`, route and `fetch_json` in `rs` (api, infrastructure). The contracts package change is rca-only (confirm no other consumer).

Test (flow, `rs/tests/flow/inventory.rs`; failing first): Given the opencode double returns an array / `null` / `{github:{status:"bogus"}}`, When `GET /api/mcp`, Then 502 `unexpected_mcp_reply`; Given it returns the real five-status map (including `failed` with `error` and `needs_auth` without `url`), Then 200 unchanged; Given connect returns `true`, Then `{ok:true}`. The double's toggle reply becomes a bare boolean. Arrived probe: the double counted the `GET /mcp`. Real opencode: not needed (deployment has zero MCP servers; see unverified).

CI budget: 8-10 min, plus a contracts unit test (about +1 min).

### Step 14. Verify `source_control::create` live (issue K)

Nothing to fix in code; the open item is evidence. `create` (`rs/src/api/routes/source_control.rs:102`) has never run against a real `gh`; list/get ran live on 2026-10-08 but gh has since moved behind the `Commands` port (commit 50161a0a6). Sandboxes cannot reach GraphQL (`gh api graphql` answers 403), so this cannot run from a session.

Plan: add a deployment-verification test (`rs/tests/deployed_verification.rs`, which today has one logging test) that, against the deployed service with the real secret, (1) lists and gets an open PR, and (2) creates a draft PR from a scratch branch of a scratch repository and closes it. Needs a scratch repo and a gh token on the service that may write to it (open question). Until then keep the gap's current text; the flow test `source_control.rs:143-234` pins the contract against the gh double. Smallest honest alternative: add only (1) and record that `create` stays unexercised.

CI budget: 8-10 min plus about 1 min of deployment verification.

### Step 15. Token usage in the run-log row of a remote run (issue L, slice only)

Symptom: the answer of `POST /runs` carries `usage` (`rs/src/application/runs.rs:684-686`) but the run-log row appended by `append_run_log` (`runs.rs:502-512`) has no tokens or cost, so `wf offload ask --stats` shows zeros for remote rows.

Fix design: write the attempt's `usage` into the row where it is appended; keep the format `--stats` already reads on the local path. Layer today: `rs/application/runs.rs`; after R8 it moves with the `RunHost` implementation to `infrastructure/run_host.rs`. Wait for R8 (decision: do this step after R8; it edits the code R8 moves, and nothing is exposed meanwhile). Test: runs flow (`rs/tests/flow/runs.rs`): Given a run whose double reports usage, When it completes, Then the row `GET /runs/log` returns has non-zero tokens (arrived probe: the same row has the attempt's model and status). The runbook-on-service (a checkout the service runs in, deleting `dispatch_local`) is a feature in `clis/offload` and is not in this plan; the note stays open for it.

CI budget: 8-10 min (rca) plus the `wf offload` e2e if its inputs change; do not touch `clis/offload` in this step.

## Order and what can run before the hexagonal split

| Order | Step | Crate / layer today -> after split | Before R6-R8? | CI |
| --- | --- | --- | --- | --- |
| 1 | 1 diff cwd bound | `rs/api/routes` + `core/exec` -> `api` + `core` | Yes, first: security, small, and R8 then moves a tested rule | 8-10 min |
| 2 | 2 diff-viewer command | `.agents/commands` | Yes | ~5 min |
| 3 | 3 shim secret | `rs/api/routes` | Yes | 8-10 min |
| 4 | 4 ws turn deadline | `core/application` + config value | Yes (core is not moved by R6-R8; the value comes through `Options`) | 8-10 min |
| 5 | 5 failed calls reported | `core/application` + `rs/api/routes` | Yes | 8-10 min |
| 6 | 6 queued-turn subscription | `core/application/ingestion.rs` | Yes | 8-10 min + probe |
| 7 | 7 bad thread log | `core/application/engine.rs` + `rs/infrastructure/file_store.rs` | Yes | 8-10 min each |
| 8 | 8 retire the shim | `rs/api`, `core/domain`, contracts, tests | Yes, before R6: one orchestrator to move instead of two | 10-12 min |
| 9 | 9 restart session adoption | `core` port + `rs/infrastructure` | After 8 (no chat threads to cover); any time vs R6-R8 | 8-10 min |
| 10 | 10 ws overlap | `rs/api/ws` | Any time | 8-10 min |
| 11 | 11 client double send | `frontend/.../coding-agent/data` | Independent | 15-25 min |
| 12 | 12 reject ws attachments | `rs/api/wire.rs` | After 8; any time vs R6-R8 | 8-10 min |
| 13 | 13 MCP validation | `core/domain` + `rs/api` + contracts | Any time | 8-10 min |
| 14 | 14 live `create` | deployment verification | Independent | 9-11 min |
| 15 | 15 usage in run log | `rs/application/runs.rs` -> `infrastructure/run_host.rs` | After R8 | 8-10 min |

Why "fixes first, split after" for steps 1-13: the split's R6-R8 mostly move `rs` construction, env reads and fs calls (`app.rs`, `diff.rs` file reads, `runs.rs` host), while steps 4-7, 9 and 12 edit `core` use cases and ports that R6-R8 do not move, and steps 1, 3, 5 and 13 are small route edits whose tests become the safety net for the move. The only steps that conflict with moving code are 1 (the `expand` fs read, which R8 puts behind `WorkspaceFiles`) and 15 (the `RunHost`); step 1 is kept before R8 because the security exposure outweighs a trivial merge, step 15 waits.

Landing rule for every step: `git fetch origin dev && git rebase origin/dev`, lefthook suite, push; confirm the remote head equals the pushed commit before the deployment-verification tier; one step per push; never combine two.

## Open questions

1. Hard cap or inactivity timer for the ws turn (step 4)? Recommendation: hard cap equal to `OPENCODE_HARD_TIMEOUT_MS` (600 s default), the same behaviour `/api/chat` had; an inactivity timer would kill long silent tool calls unless opencode's heartbeat is confirmed (not verified).
2. Retire or repair the `diff-viewer` command (step 2)? Recommendation: retire it if nobody uses it (nothing injects `$DIFF_ENDPOINT`, no repo client calls `/api/diff`), and then also drop `POST /api/diff` registration for non-secret deployments; repair only if the owner uses it from a phone.
3. Merge steps 3 and 8? Recommendation: land step 3 now (it is a 10-line security fix) and 8 within the same week; do not wait for 8 to close the open route.
4. A cut or corrupt thread log (step 7): skip and log, or quarantine the file (rename to `events.ndjson.bad`)? Recommendation: skip and log now (revertable, loses nothing); quarantine only if a thread shows up that is bad twice.

## What I could not verify

- Settled: opencode emits one `session.idle` for two queued prompts (live probe on the deployed opencode: 1 idle, 2 user messages, last status ready), and step 6 dropped the counter. Not covered: a prompt sent after opencode emitted its idle but before the loop read it joins the loop and is not observed, and a joining turn's running mark landing between the idle's ingest and the loop ending leaves the thread running; a sequential second idle is no longer read.
- Whether opencode keeps a turn running when only `agent` restarts, and what its `/event` stream carries for a heartbeat (steps 4 and 9); the flow double assumes both.
- `docs/plans/rca-hexagonal/plan.md` does not exist yet; the post-split layer names follow `docs/plans/hexagonal-architecture/spec.md` Phase 4 (R6-R9) and may differ from the final plan. R9 (Clock/IdSource) already landed on 2026-11-01.
- Whether core's tokio features still include `time` (step 4), and whether `backend/remote-chat-agent-contracts` is consumed by anything besides rca (steps 8, 13). Both are one grep each at implementation time.
- The frontend CI time for step 11 (15-25 min is an estimate); the rca-only 8-10 min figures are the repo's stated budget, not measured by me.
- The opencode SDK shapes in step 13 are quoted from the gap note's reading of the 1.18.34 npm tarball; I did not fetch it.
- `source_control::create` live behaviour (step 14), for the sandbox reason in the gap note.
- I ran no tests, builds or probes; the "still real" verdicts are from reading the code at the cited lines and the notes' own probes (judge probes of 2026-10-19/20).

## Gap notes

Removed by this plan (covered above): the 11 notes of A-K. Kept: `docs/gaps/rca-runbook-and-remote-run-usage.md` (issue L: only the usage slice is planned; the runbook-on-a-service checkout and the parity differences are its own feature) and `docs/gaps/rca-layers-without-ports.md` (the hexagonal plan's). A gap closes only through `/cf-compound <slug>`; step PRs should run it for the note their step closes.
