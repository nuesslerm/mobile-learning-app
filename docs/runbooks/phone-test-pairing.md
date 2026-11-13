---
title: phone-test-pairing - pair the owner's phone with t3-serve and orca-serve for the App Store phone test
verifies: The owner's phone, on the tailnet, pairs with the deployed t3-serve and orca-serve through one-time links minted inside their containers and can drive them with the App Store builds of T3 Code and Orca.
tier: runbook
---

# phone-test-pairing

Phase 8 of the plan issue (`#78`) is the owner's: the phone test against `t3-serve` and `orca-serve` with the **App Store** builds. This page is the pairing part. An agent wrote it from the code (`clis/t3-serve/src/verbs.rs`, `clis/orca-serve/src/verbs.rs`, the compose files and the registry entries); **no agent has run these steps on a phone**, so every step that happens inside the apps is marked as unverified.

A pairing link is a credential (a one-time code or a device token). Do not paste it into a chat, an issue or a log; open it on the phone and delete the message afterwards.

## Before you start

1. The phone is on the tailnet (`iphone-12-pro-max` is a device of the tailnet; check `wf vm net device list` shows it). Both servers are reachable only over the tailnet.
2. Both services run and answer: `wf vm ops probe t3-serve orca-serve` exits 0.
3. Their deploys passed the smoke test, which proves the rca bearers (`T3_RCA_BEARER`, `ORCA_RCA_BEARER`) are valid. If a recreate happened and no backup has restored `device-bearers.json`, re-mint them first (`docs/runbooks/vm-recreate.md`).

## Get a shell on the VM

`wf vm exec` reaches agent-shell only, not the other containers. Use the VM's SSH access (Termius, the deploy key or the break-glass key of `infra/oracle-cloud/AGENTS.md`) as `ubuntu@<vm public ip>`; the IP is `terraform output vm_public_ip` (`wf vm terraform exec infra/oracle-cloud output vm_public_ip`). Both commands below run on the VM's own Docker.

## T3 Code

```sh
docker exec t3-serve t3-serve pair --base-url http://t3-serve:8797 --label phone
```

It prints one JSON line. `url` is `http://t3-serve:8797/pair#token=<code>`; the code is one-time and expires after `ttlSeconds` (`--ttl <seconds>` changes it). `--scope` narrows what the device may do (the 17 scopes of `t3-serve pair --help`); without it the pairing gets the default set.

On the phone: open `url` (or paste the server address and the code into the T3 Code app's connect screen). *Unverified: how the App Store build takes the link.*

## Orca

```sh
docker exec orca-serve orca-serve pair --endpoint ws://orca-serve:6768
```

It prints one JSON line. `url` is `orca://pair?code=...`; it carries the host's public key, the tailnet address and the device's token, and the device is known to the running server at once. On the phone: open `url` in the Orca app. *Unverified: whether the App Store build registers the `orca://` scheme, takes a pasted link or reads a QR code (this runbook does not make one).*

## What to try on the phone

Connect, list the projects and chats, open one, send a message and watch it stream, answer an approval or a question request, cancel a turn, open a file, and look at the git view. Note what fails with the exact screen and time.

## Afterwards

- Findings go in `#79` (an entry under "needs the owner" or "an agent can fix this"); do not paste a link or code into it.
- Revoke a test device when done: for Orca remove its entry from `devices.json` on the `orca-state` volume; a T3 device that has paired stays in the store and no revoke verb is known to this page, so note it in #365 if it matters.
