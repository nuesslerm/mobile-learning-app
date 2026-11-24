--[[---
platform: browser
against: browser=export
timeout: 60
features: leetcode
---]]
-- Submit on the console against a stubbed leetcode-backend (wire format of webBridgeClient.ts fetchRunLocal: {ok, results}): the stub's 3 of 3 (the live backend
-- runs 12 cases) proves the canned answer reached the Run Result tab, and no request left the device. Browser only: the Android unit is not registered.
-- wf browser test e2e/stubbed-run-local.lua --env base=http://127.0.0.1:8800/mla-tamagui
local base = env.base or "http://127.0.0.1:8800/mla-tamagui"
local function stub_run() device:add_network_stub("POST", "*/api/leetcode/run", { body_file = "e2e/stubs/leetcode-run-two-sum.json", headers = { ["content-type"] = "application/json" } }) end
stub_run()

local function logged(needle)
  for _, l in ipairs(device:get_logs{level='info'}) do
    if l:find(needle, 1, true) then return true end
  end
  return false
end

-- Run on the device by default (issue 86, B5); this test is about the server path, so the server is chosen on the problem list,
-- and the default is put back at the end: later tests expect it. The static page paints its buttons before its scripts have hydrated them, and a tap
-- before that is lost: it is repeated until the screen logs it
local function tap_choice(id, event)
  device:open_link(base .. "/")
  device:assert_visible({id=id}, {timeout=60000})
  device:clear_logs()
  for _ = 1, 10 do
    device:scroll_until_visible{element={id=id}, direction='DOWN', timeout=30000}
    device:tap_on({id=id})
    if device:extended_wait_until{condition=function() return logged(event) end, timeout=2000, interval=250, optional=true} then return end
  end
  error("the choice " .. id .. " never reached the screen")
end

run_flow("choose the server", function()
  tap_choice('LeetcodeListRunnerBackend', 'leetcode.list.runner_tap')
end)
local ok, failure = pcall(function()
  run_flow("open route", function()
    device:open_link(base .. "/leetcode/two-sum")
    device:assert_visible({id='ProblemEditorScreen'}, {timeout=60000})
  end)
  -- the web export's first load hydrates a cold deep link with React #419 (docs/gaps/web-export-cold-deep-link-hydration-418.md); the stubbed interaction itself must stay clean
  device:clear_logs()
  -- the editor may open focused with the keyboard over the dock, and a slow page focuses it late: focus is waited for, a tap on the part of the
  -- language row the taller rail does not cover lowers the keyboard, and the shot is taken of the settled screen
  device:extended_wait_until{visible={id='EditorHelperRails'}, timeout=3000, optional=true}
  if device:get_element({id='EditorHelperRails'}) then
    local row, rail = device:get_element({id='ProblemEditorDiffRow'}).bounds, device:get_element({id='EditorHelperRails'}).bounds
    device:tap_on({point=('%d,%d'):format((row.left + row.right) // 2, (row.top + math.min(row.bottom, rail.top - 4)) // 2)})
    device:assert_not_visible({id='EditorHelperRails'})
  end
  device:assert_screenshot("two-sum-before-run")
  local function editor_text()
    local t = (device:get_element({id='CodeEditorInput'}) or {}).text
    if t and t ~= "" then return t end
  end
  local code_before = editor_text()
  assert_true(code_before and code_before:find("def twoSum", 1, true), "the editor does not hold the starter before the run: " .. tostring(code_before))
  local shown
  run_flow("submit", function()
    stub_run()
    device:tap_on({id='ConsoleBarSubmit'})
    shown = device:extended_wait_until{visible={id='ConsoleResultCounts', text='.*3 / 3.*'}, timeout=12000, optional=true}
  end)
  local now = device:get_element({id='ConsoleResultCounts'})
  local totals = (shown and shown.text) or (now and now.text) or ""
  log("totals", totals)
  assert_true(shown, "the stubbed 3 of 3 is not on screen: " .. totals)
  assert_true(editor_text() == code_before, "the run changed the code in the editor")
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
  assert_true(info:find("editor_v2.console.run_end", 1, true), "no editor_v2.console.run_end line in the app log")
end)
-- the default goes back whatever failed above: later tests expect it
pcall(function() tap_choice('LeetcodeListRunnerDevice', 'leetcode.list.runner_tap') end)
if not ok then error(failure, 0) end
