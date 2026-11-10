---
status: open
area: rca-parity
paths:
  - "clis/orca-serve/map-agent/src/**"
opened: 2026-11-05
last_verified: 2026-11-05
links:
  - "docs/plans/rca-parity/plan.md"
---
# The Orca agent mapper serves a narrower reply than the Orca host in five places

## What is broken

`orca-map-agent` answers the served M7 rows from the rca agent API, and five replies are narrower than the pinned host's: `agent.launch` and
`agentSession.create` accept only an existing worktree as the target (`create-worktree` and `create-folder-workspace` are refused); a send
names images only as attachments uploaded to this host (an image url is refused); an empty upload is refused; every journal item carries
`observedAt` 0 because rca's events have no clock; and an upload belongs to the connection that started it, so a phone that reconnects
mid-upload must start again. The unknown-chat refusal text (`agent_session_not_found`) is a guess at the host's wording.

## Why blocked

rca has no event clock, no create-and-run launch and no image-by-url concept, and the orcad oracle (VM and CI emulator) has not been run to
settle the host's wording or its reconnect behaviour.

## What unblocks it

An orcad oracle run that replays create, launch, a send with an image and an upload across a reconnect against a real Orca host, then the
mapper changes the differences name. The two rows still pending in M7 (`agentSession.subscribeStatus`, `nativeChat.subscribe`, the subscribe streams) are tracked in the plan's M7 entry.

## The catalog rows (M7b)

`orca-map-agent` answers `agentSession.options/modelCatalog/commands/createSupport/setOption`, `preflight.detectAgents/refreshAgents/detectRemoteAgents`
and `skills.discover` over `agent.catalog`, narrower than the host: the models are the service's table (no live listing), so `modelCatalog`
answers `origin: "probe"` at once and `waitForListing` and `savedOnly` change nothing; `setOption` takes the `model` key only (a turn takes no
effort or permission mode of the phone's); detection asks the only attached executor (workspaces carry no executor name), a project detection
(`project`) is refused, and `refreshAgents` hydrates no shell PATH (`shellHydrationOk: false`); `skills.discover` reads the three skills
directories of the executor, with `updatedAt` null and a source's `exists` meaning it holds skills. `detectRemoteAgents` answers for an executor id where the pinned host refuses every ssh connection (`SshHostNotServedError`); that is an intended extension, as an executor is the machine here, and an unknown id gets the host's "Remote connection dropped" text. The rows are served with chain evidence in `clis/orca-serve/tests/integration/agent_catalog_m7b.rs`.
`preflight.check` is not offered (decisions file), a deferred choice and not an impossibility. Alternative: extend `cli_detect` to git, gh and glab (the same
no-shell version probe) and answer the git, gh and glab fields from it; Bitbucket and Azure DevOps and every signed-in state would still need new probes,
so the phone's source-control onboarding keeps showing them as unavailable without it. Until the owner confirms, the proposed default stands. The orcad oracle (VM and CI
emulator, not run) would compare these reply field sets with a real Orca host's.
