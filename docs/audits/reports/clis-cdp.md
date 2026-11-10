# clis/cdp: the DevTools client on a standard WebSocket and the shared HTTP client

Date: 2026-10-09. Plan: `docs/plans/rust-services/standard-crates.md` (phase 4). Decision log: "cdp, the browser and vnc servers and the encoders on standard crates". Branch `claude/clis-ureq` (PR #338).

## Scope

`cdp` is the Chrome DevTools Protocol client under `wf browser`, `wf visual` and the browser daemon. It carried its own WebSocket client (an upgrade request with a fixed key, masked frames written by hand, pong replies, length decoding), a hand-written base64 encoder and decoder, a `form_encode`, an HTTP/1.1 GET over a raw `TcpStream` for `/json/version`, and, for the network proxy that answers a paused request with the real response, a `curl` spawn with files for the head, body and post data. It now uses `tungstenite` 0.30 for the WebSocket, the `base64` crate, the shared client (`clis/http`, see `clis-http.md`) for every HTTP call, and `cli-http`'s encoders. No tokio.

`tungstenite` has no split read and write halves, and `cdp` reads on a thread while any thread may send (the answer to a paused request, a screencast acknowledgement). The connection is one TCP stream cloned twice into a reader socket and a writer socket, each over a `Wire` whose writes take one shared lock for the whole buffer tungstenite hands it, so a command and the pong tungstenite answers a ping with are never interleaved. Frames, masking, ping, pong, close and the handshake are the library's.

## Before and after (n=5 medians, release builds, one headless Chromium, the same example binary built against the parent commit and this one)

| Measure | Before | After |
| --- | --- | --- |
| `eval("1 + 1")` round trip p50 (300 calls per run) | 0.195 ms | 0.191 ms |
| `eval` p95 | 0.44 ms | 0.39 ms |
| Full screenshot p50 (20 per run) | 34.1 ms | 34.0 ms |
| Hand-written protocol code | WebSocket client, base64, `form_encode`, HTTP GET, `curl` spawn | none |
| `curl` needed for the network proxy | yes | no |

## Tests, by tier

- Real-dependency integration (a real headless Chromium, `cargo test -p cdp`): 22 tests (17 on the attached session, 5 on the raw connection), all passing and none skipped. Two are new, for the path that was `curl`: a page fetches a URL the network proxy routes through this process and the upstream sees this process's `Accept-Encoding: gzip` rather than the browser's list, and a proxied URL that cannot be reached shows the 502 body. Before, the profile directory was shared, so a parallel run printed `skipped: no Chromium` for three of the five raw-connection tests while still reporting them as passed; each launch has its own profile now.
- Black box through `wf` (`tooling/wf-blackbox`, the browser, visual and bench suites): passing on the debug and release binaries.

## Deliberate behaviour changes

- Base64 text with embedded whitespace is no longer accepted (the browser never sends any).
- The proxied fetch sends this process's `Accept-Encoding` and decodes gzip itself; redirects are not followed, as with `curl` without `-L`.
- A transport failure of a DevTools HTTP call carries the client's error text instead of a `curl` exit code.

## Deviations from the instructions

- None for the crate. The stroke generator `mulberry32` stays: it is a deliberate port of the browser harness's seeded generator so a seed gives the same strokes as before, not infrastructure.

## Known gaps

- `Attached` is one 1,580-line file; this migration did not split it.
- `given_a_page_when_navigating_and_evaluating_then_the_script_sees_the_document` (real Chromium) failed once, with the page read as an empty string instead of `hello`, while the `wf` black-box suites were running on the same machine (2026-10-09, `cargo test --workspace`). Three isolated runs of `-p cdp --test chromium` and a full `cargo test --workspace --no-fail-fast` (16 suites, 148 passed, 0 failed) passed afterwards. Root cause: the test read `document.title` right after `Page.navigate`, which answers when the navigation starts, so until the new document commits the script still sees `about:blank`. The test now polls (up to 5 s) for the title; five runs of the file and the full workspace pass. The library was not at fault. A six-busy-loop stress could not reproduce the failure (0 of 20), so the fix is by the CDP contract, not by a red-to-green run.
- `Attached::navigate` returns when `Page.navigate` answers, that is when the navigation starts, as it did before the migration (no change to it in this work). Twelve tests of `attached.rs` read the page right after it; a second one failed once in three runs (the title read as the previous document's). They now go through a helper that waits for the new document to be complete, and twelve runs of the file pass. `wf browser navigate` still does not wait for the load.

