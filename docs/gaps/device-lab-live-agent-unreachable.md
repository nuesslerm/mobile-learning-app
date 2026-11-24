---
status: blocked
area: device-lab
paths:
  - "frontend/mla-tamagui/lib/device-lab/**"
  - "frontend/mla-tamagui/features/tamil-writing/data/httpWritingCheck.ts"
  - "frontend/mla-tamagui/features/leetcode/data/voiceEditTransport.ts"
  - "docs/plans/device-lab/**"
  - "backend/remote-chat-agent/**"
  - "frontend/mla-tamagui/eas.json"
  - "clis/harness-browser/src/tailnet_forward.rs"
  - "docs/runbooks/device-lab-tamil-writing-android.md"
  - "docs/runbooks/device-lab-voice-edit.md"
  - "docs/runbooks/device-lab-tamil-writing.md"
opened: 2026-10-05
last_verified: 2026-10-05
links:
  - gaps/vm-bootstrap-bws-token-revoked.md
---
# device-lab: the live agent is unreachable from the sandbox

As of 2026-10-19 the agent is back on the tailnet and G1 and G2 have passed against it; what still fails is narrower and listed under "Re-checked 2026-10-19".

## What is broken

Gate G1 (voice edit) and the marker steps of G2 need the deployed remote-chat-agent. From the cloud sandbox:
- `remote-chat-agent-prod` is missing from `wf vm net device list` and from `tailscale status` (10 peers, none of them the agent).
- A SOCKS connect to it fails with `curl: (97)`.
- `wf browser tailnet-forward` to it gives an empty reply.
- The Tamil write pad answers "Could not reach the marker".

`leetcode-backend` answers only under its collision-suffixed name, `leetcode-backend-prod-2`.

Workaround in use: the same agent runs from the checkout in the sandbox (opencode serve with the vault's
provider keys, `remote-chat-agent` from source on 8787, where the export points), as the G2 runbook's setup
describes. G2 passed that way on 2026-10-05. It stands in for the code, not for the deployment: a check
against the deployed agent waits on this gap.

## Why blocked

The app's tailscale sidecar is offline. The likely root cause is the revoked bootstrap Bitwarden token (`vm-bootstrap-bws-token-revoked.md`).

## What unblocks it

1. Resolve that gap, which brings `remote-chat-agent-prod` back on the tailnet during the greenfield cutover's full redeploy.
2. Retire the stale `leetcode-backend-prod` record with `wf vm net device retire --apply`.
3. Re-run G1 and G2 against the deployed agent.

## Re-checked 2026-10-19

The tailnet half is fixed: `tailscale status` lists `remote-chat-agent-prod` at 100.92.149.49 and `leetcode-backend-prod` under its plain name, and G1 and G2 passed against the deployed agent on 2026-10-15 (`docs/runbooks/.results/device-lab-voice-edit.json`, `docs/runbooks/.results/device-lab-tamil-writing.json`).
The shared dev Metro found the agent by name and fell back to the `-1`..`-5` names a stale record forces (the vm-ops-api dev-bridge script and tailnet resolver, both deleted on 22 Oct with the shared dev Metro).
At 19:50 UTC `/health` and a real `POST /api/tamil/check` both answered 200 over SOCKS from the sandbox, the marker in 4 s.

What still fails:
The Android lab APK bakes dead addresses: `frontend/mla-tamagui/eas.json:24-27` (`android-apk-lab`) points the agent at 100.91.40.14 and leetcode-backend and vm-ops-api at 100.70.25.11 and 100.90.100.94, all "peer not found" in `tailscale whois`, and `docs/runbooks/device-lab-tamil-writing-android.md:22,24` still builds and probes that agent IP.
`/health` answers ok without touching the opencode sidecar that the marker and the planner call at `localhost:4096` (`backend/remote-chat-agent/rs/src/api/routes/mod.rs:137-138`, `backend/remote-chat-agent/opencode/src/http.rs:85`), so from 19:18 to 19:28 UTC the judges got 502 from `/api/tamil/check` and 500 from `/api/voice-plan` ("fetch failed ... localhost:4096") behind a green `/health`, which the pad shows as "Could not reach the marker" (`frontend/mla-tamagui/features/tamil-writing/ui/TamilWriteScreen.tsx:75-84`).
No deployment verification test calls either endpoint: `backend/remote-chat-agent/tests/e2e/` covers only health, chat and convert, and `backend/remote-chat-agent/rs/tests/deployed_verification.rs:7` probes `/health`; only the focused browser/Android e2e tests `e2e/voice-edit.lua` and `e2e/tamil-writing.lua` reach them, through the app (a web export for the browser, the EAS Update CI published for Android; neither is the dev Metro).
In the same window a CI run overloaded the VM (`vm-memory-saturation.md`) and SOCKS connects failed again with `curl: (97)`, while `wf browser tailnet-forward` still drops a failed dial without a word (`clis/harness-browser/src/tailnet_forward.rs:30`), so the client sees an empty reply.
`docs/runbooks/device-lab-voice-edit.md:35` and `docs/runbooks/device-lab-tamil-writing.md:35` still say the agent is off the tailnet.
