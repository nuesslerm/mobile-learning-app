# Concepts

Shared domain vocabulary for this project — entities, named processes, and status concepts with project-specific meaning. Seeded with core domain vocabulary, then accretes as ce-compound learnings; direct edits are fine. Glossary only, not a spec or catch-all.

## Turn

One complete request-and-response exchange through the chat agent: a session is resolved, a prompt fired, the agent's event stream consumed, and the exchange ends at a terminal idle. A turn is the unit everything else counts in — one in flight per session at a time, one hard wall-clock budget, one inactivity watchdog.

Two rules are load-bearing, both learned the hard way. First, a turn that reaches its terminal idle having produced no content at all is a FAILURE, not an empty answer: the agent backend runs out-of-process and can fail a prompt internally without ever publishing that as an error event, so a structurally perfect stream carrying nothing is the shape a broken turn takes. Second, the working directory a turn names must exist on the agent backend's own filesystem, not merely on the caller's — the two are separate namespaces even when they share a network namespace, and a directory the backend cannot resolve yields exactly the contentless turn above.

## Auth Seed

The long-lived shared value from which a VM's short-lived bearer tokens are derived, not a token that is itself transmitted — callers send a one-way, date-derived value, so a captured credential expires within a short grace window and cannot be reversed back into the seed.

The distinction is load-bearing, not pedantic: because the seed is only ever held in a running process's memory and never sent, the value in the vault and the value a live service will accept can silently diverge the moment the vault is written. Rotating the seed is therefore an ordered handoff across every holder, not a single write — sequenced so the credential that authorizes each service's reload is still valid when used. A second, independent request signature is layered on top, so a captured derived token alone is not sufficient to act.

## Redeploy

The remote, HTTPS-triggered operation that brings a deployed service up to a
published image on the VM — the whole deployment path, since no interactive shell
into the host is available: an image is published from elsewhere, a redeploy is
requested, and the host pulls and restarts against it.

Three rules are load-bearing. First, a redeploy composes the service's runtime
environment fresh from the vault, so what the deploy step *inherits* decides
whether the vault wins: an ambient value — even an empty one — is read as a
deliberate override and suppresses the vault copy, making a missing credential
self-sustaining, because the redeploy meant to supply it is the thing that keeps
skipping it. Second, a service that redeploys *itself* generates the script for
its own next redeploy while running, so a change takes effect one cycle later:
the first redeploy after a change still runs the previous script; only the one
after that runs the new one. The intermediate cycle reproduces the old behavior
exactly, indistinguishable from the change not having worked. Third, a redeploy's
environment reaches the container through two independently-maintained
descriptions of the same variable set — one resolving values from the vault,
another declaring which of those resolved values the container actually
receives. Correctly resolving a value in the first says nothing about whether the
second was updated to admit it: an unlisted value is silently dropped with no
error, indistinguishable from never having been configured at all.

## wf

The single developer-facing command-line entrypoint for this repo, mounting the capabilities of every CLI package as subcommands rather than exposing each package separately.

New capability is expected to reach engineers and agents through `wf`, not through an ad-hoc shell script: since `wf` is the one surface everyone already searches, a capability placed anywhere else is effectively undiscoverable, and a script that wraps a tool only covers the callers who remember to invoke it — never the other code paths that reach the same tool directly. When a `wf` command is broken or missing, the expectation is to fix or extend it rather than work around it.

## Bundle

The single, self-contained, committed build output a CLI package produces — one per package, containing everything it needs to run except its declared externals.

