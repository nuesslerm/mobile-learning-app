# T3 rows marked not-offered: owner decisions

Every row is `not-offered` in `t3-decisions.json` (decidedBy M12), the **proposed default awaiting owner confirmation** (plan section 7, decision 1). Confirm a row to keep it out, or reject it and it returns to `pending` with its milestone. Reasons are copied from `t3-decisions.json`; none of these can be built and proved on `rca-exec` in this pass. The orca side is not covered here.

## Owner candidates in decision 1

### Device

What would make it offered: an emulator the executor may run outside CI (`device_*` executor actions; gap `docs/gaps/preview-device-browser-actions-not-built.md`). decidedBy M12; proposed default awaiting owner confirmation.

| Row | Reason |
| --- | --- |
| `device.action` | the device rows drive an emulator, a simulator or a physical device (open and close it, its detail and actions, the EAS account and SIM-lock identity it is set up with): that needs a real emulator, which rca-exec does not run (the emulator runs only in CI) and the chain cannot prove, and the executor contract has no device_* action. Proposed default awaiting owner confirmation (plan decision 1 lists device as a T3 candidate). |
| `device.close` | the device rows drive an emulator, a simulator or a physical device (open and close it, its detail and actions, the EAS account and SIM-lock identity it is set up with): that needs a real emulator, which rca-exec does not run (the emulator runs only in CI) and the chain cannot prove, and the executor contract has no device_* action. Proposed default awaiting owner confirmation (plan decision 1 lists device as a T3 candidate). |
| `device.configure` | the device rows drive an emulator, a simulator or a physical device (open and close it, its detail and actions, the EAS account and SIM-lock identity it is set up with): that needs a real emulator, which rca-exec does not run (the emulator runs only in CI) and the chain cannot prove, and the executor contract has no device_* action. Proposed default awaiting owner confirmation (plan decision 1 lists device as a T3 candidate). |
| `device.detail` | the device rows drive an emulator, a simulator or a physical device (open and close it, its detail and actions, the EAS account and SIM-lock identity it is set up with): that needs a real emulator, which rca-exec does not run (the emulator runs only in CI) and the chain cannot prove, and the executor contract has no device_* action. Proposed default awaiting owner confirmation (plan decision 1 lists device as a T3 candidate). |
| `device.list` | the device rows drive an emulator, a simulator or a physical device (open and close it, its detail and actions, the EAS account and SIM-lock identity it is set up with): that needs a real emulator, which rca-exec does not run (the emulator runs only in CI) and the chain cannot prove, and the executor contract has no device_* action. Proposed default awaiting owner confirmation (plan decision 1 lists device as a T3 candidate). |
| `device.open` | the device rows drive an emulator, a simulator or a physical device (open and close it, its detail and actions, the EAS account and SIM-lock identity it is set up with): that needs a real emulator, which rca-exec does not run (the emulator runs only in CI) and the chain cannot prove, and the executor contract has no device_* action. Proposed default awaiting owner confirmation (plan decision 1 lists device as a T3 candidate). |
| `device.shutdown` | the device rows drive an emulator, a simulator or a physical device (open and close it, its detail and actions, the EAS account and SIM-lock identity it is set up with): that needs a real emulator, which rca-exec does not run (the emulator runs only in CI) and the chain cannot prove, and the executor contract has no device_* action. Proposed default awaiting owner confirmation (plan decision 1 lists device as a T3 candidate). |
| `subscribeDeviceState` | the device rows drive an emulator, a simulator or a physical device (open and close it, its detail and actions, the EAS account and SIM-lock identity it is set up with): that needs a real emulator, which rca-exec does not run (the emulator runs only in CI) and the chain cannot prove, and the executor contract has no device_* action. Proposed default awaiting owner confirmation (plan decision 1 lists device as a T3 candidate). |

### MCP apps

What would make it offered: the mapper hosting the agent's MCP servers, or an executor action that relays a tool's UI resource. decidedBy M12; proposed default awaiting owner confirmation.

