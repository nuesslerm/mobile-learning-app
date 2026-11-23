--[[---
platform: android, browser
against: android=update, browser=export
timeout: 600
focus: true
features: leetcode
---]]
-- Run Local on the device (issue 86, B2): with "This device" chosen on the problem list, Python runs in the hidden Pyodide page and the
-- backend is never asked (a stub counts the requests that would reach it). A good solution is Accepted over all 12 cases, a solution that
-- raises on one input shows 11 / 12 and the error, one that never ends is stopped by the per-case limit, a syntax error shows the compile
-- error. The input zone's Run input runs the one typed case the same way: the card shows what came back and no verdict, and an input that
-- is not the arguments is refused under the field. The default (this device) is chosen first and never changed after, so later tests on
-- this device find what they expect.
-- wf browser test e2e/local-run.lua --env base=http://127.0.0.1:8800/mla-tamagui
-- wf android test e2e/local-run.lua --env base=exp://u.expo.dev/<projectId>/group/<group>/--
-- --env out=<dir> saves a screenshot of the input zone and of the custom card there.
-- --env console=1 (browser only) runs the same flows through the console: Submit is the old Run Local, a custom chip is Run input, and the verdict, counts and
-- error are read from the Run Result tab. wf test run local-run --platform browser -- --env console=1
local android = device.platform == "android"
local base = env.base or (not android and "http://127.0.0.1:8800/mla-tamagui" or nil)
assert_true(base, "pass --env base=<update link>; wf test run does")
local console = env.console == "1"
assert_true(not (console and android), "the console path runs in the browser only")
-- the dock shows while the keyboard is down, so the console path waits for the screen and checks the dock once the keyboard is lowered
local READY = console and 'ProblemEditorScreen' or 'ProblemEditorRunLocalButton'
local function shot(name) if env.out then device:take_screenshot(env.out .. "/" .. name .. ".png") end end
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
  -- raises on the input the field starts with, so a phone never has to replace text
  raisesOnDefault = HEAD .. "        if nums == [2, 7, 11, 15]:\n            raise IndexError('boom')\n" .. GOOD:sub(#HEAD + 1),
  hang = HEAD .. "        if nums == [3, 3]:\n            while True:\n                pass\n" .. GOOD:sub(#HEAD + 1),
  broken = "class Solution:\n    def twoSum(self, nums, target)\n        return []\n",
}

local TRIE = "class Trie:\n    def __init__(self):\n        self.words = set()\n\n    def insert(self, word):\n        self.words.add(word)\n\n    def search(self, word):\n        return word in self.words\n\n    def startsWith(self, prefix):\n        return %s\n"
local TRIE_GOOD = TRIE:format("any(w.startswith(prefix) for w in self.words)")
local TRIE_WRONG = TRIE:format("False")
local FLOYD = "class Solution:\n    def hasCycle(self, head: Optional[ListNode]) -> bool:\n        slow = fast = head\n        while fast and fast.next:\n            slow = slow.next\n            fast = fast.next.next\n            if slow is fast:\n                return True\n        return False\n"

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
  if device:get_element({id=READY}) or device:get_element({id='ConsoleSheet'}) then
    device:open_link(base .. "/settings")
    device:assert_not_visible({id=READY})
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
    if console then
      -- the console's rail is taller and covers the whole language row on a short screen: Done, at the end of the action toolbar, lowers the keyboard
      -- the rail rises with the keyboard's frame: a tap before it sits above the keyboard would land on the keys
      device:extended_wait_until{condition=function()
        local kb, rail = device:get_device_state('keyboard'), device:get_element({id='EditorHelperRails'})
        return kb.visible and kb.bounds ~= nil and rail ~= nil and rail.bounds.bottom <= kb.bounds.top + 2
      end, timeout=10000, interval=250, label='the rail sits above the keyboard'}
      device:tap_on({id='EditorAction-Done'})
    else
      local row, rail = device:get_element({id='ProblemEditorDiffRow'}).bounds, device:get_element({id='EditorHelperRails'}).bounds
      device:tap_on({point=('%d,%d'):format((row.left + row.right) // 2, (row.top + math.min(row.bottom, rail.top - 4)) // 2)})
    end
    device:assert_not_visible({id='EditorHelperRails'})
  end
end

-- opens the problem with `code` in the editor, hydrated and with its keyboard lowered
local function open_editor(problem, code)
  leave_editor()
  device:clear_logs()
  open("/leetcode/" .. problem .. "?seed64=" .. base64url(code), READY)
  -- the static page paints its buttons before its scripts have hydrated them, and a tap before that is lost: the first route.change is awaited
  if not android then
    device:extended_wait_until{condition=function()
      for _, l in ipairs(device:get_logs{level='info'}) do
        if l:find('route.change', 1, true) then return true end
      end
    end, timeout=60000, interval=250, label='the page hydrated'}
  end
  lower_keyboard()
  if console then device:assert_visible({id='ConsoleDock'}, {timeout=10000}) end
end

-- opens the problem and presses `button` (Run Local unless told); returns when the outcome card names a verdict
local function run_seed(problem, code, expect_text, secs, button)
  open_editor(problem, code)
  device:tap_on({id=button or 'ProblemEditorRunLocalButton'})
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

local function logged(needle)
  for _, l in ipairs(device:get_logs{level='info'}) do
    if l:find(needle, 1, true) then return true end
  end
  return false
end

local function bounds(id) local e = device:get_element({id=id}); return e and e.bounds or nil end

local function set_console(on)
  open("/", 'LeetcodeListConsoleRow')
  local id = on and 'LeetcodeListConsoleOn' or 'LeetcodeListConsoleOff'
  device:scroll_until_visible{element={id=id}, direction='DOWN', timeout=30000}
  device:clear_logs()
  for _ = 1, 10 do
    device:tap_on({id=id})
    if device:extended_wait_until{condition=function() return logged('leetcode.list.console_tap') end, timeout=2000, interval=250, optional=true} then return end
  end
  error("the console choice never reached the screen")
end

local function open_sheet()
  local b = bounds('ConsoleBarOpen')
  device:tap_on({point=('%d,%d'):format(b.right - 6, (b.top + b.bottom) // 2)})
  device:extended_wait_until{visible={id='ConsoleSheet'}, timeout=15000, label='the sheet'}
end

-- opens the editor, then runs a button of the dock and waits for the Run Result tab to name the verdict
local function con_run(problem, code, button, want, secs)
  open_editor(problem, code)
  device:tap_on({id=button})
  device:extended_wait_until{visible={id='ConsoleResultVerdict', text=want}, timeout=(secs or 120) * 1000, label=problem .. ': ' .. want}
end

-- opens the editor and a custom case copied from Case 1, with the sheet up
local function con_custom(code)
  open_editor("two-sum", code)
  open_sheet()
  device:tap_on({id='ConsoleChipAdd'})
  device:extended_wait_until{visible={id='ConsoleField-nums'}, timeout=10000, label='the custom case field'}
end

-- the first tap blurs a focused field and the keyboard's leaving moves the footer under it: a tap is repeated until the run starts
local function footer_run()
  local before = 0
  for _, l in ipairs(device:get_logs{level='info'}) do if l:find('editor_v2.console.run_start', 1, true) then before = before + 1 end end
  for _ = 1, 5 do
    device:tap_on({id='ConsoleFooterRun'})
    local started = device:extended_wait_until{condition=function()
      local n = 0
      for _, l in ipairs(device:get_logs{level='info'}) do if l:find('editor_v2.console.run_start', 1, true) then n = n + 1 end end
      return n > before
    end, timeout=2500, interval=250, optional=true}
    if started then return end
  end
  error("the footer's Run never started a run")
end

stub_run()
local ok, failure = pcall(function()
  run_flow("choose this device", function()
    choose('device')
    if console then set_console(true) end
  end)

  run_flow("accepted", function()
    -- the first run loads the interpreter: a cold phone takes a while
    if console then
      con_run("two-sum", SEEDS.good, 'ConsoleBarSubmit', 'Accepted', 240)
      device:extended_wait_until{visible={id='ConsoleResultCounts', text='.*12 / 12.*'}, timeout=10000, label='12 / 12'}
      return
    end
    run_seed("two-sum", SEEDS.good, { id = 'ProblemEditorRunOutcomeTotals', text = '.*12 / 12.*', label = '12 / 12' }, 240)
    device:extended_wait_until{visible={id='ProblemEditorRunOutcomeStatus', text='Accepted'}, timeout=10000, label='Accepted'}
  end)

  run_flow("a case raises", function()
    if console then
      con_run("two-sum", SEEDS.raises, 'ConsoleBarSubmit', 'Runtime Error')
      device:extended_wait_until{visible={id='ConsoleResultCounts', text='.*11 / 12.*'}, timeout=10000, label='11 / 12'}
      device:extended_wait_until{visible={id='ConsoleResultErrorText', text='.*IndexError.*'}, timeout=10000, label='the error text'}
      return
    end
    run_seed("two-sum", SEEDS.raises, { id = 'ProblemEditorRunOutcomeTotals', text = '.*11 / 12.*', label = '11 / 12' })
    device:extended_wait_until{visible={id='ProblemEditorRunOutcomeRuntimeError', text='.*IndexError.*'}, timeout=10000, label='the error text'}
  end)

  run_flow("a case never ends", function()
    -- the harness's per-case limit (5 s) stops it; the rest of the cases still run
    if console then
      con_run("two-sum", SEEDS.hang, 'ConsoleBarSubmit', 'Time Limit Exceeded', 90)
      device:extended_wait_until{visible={id='ConsoleResultErrorText', text='.*Timeout.*'}, timeout=10000, label='Timeout'}
      return
    end
    run_seed("two-sum", SEEDS.hang, { id = 'ProblemEditorRunOutcomeRuntimeError', text = '.*TimeoutError.*', label = 'TimeoutError' }, 90)
  end)

  run_flow("a syntax error", function()
    if console then
      con_run("two-sum", SEEDS.broken, 'ConsoleBarRun', 'Compile Error')
      device:extended_wait_until{visible={id='ConsoleResultErrorText', text='.*CompileError.*'}, timeout=10000, label='CompileError'}
      return
    end
    run_seed("two-sum", SEEDS.broken, { id = 'ProblemEditorRunOutcomeCompileError', text = '.*CompileError.*', label = 'CompileError' })
  end)

  run_flow("another problem", function()
    -- a deep link reloads the web page, so the warm interpreter is not what is proved here (the provider's flow test proves a second run reuses the page): a second problem's tests run on the device too
    local palindrome = "class Solution:\n    def isPalindrome(self, s: str) -> bool:\n        t = [c.lower() for c in s if c.isalnum()]\n        return t == t[::-1]\n"
    if console then
      con_run("valid-palindrome", palindrome, 'ConsoleBarSubmit', 'Accepted')
      return
    end
    run_seed("valid-palindrome", palindrome, { id = 'ProblemEditorRunOutcomeStatus', text = 'Accepted', label = 'Accepted' })
  end)

  if console then
    run_flow("a custom case runs on the device", function()
      con_custom(SEEDS.good)
      footer_run()
      device:extended_wait_until{visible={id='ConsoleResultVerdict', text='Custom case'}, timeout=120000, label='Custom case'}
      device:assert_visible({id='ConsoleResultOutput', text='\\[0,1\\]'}, {timeout=10000})
      device:assert_not_visible({id='ConsoleResultCounts'})
    end)

    run_flow("a custom case that raises", function()
      con_custom(SEEDS.raisesOnDefault)
      footer_run()
      device:extended_wait_until{visible={id='ConsoleResultVerdict', text='Runtime Error'}, timeout=120000, label='Runtime Error'}
      device:assert_visible({id='ConsoleResultErrorText', text='.*IndexError.*'}, {timeout=10000})
    end)

    -- a field left focused keeps the browser's keyboard model up for the next page, so this flow runs last
    run_flow("a design problem", function()
      -- a list of operations on one Trie: the harness builds the instance, calls each operation and compares the list of results
      con_run("implement-trie-prefix-tree", TRIE_GOOD, 'ConsoleBarSubmit', 'Accepted')
      -- a wrong startsWith fails the cases that call it, and the first one lists its operations and its arguments
      con_run("implement-trie-prefix-tree", TRIE_WRONG, 'ConsoleBarSubmit', 'Wrong Answer')
      device:extended_wait_until{visible={id='ConsoleResultInput-operations', text='.*Trie.*'}, timeout=10000, label='the operations'}
      device:extended_wait_until{visible={id='ConsoleResultInput-arguments'}, timeout=10000, label='the arguments'}
    end)

    run_flow("a cyclic list", function()
      con_run("linked-list-cycle", FLOYD, 'ConsoleBarSubmit', 'Accepted')
    end)

    run_flow("a field that is not a value", function()
      con_custom(SEEDS.good)
      -- the custom case starts with Case 1's fields: taking the closing bracket off the first one leaves text that is not JSON
      device:tap_on({id='ConsoleField-nums'})
      device:press_key('Ctrl+End')
      device:press_key('Backspace')
      for _ = 1, 5 do
        device:tap_on({id='ConsoleFooterRun'})
        if device:extended_wait_until{visible={id='ConsoleFieldError', text='.*nums is not JSON.*'}, timeout=2500, optional=true} then break end
      end
      device:assert_visible({id='ConsoleFieldError', text='.*nums is not JSON.*'}, {timeout=5000})
      device:assert_not_visible({id='ConsoleResultVerdict'})
    end)

    run_flow("turn the console off", function() set_console(false) end)
    assert_true(#device:get_network_stub_hits() == 0, "a run reached the backend")
    return
  end

  run_flow("a custom input runs on the device", function()
    open_editor("two-sum", SEEDS.good)
    shot("input-zone")
    device:tap_on({id='ProblemEditorRunInputButton'})
    device:extended_wait_until{visible={id='ProblemEditorCustomCase-Result', text='\\[0,1\\]'}, timeout=120000, label='the result'}
    device:assert_visible({id='ProblemEditorRunOutcomeStatus', text='Custom case'})
    device:assert_not_visible({id='ProblemEditorRunOutcomeTotals'})
    shot("custom-card")
  end)

  run_flow("a custom input that raises", function()
    run_seed("two-sum", SEEDS.raisesOnDefault, { id = 'ProblemEditorCustomCase-Error', text = '.*IndexError.*', label = 'the error' }, 120, 'ProblemEditorRunInputButton')
    device:assert_visible({id='ProblemEditorRunOutcomeStatus', text='Error'})
    shot("custom-card-error")
  end)

  run_flow("an input that is not the arguments", function()
    open_editor("two-sum", SEEDS.good)
    -- the field starts with the two arguments of the first case (the last is the 9): deleting it leaves one value too few
    device:tap_on({id='ProblemEditorTestInput'})
    device:press_key('Ctrl+End')
    device:press_key('Backspace')
    lower_keyboard()
    device:tap_on({id='ProblemEditorRunInputButton'})
    device:extended_wait_until{visible={id='ProblemEditorTestInputError', text='.*Expected 2 values.*'}, timeout=15000, label='the error under the field'}
    device:assert_not_visible({id='ProblemEditorRunOutcome'})
  end)

  run_flow("a design problem", function()
    -- a list of operations on one Trie: the harness builds the instance, calls each operation and compares the list of results
    run_seed("implement-trie-prefix-tree", TRIE_GOOD, { id = 'ProblemEditorRunOutcomeStatus', text = 'Accepted', label = 'Accepted' })
    -- a wrong startsWith fails the cases that call it, and the first one lists its whole operations line once its Input is tapped open
    run_seed("implement-trie-prefix-tree", TRIE_WRONG, { id = 'ProblemEditorRunOutcomeStatus', text = 'Wrong Answer', label = 'Wrong Answer' })
    device:tap_on({id='ProblemEditorRunCase-0-Input-Toggle'})
    device:extended_wait_until{visible={id='ProblemEditorRunCase-0-Input', text='.*arguments = .*'}, timeout=10000, label='the whole operations line'}
  end)

  run_flow("a cyclic list", function()
    -- the case builds the list and links its tail back to the node `pos` names: Floyd's walk ends, a walk to the end would not
    run_seed("linked-list-cycle", FLOYD, { id = 'ProblemEditorRunOutcomeStatus', text = 'Accepted', label = 'Accepted' })
  end)

  assert_true(#device:get_network_stub_hits() == 0, "a run reached the backend")
end)
if not ok then error(failure, 0) end
log("local run: pass")
