---
title: "Chromium's post-quantum TLS ClientHello breaks headless browser tests through the sandbox's proxy"
date: 2026-08-10
category: mcp
tags: [chromium, playwright, tls, post-quantum, ml-kem, proxy, sandbox, github-pages]
problem_type: tooling_decision
module: sandbox-networking
---

## Context

The then-current live-URL leg of the web validation flow — `pnpm --filter mobile-learning-app run test:web:playwright -- --base-url https://nuesslerm.github.io/expo-hello-world-app-2`, which drove headless Chromium via `validate-web-pages.mjs` (the package and the script were both later removed; the surviving app is `frontend/mla-tamagui/`) — failed inside a sandboxed Claude Code agent session with `page.goto: net::ERR_CONNECTION_RESET`. This happened against a fully public, reachable GitHub Pages URL, right after the deploy step (`git push` to `gh-pages`) had gone through the *same* sandbox egress proxy without incident, while a plain `curl` to the *same* URL through the *same* proxy also succeeded. That combination — deploy works, `curl` works, only the browser navigation fails — made the first two hypotheses tempting and, in the end, both wrong.

**First hypothesis, ruled out: a private-destination / Tailscale-style problem.** This repo has prior history (issue #176) investigating Tailscale Funnel for reaching private Oracle Cloud VMs from a sandbox with no direct route in. GitHub Pages doesn't fit that shape — it's fully public, reachable by anyone, no tunnel required. Tailscale Funnel solves "expose something private to the outside"; nothing here was private. The failure was on the *outbound* leg from the sandbox, not an inbound-access problem, so this hypothesis was architecturally irrelevant from the start.

**Second hypothesis, ruled out: a general Playwright-vs-proxy or Playwright-vs-GitHub-Pages incompatibility.** This was ruled out empirically: capturing a Chromium `--log-net-log=<path> --net-log-capture-mode=Everything` trace and following one socket's full lifecycle by its `source.id` in the netlog JSON's `events` array showed the CONNECT tunnel to the sandbox's local proxy (`127.0.0.1:<port>`) completing normally (`HTTP/1.1 200 Connection Established`). Chromium then sent its TLS ClientHello (`SOCKET_BYTES_SENT`, ~1729–1825 bytes across two captures) — and only then did `SOCKET_READ_ERROR {"net_error": -101, "os_error": 104}` (`ERR_CONNECTION_RESET` / `ECONNRESET`) fire, immediately followed by `SSL_HANDSHAKE_ERROR`. No TLS alert, no certificate exchange — a raw socket reset on the server side of the handshake, i.e. inside the proxy's own TLS-termination layer (this sandbox's `/root/.ccr/README.md` documents "TLS is re-terminated there"). The proxy tunnel itself was fine; something about the specific bytes Chromium sent afterward was the trigger — not a blanket Playwright/Chromium-proxy incompatibility, and nothing specific to GitHub Pages as a destination.

Base64-decoding the `SOCKET_BYTES_SENT` payload and walking the TLS record/handshake/extension structure pinned the trigger further: the `supported_groups` extension advertised group ID `0x11ec` and the `key_share` extension carried a 1263-byte payload — the signature of `X25519MLKEM768`, Chrome's default post-quantum hybrid TLS 1.3 key-exchange group. The ML-KEM768 share alone is ~1.2KB, which is why the whole ClientHello (~1.7–1.8KB) no longer fits in a single ~1500-byte TCP segment the way a classic ClientHello does.

Note this repo's `validate-web-pages.mjs` already carried an earlier, separate fix from this same investigation: `validate-web-pages.mjs:393-404` (later removed along with app #1; the survivor is `frontend/mla-tamagui/`) explicitly passed `HTTPS_PROXY`/`https_proxy` into `chromium.launch({ proxy: { server: proxyServer, bypass: 'localhost,127.0.0.1' } })`, because Chromium does not pick up the sandbox's mandated proxy automatically. That fix is necessary but not sufficient — it gets Chromium's traffic *into* the proxy tunnel; the oversized-ClientHello reset happens one step later, *inside* that tunnel, on the proxy's TLS-termination side, and no client-side proxy wiring touches it.

## Guidance

**Recognize the signature first, before investigating anything else.** This failure looks like:

- Any Chromium-driven navigation (`page.goto`, and by extension anything Playwright/Puppeteer-based) to an external HTTPS host fails with `net::ERR_CONNECTION_RESET` (or the raw `ECONNRESET`/`net_error: -101` in a netlog).
- The failure happens from inside a sandboxed agent session that routes outbound HTTPS through a mandated local proxy.
- `curl` (or `git push`, or any non-Chromium client) to the *exact same* host through the *exact same* proxy succeeds without issue.
- The destination is a perfectly ordinary, publicly reachable HTTPS host — nothing tunnel-shaped, nothing private about it.

If all four hold, skip re-litigating the destination or the proxy's basic reachability — go straight to the ClientHello.

**Diagnose it for real with a netlog, not with guesses.** Capture `chromium.launch({ args: ['--log-net-log=<path>', '--net-log-capture-mode=Everything'] })` (or equivalent CLI flags), reproduce the failing navigation, then parse the netlog JSON's `events` array, filter by `source.id` to isolate one socket, and read its full lifecycle in order: CONNECT request → `HTTP/1.1 200 Connection Established` → `SOCKET_BYTES_SENT` (the ClientHello) → either a normal handshake continuation or `SOCKET_READ_ERROR` + `SSL_HANDSHAKE_ERROR`. On the reset pattern, base64-decode the `SOCKET_BYTES_SENT` payload and manually parse the TLS record: a `supported_groups` extension listing `0x11ec` and a `key_share` extension in the ~1200+ byte range confirms `X25519MLKEM768` is blowing the ClientHello past one TCP segment.

**What NOT to waste time on, once you're in this failure signature:**

- **Don't reach for Chrome flags to disable post-quantum TLS.** `--disable-features=PostQuantumKyber,UseMLKEM` and its `chrome://flags` equivalents were tried here and are confirmed no-ops — a second netlog capture after applying them showed a byte-for-byte unchanged ClientHello. The post-quantum-specific flags cannot switch the hybrid group off; capping TLS at 1.2 (`--ssl-version-max=tls1.2`) is the fix instead, see the 2026-08-14 addendum below and `clis/harness-browser/src/browser_launch.rs`.
- **Don't assume it's specific to the destination host.** GitHub Pages wasn't special; any external HTTPS host reachable only through this proxy would likely hit the identical wall, because the trigger is the shape of Chromium's ClientHello, not who's on the other end.
- **Don't reach for a VPN/tunnel-style fix.** Tailscale Funnel, SSH tunneling, or anything designed to expose a *private* destination solves a different problem. The destination is already public and reachable; the break is in outbound proxy TLS termination, not in access to anything private.

## Why this matters

Since a recent Chrome release, Chrome's TLS 1.3 `ClientHello` includes `X25519MLKEM768` — a post-quantum hybrid key-exchange group — by default and unconditionally, adding a ~1.2KB `key_share` that pushes the whole ClientHello past a single TCP segment. A documented class of TLS-terminating proxies and middleboxes was written assuming a ClientHello always arrives in one packet; those implementations mishandle (and reset) a ClientHello spanning two segments. Per web research current as of this session (August 2026), this is a real, dated, industry-wide phenomenon: it broke ~0.05% of connections, concentrated in the enterprise-network gear hardest to update. Google shipped a temporary admin escape hatch — the `PostQuantumKeyAgreementEnabled` enterprise policy — letting fleets disable the hybrid group while middlebox vendors caught up, but that policy stopped actually disabling ML-KEM as of Chrome 138 and is being removed entirely over 2026. Whatever Chrome build lands in a sandbox image, if it postdates that removal no flag, policy, or command-line switch removes the hybrid group from a TLS 1.3 ClientHello. Capping the handshake at TLS 1.2 (`--ssl-version-max=tls1.2`) still works, because a TLS 1.2 ClientHello carries no `key_share` extension at all (see the 2026-08-14 addendum below); short of that, the fix must happen on the proxy/middlebox side.

That lines up exactly with why `curl` and `git push` sailed through the same proxy: neither sends a post-quantum ClientHello, so neither has a payload large enough to trip this specific TCP-segment-boundary bug in the proxy's TLS-termination stack. "The deploy worked but the browser check didn't" is not a git-vs-browser or trusted-tool-vs-untrusted-tool story — it's "small ClientHello vs. large ClientHello," full stop.

This also matches the sandbox's own stated posture: `/root/.ccr/README.md` documents that the proxy re-terminates TLS itself, and its guidance for a proxy-side incompatibility like this is to report it rather than keep working around it client-side. This doc first concluded here that no client-side lever was left to pull; the 2026-08-14 addendum below supersedes that, since the TLS 1.2 cap is one and `wf browser` applies it.

## When to apply

Recognize this class of problem — in this repo or in any other sandboxed/proxied agent environment — when:

- A Chromium-driven browser check fails with `ERR_CONNECTION_RESET` (or a netlog `net_error: -101` / `ECONNRESET`) navigating to an external HTTPS host, but a non-browser client (`curl`, `git`, `wget`) succeeds against the identical host through the identical proxy.
- The environment routes outbound HTTPS through a mandated local proxy that re-terminates TLS (rather than passing raw TCP through end-to-end).
- The failing destination is genuinely public and otherwise reachable — ruling out any "this is a private/internal service" explanation before it starts.
- You're on a Chrome/Chromium build recent enough to default to `X25519MLKEM768` in its ClientHello (current as of mid-2026 releases), and especially one recent enough that `PostQuantumKeyAgreementEnabled` no longer has effect (Chrome 138+).
- You've already burned time on post-quantum flags (`--disable-features=...`) with zero observable change to network behavior — itself a signal you're looking at this bug, not a config mistake.

In each case the actionable response is the same: stop trying post-quantum-specific flags, and either (a) point the check at something that never crosses the proxy at all (localhost/loopback, if the proxy config bypasses it), (b) move that validation step to a machine with ordinary unrestricted network access, or (c) cap Chromium at TLS 1.2 (`--ssl-version-max=tls1.2`), which `wf browser` does by default whenever it routes Chromium through `HTTPS_PROXY`.

## Examples

**Netlog evidence, the socket's full lifecycle** (via `source.id` in the netlog JSON):

1. `CONNECT 127.0.0.1:<port>` → `HTTP/1.1 200 Connection Established` (proxy tunnel opens fine).
2. `SOCKET_BYTES_SENT`, ~1729–1825 bytes across two separate captures — this is the TLS ClientHello.
3. `SOCKET_READ_ERROR {"net_error": -101, "os_error": 104}` — `ERR_CONNECTION_RESET` / Linux `ECONNRESET` — with no TLS alert and no certificate ever presented.
4. `SSL_HANDSHAKE_ERROR` immediately after, closing out the socket.

**Decoded ClientHello fields** (from base64-decoding the `SOCKET_BYTES_SENT` payload and walking the TLS record/handshake/extension structure by hand):

- `supported_groups` extension includes group ID `0x11ec`.
- `key_share` extension is 1263 bytes.
- Both are the signature of `X25519MLKEM768` — Chrome's default post-quantum hybrid key-exchange group, whose ML-KEM768 public share alone is ~1.2KB, explaining why the ~1.7–1.8KB ClientHello no longer fits in one ~1500-byte TCP segment.

**Attempted fix confirmed to be a no-op** (verified via a second netlog capture showing a byte-for-byte unchanged ClientHello):

- `chromium.launch({ args: ['--disable-features=PostQuantumKyber,UseMLKEM'] })`

This list once also named `chromium.launch({ args: ['--ssl-version-max=tls1.2'] })`; that entry is superseded, because the 2026-08-14 addendum below shows the TLS 1.2 cap is the fix.

**Prior, separate fix that was present in this repo's tooling at the time** — necessary but not sufficient on its own — `validate-web-pages.mjs:393-404` (later removed; the surviving app is `frontend/mla-tamagui/`):

```js
// Sandboxed environments route all outbound HTTPS through a mandated proxy
// (see /root/.ccr/README.md) that Chromium does not pick up automatically;
// without this, requests to a real base-url (e.g. the live Pages URL) get
// reset instead of proxied. Harmless when validating the local export
// server, since 127.0.0.1 is always bypassed.
const proxyServer = process.env.HTTPS_PROXY ?? process.env.https_proxy;
const browser = await chromium.launch({
  headless: true,
  ...(proxyServer && {
    proxy: { server: proxyServer, bypass: 'localhost,127.0.0.1' },
  }),
});
```

This gets Chromium's traffic into the proxy tunnel; it does not — and cannot — change the shape of the ClientHello sent once that tunnel is open, which is the actual point of failure documented above.

**The resolution actually applied in this repo:**

- `AGENTS.md`'s "Definition of done (web)" section then required Playwright validation against a **local** export, not the live Pages URL, with these commands (the `mobile-learning-app` package and its scripts have since been removed):
  ```bash
  pnpm --filter mobile-learning-app run test:web:playwright:export
  pnpm --filter mobile-learning-app run deploy:web:pages -- --initial-route /design-system/storybook
  ```
  Today the root `AGENTS.md`/`CLAUDE.md` (source `.agents/instructions/root.md`) carries the same rule: web UI changes are validated with Playwright against a local export first, and browser verification is the `wf browser` CLI against a local `expo export --platform web` (in `frontend/mla-tamagui/`, `pnpm --filter mla-tamagui run export:web`, or `wf test prepare --web`); since 22 Oct the CI browser e2e tests run only on an export, and the shared dev Metro is retired.
- The local-export mode passed `--export-dir` to `validate-web-pages.mjs` (later removed with app #1), which spun up a local static HTTP server (`startExportServer`) and served the export over `127.0.0.1` — a loopback address, which `chromium.launch`'s `bypass: 'localhost,127.0.0.1'` (line 402 above) always exempted from the proxy, so this path never touched the proxy's TLS-termination layer and never triggered the bug. `wf browser` keeps the same loopback bypass today (`--proxy-bypass-list=localhost;127.0.0.1;*.ts.net` in `clis/harness-browser/src/browser_launch.rs`).
- `deploy-gh-pages.sh:147-148` (later removed; the surviving app is `frontend/mla-tamagui/`) already ran exactly this local-export validation as part of its own pre-push check, before this session's change: it printed `[deploy:web:pages] validating local export via Playwright export-dir server` and then ran `validate-web-pages.mjs --export-dir "$OUTPUT_DIR" --base-path "$BASE_PATH" --port 4173`.
  Both this leg and the live-URL leg below shared the same `$VALIDATE` gate (`deploy-gh-pages.sh:146`, default `true`, toggled together by `--skip-validate`/`--validate`) — the two weren't asymmetric in the script; only `AGENTS.md`'s separate, explicit "definition of done" requirement singled out the live-URL leg as mandatory, and that's what this session removed.
- The script's live-URL leg was at `deploy-gh-pages.sh:197-199` (also removed with the first app), inside that same `if [[ "$VALIDATE" == "true" ]]` block, calling `validate-web-pages.mjs --base-url "$PAGES_URL"`. Live-URL Playwright validation is still a reasonable check — just one to run from a machine with ordinary, unrestricted network access (a developer's own machine, a CI runner) rather than from inside this sandbox.
- Two other places that unconditionally prescribed the now-broken live-URL step were updated the same way: `.agents/skills/expo-web-validation/SKILL.md` (its "Playwright workflow" step 4 — this is the skill an agent reaches for that led to trying the live-URL check in the first place) and `docs/archive/solutions/workflow/eas-publishing-checklist.md`.

## Addendum (2026-08-14): Firefox also can't do it — but for a DIFFERENT, separately-diagnosed reason, not the same ClientHello bug

A later session, asked to prove a live GitHub Pages page was actually rendering fetched data, tried the obvious next idea: since the root cause is specifically Chromium's oversized post-quantum ClientHello, and Firefox's TLS stack doesn't send that extension by default, wouldn't Firefox get through the proxy fine?

It does not — but this time the diagnosis went past "it timed out" into the actual failure mechanism, using `MOZ_LOG=nsHttp:5,nsSocketTransport:5,nsHostResolver:5,timestamp` (Firefox's built-in verbose networking log) plus cross-checking the proxy's own `$HTTPS_PROXY/__agentproxy/status` failure log at the same moment:

- **Firefox's own networking stack is fine.** Control test: `firefox.launch()` with no proxy at all, navigating to a genuinely local `python3 -m http.server`, returned `200` in `182ms`. Ruled out "Firefox is just broken in this container."
- **With the proxy configured** (`firefox.launch({ proxy: { server: 'http://127.0.0.1:<port>' } })` — Firefox doesn't auto-inherit `HTTPS_PROXY` any more than Chromium does), navigating to the live GitHub Pages URL *and*, separately, to an unrelated external HTTPS host (an OCI Autonomous DB's ORDS endpoint, independently confirmed reachable via `curl`/Node's `fetch` moments earlier) both hung to the exact configured timeout, even with `waitUntil: 'commit'` (the lowest bar Playwright has — just needs any bytes back).
- **The MOZ_LOG trace shows *why*, and it's a different failure point than Chromium's.** The connection manager creates a `DnsAndConnectSocket`, resolves `127.0.0.1` instantly, pushes the SSL I/O layer onto the socket (`pushing io layer [0:ssl]`) — and then nothing. No connect-succeeded, no connect-refused, no TLS bytes ever logged as sent. Firefox's own backup-connection timer fires roughly every 6–7 seconds, and the *entire* `DnsAndConnectSocket` creation cycle repeats from scratch, forever, until Playwright's outer timeout kills it.
- **The proxy's own request log confirms Firefox's attempts never arrived at all.** Checked `$HTTPS_PROXY/__agentproxy/status`'s `recentRelayFailures` immediately after each Firefox test — zero entries correlate with any test timestamp (only unrelated `nx cloud` traffic shows up there). Contrast with Chromium's documented failure above, where the netlog showed the CONNECT tunnel completing (`HTTP/1.1 200 Connection Established`) and the proxy *did* see and reset the connection after the ClientHello. Firefox's connection to the proxy port doesn't complete, register, or fail visibly on the proxy side — the breakdown is somewhere before the proxy is even reached.
- **Ruled out Firefox's own content/socket-process sandboxing** as the cause: relaunching with `MOZ_DISABLE_CONTENT_SANDBOX=1`/`MOZ_DISABLE_SOCKET_PROCESS_SANDBOX=1` and `security.sandbox.content.level: 0` made no difference.

So: Chromium fails *after* reaching the proxy (oversized ClientHello reset mid-tunnel, the bug this whole doc is about). Firefox fails *before* reaching the proxy at all, for a still-unisolated reason specific to how this Firefox build's proxy-connect path behaves in this container (not this sandbox's TLS-termination bug — that layer is never reached). Two different bugs that happen to produce the same symptom. WebKit was the third engine on the list and couldn't be tested at all — it fails to even launch on this distro, missing ~15 unrelated system shared libraries (`libatk`, `libwoff2dec`, etc.), a real but unrelated environment gap.

**Practical upshot at the time of writing this addendum:** no headless browser engine tested in this sandbox (Chromium, Firefox) could complete a live external-URL navigation, each for its own distinct reason, and a third (WebKit) couldn't even launch here. **This has since been superseded for Chromium — see the next addendum.** Firefox's separate bug (never even reaches the proxy) remains unresolved and undiagnosed further.

## Addendum (2026-08-14, same day): Chromium IS fixable after all — `--ssl-version-max=tls1.2` — earlier conclusion in this doc was wrong

A third session, pushed by a reviewing hook to keep trying rather than accept "no client-side lever" as final, tried the one flag that addresses the *mechanism* (an oversized TLS 1.3 ClientHello) instead of the *feature* (`PostQuantumKyber`, already shown not to work): force the TLS version ceiling down to 1.2, which has no `key_share` extension at all, post-quantum or otherwise.

```
chromium.launch({
  proxy: { server: proxyServer, bypass: 'localhost,127.0.0.1' },
  args: ['--ssl-version-max=tls1.2'],
})
```

Result, tested live against the real deployed dashboard (`https://nuesslerm.github.io/expo-hello-world-app-2/service-report`), same target that reset on every previous attempt: navigation succeeded in ~1.7 seconds, `networkidle` resolved cleanly, and `document.body.innerText` contained the real, live-fetched report data (`Generated 2026-08-14T16:52:10.704Z`, matching a row published minutes earlier via `wf check --health --publish`) — not a cached shell, not a placeholder. Zero console errors. Re-ran the project's own `validate-web-pages.mjs --base-url https://nuesslerm.github.io/expo-hello-world-app-2` (not an ad-hoc script; later removed along with app #1) and it passed too: `Validated Expo web parity at https://nuesslerm.github.io/expo-hello-world-app-2`.

This fully reverses the "resolution actually applied" section above and the first addendum's "no headless browser test against a live external URL will work from this sandbox" conclusion, **for Chromium specifically**. That conclusion came from testing exactly one mitigation (the Kyber-specific feature flag, confirmed via netlog byte counts to do nothing on this build) and stopping there instead of trying a mitigation aimed at the actual mechanism. The fix was applied in `validate-web-pages.mjs`'s live-URL launch path at the time; today `wf browser` passes the same `--ssl-version-max=tls1.2` whenever it routes Chromium through `HTTPS_PROXY` (`clis/harness-browser/src/browser_launch.rs`), so this is closed, not just diagnosed.

Firefox's bug (documented in the addendum above) is unaffected — it never reaches the TLS layer at all, so a TLS-version cap can't touch it. That one remains open.

**Lesson, stated plainly because it cost three separate rounds of pushback to reach:** "I tried the flag with this feature's exact name and it didn't work" is not the same claim as "there is no client-side fix" — a different flag targeting the same underlying wire-format symptom (ClientHello size) can still work even when the feature-specific one doesn't. Test the mechanism, not just the feature.

## Related

- The since-removed Docker-in-the-Claude-container runbook — a different subsystem (Docker builds, not browser TLS) hitting the same root-cause family: this sandbox's egress proxy intercepts/re-terminates TLS in a way some tools (there: `corepack`/`apt-get` cert validation; here: Chromium's ClientHello) can't get through, resolved the same way — validate locally in-sandbox, defer the real check to CI or elsewhere with unrestricted network access.
- `.agents/skills/expo-web-validation/SKILL.md` and `docs/archive/solutions/workflow/eas-publishing-checklist.md` — both updated (see above) to stop prescribing the live-URL check unconditionally.
- `docs/archive/solutions/integration-issues/a-tls-only-egress-silently-swallows-every-non-tls-protocol.md` — the same egress gateway seen from the opposite side. Here a ClientHello was sent and reset because it was too big; there a client sends no ClientHello at all, so the gateway waits forever and the connect hangs with no error whatsoever. This doc's original "no client-side lever left to pull" turned out wrong even for Chromium (the TLS 1.2 cap is one, see the 2026-08-14 addendum), and it does **not** carry over to that case either: for a non-TLS protocol there *is* a client-side fix, namely switching to the vendor's TLS-based web transport.
