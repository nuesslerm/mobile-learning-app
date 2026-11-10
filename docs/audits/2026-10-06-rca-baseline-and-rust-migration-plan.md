# remote-chat-agent: baseline and the Rust migration plan

## Why

The remote-chat-agent (RCA) is the long-running service every Telegram and app chat turn goes through. It runs on the
one Always-Free A1 VM next to every other app, and it is the heaviest resident process there. Moving it to Rust follows
the same reasoning as the `wf` migration: the dominant cost is the Node runtime plus a very large dependency graph
(Express, ws, sharp, playwright, tesseract.js, pdfjs, shiki, ffmpeg-static, whisper-node, the opencode SDK), not the
work the service does.

## Baseline (before any port)

Measured on this container, `node --import @lib/logging/babel-register dist/index.js`, mock-less defaults, three runs.

| | TypeScript service |
| --- | --- |
| Source | 21,855 non-test lines across 13 areas (`data` 6.6k, `routes` 4.3k, `lib` 2.4k, `orchestration` 2.4k, `di` 1.1k, `use-cases` 1.7k, `plugins` 1.2k, `contracts` 0.9k, `ws` 0.5k) |
| Cold start to first `/health` answer | 2.89 s, 2.96 s, 3.06 s |
| Idle resident memory | 237 to 238 MB (the service's own report said 214 MB at 10 s) |
| `/health` latency, sequential | p50 1.09 ms, p99 37.6 ms |
| `/health` under load | Express rate limiter (120 per minute) answers 429 after 120 calls, so a health probe can be refused |
| Dependencies | 22 direct runtime packages |

## Method: a Rust front door that takes routes one at a time

A rewrite of 22k lines in one step cannot be verified or reverted. Instead `clis/rca-edge` (a std-only Rust binary, no
dependencies) listens on the public port and the Node service moves to an internal one:

1. A request for a route the edge has ported is answered by the edge.
2. Every other request, WebSocket upgrades and streaming responses included, is tunnelled byte for byte to the Node
   service with the caller's address in `X-Forwarded-For` (the Node app now trusts a loopback peer for it).
3. Each later PR ports one route group into the edge and deletes the Node code it replaces, with the same black-box
   request/response tests run against the Node app first and the edge second.
4. When the last route is ported the Node service, and with it the 22 packages, is removed and the edge is the service.

Memory shrinks in two steps: first the edge answers cheap routes without touching Node (no saving until Node is gone),
then Node is stopped once nothing needs it.

## First slice (this change)

- `clis/rca-edge` with native `GET /health`, tunnelling for everything else, 7 black-box socket tests.
- A request carrying `Origin` or `Upgrade` is never answered natively, because Express's CORS and rate-limit middleware
  run before `/health` and have not been ported. That means a browser keeps the old behaviour; a plain probe (the VM
  health check, curl) gets the native answer and is no longer rate limited.
- Not yet wired into docker-compose or the VM: nothing deploys it until a service entry moves Node to port 18787
  (`PORT`) and starts the edge on 8787 (`RCA_EDGE_PORT`, `RCA_UPSTREAM`).

| | Node service | rca-edge |
| --- | --- | --- |
| `/health` p50 / p99, 100 vs 2000 sequential calls | 1.09 ms / 37.6 ms | 0.48 ms / 2.15 ms |
| Resident memory | about 249 MB | about 2.2 MB |
| Binary | `dist` 2.8 MB plus Node and `node_modules` | 334 KB, no runtime |

The edge's 2.2 MB is only honest as a floor: until Node is gone the VM runs both.

## Order of the next slices

1. Cheap, stateless routes: `/api/edit-stats`, `/api/convert`, `/api/plan`, `/runs`.
2. CORS and the rate limiter in the edge, then `/health` for browsers.
3. Read-only git and file routes: `/api/vcs`, `/api/diff`, `/api/file`, `/api/worktree`.
4. The WebSocket server and the orchestration engine (the stateful core, last because every route depends on it).
5. Chat, voice and vision routes that call the opencode gateway; the heavy dependencies (sharp, tesseract, ffmpeg,
   whisper) become child processes or Rust crates, decided per route from measured memory.

## Second slice: CORS and preflights in the edge

`rca-edge` now owns every `OPTIONS` request and applies the Node service's origin policy to the ported routes: localhost on any port, the dev-bridge `metro-runner-*` host on any port and suffix, and the `CHAT_AGENT_ALLOWED_ORIGINS` list (trimmed, trailing slashes removed). An allowed origin gets the same `Access-Control-*` and `Vary` headers; any other origin gets the 204 or 200 without them, exactly as the Express middleware does. Browser `/health` calls (which carry `Origin`) are therefore answered by the edge too. Checked against the running Node service for four origins: identical status and CORS headers. Nine socket tests pass. The rate limiter followed in the third slice (below).

## Third slice: the rate limiter in the edge

The edge now applies the same fixed window as the Node service (120 hits a minute per caller address) to every non-preflight, non-upgrade request, ported or proxied, in front of Node's own limiter. Preflights are answered before it and upgrades never pass it, as in Express. A limited caller gets the same 429 as before: the CORS headers, `Retry-After`, and `{"error":"rate_limited"}`; compared with the running Node service after 125 calls, the two answers match apart from the seconds left in the window. The ceiling is unchanged (Node's limiter still counts what reaches it, the edge's counts everything), and a route no longer needs the limiter ported separately when it moves. Test seams: `RCA_EDGE_RATE_MAX` and `RCA_EDGE_RATE_WINDOW_MS`. Thirteen socket tests pass. With the limiter in place, ported routes can do real work: the next slice is the first stateless route group.
