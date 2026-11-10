# clis/http (`cli-http`): the one HTTP client of the CLIs

Date: 2026-10-09. Plan: `docs/plans/rust-services/standard-crates.md` (phase 4). Decision log: "wf outbound HTTP and OTLP on standard crates" and "cdp, the browser and vnc servers and the encoders on standard crates". Branch `claude/clis-ureq` (PR #338). New package.

## Scope

`wf` reached every HTTP endpoint by spawning `curl` (nineteen `Command::new("curl")` sites: the Metro probes, the signed vm-ops-api and tailnet calls, the BetterStack ClickHouse reads, the Gemini, Tailscale and ORDS APIs, the opencode preflight, the cooldown mirror, the live dispatch tee, the telemetry posts), and `cdp` spawned it for the network proxy. The crate puts one blocking `ureq` 3 behind a small builder: `Request` (method, headers, body, JSON, basic auth, a timeout or none, redirects only when asked), `Via` (the environment's proxies, a direct connection, or a SOCKS5 proxy that resolves the host name itself, as the tailnet's userspace listener does), `Response` (status, every header, body; any HTTP status is a response, only a transport failure is an error), `download`, `answers` and `fetch_local`, and the encoders (`encode_uri_component`, `form_encode`, `percent_decode` over `percent-encoding` and `form_urlencoded`). TLS is rustls with the platform verifier; gzip is decoded. No tokio.

## Before and after

| Measure | Before | After |
| --- | --- | --- |
| Outbound HTTP in the CLIs | 19 `curl` spawns in `wf`, 1 in `cdp`, an HTTP GET over `TcpStream` in `cdp`, a `browser.rs` client with its own chunked decoding | one client |
| Percent and form codecs | three `percent_decode`, three `form_encode`, two `encode_uri_component` copies | one set of three functions |
| One command that posts telemetry (wall p50, n=5, release) | 11.9 ms (fork, exec and TLS of `curl`) | 5.6 ms |
| `wf` release binary | 5.71 MB | 7.36 MB at the end of phase 4 (the client, rustls and the proxy types are about 1.3 MB of it) |

## Tests, by tier

- Unit and flow (`cargo test -p cli-http`, 7 tests against `tiny_http` servers on loopback): a JSON body arrives with its headers and the reply comes back, basic credentials reach the `Authorization` header, an error status is a response and not an error, a redirect is not followed unless asked (and then is), nothing listening is an error and the probe says no, a slow server fails at the timeout, a download leaves the file or none.
- Integration (the wf black-box suite, the real `wf` binary against mock servers): every command that calls out is covered; see `tooling/wf-blackbox`.

## Deliberate behaviour changes

- A transport failure has no `curl` exit code; callers report the client's error text (`wf vm logs` exits 1).
- Redirects are followed only where a call site asks (the downloads), as `curl` without `-L`.

## Deviations from the instructions

- The client is blocking and each command calls it directly; there is no port trait for outbound HTTP in the CLIs. The black-box tier, with the fakes at network level, is the check for a CLI entry point, as the plan says.

## Known gaps

- None. `wf`'s argument parser and the VNC token gate moved onto `clap` and tokio in later slices (see `clis-wf.md`).
