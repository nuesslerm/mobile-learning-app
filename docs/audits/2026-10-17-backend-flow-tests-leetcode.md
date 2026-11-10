# Backend flow tests: leetcode-backend and leetcode-auth-cookie-capture (2026-10-17)

Owner decision (17 Oct): each service's unit tests and TypeScript black-box suites are replaced by a few long flow tests in Rust, one
test binary per service (`<service>/rs/tests/flow.rs`, run by `pnpm run test:flow` in the default test profile). Every request
enters through the builder `main` serves (`api::app`), in process through `tower::ServiceExt::oneshot`. Every external dependency is
a double injected at construction: in-memory fakes from the core crate's `testing` feature, local listeners for the HTTP services a
real adapter dials, stub programs in a temporary directory for the processes, and the session file in a temporary directory.

To make that possible `main` now builds each app from one `Ports` struct of real adapters (`infrastructure::ports(..)`), and the
tests build it with doubles:

- leetcode-backend: `Service::new(Ports)`, `api::app(Ports, Settings)`; the rate limit (120/min) is inside `app`. `AppState` and
  `routes` are private.
- leetcode-auth-cookie-capture: `Capture::new(CaptureConfig, Ports)`, `api::app(Arc<Capture>, secret)`; `WfBrowser::with_program`
  and `XvfbStacks::in_dir` name the programs (`WfBrowser::new` and `XvfbStacks::on_path` keep what `main` did before).

One deliberate exception: leetcode-backend's code runner is the real `ProcessRunner`. Its harness scripts (`rs/harness/*.{py,js}`)
are this service's own code, and a double would leave every `/run` and `/format` behaviour unasserted. The flow is skipped, as the
old suite was, when `python3` is missing.

## Counts

| | before | after |
| --- | --- | --- |
| leetcode-backend Rust | 32 tests: 2 `#[cfg(test)]` modules (5), `core/tests/flow_service.rs` (12), `rs/tests/flow_api.rs` (15) | 4 flows in `rs/tests/flow.rs` |
| leetcode-backend TS | 6 vitest black-box files, 78 cases | none (the benchmark keeps its harness in `tests/bench/`) |
| capture Rust | 17 tests: 3 `#[cfg(test)]` modules (5), `core/tests/flow_capture.rs` (9), `rs/tests/flow_api.rs` (3) | 4 flows in `rs/tests/flow.rs` |
| capture TS | 2 vitest black-box files, 20 cases | none |
| Rust test targets compiled | 4 integration targets plus the lib and bin unit harnesses of 4 crates, all `--release` (fat LTO), plus release builds for vitest | 2 (`--test flow`, default profile) |

## leetcode-backend

Flows in `backend/leetcode-backend/rs/tests/flow.rs`:

- **edge**: `given_callers_with_and_without_credentials_when_they_probe_the_edge_then_cors_auth_body_rules_and_the_rate_limit_hold_and_nothing_reaches_leetcode`
- **session**: `given_no_session_when_a_person_captures_one_uses_it_and_lets_it_go_stale_then_every_step_answers_as_the_session_allows`
- **adapters**: `given_the_real_adapters_when_a_caller_goes_from_no_session_to_relayed_runs_then_the_capture_tool_and_leetcode_see_their_own_protocols`
- **runner**: `given_no_leetcode_session_when_code_is_run_and_formatted_then_this_host_runs_it_case_by_case_and_reports_each_outcome`

