---
status: open
area: rca-parity
paths:
  - "clis/t3-serve/core/src/usecases/queries.rs"
  - "docs/plans/rca-parity/coverage/t3-decisions.json"
  - "clis/rca-exec/src/adapters/mcp.rs"
  - "clis/rca-exec/src/adapters/sampling.rs"
opened: 2026-11-20
last_verified: 2026-11-21
links:
  - "docs/plans/rca-parity/plan.md"
---
# T3 mcp-misc rows (O3) stay not offered: the recordings hold only the failure shape

## What is broken

Nothing is broken; these rows were asked for in #83 O3 and stay `not-offered`:

- `mcpApps.callTool`, `readResource`, `toolInfo`, `updateModelContext`
- `secrets.answerRequest`
- `guestShare.create`, `thread.guest-share.close`, `thread.guest-message.dismiss` (served since 21 Nov, O3c: see the end of this file)
- `subscribeResourceTelemetry`

## What the recordings hold

The scenarios cassette (`t3-v0.1.9-scenarios`) drove each row against a thread with nothing behind it and got only the failure:
`McpAppRequestError {reason: "not-an-app"}` for the four MCP rows, `SecretRequestError {reason: "not_found"}` for the secret answer and
`GuestShareCreateError {reason: "disabled"}` for the share. No recording shows a tool result, an app resource, a secret request turn item,
a share link or a guest message. A success answer would be invented, which the plan forbids.

## Why each one waits

- MCP apps: the app resource, the tool's `_meta.ui` and a tool call result are not in any recording. A scripted stdio MCP server proves the
  transport, not the T3 wire shape. Needs a recording of a real MCP app (a server that ships a `ui://` resource) through the pinned T3.
- Secret request: rca's agents never ask for a secret, so the turn item that `answerRequest` answers is never produced. `answerRequest` keeps
  answering `not_found`, the recorded shape. No registry entry is added, because no secret value is introduced.
- Guest share: needs a public guest endpoint and a guest identity, and a recording of an enabled share. The recorded `disabled` answer is
  the only evidence, and it is the honest answer for rca today.
- Process sampling: the recorded stream samples the T3 install's own process tree (`server`, `provider-root`, `electron-main`). rca-exec has
  no sampling action, and mapping executor processes onto those categories would be a guess at what the phone's resource view shows. Owner
  decision 7 stands.

## Unblock

Record each success path against the pinned T3 (an MCP app with a UI resource, a thread with a secret-asking provider, a guest share enabled
by a setting, a resource monitor with a live process tree), then add the executor action and mapper row in the order action, contract
method, service group, mapper row.

## What exists below the mapper (21 Nov, O3b)

The three executor-side halves are built and tested; the T3 rows above stay `not-offered` because the wire shapes still have no recording.

- `secret_file_put` (exec-fs), `proc_sample` and `mcp_request` (exec-proc) in rca-exec, with fixtures and flow tests on real files and processes
  (`clis/rca-exec/tests/flow/mcp_secret_sampling.rs`). The scripted `sh` MCP servers are authored doubles, not recordings of a real MCP server.
- Agent API methods `file.putSecret`, `process.sample` and `mcp.request` in `files.edit`, relayed to the workspace's executor, with replayed fixtures.
  `mcp.request` is held to an executor's root workspace (the executor reads `.mcp.json` at its root). The secret's value is in no result, no
  error and no `Debug` text.
- Not built: the tool server's `request_secret` tool. It needs a thread event for a request whose answer carries no value, a lifecycle command
  that closes it, and the mapper's turn item; the value must not enter the event log, so it cannot reuse `input.answer`. It also has no recorded
  T3 turn item to map to (see "Why each one waits").
- Not built: MCP app items. `mcpApps.*` are keyed by `(threadId, itemId)` and restricted to the server of the producing tool call; rca's agent
  events carry no MCP server name for a tool call, and an app resource (`_meta.ui`) has never been produced by a real server through rca.

## Guest share (21 Nov, O3c)

Served, off by default. `guestShare.create` makes a link only when the owner's `guestSharingEnabled` setting is on (default off, which answers the
recorded `disabled`) and the host names its address in `T3_GUEST_HOST` (else `connect_unavailable`). A guest redeems the invite once for a name
(`POST /api/guest/redeem`, a session cookie that scripts cannot read) and writes text messages (`POST /api/guest/messages`, three waiting at most);
the owner dismisses a message and closes the link with the two commands `thread.guest-message.dismiss` and `thread.guest-share.close`. Credentials
are claims signed under the store's key and kept nowhere. Decision by the agent under the safer-option rule, reversible by turning the setting off.
Flow tests: `clis/t3-serve/tests/flow/guest.rs`; chain test: `clis/t3-serve/tests/integration/guest_share.rs`.
Not proven: the success shapes are authored from the pinned source (no recording shows an enabled share); not built: the guest's live stream, its
page and file uploads, and a public route for the guest page (the host must serve `https://<T3_GUEST_HOST>/guest`).