| Row | Reason |
| --- | --- |
| `mcpApps.callTool` | MCP apps render a tool's UI resource inside the T3 desktop webview and call a server the T3 process hosts; rca runs the agent's MCP servers inside the CLI, not in the mapper, so there is nothing to call or read. Proposed default awaiting owner confirmation. |
| `mcpApps.readResource` | MCP apps render a tool's UI resource inside the T3 desktop webview and call a server the T3 process hosts; rca runs the agent's MCP servers inside the CLI, not in the mapper, so there is nothing to call or read. Proposed default awaiting owner confirmation. |
| `mcpApps.toolInfo` | MCP apps render a tool's UI resource inside the T3 desktop webview and call a server the T3 process hosts; rca runs the agent's MCP servers inside the CLI, not in the mapper, so there is nothing to call or read. Proposed default awaiting owner confirmation. |
| `mcpApps.updateModelContext` | MCP apps render a tool's UI resource inside the T3 desktop webview and call a server the T3 process hosts; rca runs the agent's MCP servers inside the CLI, not in the mapper, so there is nothing to call or read. Proposed default awaiting owner confirmation. |

### Desktop and server updates

What would make it offered: an update path for the service image that is not CI (owner decision). decidedBy M12; proposed default awaiting owner confirmation.

| Row | Reason |
| --- | --- |
| `server.commitDesktopUpdate` | the desktop and server self-update replace the T3 binary; the service's image is updated by shipping an image through CI, and the phone has no desktop updater. Proposed default awaiting owner confirmation. |
| `server.updateServer` | the desktop and server self-update replace the T3 binary; the service's image is updated by shipping an image through CI, and the phone has no desktop updater. Proposed default awaiting owner confirmation. |
| `server.updateServerWithProgress` | the desktop and server self-update replace the T3 binary; the service's image is updated by shipping an image through CI, and the phone has no desktop updater. Proposed default awaiting owner confirmation. |

### Provider feedback upload

What would make it offered: the owner approving a destination for thread data. decidedBy M12; proposed default awaiting owner confirmation.

| Row | Reason |
| --- | --- |
| `provider.uploadFeedback` | uploading provider feedback sends a transcript to the T3 vendor's endpoint; the owner has not approved sending any thread data off the service. Proposed default awaiting owner confirmation. |

## Rows that depend on a desktop, an account or the T3 install

### Guest sharing, Connect and peer links

What would make it offered: a guest endpoint, a signing key or a person's credential on the service. decidedBy M12; proposed default awaiting owner confirmation.

