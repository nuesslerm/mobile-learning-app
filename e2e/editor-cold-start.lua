--[[---
platform: browser
against: browser=export
timeout: 300
focus: true
features: leetcode
---]]
-- The editor's cold start in the browser: two-sum is reloaded five times for each arm of the runner setting (device, whose warm-up starts at hydration,
-- and backend, which warms nothing), and the medians of evalMs, readyToHydratedMs and totalMs of the app's one editor.cold_start line are printed.
-- Nothing here judges a number: the script asserts only that the line exists. The iframe shares the app's main thread and a desktop parses fast, so
-- these are not phone numbers; the Android units of editor-typing print the emulator's.
-- wf browser test e2e/editor-cold-start.lua --env base=http://127.0.0.1:8800/mla-tamagui
local base = env.base or "http://127.0.0.1:8800/mla-tamagui"
local RELOADS = 5

local function logged_line(needle)
  for _, l in ipairs(device:get_logs{level='info'}) do
    if l:find(needle, 1, true) then return l end
  end
  return nil
end

local function field(line, name)
  return tonumber(line:match('"?' .. name .. '"?%s*[:=]%s*(-?[%d%.]+)'))
end

local function median(values)
  table.sort(values)
  return values[(#values + 1) // 2]
end

local function choose(runner)
  local id = runner == 'device' and 'LeetcodeListRunnerDevice' or 'LeetcodeListRunnerBackend'
  device:clear_logs()
  device:open_link(base .. "/")
  device:assert_visible({id='LeetcodeListRunnerRow'}, {timeout=60000})
  device:extended_wait_until{condition=function() return logged_line('route.change') ~= nil end, timeout=60000, interval=250, label='the page hydrated'}
  device:scroll_until_visible{element={id=id}, direction='DOWN'}
  device:tap_on({id=id})
  device:extended_wait_until{condition=function() return logged_line('leetcode.list.runner_tap') ~= nil end, timeout=15000, interval=250, label='the tap reached the screen'}
end

for _, runner in ipairs({'device', 'backend'}) do
  run_flow("cold start with the " .. runner .. " runner", function()
    choose(runner)
    local evals, hydrate, totals = {}, {}, {}
    for i = 1, RELOADS do
      device:clear_logs()
      device:open_link(base .. "/leetcode/two-sum")
      device:assert_visible({id='ProblemEditorRunLocalButton'}, {timeout=60000})
      device:extended_wait_until{condition=function() return logged_line('editor.cold_start') ~= nil end, timeout=30000, interval=250, label='the editor.cold_start line'}
      local line = logged_line('editor.cold_start')
      if i == 1 then log(runner .. " first line: " .. line) end
      evals[#evals + 1] = field(line, 'evalMs') or -1
      hydrate[#hydrate + 1] = field(line, 'readyToHydratedMs') or -1
      totals[#totals + 1] = field(line, 'totalMs') or -1
    end
    log(("%s runner, %d reloads: median evalMs %s, readyToHydratedMs %s, totalMs %s"):format(runner, RELOADS, median(evals), median(hydrate), median(totals)))
  end)
end
