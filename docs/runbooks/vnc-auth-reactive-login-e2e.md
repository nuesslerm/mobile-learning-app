---
title: VNC auth-session reactive login — agent-driven e2e runbook
verifies: Opening a wf secret login <tool> link shows that tool's own login screen, including when another tool's session is already in-flight on the shared desktop.
tier: agent-driven
maxAgeDays: 60
---

# VNC auth-session reactive login — e2e verification

Verifies U6a (remote-chat-agent's reactive-interrupt) end-to-end (U6b, the
Telegram bot's owner-gated delivery, was deleted with the bot in 2026-10), driven from two
angles, for both a real production trigger and the underlying login
mechanism directly. Meant to be run entirely by an agent with tailnet access
(`wf vm net up`) and the `wf` CLI — no human interaction required except
where explicitly marked HUMAN-ONLY (credential entry past a login screen).

Every step names the exact command, and the exact response that is a PASS —
distinct enough from a plausible failure that a generic error can't be
mistaken for it (see CLAUDE.md's "a negative probe must prove the request
arrived").

## Pre-flight

1. `wf vm net up` → PASS: "already on the tailnet" or a fresh join, not an error.
2. Confirm both services are running the tag built for this change:
   `vm-ops-api`'s `GET /services/<app>/ps` for `vnc-auth-session` and
   `remote-chat-agent` → PASS: each `image` field
   shows the tag just pushed, not an older one.
3. Probe the listener's new route directly:
   `POST http://vnc-auth.taila61461.ts.net:8795/auth/launch-url` with valid
   trigger/owner headers and a garbage `url` → PASS: `400` with body
   `{"error":"url origin not allowlisted"}` (proves the route exists and
   the allowlist runs) — a `404` means the deployed image predates this
   route.

## Angle 1 — agent-shell script driving the listener directly, both CLIs

For `claude` and `codex` in turn:

1. `wf secret login <tool>` → PASS: prints a `linkUrl` (never echo the
   full tokened URL in a shared log — this runbook's own execution transcript
   is fine since it's not persisted anywhere public, but never write it to a
   committed file).
2. Open the link from step 1 and screenshot what it shows (vm-ops-api's
   desktop screenshot route is gone) → PASS: the
   captured frame shows the tool's OWN real login page (claude.ai's login for
   `claude`, platform.openai.com's for `codex`) rendered full-size, chromeless
   `--app=` window, no dev-menu/crash-restore chrome.

**Do NOT tear down either session before step 3 below.** Tearing down each
tool's session immediately after screenshotting it — the shape this runbook
used to have — means only ONE tool's session is ever in-flight at a time,
which can never exercise the reuse path against a desktop that also has a
DIFFERENT tool's window open. That gap is exactly how a real bug reached a
human: this runbook's own PASS criteria never caught a reused session's link
opening to show the wrong tool's screen, because it never left two tools'
sessions in-flight together the way real usage (or a prior test run) does.

3. **With both sessions still in-flight**, re-request `claude`'s link:
   `wf secret login claude` again (or the listener's `POST /auth/start
   {tool:"claude"}` directly) → PASS: `sessionId` is unchanged from step 1's
   claude run (proves this hit the reuse path, not a fresh session).
   Screenshot again → PASS: the frame shows claude's OWN login page again,
   not codex's — even though codex's session (and its window) is still
   open from this same run. A codex/OpenAI screen here is the exact
   regression `bringBrowserToFront` (`kasmvnc-session.ts`) exists to prevent;
   if it reappears, the raise call was removed from one of `start()`'s two
   branches, or the launcher lost its CDP connection to the desktop's browser
   (check that `--remote-debugging-port` is still on the wrapper
   `docker/custom-cont-init.d/20-kiosk-chromium.sh` installs).
4. Tear down both sessions: call the listener's `POST /auth/done` for each
   `sessionId` directly (faster than waiting out the full 5-minute timeout).
5. Confirm cleanup: a fresh `POST /auth/start` for either tool succeeds
   (proves the prior sessions' teardown actually freed both slots, not just
   that the commands exited).

## Angle 2 — a real chat turn through remote-chat-agent, relayed via Telegram

> Not runnable since 2026-10: the Telegram bot that relayed the link and handled
> `/done` was deleted, and remote-chat-agent's `VNC_AUTH_OWNER_NOTIFY_URL` is unset.
> Kept as the record of what a replacement owner-DM route must satisfy.

This drives the ACTUAL reactive-interrupt code path (`createStreamChatWithAuthRecovery`
in `src/lib/opencodeClient.ts`), not a simulation of it.

1. **Induce a real, reversible auth failure.** Temporarily set
   remote-chat-agent's `OPENCODE_SERVER_PASSWORD` to a wrong value (NOT
   opencode's real provider API key — this is the Basic-Auth password
   between the agent and its own `opencode serve` sidecar, a distinct,
   lower-stakes credential) and redeploy just that container. This makes
   `probeDirectory` receive a real `401` from opencode server, which
   `WorkingDirectoryNotVisibleError`'s message embeds verbatim — enough for
   `classifyError`/`isAuthError` to match `opencodeDescriptor.authErrorPatterns`
   via the substring `401`.
2. Send a real message to the bot in Telegram (any chat bound to the remote
   instance) → PASS: the chat does NOT get an ordinary reply. Instead:
   - the `VNC_AUTH_OWNER_NOTIFY_URL` bot DM arrives in the owner chat within
     a few seconds, containing a real `linkUrl` and the sessionId context.
   - opening that link shows the VNC desktop running opencode's real
     `opencode auth login` flow (screenshot).
3. Send `/done` in the owner chat (bot reply: "✅ Marked the login session
   done.") → PASS: the listener's session for that id is gone (`POST
   /auth/done` again for the same id now 404s/says unknown, proving `/done`
   really reached the listener and wasn't a no-op UI reply).
4. Confirm the ORIGINAL chat turn, having failed to complete a real opencode
   login within the poll budget (nothing here actually re-authenticates
   opencode — this run only proves the WIRING, not a real credential
   refresh), surfaces the terminal `login_required` SSE frame → observable
   as the bot's own error-surfacing behavior for that code, or directly via
   `GET /api/chat` inspection if the bot's own UI swallows it — PASS: the
   turn resolves (does not hang past `VNC_AUTH_CAPTURE_TIMEOUT_MS`), one way
   or another, rather than leaving `inFlight` stuck (verify via
   remote-chat-agent's `GET /session-debug`).
5. **Restore.** Set `OPENCODE_SERVER_PASSWORD` back to the real value and
   redeploy remote-chat-agent again. PASS: a fresh chat message gets a real
   model reply again (proves production chat is un-broken, not just that the
   env var changed).

## What this runbook does NOT cover (HUMAN-ONLY, by design)

Completing a real `opencode auth login` OAuth exchange, or any tool's actual
credential entry, is never part of this runbook — per this session's standing
rule, automation stops at handing a human a link; a human completes anything
past a verification/credential gate. Steps above stop at "the correct screen
rendered" and "the session tears down cleanly," never at "logged in."
