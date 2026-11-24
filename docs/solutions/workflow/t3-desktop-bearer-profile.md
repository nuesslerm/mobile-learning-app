---
title: t3 serve as the bearer connection profile of a T3 desktop
---

A T3 desktop connects to a remote server by a bearer connection profile (`BearerConnectionProfile {connectionId, environmentId, label, httpBaseUrl,
wsBaseUrl}`, `packages/client-runtime/src/connection/catalog.ts`). t3 serve is such a server: its flow test `tests/flow/desktop_profile.rs` walks
the desktop's onboarding in the order of `onboarding.ts`.

## Steps

1. Start t3 serve with `--rca-url`, `--rca-workspace` and `T3_RCA_BEARER` in the environment (the bearer is minted on the rca service; until it is, the
   server answers without an executor: `docs/gaps/t3-rca-bearer-not-minted-on-the-deployed-service.md`).
2. Mint a one-time code: `wf t3 pair`.
3. The desktop fetches `GET /.well-known/t3/environment` and checks the protocol (`orchestrationProtocolVersion` must be 2). The descriptor carries
   no `serverSelfUpdate` capability, so a refusal never sets `serverUpdateRequired`.
4. It redeems the code at `POST /oauth/token`, asks `POST /api/auth/websocket-ticket/rpc` for a ticket and opens `/ws?orchestrationProtocol=2&wsTicket=`.
   `/health` answers without a credential and every route sends `access-control-allow-origin: *`.

## On the desktop

`setLocalEnvironmentEnabled(false)` runs the desktop on the remote environment alone. It is a desktop IPC setting, not a server one; windows, dialogs
and the updater stay in Electron.

## Not done

- An `https://` / `wss://` URL for the VM needs TLS at the VM (infra work); a desktop on a public network cannot use the plain URL.
- Proof without Electron (the pinned `apps/web` build served through a `T3_WEB_DIR` and driven with `wf browser`, mimic recording the methods the
  desktop calls on connect) is the serial agent's.
