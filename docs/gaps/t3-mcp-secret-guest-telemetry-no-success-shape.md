---
status: open
area: rca-parity
paths:
  - "clis/t3-serve/core/src/usecases/queries.rs"
  - "docs/plans/rca-parity/coverage/t3-decisions.json"
opened: 2026-11-20
last_verified: 2026-11-20
links:
  - "docs/plans/rca-parity/plan.md"
---
# T3 mcp-misc rows (O3) stay not offered: the recordings hold only the failure shape

## What is broken

Nothing is broken; these rows were asked for in #83 O3 and stay `not-offered`:

- `mcpApps.callTool`, `readResource`, `toolInfo`, `updateModelContext`
- `secrets.answerRequest`
- `guestShare.create`, `thread.guest-share.close`, `thread.guest-message.dismiss`
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
