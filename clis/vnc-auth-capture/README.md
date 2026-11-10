# @cli/vnc-auth-capture

One universal mechanism to perform an interactive browser login for an arbitrary
coding CLI (claude, codex, opencode, gemini, gcloud) or site login (leetcode), capture
the resulting credential, and, when the agent is headless on a VM, hand the interactive
step to a human through a link on the tailnet (`tailscale serve`, tailnet-only, HTTPS).

The human sees only the browser: a headed Chromium in kiosk mode on a bare Xvfb screen (Google refuses a headless one) whose page the listener serves as
JPEG frames to a small viewer page and whose input (taps, scrolls, keys) comes back as
CDP events. There is no desktop, X server, window manager or video stream; the login
command's own localhost callback lands in the same container, where the listener
captures the credential.

This directory holds the **image** (Dockerfile, compose, init hooks) and the on-demand
installer for the tool CLIs; the code is the Rust crate `clis/wf-login` (`src/vnc/`, `src/auth.rs`). Three
entrypoints: `wf secret login <tool> [--copy]` (starts the login in this container's browser and waits; `--copy` also
copies the credential files the tool's descriptor lists under `copy` to the caller's home), `wf secret login
capture|done|launch-url` (the control channel) and `wf secret login listener` (the persistent listener).

The image does not carry `wf`. Its listener is `vnc-auth-listener` (`listener/`): a crate
that depends on `clis/wf-login` and `clis/wf-kit` by path and so builds the crates of their cargo closure
(`clis/secrets` for the vault, `clis/wf-vmops` for the tailnet, `clis/cdp`, `clis/http`, `libraries/rca-core` and the
shared `clis/telemetry`), with its own lockfile, in the Dockerfile's `listener` stage. A change to the rest of wf neither
rebuilds nor redeploys this image: the vnc-auth-session leg in `clis/pipeline/src/legs.rs` triggers on this package
and on exactly the directories that stage copies, and the flow test
`ship::the_ship_plan_deploys_what_changed_since_each_service_s_running_commit` keeps the Dockerfile, that leg and
the listener's cargo closure equal. Run the listener flow tests against this build with
`VNC_AUTH_LISTENER_BIN=<path to vnc-auth-listener> cargo test -p wf-login --test flow auth::the_listener`.

## Layout (`clis/wf-login/src/vnc/`)

| Module | What |
|---|---|
| `descriptor.rs` | The login descriptor schema a built-in and a spec both satisfy, the built-in tools, `${ENV}` interpolation, capture / deliver / verify |
| `engine.rs` | Sessions, the per-tool idempotency rule, the reusable TTL link tokens |
| `desktop.rs` | The browser: Chromium over CDP (`cdp::attached`), the login command run in the background, frames and input for the viewer |
| `view.rs` | The token gate behind `tailscale serve` and the viewer: `GET /` (page), `GET /frame` (JPEG), `POST /input` |
| `js/viewer.html` | The viewer page: canvas, tap and scroll mapping, a keyboard button for phones |
| `relay.rs` | A captured credential FILE back to agent-shell through vm-ops-api |
| `listener.rs` | `POST /auth/start`, `/auth/done`, `/auth/launch-url` |
| `serve.rs` | The `tailscale` CLI: this node's address and the serve route |

The flow tests are the `the_listener_*` journeys of `clis/wf-login/tests/flow/auth.rs` (the real binary,
the real Chromium).

## Security — operator checklist (defense in depth)

The threat model treats the VNC frame, the credential, and the tokened link as
sensitive. Several controls are configuration/ACL, not code — enforce ALL of them
when deploying the auth-session container:

- [ ] **`tailscale serve`, never `funnel`** — the endpoint is tailnet-only.
- [ ] **Tailscale ACL/grant + device-tag** scope the serve endpoint to the
      owner identity/device, not merely "anyone on the tailnet". The tailnet
      identity is the **second factor** behind the link token.
- [ ] **Viewer bound to loopback** — the only ingress is the token-gated
      listener behind the `tailscale serve` proxy (see `docker-compose.auth.yml`).
- [ ] **Reusable, short-TTL tokened link** — `engine.rs` mints a
      session-bound token valid for its whole TTL (default 12h), openable from
      more than one device (deliberately not single-use, so a login can be
      handed off mid-flow); an expired or unknown token is rejected. The
      token store is in-memory, so a listener restart invalidates every
      outstanding link regardless of TTL. The token is a **bearer secret** —
      never logged, never printed to chat/PR/commit.
- [ ] **Credential files `0600`** + dedicated ownership on the shared volume.
- [ ] **CDP stays on loopback.** The desktop's Chromium exposes
      `--remote-debugging-port` so the listener can drive the window a human is
      watching (see `clis/wf-login/src/vnc/desktop.rs`). Anyone who can reach that
      port can drive a logged-in browser, so it must never be published, proxied,
      or bound beyond `127.0.0.1` — only processes inside this container should
      see it.
- [ ] **During login, mount only the target tool's credential path** — not the
      whole credential set.