| Old test or suite | Covered by |
| --- | --- |
| `session.rs`: cookie string parsed, values with `=`, other parts ignored | session (`/session` with `foo=1; LEETCODE_SESSION=a=b==; csrftoken=c=d;  ; bare`) |
| `session.rs`: no session value | session (400, nothing stored) |
| `session.rs`: capture registry refuses a second start until the first expires | session (409, status, clock past the TTL, restart) |
| `session.rs`: unknown fields survive `expired` | session (stale session rejected: `status` expired, `extra` kept) |
| `leetcode_http.rs`: path segments encoded, dot segments refused | adapters (`a%20b`, `a%2Fb%3Fc` on the wire; `..` as poll id and as slug: 502 `Invalid path segment: ..`); an empty segment cannot reach the adapter (an empty slug is 422, an empty poll id has no route) |
| `flow_service.rs`: no session: signed out, LeetCode not called, capture tool asked | session |
| `flow_service.rs`: no session on a route that needs one, tool asked | session (401 `no_session`) |
| `flow_service.rs`: capture in progress: tool not asked again | session |
| `flow_service.rs`: second capture refused with the first expiry | session |
| `flow_service.rs`: fresh session: answer passes through, not re-verified | session (calls `user_status, test, submit, poll`) |
| `flow_service.rs`: stale session accepted: re-verified, time moves | session |
| `flow_service.rs`: stale session rejected: expired, treated as none | session |
| `flow_service.rs`: stale session, LeetCode failing | session (502 `maintenance`) |
| `flow_service.rs`: store fails | session (`{not json` and `[1]` session files: 500; `null` is no session) |
| `flow_service.rs`: LeetCode failing on a relay | session (502 for test, submit, poll) |
| `flow_service.rs`: cookie set, stored active | session (whole stored document compared) |
| `flow_service.rs`: cookie without session value, nothing stored | session |
| `flow_api.rs` (15 router tests: health, bearer, admin, both, 400 cookie, unknown path, relay, 422s, no session, 502, encoded id, 409, run without session, 415) | edge, session, runner |
| `http.blackbox`: health with commit, CORS any origin, preflight | edge |
| `http.blackbox`: five bad `Authorization` forms, right bearer let through | edge |
| `http.blackbox`: three bad admin secrets, admin without bearer, unknown path 401 then 404 | edge (also: refused before a malformed body is read, and on `POST /login-session`) |
| `http.blackbox`: body over 1 MB 413, malformed JSON 400 | edge |
| `http.blackbox`: 121st request in a minute 429 with `Retry-After` | edge (from a second caller address; the first caller still admitted) |
| `gateway.blackbox`: GraphQL request shape, headers and cookie | adapters (headers asserted on every relayed request) |
| `gateway.blackbox`: signed-out reply maps to a null username | adapters |
| `gateway.blackbox`: test, submit, poll paths and bodies; answers pass through | adapters |
| `gateway.blackbox`: five 422 bodies, LeetCode never called | edge |
| `gateway.blackbox`: LeetCode 500 is 502 with status and body for test, submit, poll | adapters |
| `gateway.blackbox`: non-JSON answer is 502 | adapters |
| `gateway.blackbox`: no session: signed out, 401 `no_session` | session |
| `gateway.blackbox`: capture tool asked once per request with its trigger secret | adapters (`POST /trigger`, `X-Capture-Trigger-Secret`) and session (count) |
| `gateway.blackbox`: capture tool down: response unaffected | adapters |
| `gateway.blackbox`: stale accepted, stale rejected, stale and failing | session |
| `gateway.blackbox`: session file not JSON: 500, service up | session (the next request is answered) |
| `gateway.blackbox`: capture active: tool not asked again | session |
| `session.blackbox`: missing or empty cookie 422 | session |
| `session.blackbox`: both values stored; only the session value (csrf empty); `=` kept; no session value 400 | session |
| `session.blackbox`: capture inactive, started, 409, active | session |
| `executor.blackbox`, all 23 cases (pass with input and timing, wrong answer, captured stdout, exception then next case, `sorted` / `setOfSorted` / exact compare, `ListNode`, compile error, no `Solution`, unsupported language, 5 s case timeout, JavaScript, seven 422 bodies, format, unsupported format, no session needed) | runner (the 422 bodies in edge) |
| `protocol.blackbox`: 422 names the field | edge |
| `protocol.blackbox`: body sent in chunks read as one | runner (streamed request body) |
| `protocol.blackbox`: HEAD answers without a body | edge (status and empty body) |
| `protocol.blackbox`: `/HEALTH/` does not match | edge |
| `protocol.blackbox`: 415; JSON that is not an object 422 | edge |
| `protocol.blackbox`: encoded space in the poll id is one segment upstream | adapters |
| `protocol.blackbox`: two requests on one keep-alive connection; `Connection: close`; a broken request line is 400 and the service stays up; HEAD's `Content-Length` | dropped: behaviour of hyper's HTTP/1 server, to which `main` hands the router (`axum::serve`); no code of this service decides it, and a flow test has no socket. The deployed-service runbook reaches it. |

## leetcode-auth-cookie-capture

Flows in `backend/leetcode-auth-cookie-capture/rs/tests/flow.rs`:

- **guards**: `given_callers_with_and_without_the_secret_when_they_trigger_then_only_post_trigger_with_it_starts_one_attempt_at_a_time_within_the_rate_limit`
- **outcomes**: `given_each_way_a_sign_in_can_go_when_an_attempt_is_triggered_then_its_steps_run_in_order_and_everything_opened_is_closed`
- **adapters**: `given_the_real_adapters_when_attempts_run_remote_and_then_local_then_the_backend_telegram_the_browser_daemon_and_the_display_see_their_own_protocols`
- **start-up**: `given_a_required_setting_that_is_empty_or_a_mode_that_does_not_exist_when_the_listener_starts_then_it_refuses_and_names_the_variable`

