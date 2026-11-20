--[[---
platform: android
against: update
timeout: 60
focus: true
features: leetcode
---]]
-- Run Local against a stubbed leetcode-backend (wire format of webBridgeClient.ts fetchRunLocal: {ok, results}): the stub's 3 of 3 (the live backend runs 12 cases) proves the canned
-- answer reached the screen, and no request left the device (Android: the stub server behind the DNAT rule answered it).
-- wf browser test e2e/stubbed-run-local.lua --env base=http://127.0.0.1:8800/mla-tamagui
-- wf android test e2e/stubbed-run-local.lua --env base=exp://u.expo.dev/<projectId>/group/<group>/--
local android = device.platform == "android"
local base = env.base or (not android and "http://127.0.0.1:8800/mla-tamagui" or nil)
assert_true(base, "pass --env base=<update link>; wf test run does")
local function stub_run() device:add_network_stub("POST", "*/api/leetcode/run", { body_file = "e2e/stubs/leetcode-run-two-sum.json", headers = { ["content-type"] = "application/json" } }) end
stub_run()
if android then
  -- is the path emulator -> DNAT rule -> stub alive at all, before the app is involved? One bare connect from the device shell: the stub logs the connection it accepts
  log("stub probe", (device:run_shell("echo | nc -w 3 10.0.2.2 8789; echo rc=$?")))
end
-- Run Local runs on the device by default (issue 86, B5); this test is about the server path, so the server is chosen on the problem list,
-- and the default is put back at the end: later tests on this device expect it
local function choose(id)
  for _ = 1, 3 do
    device:open_link(base .. "/")
    if device:extended_wait_until{visible={id='LeetcodeListRunnerRow'}, timeout=15000, optional=true} then break end
  end
  device:tap_on({id=id})
end
run_flow("choose the server", function() choose('LeetcodeListRunnerBackend') end)
-- a reused app may still show this screen: a link to it remounts it a moment later, so the first wait would pass on the
-- old one (18 Oct); the app leaves for Settings first (a link to "/" does not navigate a running Expo Go)
if android and device:get_element({id='ProblemEditorRunLocalButton'}) then
  device:open_link(base .. "/settings")
  device:assert_not_visible({id='ProblemEditorRunLocalButton'})
end
run_flow("open route", function()
  device:open_link(base .. "/leetcode/two-sum")
  device:assert_visible({id='ProblemEditorRunLocalButton'}, {timeout=60000})
end)
-- the web export's first load hydrates a cold deep link with React #419 (docs/gaps/web-export-cold-deep-link-hydration-418.md); the stubbed interaction itself must stay clean
device:clear_logs()
device:assert_screenshot("two-sum-before-run")
-- the editor may open focused with the keyboard over the action row; Done at the end of the toolbar lowers it (on Android
-- the editor often opens unfocused, with no keyboard and no toolbar to lower)
if not (android and not device:get_device_state('keyboard').visible) then
  device:scroll_until_visible{element={id='EditorAction-Done'}, direction='RIGHT', from={id='EditorActionToolbar'}}
  device:tap_on({id='EditorAction-Done'})
  device:assert_not_visible({id='EditorHelperRails'})
end
-- a run leaves the code as it was: the screen scrolls down to the result card, which puts the first lines of the editor behind the header
local code_before = (device:get_element({id='CodeEditorInput'}) or {}).text
assert_true(code_before and code_before:find("def twoSum", 1, true), "the editor does not hold the starter before the run: " .. tostring(code_before))
-- a reused app may still show an earlier run's outcome on this screen, so the stubbed one is waited for, not the first
local shown
run_flow("run local", function()
  -- declaring the stub again asserts the device's redirect rules again: the device can lose them between the start of the run and this tap
  stub_run()
  device:tap_on({id='ProblemEditorRunLocalButton'})
  shown = device:extended_wait_until{visible={id='ProblemEditorRunOutcomeTotals', text='.*3 / 3.*'}, timeout=12000, optional=true}
  if android and not shown then
    -- on the hosted runner the first request of a reused app sometimes sits unacknowledged on a connection the DNAT rule never saw (the device's socket
    -- table shows bytes queued to port 8789 and no answer): what the stub saw and the NAT rules are logged, and one more tap opens a fresh connection
    log("first run: stub hits", #device:get_network_stub_hits())
    log("nat rules", (device:run_shell("iptables -t nat -L OUTPUT -v -n -x; iptables -L OUTPUT -v -n -x")))
    -- a fresh app process opens fresh sockets, all of them behind the rules
    device:run_shell("am force-stop host.exp.exponent")
    stub_run()
    device:open_link(base .. "/leetcode/two-sum")
    device:assert_visible({id='ProblemEditorRunLocalButton'}, {timeout=60000})
    device:tap_on({id='ProblemEditorRunLocalButton'})
    shown = device:extended_wait_until{visible={id='ProblemEditorRunOutcomeTotals', text='.*3 / 3.*'}, timeout=30000, optional=true}
  end
end)
local ok, failure = pcall(function()
  local now = device:get_element({id='ProblemEditorRunOutcomeTotals'})
  local totals = (shown and shown.text) or (now and now.text) or ""
  log("totals", totals)
  if android and not shown then
    -- where the request went: the app's own log of the run and the device's open connections (8789 = 0x2255)
    local rn = device:get_logs{level='debug'}
    log("rn log", table.concat(rn, "\n", math.max(1, #rn - 29)))
    log("tcp", (device:run_shell("cat /proc/net/tcp /proc/net/tcp6 | grep -iE ':2255|:2253' | head -20")))
  end
  assert_true(shown, "the stubbed 3 of 3 is not on screen: " .. totals)
  assert_true((device:get_element({id='CodeEditorInput'}) or {}).text == code_before, "the run changed the code in the editor")
  device:assert_visible({text='.*Accepted.*'}, {timeout=5000, label='stubbed run shown as Accepted'})
  assert_true(not device:extended_wait_until{visible={text='.*Unknown error.*'}, timeout=1000, optional=true}, "the app could not read the stubbed answer")
  device:assert_screenshot("two-sum-stubbed-result")
  local hits = device:get_network_stub_hits()
  assert_true(#hits == 1, "expected one stubbed request, got " .. #hits)
  log(json.encode(hits[1]))
  local w = device:get_logs()
  assert_true(#w == 0, "JS warnings after the load: " .. table.concat(w, " | "))
  -- BetterStack holds the run's result line for the session: a run that reached the screen must have logged it
  local info = table.concat(device:get_logs{level='info'}, "\n")
  assert_true(info:find("editor_v2.run_local.result", 1, true), "no run_local.result line in the app log")
end)
pcall(function() choose('LeetcodeListRunnerDevice') end)
if not ok then error(failure, 0) end