| Row | Reason |
| --- | --- |
| `thread.guest-message.dismiss` | guest sharing needs a public guest endpoint and a guest identity on the service; rca has no guest concept and no share can exist through the chain, so the dismiss and close commands could only pass on an empty set. Proposed default awaiting owner confirmation. |
| `thread.guest-share.close` | guest sharing needs a public guest endpoint and a guest identity on the service; rca has no guest concept and no share can exist through the chain, so the dismiss and close commands could only pass on an empty set. Proposed default awaiting owner confirmation. |
| `connect.linkProof` | T3 Connect and relay need an environment signing key and a hosted relay account (a person's credential); the service holds none and the mapper never mints one. Proposed default awaiting owner confirmation. |
| `connect.relayConfig` | T3 Connect and relay need an environment signing key and a hosted relay account (a person's credential); the service holds none and the mapper never mints one. Proposed default awaiting owner confirmation. |
| `peerLinks.answerRequest` | peer links sign this server in at another environment with a person's credential; rca holds no peer link and the mapper makes none. Proposed default awaiting owner confirmation. |
| `peerLinks.link` | peer links sign this server in at another environment with a person's credential; rca holds no peer link and the mapper makes none. Proposed default awaiting owner confirmation. |
| `peerLinks.list` | peer links sign this server in at another environment with a person's credential; rca holds no peer link and the mapper makes none. Proposed default awaiting owner confirmation. |

### Provider accounts, usage and updates

What would make it offered: a credential path that never crosses the mapper, a usage source in the model API, or a provider update RPC. decidedBy M12; proposed default awaiting owner confirmation.

| Row | Reason |
| --- | --- |
| `provider.chatgpt.import-profile` | a ChatGPT profile is a credential: importing or reconnecting one would pass tokens through the mapper, which never holds one, and the Codex CLI behind rca keeps its own login. Proposed default awaiting owner confirmation. |
| `provider.chatgpt.reconnect-profile` | a ChatGPT profile is a credential: importing or reconnecting one would pass tokens through the mapper, which never holds one, and the Codex CLI behind rca keeps its own login. Proposed default awaiting owner confirmation. |
| `provider.consumeResetCredit` | the service's model API reports no reset credit and rca keeps no price table; the rows could only pass by refusing. Proposed default awaiting owner confirmation. |
| `server.refreshUsageRates` | the service's model API reports no reset credit and rca keeps no price table; the rows could only pass by refusing. Proposed default awaiting owner confirmation. |
| `server.updateProvider` | the provider CLIs ship inside the service's image, so a version is updated by shipping an image, not by an RPC; the row could only pass by refusing. Proposed default awaiting owner confirmation. |

### T3 install itself (triage, telemetry)

What would make it offered: a T3 home to diagnose; for telemetry a `host_resources` executor action plus a process source. decidedBy M12; proposed default awaiting owner confirmation.

| Row | Reason |
| --- | --- |
| `server.prepareTriage` | it starts an in-app triage of the T3 Code install itself: the server writes the session files `t3 triage` writes (its own logs, config and version under T3 home) and clones the T3 Code source from github.com/pingdotgg/t3code into its cache, then hands the phone a prompt for a scratch thread. t3-serve is not that server (it has no T3 home, logs or release to diagnose) and rca-exec has nothing of it to read; the feature would be a network clone of a third party's repository with no state of ours to triage, so there is nothing to build and prove on the executor. The phone's own fallback is the `npx t3 triage` command it offers to copy. Proposed default awaiting owner confirmation. |
| `subscribeResourceTelemetry` | the stream is a process monitor of the T3 install itself: samples of the T3 server, its provider and terminal roots and the Electron processes (categories `server`, `provider-root`, `electron-main`, ...) taken by a protocol-3 resource-monitor sidecar, plus update state and host power. t3-serve has no such process tree and rca-exec has no process-sampling action; the one honest part, the executor host's CPU and memory, would ship with an empty process list that the phone's resource view would present as a healthy idle server. Proposed default awaiting owner confirmation. |

### Nothing to act on in rca

What would make it offered: rca asking for a secret, preparing a run, a read of the Claude projects directory, or a pull request cache with a push source. decidedBy M12; proposed default awaiting owner confirmation.

| Row | Reason |
| --- | --- |
| `secrets.answerRequest` | rca never asks for a secret, so there is no request to answer; the call could only get not_found. Proposed default awaiting owner confirmation. |
| `prepared-run.retry` | rca prepares no workspace for a run, so there is no failed preparation to retry; the call could only get the refusal. Proposed default awaiting owner confirmation. |
| `orchestration.getWorkflowScript` | rca starts no Claude workflow scripts of its own, so a thread has none to show; an owner who wants it gets an executor action that reads one file by absolute path under the Claude projects root, with .. and links refused and 1 MiB truncation, outside the workspace confinement. Proposed default awaiting owner confirmation. |
| `pullRequests.invalidate` | t3 serve keeps no pull request cache and no push source (the executor's CLIs are polled, not pushed), so invalidate, reportState and the refresh stream have nothing to act on. Proposed default awaiting owner confirmation. |
| `pullRequests.reportState` | t3 serve keeps no pull request cache and no push source (the executor's CLIs are polled, not pushed), so invalidate, reportState and the refresh stream have nothing to act on. Proposed default awaiting owner confirmation. |
| `pullRequests.subscribeRefreshes` | t3 serve keeps no pull request cache and no push source (the executor's CLIs are polled, not pushed), so invalidate, reportState and the refresh stream have nothing to act on. Proposed default awaiting owner confirmation. |

`preview.*` and `subscribePreviewEvents` are served (the service keeps preview tabs and the phone draws them); only `preview.open` with `runtime: "server"` is refused by name.
