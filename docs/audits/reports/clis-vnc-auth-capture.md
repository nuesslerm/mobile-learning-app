# clis/vnc-auth-capture: the image that runs `wf auth listener`

Date: 2026-10-09. Plan: `docs/plans/rust-services/standard-crates.md` (phase 4b). Decision log: "wf's VNC gate and control listener on tokio" and the native VM build entry. Branch `claude/wf-vnc-gate-tokio` (PR #340), landed on `dev`. Existing package; one file changed (`Dockerfile`).

## Scope

The listener and the token gate now speak HTTP and TLS themselves (axum, reqwest with rustls, `tokio-socks`), so the image no longer needs `openssl` for the gate's TLS leg to the desktop's self-signed port; the Dockerfile installs `curl` and `ca-certificates` only (curl is what `scripts/setup.sh` downloads installers with). The image takes its `wf` binary from `nuesslerm/wf-cli:latest`, so `wf-cli` must be rebuilt as `latest` before this image is rebuilt without `openssl`; until then a rebuilt image carries the old gate.

## Before and after

| Measure | Before | After |
| --- | --- | --- |
| Child processes of the listener | `curl`, `openssl` | none |
| Packages named in the image for the listener | `curl`, `openssl`, `ca-certificates` | `curl`, `ca-certificates` |
| `wf` binary in the image | 4,936,120 B (aarch64, `wf-cli:latest` then) | built natively on the VM from the branch (`sha-43261cb4`, pushed, not redeployed); its aarch64 size was not measured (the x86-64 release binary is 8.3 MB with the HTTP stack) |

## Tests, by tier

- E2E (the VM, runbook tier): both images built natively (arm64) by `wf deploy run wf-cli vnc-auth-capture --build-only --ref claude/wf-vnc-gate-tokio` and pushed as `sha-43261cb4`, exit 0; nothing redeployed. Earlier, the image was pulled into agent-shell's docker and run with the entrypoint overridden: `wf auth listener --help` runs, the s6 service is `exec wf auth listener`, and the listener logged `vnc_auth.gate.listening` and `vnc_auth.listener.listening` and answered `POST /auth/start`.
- Integration: the listener and gate are covered by `tooling/wf-blackbox` `auth-listener.integration.test.ts` (39 tests against the real binary and mock backends).

## Deliberate behaviour changes

- None in the image's contract; the listener's HTTP behaviour changes are listed in `clis-wf.md`.

## Deviations from the instructions

- None for this package.

## Known gaps

- Not run: the whole container under s6 with the desktop and its Chromium over CDP, and the compose app (`vnc-auth-session`) with its tailscale sidecar (it needs the tailnet auth key).
