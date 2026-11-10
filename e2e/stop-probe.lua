--[[---
platform: android
against: update
timeout: 60
features: *
---]]
-- Stop probe: a run the queue's timeout stops must still end with its report and recording.
-- wf android test e2e/stop-probe.lua --env base=exp://u.expo.dev/<projectId>/group/<group>/--
-- Waits for an element that never appears, far past the runbook's timeout, so the worker's SIGTERM ends it.
local base = env.base
assert_true(base, "pass --env base=<update link>; wf test run does")
device:open_link(base .. "/settings")
device:assert_visible({id='StopProbeNeverRendered'}, {timeout=900000})
