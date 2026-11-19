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
directories of the executor, with `updatedAt` null and a source's `exists` meaning it holds skills. `detectRemoteAgents` answers for an executor id where the pinned host refuses every ssh connection (`SshHostNotServedError`); that is an intended extension, as an executor is the machine here, and an unknown id gets the host's "Remote connection dropped" text. The rows are served with chain evidence in `clis/orca-serve/tests/integration/agent_catalog.rs`.
`preflight.check` is served (issue #83, O1): git, gh and glab come from the executor's `cli_detect` (through `agents.detect`'s `tools`), and gh's and
glab's sign-in from `sourceControl.discover` at the root workspace's host, asked only for an installed CLI. Bitbucket, Azure DevOps and Gitea are the
host's own answer for a machine where none is configured, since rca keeps no credentials for them. Known differences: the host also counts gh as signed
in when `auth status` exits 1 but prints `Active account: true`, and it tries more than one copy of a program on the path.

## Speech models list (19 Nov, issue #83 O1)

The recorded host lists one model, the cloud `openai-gpt-transcribe` row, because its local recognizer is not installed in the recording; `speech.models.list` and `speech.dictation.setup` here list rca's own engine (`rca-whisper`, ready) when the service has one. The five fields of the first model differ in every replay of those rows (`known-gaps.json` of both Orca sets). Dictation needs the engine and the person's setting: `start` answers `voice_dictation_disabled` as the host does until `setup` enables it. The cloud rows (`speech.openAiKey.*`) stay not offered.

## Issue rows (19 Nov, issue #83 O2)

`github.createIssue`, `github.updateIssue`, `github.addIssueComment` and the three `gitlab.*` twins are served over rca's `issue.*` methods (`issue.create`, `issue.setState`, `issue.edit`, `issue.comment`). rca sets no assignees when it opens an issue and changes neither the labels nor the assignees of one, so a request that asks for those is the host's `{ok: false}` with that sentence, answered before anything is changed (`labels` on a new issue are sent, to GitHub as `--label` and to GitLab as `labels`). The host's `addIssueComment` answers the new comment from the API's own reply; rca's `issue.comment` answers the address on GitHub and the note on GitLab, so the GitHub comment's id is read from the address (`#issuecomment-<id>`), its author is `You` and its time is the request's. An `updateIssue` that changes nothing asks rca for one issue list to resolve the repository, as the host resolves it before it sees there is nothing to do. The recorded host's answers hold only the paths where the repository is not found or not resolved; the success shapes are written from the host's source.
