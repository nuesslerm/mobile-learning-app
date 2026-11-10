---
status: blocked
area: secrets
paths:
  - "clis/secrets/src/data/registry.json"
opened: 2026-10-04
last_verified: 2026-10-04
links: []
---
# Secret-registry entries still describe the deleted mobile-learning-app

## What is broken

As of 2026-10-04 the `WHAT:`/`WHERE:` prose in `clis/secrets/src/data/registry.json` pointed at
three files deleted with the mobile-learning-app (13 `mobile-learning-app` hits in the registry):
`CH_MOBILE_*`, `EXPO_PUBLIC_BETTERSTACK_TOKEN` and `BETTERSTACK_INGEST_HOST` named its REST log
forwarder (`lib/logging/remote.ts`), `CH_BACKEND_*` and `CH_MOBILE_*` named its `fetch-logs.sh`, and
`GITHUB_TOKEN` named its `deploy-gh-pages.sh`; `EXPO_PUBLIC_BETTERSTACK_OTLP_TOKEN` named its
`lib/logging` OTel bridge. Those paths have since been taken out of the registry. The `fetch-logs.sh`
mention was dropped outright, as was `EXPO_PUBLIC_BETTERSTACK_OTLP_TOKEN`'s plan citation (the
dangling text both first left was repaired on 2026-10-19). The other paths were reworded
only into descriptions of the same deleted consumers, so none of those entries is fixed.

`CH_BACKEND_*` no longer name anything from the removed app. What is still stale in them is the OTel bridge they give as the source's writer,
`frontend/libraries/logging/src/node`, a path that does not exist (the `BETTERSTACK_OTLP_*` entries
cite a file under it).

`EXPO_PUBLIC_BETTERSTACK_TOKEN` and `BETTERSTACK_INGEST_HOST` still name the removed app's REST
forwarder as their consumer, and nothing replaced it (`frontend/mla-tamagui/lib/logger.ts` has no
BetterStack forwarder). `EXPO_PUBLIC_BETTERSTACK_TOKEN` is still a key of six deploy groups in
`clis/secrets/src/secrets/groups.json` (leetcode-backend, remote-chat-agent, leetcode-auth-cookie-capture,
vm-gateway, vnc-auth-session, agent-shell) and is passed, with `EXPO_PUBLIC_BETTERSTACK_HOST`, into
three compose files (`backend/leetcode-backend/docker-compose.prod.yml`,
`backend/remote-chat-agent/docker-compose.prod.yml`,
`backend/leetcode-auth-cookie-capture/docker-compose.capture.yml`), although nothing reads it. `CH_MOBILE_*` still have a live reader (`wf vm logs --source mobile`,
`clis/deploy/src/vm_logs.rs`), but they describe the BetterStack source "Mobile Learning App", whose
only writer was that forwarder, so the source now receives nothing.
`EXPO_PUBLIC_BETTERSTACK_OTLP_TOKEN` is still described as read by the removed app's logging OTel
bridge once U12 ships, and no code or deploy group outside the registry reads it (only archived
docs mention it). `GITHUB_TOKEN` still justifies Pages=Read+Write with the removed app's
`deploy:web:pages` GitHub Pages deploy, a script no `package.json` defines any more, although the
key itself is live: remote-chat-agent reads it (`backend/remote-chat-agent/group-workspace/src/worktrees.rs`).
`EXPO_TOKEN` still cites `pnpm --filter mobile-learning-app run publish:eas` and the deleted
eas-simulator skill, but its consumer
survives as `pnpm --filter mla-tamagui run publish:eas`, which `wf ship eas`
(`clis/deploy/src/ship_eas.rs`) runs. The prose is inlined into the `wf` binary
(`include_str!` in `clis/secrets/src/secrets/registry.rs`), so a rebuild keeps it.

## Why blocked

The values still exist in Bitwarden; removing or rewording entries for a consumer that no longer
exists is a secrets-lifecycle decision that risks orphaning live vault values.

## What unblocks it

Decide whether those secrets are still wanted: drop the vault values and registry entries together,
or keep the keys and reword the descriptions to name their surviving consumer. `CH_MOBILE_*` already
have one, so for them the question is whether anything should write to the "Mobile Learning App"
source again; if nothing will, they go with the source.

`EXPO_TOKEN` and `GITHUB_TOKEN` are live keys, so neither faces a drop-or-keep decision.
`EXPO_TOKEN` needs `mobile-learning-app` reworded to `mla-tamagui` and the eas-simulator skill
replaced by `wf ship eas`, which touch no vault value and are not blocked. For `GITHUB_TOKEN` the open question is whether to drop Pages=Read+Write
from the PAT or re-justify it with a surviving consumer. The `CH_BACKEND_*` OTel bridge path is likewise a reword of entries whose reader is live. Dropping
`EXPO_PUBLIC_BETTERSTACK_TOKEN` also means taking it out of those six groups and three compose files.
