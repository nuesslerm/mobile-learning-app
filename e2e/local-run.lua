--[[---
platform: browser
against: browser=export
timeout: 600
focus: true
features: leetcode
---]]
-- Run on the device (issue 86, B2): with "This device" chosen on the problem list, Python runs in the hidden Pyodide page and the backend is never
-- asked (a stub counts the requests that would reach it). The console's Submit shows Accepted over all 12 cases, a solution that raises on one input shows
-- 11 / 12 and the error, one that never ends is stopped by the per-case limit, a syntax error shows the compile error; a custom case runs the one typed case
-- with no verdict, and a field that is not a value is refused under it. The default (this device) is chosen first and never changed after.
-- wf browser test e2e/local-run.lua --env base=http://127.0.0.1:8800/mla-tamagui
local android = device.platform == "android"
local base = env.base or (not android and "http://127.0.0.1:8800/mla-tamagui" or nil)
assert_true(base, "pass --env base=<update link>; wf test run does")
-- the dock shows while the keyboard is down, so the screen is awaited and the dock checked once the keyboard is lowered
local READY = 'ProblemEditorScreen'
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
  -- the web editor autofocuses and its keyboard covers the dock
  if device:extended_wait_until{visible={id='EditorHelperRails'}, timeout=3000, optional=true} then
    -- the console's rail is taller and covers the whole language row on a short screen: Done, at the end of the action toolbar, lowers the keyboard
    -- the rail rises with the keyboard's frame: a tap before it sits above the keyboard would land on the keys
    device:extended_wait_until{condition=function()
      local kb, rail = device:get_device_state('keyboard'), device:get_element({id='EditorHelperRails'})
      return kb.visible and kb.bounds ~= nil and rail ~= nil and rail.bounds.bottom <= kb.bounds.top + 2
    end, timeout=10000, interval=250, label='the rail sits above the keyboard'}
    device:tap_on({id='EditorAction-Done'})
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
  device:assert_visible({id='ConsoleDock'}, {timeout=10000})
end

local function choose(runner)
  open("/blind75", 'LeetcodeListRunnerRow')
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
  end)

  run_flow("accepted", function()
    -- the first run loads the interpreter: a cold phone takes a while
    con_run("two-sum", SEEDS.good, 'ConsoleBarSubmit', 'Accepted', 240)
    device:extended_wait_until{visible={id='ConsoleResultCounts', text='.*12 / 12.*'}, timeout=10000, label='12 / 12'}
  end)

  run_flow("a case raises", function()
    con_run("two-sum", SEEDS.raises, 'ConsoleBarSubmit', 'Runtime Error')
    device:extended_wait_until{visible={id='ConsoleResultCounts', text='.*11 / 12.*'}, timeout=10000, label='11 / 12'}
    device:extended_wait_until{visible={id='ConsoleResultErrorText', text='.*IndexError.*'}, timeout=10000, label='the error text'}
  end)

  run_flow("a case never ends", function()
    -- the harness's per-case limit (5 s) stops it; the rest of the cases still run
    con_run("two-sum", SEEDS.hang, 'ConsoleBarSubmit', 'Time Limit Exceeded', 90)
    device:extended_wait_until{visible={id='ConsoleResultErrorText', text='.*Timeout.*'}, timeout=10000, label='Timeout'}
  end)

  run_flow("a syntax error", function()
    con_run("two-sum", SEEDS.broken, 'ConsoleBarRun', 'Compile Error')
    device:extended_wait_until{visible={id='ConsoleResultErrorText', text='.*CompileError.*'}, timeout=10000, label='CompileError'}
  end)

  run_flow("another problem", function()
    -- a deep link reloads the web page, so the warm interpreter is not what is proved here (the provider's flow test proves a second run reuses the page): a second problem's tests run on the device too
    local palindrome = "class Solution:\n    def isPalindrome(self, s: str) -> bool:\n        t = [c.lower() for c in s if c.isalnum()]\n        return t == t[::-1]\n"
    con_run("valid-palindrome", palindrome, 'ConsoleBarSubmit', 'Accepted')
  end)

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

  assert_true(#device:get_network_stub_hits() == 0, "a run reached the backend")
end)
if not ok then error(failure, 0) end
log("local run: pass")