- [ ] **Restrict the session container's egress** to the provider's auth
      domains where feasible; lock the browser profile to the login flow.
- [ ] **Owner allowlist** for the control channel — the listener rejects
      `POST /auth/start` / `/auth/done` from any id not in
      `VNC_AUTH_OWNER_CHAT_ID` (401 without the trigger secret, 403 without a
      listed owner).
- [ ] **Pin the linuxserver webtop base image tag** (a digest for prod) and track it.

The logging discipline (tokened URL + credential paths never reach a log sink)
is asserted by the flow tests (the whole log stream of a real login, scanned for the link token, the cookie values, the trigger secret and the delivery secret).

## Operating

Local (native browser, no VNC):

```bash
wf secret login list         # claude, codex, opencode, gemini, gcloud, leetcode
wf secret login codex        # runs the tool's native login, confirms capture
```

Remote (VM), one-time provisioning then deploy:

```bash
# On the VM, compose .env from the committed non-secret base + Bitwarden secrets,
# then bring up the stack:
wf secret sync-env --for vnc-auth-session --backend bitwarden --base .env.defaults --out .env
docker compose -f docker-compose.auth.yml up -d
```

The agent triggers a login by calling the listener; the bot DMs the owner the
tokened link with "log in to X, then reply `/done`".

## Capturing an arbitrary login (`wf secret login capture`)

A tool does **not** need to exist in this codebase. `POST /auth/start` accepts a
whole descriptor, validated against the same schema the built-ins satisfy — a
built-in is just a descriptor that shipped with the image.

```bash
wf secret login capture --spec ./my-tool.json     # prints a link; open it on your phone
# ... a human completes the login in the shared desktop ...
wf secret login done <sessionId>          # THIS is what captures the credential
```

`wf secret login capture` with no spec prints a complete worked example.

```jsonc
{
  "id": "my-tool",
  "loginUrl": "https://example.com/accounts/login/",
  "authErrorPatterns": ["401", "403", "session expired"],
  // What to take once the human is done. A browser-only credential exists
  // nowhere on disk, so unless it is read out at that moment it is lost.
  "capture": {
    "kind": "cookies",
    "urls": ["https://example.com"],
    "require": ["SESSION", "csrftoken"],   // ALL of them, or it fails
    "format": "cookie-header"               // or "json"
  },
  // Where it goes. ${credential} is the captured value.
  "deliver": {
    "kind": "http",
    "url": "${MY_BACKEND_BASE_URL}/api/session",
    "method": "POST",
    "headers": { "x-admin-secret": "${MY_BACKEND_ADMIN_SECRET}" },
    "jsonBody": { "cookie": "${credential}" }
  },
  // Optional: how to tell later whether the credential is still good.
  "verify": {
    "url": "${MY_BACKEND_BASE_URL}/api/user-status",
    "trueField": "isSignedIn"
  }
}
```

Things worth knowing before writing one:

- **`${ENV_VAR}` resolves in the LISTENER's environment, at use time.** A spec
  travels over the control channel and may be saved or pasted somewhere, so it
  must be able to *name* a secret without containing one. An unset variable
  throws rather than substituting empty — sending `x-admin-secret: ` produces a
  403 that reads as a bad credential rather than a missing one.
- **`capture` and `deliver` go together**, enforced at parse time. Capturing
  with nowhere to put it, or delivering nothing, is always a mistake.
- **`require` is all-or-nothing.** A partial cookie set is not a smaller
  success: the receiver stores it happily and can never authenticate with it.
- **`verify` counts only a strictly `true` field.** A truthy `"yes"` or `1`
  would be a silent false positive on "is this session still valid".
- **A tool whose login writes a FILE needs none of this.** Use
  `credentialPaths`/`validCheck`; the agent-shell relay picks the file up on its
  own. `capture`/`deliver` exist for credentials that live only in the browser.
- **`wf secret login done` is not a formality** for a browser-only credential — it is
  the only moment the cookie can be taken. Until it runs, nothing has been
  stored anywhere.

## Driving the desktop's browser

Everything browser-shaped goes over CDP against the browser the human is looking
at, never through synthetic input at pixel coordinates. The flag is set on the
wrapper `docker/custom-cont-init.d/20-kiosk-chromium.sh` installs at the real
binary path, which is the one interception point every caller passes through —
including `codex login`, which execs `/usr/lib/chromium/chromium` by absolute
path, bypassing `$PATH`, the `.desktop` entry, and `wrapped-chromium`.

`desktop.rs` attaches to that browser over CDP (`cdp::attached`) and exposes
`navigate` / `prefill_login` / `bring_browser_to_front` / `fit_display_to_device` /
`cookies` / `check_launch_invariants`. The prefill is generic across every tool: it matches a consent
button by its visible text and an email field by type/autocomplete/name, in the
live DOM. Descriptors carry no UI automation at all.

Cold start is the only path that still spawns a process; from then on the same
browser is reused, so a human never watches their window die and come back.
