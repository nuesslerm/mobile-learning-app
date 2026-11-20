--[[---
platform: android, browser
against: android=update, browser=export
timeout: 420
focus: true
features: leetcode
---]]
-- Run Local on the device (issue 86, B2): with "This device" chosen on the problem list, Python runs in the hidden Pyodide page and the
-- backend is never asked (a stub counts the requests that would reach it). A good solution is Accepted over all 12 cases, a solution that
-- raises on one input shows 11 / 12 and the error, one that never ends is stopped by the per-case limit, a syntax error shows the compile
-- error. The setting is put back to the server at the end: later tests on this device expect the backend.
-- wf browser test e2e/local-run.lua --env base=http://127.0.0.1:8800/mla-tamagui
-- wf android test e2e/local-run.lua --env base=exp://u.expo.dev/<projectId>/group/<group>/--
local android = device.platform == "android"
local base = env.base or (not android and "http://127.0.0.1:8800/mla-tamagui" or nil)
assert_true(base, "pass --env base=<update link>; wf test run does")
local function stub_run() device:add_network_stub("POST", "*/api/leetcode/run", { body_file = "e2e/stubs/leetcode-run-two-sum.json", headers = { ["content-type"] = "application/json" } }) end

local function base64url(s)
  local abc, out = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_", {}
  for i = 1, #s, 3 do
    local a, b, c = s:byte(i, i + 2)
    local n = a * 65536 + (b or 0) * 256 + (c or 0)
    for k = 1, 2 + (b and 1 or 0) + (c and 1 or 0) do
      local idx = math.floor(n / 64 ^ (4 - k)) % 64
      out[#out + 1] = abc:sub(idx + 1, idx + 1)
    end
  end
  return table.concat(out)
end

local HEAD = "class Solution:\n    def twoSum(self, nums: List[int], target: int) -> List[int]:\n"
local GOOD = HEAD .. "        seen = {}\n        for i, n in enumerate(nums):\n            if target - n in seen:\n                return [seen[target - n], i]\n            seen[n] = i\n        return []\n"
local SEEDS = {
  good = GOOD,
  raises = HEAD .. "        if nums == [3, 2, 4]:\n            raise IndexError('boom')\n" .. GOOD:sub(#HEAD + 1),
  hang = HEAD .. "        if nums == [3, 3]:\n            while True:\n                pass\n" .. GOOD:sub(#HEAD + 1),
  broken = "class Solution:\n    def twoSum(self, nums, target)\n        return []\n",
}

-- Expo Go bundles a route on first load; a link that lands elsewhere is opened again
local function open(path, id, secs)
  local t0 = os.time()
  while os.time() - t0 < (secs or 120) do
    device:open_link(base .. path)
    if device:extended_wait_until{visible={id=id}, timeout=15000, optional=true} then return end
  end
  device:assert_visible({id=id}, {timeout=5000, label='route ready'})
end

local function leave_editor()
  -- a link to the route the app already shows remounts it a moment later: the app leaves first
  if device:get_element({id='ProblemEditorRunLocalButton'}) then
    device:open_link(base .. "/settings")
    device:assert_not_visible({id='ProblemEditorRunLocalButton'})
  end
end

local function lower_keyboard()
  if android then
    -- the editor often opens unfocused on the phone; when the keyboard is up, Done at the end of the toolbar lowers it
    if device:get_device_state('keyboard').visible then
      device:scroll_until_visible{element={id='EditorAction-Done'}, direction='RIGHT', from={id='EditorActionToolbar'}}
      device:tap_on({id='EditorAction-Done'})
    end
    return
  end
  -- the web editor autofocuses and its keyboard covers Run Local: a tap on the page outside the editor lowers it, aimed at the part of the
  -- language row the rail does not cover (as the typing test does)
  if device:extended_wait_until{visible={id='EditorHelperRails'}, timeout=3000, optional=true} then
    local row, rail = device:get_element({id='ProblemEditorDiffRow'}).bounds, device:get_element({id='EditorHelperRails'}).bounds
    device:tap_on({point=('%d,%d'):format((row.left + row.right) // 2, (row.top + math.min(row.bottom, rail.top - 4)) // 2)})
    device:assert_not_visible({id='EditorHelperRails'})
  end
end

-- opens the problem with `code` in the editor and runs it; returns when the outcome card names a verdict
local function run_seed(problem, code, expect_text, secs)
  leave_editor()
  open("/leetcode/" .. problem .. "?seed64=" .. base64url(code), 'ProblemEditorRunLocalButton')
  lower_keyboard()
  device:tap_on({id='ProblemEditorRunLocalButton'})
  device:extended_wait_until{visible={id=expect_text.id, text=expect_text.text}, timeout=(secs or 120) * 1000, label=problem .. ': ' .. expect_text.label}
end

local function choose(runner)
  open("/", 'LeetcodeListRunnerRow')
  local id = runner == 'device' and 'LeetcodeListRunnerDevice' or 'LeetcodeListRunnerBackend'
  if android then
    device:tap_on({id=id})
    return
  end
  -- the static page paints its buttons before its scripts have hydrated them, and a tap before that is lost: it is repeated until the
  -- screen logs that it got it
  device:clear_logs()
  local function got_it()
    for _, l in ipairs(device:get_logs{level='info'}) do
      if l:find('leetcode.list.runner_tap', 1, true) then return true end
    end
  end
  for _ = 1, 10 do
    device:tap_on({id=id})
    if device:extended_wait_until{condition=got_it, timeout=2000, interval=250, optional=true} then return end
  end
  error("the runner choice never reached the screen")
end

stub_run()
local ok, failure = pcall(function()
  run_flow("choose this device", function() choose('device') end)

  run_flow("accepted", function()
    -- the first run loads the interpreter: a cold phone takes a while
    run_seed("two-sum", SEEDS.good, { id = 'ProblemEditorRunOutcomeTotals', text = '.*12 / 12.*', label = '12 / 12' }, 240)
    device:extended_wait_until{visible={id='ProblemEditorRunOutcomeStatus', text='Accepted'}, timeout=10000, label='Accepted'}
  end)

  run_flow("a case raises", function()
    run_seed("two-sum", SEEDS.raises, { id = 'ProblemEditorRunOutcomeTotals', text = '.*11 / 12.*', label = '11 / 12' })
    device:extended_wait_until{visible={id='ProblemEditorRunOutcomeRuntimeError', text='.*IndexError.*'}, timeout=10000, label='the error text'}
  end)

  run_flow("a case never ends", function()
    -- the harness's per-case limit (5 s) stops it; the rest of the cases still run
    run_seed("two-sum", SEEDS.hang, { id = 'ProblemEditorRunOutcomeRuntimeError', text = '.*TimeoutError.*', label = 'TimeoutError' }, 90)
  end)

  run_flow("a syntax error", function()
    run_seed("two-sum", SEEDS.broken, { id = 'ProblemEditorRunOutcomeCompileError', text = '.*CompileError.*', label = 'CompileError' })
  end)

  run_flow("another problem", function()
    -- a deep link reloads the web page, so the warm interpreter is not what is proved here (the provider's flow test proves a second run reuses the page): a second problem's tests run on the device too
    run_seed("valid-palindrome", "class Solution:\n    def isPalindrome(self, s: str) -> bool:\n        t = [c.lower() for c in s if c.isalnum()]\n        return t == t[::-1]\n", { id = 'ProblemEditorRunOutcomeStatus', text = 'Accepted', label = 'Accepted' })
  end)

  assert_true(#device:get_network_stub_hits() == 0, "a run reached the backend")
end)
-- later tests on this device expect the backend: the setting goes back even when a step failed
pcall(function() choose('backend') end)
if not ok then error(failure, 0) end
log("local run: pass")