*Avoid: bootstrap, dist (for a CLI package's own entrypoint)* — these named two separate outputs a CLI package previously produced side by side, with runtime fallback-preference logic choosing between them at startup. Both were retired in favor of the single Bundle, which needs no such choice.

This retirement is scoped to a package's own CLI entrypoint, not every compiled output a package produces. A package also consumed as a library by other in-repo code keeps a separate compiled build for those importers — that output is not deprecated by a Bundle elsewhere in the repo. The two can interact in an easy-to-miss way: a downstream package's Bundle-build can statically inline a library dependency's compiled output as data (not just link against its code), freezing that data at whatever the dependency's output held when the Bundle was built. The dependency's own build then becomes a correctness prerequisite for the downstream Bundle, not merely a convenience — an ordering the build graph must declare explicitly, or a stale Bundle silently ships data one build cycle behind its true source.

## Agent Approve Guard

The PreToolUse hook gating every Bash/Edit/Write tool call against a repo-committed approval-rule config, denying calls that match a configured guard rule.

When its config is invalid, the Guard does not fail the same way for every tool: it fails closed on Bash (refusing all command execution, since a silently-disabled guard there is a real risk), but never on Edit/Write, because editing is the only path available to repair a broken config — including the config file itself. A Guard that failed closed uniformly could wall off its own fix and deadlock the session.

## Remote Control

The Claude Code platform capability letting an agent programmatically create, list, and manage other Claude Code sessions on its own initiative — as distinct from a human manually creating a session through the normal client UI, which is a separate, unaffected path.

Remote Control can be disabled per account or organization via a fetched-fresh policy value, independent of any local repo configuration or MCP permission setup. When disabled, calls to session-orchestration tools fail immediately with an approval-required error regardless of whether a human explicitly approves the in-app permission prompt — the prompt is not the actual gate. Organizations restricting usage-overage spending will likely restrict Remote Control too, since agent-driven session-spawning has no natural throughput ceiling the way manual, human-driven session creation does.

## Tailnet Forward

A local TCP listener that publishes a tailnet-only service on plain `127.0.0.1`, tunneling every connection to it through the tailnet's own SOCKS5 proxy — so a client that cannot speak SOCKS5 itself (a browser, a bare `curl` call) can reach the service as an ordinary local port instead. Complements the Egress Gateway: that governs traffic leaving the sandbox; this governs reaching a tailnet-only destination from inside it.

A Tailnet Forward is keyed by destination host and port, not by caller: a second request for the same destination reuses the existing forward rather than opening a duplicate listener, and the forward persists independently of whatever asked for it, for as long as the hosting process runs. It optionally accepts a shared observer, invoked for every connection it carries, that sees every raw byte in both directions before relay — enough to diagnose a client that silently fails against the real destination, and to correct its traffic in flight as a live workaround when the defect cannot be fixed at its source.

## Egress Gateway

The mandatory outbound proxy every sandboxed agent session's traffic passes through — a TLS-terminating intermediary rather than a port filter, re-signing each connection under its own certificate authority instead of merely permitting or denying one.

Two consequences are load-bearing, and both present as something else. First, the gateway only speaks TLS: a protocol sending raw bytes completes its TCP handshake then waits forever for a reply that cannot come, so a non-TLS client hangs with no error at all while a raw reachability probe of that same port reports it open. Read that shape as a wrong transport, not a blocked network — the remedy is to switch to whatever TLS-based transport the vendor's own browser client uses, which is TLS by construction. Second, because the gateway re-signs connections and must parse each handshake itself, two client behaviours fail for reasons that look host-specific but are not: a cert-pinning client cannot validate a gateway-signed certificate, and a client whose handshake spans more than one packet can have the connection reset before any certificate is presented. In both cases the same host reached by a different client succeeds — one tool's failure is never evidence that a host is unreachable.

## Agent Shell

The long-running deployed container an agent connects into over the private network as a real interactive user, so the developer command-line entrypoint is exercised the way a human would meet it rather than through a container-exec shortcut.

The distinction from an exec is load-bearing, not stylistic: a real login runs the host's pluggable-authentication stack and loads the system-wide environment file, while an exec inherits the init process's environment directly. So a variable the login path never populates is present under exec and absent under login, and a check built on exec can pass indefinitely while every real session is broken — the class of bug this surface exists to catch. Its network name is also unstable across recreation: the name service appends a numeric suffix when a node re-registers while the old record still holds the name, so a hardcoded name resolves to nothing and every caller fails at the proxy hop with an error that reads as a broken proxy rather than a missing host. Resolve the name from the live peer list instead.

## Verification ladder

A six-tier scale — `unit, contract, flow, integration, e2e, visual`, lowest first — that names the *kind* of proof a change offers: a single function's pure logic, an adapter honouring its port, the composite at its composition root, the adjacent real service, the deployed system through its real entry point, or the rendered result on a screen. Every `units.jsonl` unit carries a `tier` and every report names the highest tier that ran.

The load-bearing rule is the reporting order, top-down: a green unit run is not verification when an upper tier is reachable, because the defects that have actually broken this repo — a `default-repo` 400, raw MTProto through a TLS-only egress, a missing CLI bundle, an always-passing `not.toContain('<tg-spoiler>')` — live in the seams between real processes, not in isolated functions. Saying "331 passing" without naming the tier that ran is the wording that hid them. Doctrine: `docs/solutions/testing/verification-ladder.md`.

## Flow test

A test shaped like a unit test whose subject is a significant entry point (a server router, a CLI `main`/`run`, a mounted route), called in-process with every external dependency replaced in code. It boots the real Composition Root with only its ports replaced (plus the harness's native-module stubs where jsdom has no runtime), and asserts on the observable output through the real entry point — never by reaching into internals. In Rust the same shape is the real router driven with `tower::ServiceExt::oneshot` and fakes behind port traits.

The shape is what separates a flow test from a unit and from an integration: the subject is the composite as wired, not a function in isolation and not the package plus its neighbour, and the only doubles sit at the outermost seam (network, fs, clock, the other services). For a screen, the real screen, design-system components and state wiring render in a browser-like document as the shipped web build does, driven as a user would; its assertions are what appears on screen and where the app navigates, never store internals or mocked callbacks. A test that exercises a store or a hook without rendering the screen is a unit test, whatever it is called. Doctrine: `docs/solutions/testing/verification-ladder.md`.

## Integration test

A test that starts the built program as a black box (the real binary or process, real sockets) and replaces its neighbours at the network or process level: a fake server on a port, a stub binary on `PATH`, a mock upstream. Nothing is injected through code; the program does not know it is under test.

It differs from a flow test, which calls the entry point in-process and replaces dependencies in code, and from an e2e test, which replaces nothing. Table and worked example: `docs/solutions/testing/verification-ladder.md`.

## E2E test

A test of the same entry point against the real running production environment: the VM, the real vault, the real services and network. Nothing is replaced. It runs through runbooks (`wf offload ask --runbook`) and `wf vm exec`, and it is the tier that catches the real environment disagreeing with the assumptions the lower tiers encode. See `docs/solutions/testing/verification-ladder.md`.

## Verification artifacts bucket

The OCI Object Storage bucket (`mobile-learning-verification-artifacts`) that every `e2e`/`visual` unit group's closing step publishes into (today via `wf check --deployed <sha> --publish`, which renders the verify report to `verification/verify/dev/<sha>/`), holding the run's logs, screenshots, and judge verdicts behind a PAR; the printed public URL is the durable pointer a report cites, independent of whatever the VM-side artifact store retains.

## Sandbox-only orchestrator

The git-state worker in the sandbox (`wf offload plan --site sandbox`) that dispatches mechanical work to the opencode agents and runs `unit`/`contract`/`flow`/`integration` gates in the local sandbox or laptop where the repository is checked out; it also runs a browser `e2e` gate itself, against a served web export (`wf test prepare --web`, then `wf test run <test> --platform browser`); an Android `e2e` runs only in CI, and a `visual` check or a runbook travels to the VM through a `wf vm exec` script running `wf offload ask --runbook` for the verdict to bind.

## Pty Session

A persistent real terminal, held open on the Agent Shell across separate, independent tool calls: one call starts it and returns a handle, later calls send keystrokes and read the screen, so each response can be read before the next input is chosen.

Reserved for cases where the terminal *rendering* is the thing under test — an interactive picker, a read-eval-print loop's raw-mode input handling. A check that merely reads a command's output does not need one, and routing such a check through a terminal is what forces the workarounds a pty imposes: input echoed back before the command runs, carriage returns, bracketed-paste toggles, prompt redraws. Two properties make the session usable. Its multiplexer answers a curses program's cursor-position query with no client attached, so no manual escape-sequence reply is needed. And it tees its full raw output for its lifetime alongside the rendered screen, because a screen is only a snapshot: output overwritten between two reads is invisible, a long reply scrolls an earlier match away, and an exited session can no longer be captured at all. Match against the durable stream; quote the screen.

## Model ladder

The single, git-reviewed file (`libraries/rca-core/plugins/ladder/data/model-ladders.json`) that names every provider pool, every model with its `strength / vision / automatic` triple, and one ordered ladder per `Shape = implementer:${Tier} | judge:text | judge:vision | document | runbook` — the seam the `wf offload plan` worker and `wf offload ask` surfaces route through, replacing the old hard-coded `MODEL_ROTATION` and the parallel `CHEAP_MODELS` list whose disagreement was a failure mode in itself.

A `Shape` is what the caller is doing, not which agent or which CLI: cf-work passes `implementer:${unit.tier}`, the judge gate passes `judge:text` / `judge:vision`, `run-prose.ts` passes `document` and reports `bad-output` (reply not a document) so the chain rotates instead of retrying the same model. The `Shape` is what chooses the start rung and what decides whether a `fatal` stops the loop; a per-unit `models` list is a pin (it replaces the ladder, cold-filtered), not an allowlist, mirroring the `models`-replaces-rotation rule from `cf-check.md`. `automatic: false` rungs (`claude-opus-5`, `claude-fable-5-1`) are invisible to automatic dispatch and reachable only by an explicit `models` pin — never-Opus is a property of the ladder, not a check at the call site.

The ladder's runtime behaviour is four contracts: a failure classifier (`cap | crash | timeout | wrote-nothing | bad-output | fatal`) where only `fatal` stops the loop and `cap` marks the pool cold, an escalation gate that promotes one strength level on a second identical failure class in a row (skipping equal-strength rungs), a per-pool cooldown ledger that learns its window from real recoveries (`observeOk` shrinks `learnedResetMs`, a cap after recovery grows it by 25 %), and a stats-driven start rung that begins at the cheapest model with `okRate >= 0.8 && runs >= 5` for the shape when there is enough evidence, else the first rung. The console reconcile (`wf offload ask --stats --reconcile`) is the audit trail against the OpenCode Console CSV — drift between the local ledger and the console is the bug we would otherwise only learn from a missing row in the next plan. Doctrine: the model-chain doc under `docs/solutions/workflow/`.

## Image-Baked CLI

A command-line binary copied into a container image when the image is built, so what it does is frozen at that build and can lag arbitrarily behind the worktree the caller is standing in.

The trap is that its name resolves first on the path, so a script that says the bare command name runs the stale copy while the fresh one sits unused a few directories away. Any workflow that ships behaviour through the source tree rather than through an image rebuild must call the tree's own bundle by path; a bare name is only safe when the image was rebuilt from the same revision.

## Worktree Sync

Bringing a long-lived remote checkout of a branch to the exact commit a caller has just pushed, by fetching and hard-resetting it before anything branch-specific runs there.

Without it the remote checkout is whatever the last sync left behind, so a runbook or test tier executes against a mix of old code and new instructions and fails in ways that read as bugs in the change. Syncing is a precondition of the run, not a step inside it, and the sync must be verified by comparing the remote head to the pushed commit, not assumed from a clean exit.

## Ring

One layer of a package's architecture, named by its distance from the pure core: domain, application, adapters, Composition Root. A package has only the rings it needs, and every dependency points inward. Shapes per package type: `docs/solutions/architecture/hexagonal-architecture-by-package-type.md`.

## Port

An interface that the inside of a package (its domain rules and use cases) declares for something it needs from the outside world, named for that need rather than for the technology behind it.

By rule, the package that declares a port owns it and ships its test double, and tests replace the outside world only at a port; the open exception is leetcode, whose fakes still ship from `@lib/flow-test-harness`. This replaced the Feature Triad, where a feature shipped as contracts, implementation and mocks sibling packages and workspace membership chose which sibling was live.

## Adapter

An implementation of a Port over real technology (a driven adapter: an HTTP client, a process runner, a file store), or an outside entry point that turns a call into a use-case call (a driving adapter: a router, a CLI handler, a screen).

Adapters depend on the inside of the package, never the reverse.

## Composition Root

The one place per entry point that builds the concrete Adapters and hands them to the inside; nothing else names a concrete adapter.

It builds the graph and the entry point runs it. In the app it is meant to be the only reader of test overrides, so a flow test that mounts the real route runs the real wiring with only the overridden ports replaced; today only leetcode's `di/LeetCodeContainer.tsx` is, and other features read overrides in screens, hooks and stores.

## Stale-Reference Sweep

The deliberate pass that accompanies deleting or renaming a package: repoint or remove every surviving reference in the same change as the deletion, rather than leaving them to be found later.

What makes it fail is scoping the search to source imports. A reference also hides in a dependency declaration, in a test that asserts a config value by name, in a committed build output, and in a waiver list — none of which are imports, all of which break or lie once the target is gone. The sweep's gate has to cover those shapes too, because a clean grep over `src/` is exactly what a missed consumer looks like.

## Waiver

A by-name exemption recorded in a convention rule's committed config, excluding a listed package from a check the rule would otherwise fail it on.

The list is itself a reference surface. When a waived package is deleted, its waiver goes stale, and any test asserting the waiver list by name fails — so waivers belong in the stale-reference sweep alongside imports and dependency declarations.