| Old test or suite | Covered by |
| --- | --- |
| `cookies.rs`: both cookies among others, session first | adapters (daemon returns `other`, `csrftoken`, `LEETCODE_SESSION`; backend receives `LEETCODE_SESSION=…; csrftoken=…`) |
| `cookies.rs`: one cookie missing, no header | outcomes (only `csrftoken`; only `LEETCODE_SESSION`); the empty list is the same branch |
| `sign_in.rs`: signed-in reply true; null data, `{}`, `"yes"`, false are not | adapters (daemon `eval` replies walk `{"data":null}`, `"yes"`, `{}`, then true: four checks) |
| `browser_daemon.rs`: the last JSON line with a URL wins; none found | adapters (stub `wf` prints nothing usable for `status`, so `start` runs and prints a decoy URL before the daemon's) |
| `flow_capture.rs`: local success, events in order | outcomes |
| `flow_capture.rs`: remote: display first, noVNC link, stopped last | outcomes |
| `flow_capture.rs`: backend has an active capture: nothing starts | adapters (409: no daemon call, nobody told) |
| `flow_capture.rs`: backend refuses registration | adapters (500: no daemon call, nobody told, next trigger 202) |
| `flow_capture.rs`: nobody signs in: told, nothing pushed, six checks, closed | outcomes |
| `flow_capture.rs`: sign-in without session cookies | outcomes |
| `flow_capture.rs`: backend refuses the cookie: told why, torn down | adapters (`POST /session failed (500): "boom"`, browser closed) |
| `flow_capture.rs`: browser cannot open: told, nothing to close | outcomes, and adapters (daemon `open` fails) |
| `flow_capture.rs`: notifier fails: outcome unchanged | adapters (Telegram 500, cookie still pushed) |
| `flow_api.rs`: no or wrong secret 401; right secret 202; other path or method 404 without a body | guards |
| `trigger.blackbox`: 404s, 401 with JSON content type, nothing started | guards |
| `trigger.blackbox`: 30 a minute, then 429 `rate_limited` with `Retry-After`, before the secret is checked | guards (wrong secret from a second address; the first address unaffected) |
| `trigger.blackbox`: 202 and registration with the admin secret | guards (202) and adapters (`POST /api/leetcode/login-session`, `X-Admin-Secret`) |
| `trigger.blackbox`: attempt in flight: 200 `alreadyRunning`, no second attempt | guards (registration held open by a gated backend double; afterwards the listener takes the next trigger) |
| `capture.blackbox`: local login completes: daemon `open` and `navigate` args, `eval`s, `cookies`, `close`; session push with admin secret and JSON; two notices with the notify secret | adapters (remote run for the full protocol, local run for the local `open` args and notice) |
| `capture.blackbox`: login never completes; sign-in without session cookies | outcomes |
| `capture.blackbox`: backend already active; registration answered with another status, next trigger taken | adapters |
| `capture.blackbox`: backend refuses the cookie; notifier 500; daemon refuses to open | adapters |
| `capture.blackbox`: remote display stack args, `--no-sandbox`, `DISPLAY=:99`, noVNC link, stopped in reverse | adapters (stub Xvfb, x11vnc, websockify log their start and stop; none start in local mode) |
| `capture.blackbox`: each of five required variables empty, unknown `CAPTURE_MODE`: refused, named | start-up (`Config::try_parse_from`) |
| `capture.blackbox`: the process exits non-zero on those | dropped: `Config::parse` in `main` turns the same error into clap's exit status; the flow asserts the error itself |

## Timings

Shared agent VM, 2 build jobs (`CARGO_BUILD_JOBS=2`), target directory on disk, default test profile:

| | leetcode-backend | capture |
| --- | --- | --- |
| clean `cargo test --test flow` (every dependency built) | 221 s (tests 5.2 s) | 210 s (tests 2.6 s) |
| incremental, `tests/flow.rs` touched | 7.3 s | 4.8 s |
| incremental, an `rs/src` file touched | 9.3 s | |
| incremental, a `core/src` file touched | 10.5 s | 6.0 s |

The leetcode-backend run time is the runner flow's 5 s per-case timeout; the other flows take well under a second.
