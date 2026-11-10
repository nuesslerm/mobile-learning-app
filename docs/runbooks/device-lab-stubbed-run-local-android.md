---
title: device-lab stubbed Run Local on redroid (Android network stubs)
verifies: On redroid in Expo Go against the EAS Update CI published (`against: update`), a Lua script stubs the leetcode backend's run endpoint; Run Local on Two Sum shows the stub's 3 of 3 as Accepted, the stub server counts one hit, and the device's DNAT rules are gone when the run ends.
tier: agent-driven
maxAgeDays: 30
e2e: e2e/stubbed-run-local.lua
---

# device-lab stubbed Run Local on redroid

The Android half of the optional network stubs (`device:add_network_stub` in `wf android test`). The browser half is the same
script under `wf browser test`.

How it works: the first `device:add_network_stub` starts a small HTTP server inside the run (`clis/harness-android/src/stub_server.rs`, port picked
by the OS, listening on every interface of the runner's network namespace) and, as root on the device, inserts one iptables
DNAT rule per backend port (`WF_ANDROID_STUB_PORTS`, default 8787 for the agent and 8789 for the leetcode backend) towards the
device's default gateway, which is the runner's namespace. The EAS Update bakes the backends in as plain `http://<tailnet
IP>:<port>`, so the server reads each request as sent: a stubbed URL gets the canned answer and is counted, any other request
is forwarded to the host its `Host` header names. The rules carry the comment `wf-stub`; the run removes them at its end,
and a run that was killed leaves rules the next stubbing run clears first (a `wf android stop-device` drops them with the container).

Proved on the VM on 15 Oct (spike): with a rule for 10.123.45.67:8789 to 172.17.0.1, a request from the root shell and one
from an app uid (10050) both reached the server beside the runner.

## Steps

1. `wf android start-device --expo-go` (CI's android-e2e-tests job does this; never by hand).
2. `wf android test e2e/stubbed-run-local.lua --env base=exp://u.expo.dev/<projectId>/group/<group>/-- --publish` (CI's `wf test run` resolves
   the base from `latest-dev.json` and refuses an update that is not the commit's).
3. Expected: `3 / 3` and Accepted on screen, one stub hit, no JS warnings; the report's Network stubs section lists the
   route with its hit; `iptables -t nat -S OUTPUT` on the device shows no `wf-stub` rule afterwards.
