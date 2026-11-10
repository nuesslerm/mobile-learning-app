---
status: open
area: vnc-auth
paths:
  - "clis/wf-login/src/vnc/listener.rs"
  - "clis/wf-login/src/vnc/engine.rs"
  - "clis/wf-login/src/vnc/desktop.rs"
  - "clis/wf-login/src/auth.rs"
  - "clis/vnc-auth-capture/.env.defaults"
  - "clis/secrets/src/secrets/groups.json"
opened: 2026-10-04
last_verified: 2026-10-20
links: []
---
# `vnc-auth-session`'s `/auth/start` for a login-command tool can still time out behind a navigation

## What is broken

As first seen against the TypeScript listener: `POST /auth/start` ran login automation synchronously before
responding; across ~12 attempts it more often hung past 60 s than answered. A client reset while loading the
link twice left the TypeScript listener's Playwright session dead (`Execution context was destroyed`), after
which `/auth/start` stopped answering until a redeploy. A fresh Playwright context also met a `401` from an
`nginx/1.28.3` layer that a `wf browser` session passes (URL-embedded credentials work). The deployed
`CUSTOM_USER` (`test`) had drifted from `docker-compose.auth.yml`'s default (`auth`).

The code has since changed under each of these. The listener was ported to Rust (commit 9b03b119d,
2026-10-08): `clis/wf-login/src/vnc/listener.rs` answers `202` straight after `Engine::start`, which detaches the
tool's login command (`Launcher::run_login`), runs a `loginUrl` navigation on its own thread and schedules the
email prefill (`clis/wf-login/src/vnc/engine.rs`), while the credential watch runs on a thread of its own
(`clis/wf-login/src/vnc/relay.rs`). The image holds no Playwright: `clis/wf-login/src/vnc/desktop.rs` drives one headless
Chromium over CDP and drops a stale attachment after a failed navigation, so the next call attaches afresh.
Commit 7dbaa9b63 (2026-10-12) then removed the streamed desktop and with it the nginx basic-auth hop and
`CUSTOM_USER`: `clis/vnc-auth-capture/Dockerfile` keeps only the listener's own s6 service, and the listener's
token gate and viewer (`clis/wf-login/src/vnc/view.rs`) are the only ingress. `VNC_AUTH_VNC_USER=test` is left in
`clis/vnc-auth-capture/.env.defaults`; no code reads it and `clis/vnc-auth-capture/docker-compose.auth.yml` does
not map it. `VNC_AUTH_DESKTOP_PASSWORD` is left in the secrets registry, and no container code reads it or gets it
from the compose file, but it is still a `required` key of the `vnc-auth-session` group in
`clis/secrets/src/secrets/groups.json`, so `wf secret sync-env --for vnc-auth-session` fetches it from the vault on
every redeploy and fails when it cannot resolve it (Terraform's cloud-init reads the same file). The 2026-10-09
entry in `docs/audits/2026-09-02-token-diet-decision-log.md` records five scripted starts against the deployed
Rust listener (image `vnc-auth-capture:sha-0b7cb377`, built from 0b7cb37, which already contains 9b03b119d and
predates 7dbaa9b63) that answered `202` with none hanging.

What has not been re-run is the full ~12-start series and the double link load against the listener as it
stands since 7dbaa9b63, and the decision log records no start after that commit, so the hang and the dead
session are unreproduced, not disproved. One wait is still in the code: `run_login` takes the
launcher lock that a running navigation holds for its whole length (a cold start alone waits up to 30 s for
the browser), so a `/auth/start` for a `loginCmd` tool can wait behind a `loginUrl` start or an
`/auth/launch-url` call.

## Why blocked

Not blocked; the Rust port and the move to JPEG frames removed the synchronous automation and the nginx auth
hop. Five scripted starts answered `202` against the deployed Rust listener at 0b7cb37, before 7dbaa9b63, but
nobody has re-run the ~12-start series or the double link load against the listener since 7dbaa9b63.

## What unblocks it

Re-run the scripted `/auth/start` calls and the double link load against the deployed listener; if neither
hangs nor leaves the browser unusable, the owner closes the gap. Whatever still fails is fixed in
`clis/wf-login/src/vnc/desktop.rs`: recovering the CDP attachment after a client reset, and letting `run_login`
start without waiting on the navigation's lock. Drop the unread `VNC_AUTH_VNC_USER` from
`clis/vnc-auth-capture/.env.defaults`. Drop `VNC_AUTH_DESKTOP_PASSWORD` from the `vnc-auth-session` `required`
list in `clis/secrets/src/secrets/groups.json` and from the registry together, since `sync-env` fails with
`Unknown secrets registry key` for a group key the registry lacks, and drop it from the `.env.defaults` header
comment and the `VNC_AUTH_TRIGGER_SECRET` registry description too.

## Re-checked 2026-10-19

`/auth/start` now answers `202` straight after `Engine::start` (`clis/wf-login/src/vnc/listener.rs:126-127`), a
`loginUrl` navigation runs on its own thread (`clis/wf-login/src/vnc/engine.rs:122-123`), and the viewer keeps its own
CDP connection that it drops on any error and re-attaches on the next call (`clis/wf-login/src/vnc/desktop.rs:245-258`).
The flow test `the_listener_admits_only_the_owner_starts_and_reuses_sessions_and_gates_the_viewer_by_link_token`
(`clis/wf-login/tests/flow/auth.rs:529`) pins the early answer: with CDP pointed at a dead port (`auth.rs:450`) and a
20 s client (`auth.rs:309`), a `loginUrl` spec still gets `202` (`auth.rs:591`, `auth.rs:597`), and `codex` and
`loginCmd` spec starts get `202` with their commands run in the background (`auth.rs:555-563`, `auth.rs:576-583`).

Every built-in tool but `leetcode` is a `loginCmd` tool (`clis/wf-login/src/vnc/descriptor.rs:49-53`), and for those
`Engine::start` calls `run_login` on the request path (`engine.rs:117-118`), which takes the desktop lock
(`desktop.rs:213-214`) only to read `config.user`, a constant `abc` (`desktop.rs:35`). `navigate` holds that lock
for the whole navigation (`desktop.rs:221-227`), up to 30 s for a cold browser start (`desktop.rs:94-103`) and
then up to 30 s for the page to load (`clis/cdp/src/attached.rs:264`), and `/auth/launch-url` runs one before it
answers (`listener.rs:147`). So a `claude` start sent while a cold `leetcode` start or a `/auth/launch-url` call
is navigating can outlast the 30 s timeout of the `wf secret login --remote` client (`clis/http/src/lib.rs:59`,
`clis/wf-login/src/auth.rs:148`), which then prints "Failed to start a remote login session". Recovery from a dead
browser is partial: `attached()` reuses a held connection unchecked (`desktop.rs:113-123`), only a failed
`navigate` drops it (`desktop.rs:223-224`) and `cookies` keeps it after an error (`desktop.rs:296-310`), so once
the browser dies under a `leetcode` session its `/auth/done` answers `502` (`listener.rs:136-137`) and a repeat
`leetcode` start reuses the session without navigating again (`engine.rs:85-100`). No test sends a `loginCmd`
start while a navigation holds the lock or kills the browser mid-session; the real-Chromium test
(`auth.rs:714`) kills it only at teardown (`auth.rs:800`).

`VNC_AUTH_VNC_USER=test` is still unread at `clis/vnc-auth-capture/.env.defaults:14`, and
`VNC_AUTH_DESKTOP_PASSWORD` is still `required` at `clis/secrets/src/secrets/groups.json:74` (which cloud-init also
gets, `infra/oracle-cloud/main.tf:108`) and still listed at `clis/secrets/src/data/registry.json:85-90` and
`.env.defaults:4`.

## Re-checked 2026-10-20

Nothing under clis/wf-login/src/vnc, clis/wf-login/src/auth.rs or the auth flow test changed after the 19 Oct re-check (f04932559..982839ee9). Every file:line it cites still holds. run_login still takes the desktop lock that navigate holds for up to 30 s plus the page load (desktop.rs:213-227). VNC_AUTH_VNC_USER and VNC_AUTH_DESKTOP_PASSWORD are still at .env.defaults:14 and groups.json:74. No ~12-start series or double link load against the deployed listener has been run or logged since 7dbaa9b63, and CI has no deployment verification test for vnc-auth-session, so the hang is still unreproduced rather than disproved.
