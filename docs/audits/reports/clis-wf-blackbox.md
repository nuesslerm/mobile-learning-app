# clis/wf-blackbox: the wf black-box suite in Rust

Date: 2026-10-10.

## Scope

The black-box suite of the `wf` binary moved from `tooling/wf-blackbox` (TypeScript, vitest) to `clis/wf-blackbox` (Rust, `cargo test`). Each test still runs the real binary as a process against stub tools, mock HTTP, HTTPS, SOCKS5 and daemon servers, and where a test needs one a real Chromium. Tier: integration (real binary, black box). No flow tests apply; the crate is a test harness.

## Before and after

| Measure | Before | After |
| --- | --- | --- |
| Test titles | 947 | 947 (953 Rust tests including the parity and helper checks at the time of the run) |
| Cases run | 1186 | 953 tests (parametrised cases loop inside one test) |
| Clean run on the same binary | 1186 passed | 953 passed |
| Seeded faults caught, same behaviour in both suites | not applicable | 6 of 6 |
| Wall time | about 11 min | 425 s |
| Source | 101 TypeScript files, 27,539 lines | 20,812 lines of Rust |
| Node needed to run it | yes | no |

## Deliberate behaviour changes

- A parametrised TypeScript title (`it.each`) is one Rust test that loops its cases, so a failing row names its case in the message and not in the test name.
- Each browser test starts its own Chromium, four at a time, in place of a shared one per file.
- The signature check of `wf vm-ops` calls `openssl` instead of the `@lib/vm-request-auth` verifier.

## Architecture deviations

None. The crate keeps its helpers in `src/` and its suites in `tests/`, one file per TypeScript file. One TypeScript test stays, in `tooling/wf-blackbox`, because it tests JavaScript (the new-tab content script).

## Standard crates in the harness (2026-10-10)

The first port of the suite carried the TypeScript suite's hand-written helpers: a raw-socket HTTP client (`http_get`, `http_post_json`, the auth listener's `fetch` with a chunked decoder, the attach test's `raw_get` and `dechunk`), a raw HTTP page server, a hand-written 101 handshake for the gate's WebSocket pipe test, and `openssl` processes for certificates, Ed25519 keys, digests and signature checks. That broke done-when 1 (no hand-written HTTP, WebSocket framing or `openssl`). They now use `ureq` (no proxy, status codes are not errors), `tiny_http`, `tungstenite` on both ends of the WebSocket test, and `wf_blackbox::pki` (`rcgen` for the throwaway CA and leaf, `ed25519-dalek` for keys and signature checks, `sha2` and `hmac` for digests). The test that skipped when `openssl` was missing now always runs.

Deliberate behaviour changes of the suite: the oversized-body test sends a real 2,000,000-byte body with `Expect: 100-continue` (a client library cannot declare a length it does not send), and the keep-alive test asserts the answers and a non-`close` `Connection` header through a pooled agent rather than reading one socket. The WebSocket pipe test now speaks frames through the gate (text `ping` echoed as `PING`) instead of raw bytes. 951 tests, 0 failed, 0 ignored, no `skipped:` line.
