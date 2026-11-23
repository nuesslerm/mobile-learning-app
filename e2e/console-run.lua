--[[---
platform: browser
against: browser=export
timeout: 900
focus: true
features: leetcode
---]]
-- The console on the editor screen (issue 86, G8): turned on from the Console row of the problem list with "This device" chosen, Valid Parentheses
-- opens with its dock at the bottom. Console opens a sheet that stops below the header; a tap on the editor strip above it closes it (the sheet
-- is non-modal on the web too). Case 3 is picked from the chips and Run gives Accepted with the field's text on the Input card; the rail's Run
-- above the keyboard lowers it and runs; a custom case answers "Custom case"; a run that never ends (HANG typed at the end of the code) shows the Pending overlay and then
-- Time Limit Exceeded; Submit shows a wrong answer with its counts; a solution that raises at line 3
-- shows the error and its Line button closes the sheet onto the marked line. Waits are on elements, log lines and bounds, never on a clock; no run
-- reaches the backend (a stub counts the requests). The Android unit is the reduced one (open, chip, Run, result) and is proved only by the CI job.
-- wf test run --platform browser console-run   (wf browser test e2e/console-run.lua --env base=http://127.0.0.1:8800/mla-tamagui)
-- wf android test e2e/console-run.lua --env base=exp://u.expo.dev/<projectId>/group/<group>/--
local android = device.platform == "android"
local base = env.base or (not android and "http://127.0.0.1:8800/mla-tamagui" or nil)
assert_true(base, "pass --env base=<update link>; wf test run does")
device:add_network_stub("POST", "*/api/leetcode/run", { body_file = "e2e/stubs/leetcode-run-two-sum.json", headers = { ["content-type"] = "application/json" } })

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

local HEAD = "class Solution:\n    def isValid(self, s: str) -> bool:\n"
local GOOD_BODY = "        pairs = {')': '(', ']': '[', '}': '{'}\n        stack = []\n        for c in s:\n            if c in pairs:\n                if not stack or stack.pop() != pairs[c]:\n                    return False\n            else:\n                stack.append(c)\n        return not stack\n"
local SEEDS = {
  good = HEAD .. "        if globals().get('HANG'):\n            while True:\n                pass\n" .. GOOD_BODY,
  wrong = HEAD .. "        return True\n",
  raises = HEAD .. "        raise ValueError('boom')\n",
}

local function logged(needle, level)
  for _, l in ipairs(device:get_logs{level=level or 'info'}) do
    if l:find(needle, 1, true) then return true end
  end
  return false
end

local function count_logged(needle)
  local n = 0
  for _, l in ipairs(device:get_logs{level='info'}) do
    if l:find(needle, 1, true) then n = n + 1 end
  end
  return n
end

local function bounds(id) local e = device:get_element({id=id}); return e and e.bounds or nil end
local function text(id) local e = device:get_element({id=id}); return e and e.text or nil end
local function screen() return device:get_device_state('screen') end

local function hydrated()
  if android then return end
  device:extended_wait_until{condition=function() return logged('route.change') end, timeout=60000, interval=250, label='the page hydrated'}
end

local function open(path, id, secs)
  local t0 = os.time()
  while os.time() - t0 < (secs or 120) do
    device:open_link(base .. path)
    if device:extended_wait_until{visible={id=id}, timeout=15000, optional=true} then return end
  end
  device:assert_visible({id=id}, {timeout=5000, label='route ready'})
end

local function lower_keyboard()
  if android then
    if device:get_device_state('keyboard').visible then
      device:scroll_until_visible{element={id='EditorAction-Done'}, direction='RIGHT', from={id='EditorActionToolbar'}}
      device:tap_on({id='EditorAction-Done'})
    end
    return
  end
  if device:extended_wait_until{visible={id='EditorHelperRails'}, timeout=3000, optional=true} then
    local row, rail = bounds('ProblemEditorDiffRow'), bounds('EditorHelperRails')
    device:tap_on({point=('%d,%d'):format((row.left + row.right) // 2, (row.top + math.min(row.bottom, rail.top - 4)) // 2)})
    device:assert_not_visible({id='EditorHelperRails'})
  end
end

local function open_editor(code)
  if device:get_element({id='ConsoleDock'}) or device:get_element({id='ConsoleSheet'}) then
    device:open_link(base .. "/settings")
    device:assert_not_visible({id='ConsoleDock'})
  end
  device:clear_logs()
  open("/leetcode/valid-parentheses?seed64=" .. base64url(code), 'ProblemEditorScreen')
  hydrated()
  lower_keyboard()
  device:assert_visible({id='ConsoleDock'}, {timeout=10000})
  device:assert_not_visible({id='ProblemEditorRunLocalButton'})
  device:clear_logs()
end

local function set_console(on)
  open("/", 'LeetcodeListConsoleRow')
  local id = on and 'LeetcodeListConsoleOn' or 'LeetcodeListConsoleOff'
  if android then
    device:scroll_until_visible{element={id=id}, direction='DOWN', timeout=30000}
    device:tap_on({id=id})
    return
  end
  device:scroll_until_visible{element={id=id}, direction='DOWN', timeout=30000}
  device:clear_logs()
  for _ = 1, 10 do
    device:tap_on({id=id})
    if device:extended_wait_until{condition=function() return logged('leetcode.list.console_tap') end, timeout=2000, interval=250, optional=true} then return end
  end
  error("the console choice never reached the screen")
end

local function choose_device()
  open("/", 'LeetcodeListRunnerRow')
  if android then
    device:tap_on({id='LeetcodeListRunnerDevice'})
    return
  end
  hydrated()
  device:clear_logs()
  for _ = 1, 10 do
    device:tap_on({id='LeetcodeListRunnerDevice'})
    if device:extended_wait_until{condition=function() return logged('leetcode.list.runner_tap') end, timeout=2000, interval=250, optional=true} then return end
  end
  error("the runner choice never reached the screen")
end

local function open_sheet()
  local b = bounds('ConsoleBarOpen')
  device:tap_on({point=('%d,%d'):format(b.right - 6, (b.top + b.bottom) // 2)})
  device:extended_wait_until{visible={id='ConsoleSheet'}, timeout=15000, label='the sheet'}
  device:extended_wait_until{condition=function()
    local b, s = bounds('ConsoleSheet'), screen()
    return b and b.bottom <= s.height + 1 and (b.bottom - b.top) >= s.height * 0.5 and (b.bottom - b.top) <= s.height * 0.75
  end, timeout=15000, interval=250, label='the sheet has risen to its height'}
end

local function chip_in_view(id)
  device:extended_wait_until{condition=function()
    local c, row = bounds(id), bounds('ConsoleChipScroll')
    return c and row and c.left >= row.left - 1 and c.right <= row.right + 1
  end, timeout=15000, interval=250, label=id .. ' inside the chip row'}
end

local function run_end(before, secs)
  device:extended_wait_until{condition=function() return count_logged('editor_v2.console.run_end') > before end, timeout=(secs or 120) * 1000, interval=250, label='the run ended'}
end

local function verdict(want, secs)
  device:extended_wait_until{visible={id='ConsoleResultVerdict', text=want}, timeout=(secs or 120) * 1000, label='the verdict ' .. want}
end

local function assert_clean_log()
  local warnings = device:get_logs()
  assert_true(#warnings == 0, "JS warnings: " .. table.concat(warnings, " | "))
end

local function marked() return device:get_element({css='.cm-run-error-line'}) end

local ok, failure = pcall(function()
  run_flow("choose this device and turn the console on", function()
    choose_device()
    set_console(true)
  end)

  if android then
    run_flow("android: open, chip, Run, result", function()
      open_editor(SEEDS.good)
      open_sheet()
      device:tap_on({id='ConsoleChip-3'})
      device:tap_on({id='ConsoleFooterRun'})
      verdict('Accepted', 240)
      local b, s = bounds('ConsoleSheet'), screen()
      assert_true(b.top >= 0 and b.bottom <= s.height + 1, "the sheet is outside the screen")
    end)
    run_flow("turn the console off", function() set_console(false) end)
    assert_true(#device:get_network_stub_hits() == 0, "a run reached the backend")
    return
  end

  run_flow("the dock, the sheet, and the editor strip above it", function()
    open_editor(SEEDS.good)
    local dock, s = bounds('ConsoleDock'), screen()
    assert_true(dock.bottom >= s.height - 4 and dock.bottom <= s.height + 1, ("the dock bottom is %d on a screen %d high"):format(dock.bottom, s.height))
    open_sheet()
    local sheet = bounds('ConsoleSheet')
    assert_true(sheet.top > s.height * 0.2, "the sheet reaches the top of the screen")
    local zone = bounds('ProblemEditorEditorZone')
    local lo, hi = math.max(zone.top, 0), math.min(zone.bottom, sheet.top)
    assert_true(hi - lo > 20, ("no editor strip above the sheet (%d to %d)"):format(lo, hi))
    device:tap_on({point=('%d,%d'):format((zone.left + zone.right) // 2, (lo + hi) // 2)})
    device:extended_wait_until{not_visible={id='ConsoleSheet'}, timeout=15000, label='the sheet closes on a tap of the editor'}
    device:extended_wait_until{condition=function() return logged('editor_v2.console.close') end, timeout=10000, interval=250, label='the close was logged'}
    lower_keyboard()
    device:assert_visible({id='ConsoleDock'}, {timeout=10000})
    open_sheet()
    assert_clean_log()
  end)

  local input_before
  run_flow("a chip, Run, Accepted with the field's text on the Input card", function()
    device:tap_on({id='ConsoleChip-3'})
    chip_in_view('ConsoleChip-3')
    device:extended_wait_until{visible={id='ConsoleField-s'}, timeout=10000, label='the field'}
    input_before = text('ConsoleField-s')
    assert_true(input_before and #input_before > 0, "the field is empty")
    device:assert_screenshot("console-testcase")
    local before = count_logged('editor_v2.console.run_end')
    device:tap_on({id='ConsoleFooterRun'})
    run_end(before, 240)
    verdict('Accepted')
    device:extended_wait_until{visible={id='ConsoleResultInput-s'}, timeout=10000, label='the Input card'}
    assert_true(text('ConsoleResultInput-s') == input_before, ("the Input card reads %s, the field read %s"):format(tostring(text('ConsoleResultInput-s')), input_before))
    assert_clean_log()
  end)

  run_flow("the rail's Run above the keyboard lowers it and runs", function()
    device:tap_on({css='.cm-content'})
    device:extended_wait_until{visible={id='EditorHelperRails'}, timeout=15000, label='the rail'}
    device:extended_wait_until{not_visible={id='ConsoleSheet'}, timeout=15000, label='the sheet closed for the editor'}
    local run, rail = bounds('ConsoleBarRun'), bounds('EditorHelperRails')
    assert_true(run and rail and run.top >= rail.top and run.bottom <= rail.bottom, "ConsoleBarRun is not inside the rail")
    local starts = count_logged('editor_v2.console.run_start')
    local before = count_logged('editor_v2.console.run_end')
    device:tap_on({id='ConsoleBarRun'})
    device:extended_wait_until{condition=function() return logged('editor_v2.keyboard_dismiss', 'debug') end, timeout=15000, interval=250, label='the keyboard was dismissed'}
    device:extended_wait_until{condition=function() return count_logged('editor_v2.console.run_start') > starts end, timeout=15000, interval=250, label='the run started'}
    run_end(before)
    device:assert_not_visible({id='EditorHelperRails'})
    assert_true(not device:get_device_state('keyboard').visible, "the keyboard is still up")
    device:extended_wait_until{visible={id='ConsoleSheet'}, timeout=15000, label='the sheet is back'}
    verdict('Accepted')
  end)

  run_flow("a custom case answers Custom case", function()
    device:tap_on({id='ConsoleTabs-testcase'})
    device:extended_wait_until{visible={id='ConsoleChipAdd'}, timeout=10000, label='the testcase tab'}
    device:tap_on({id='ConsoleChipAdd'})
    device:extended_wait_until{visible={id='ConsoleField-s'}, timeout=10000, label='the custom case field'}
    local starts, before = count_logged('editor_v2.console.run_start'), count_logged('editor_v2.console.run_end')
    -- the first tap blurs the focused field and the keyboard's leaving moves the footer under it: a tap is repeated until the run starts
    for _ = 1, 5 do
      device:tap_on({id='ConsoleFooterRun'})
      if device:extended_wait_until{condition=function() return count_logged('editor_v2.console.run_start') > starts end, timeout=2500, interval=250, optional=true} then break end
    end
    run_end(before)
    verdict('Custom case')
    assert_clean_log()
  end)

  run_flow("a run that never ends shows Pending, then Time Limit Exceeded", function()
    device:tap_on({css='.cm-content'})
    device:extended_wait_until{visible={id='EditorHelperRails'}, timeout=15000, label='the rail'}
    device:press_key('Ctrl+End')
    device:input_text('\nHANG = True')
    device:tap_on({id='ConsoleBarRun'})
    device:extended_wait_until{condition=function() return device:get_element({id='ConsolePending'}) ~= nil end, timeout=15000, interval=50, label='the Pending overlay is mounted'}
    verdict('Time Limit Exceeded', 120)
    device:assert_not_visible({id='ConsolePending'})
  end)

  run_flow("Submit shows a wrong answer with its counts", function()
    open_editor(SEEDS.wrong)
    local before = count_logged('editor_v2.console.run_end')
    device:tap_on({id='ConsoleBarSubmit'})
    run_end(before, 240)
    device:extended_wait_until{visible={id='ConsoleSheet'}, timeout=15000, label='the sheet'}
    verdict('Wrong Answer')
    device:extended_wait_until{visible={id='ConsoleResultCounts', text='.* / .* testcases passed'}, timeout=10000, label='the counts'}
    assert_true(text('ConsoleResultOutput') ~= text('ConsoleResultExpected'), "Output equals Expected on a wrong answer")
    device:assert_screenshot("console-submit-wrong-answer")
    assert_clean_log()
  end)

  run_flow("an error at line 3 and its Line button", function()
    open_editor(SEEDS.raises)
    local before = count_logged('editor_v2.console.run_end')
    device:tap_on({id='ConsoleBarRun'})
    run_end(before, 240)
    device:extended_wait_until{visible={id='ConsoleSheet'}, timeout=15000, label='the sheet'}
    verdict('Runtime Error')
    device:extended_wait_until{visible={id='ConsoleResultLine', text='Line 3.*'}, timeout=10000, label='Line 3'}
    device:extended_wait_until{condition=function() return marked() ~= nil end, timeout=10000, label='the failing line is marked'}
    device:assert_screenshot("console-result-error")
    local sheet_top = bounds('ConsoleSheet').top
    device:tap_on({id='ConsoleResultLine'})
    device:extended_wait_until{not_visible={id='ConsoleSheet'}, timeout=15000, label='the sheet is gone'}
    device:extended_wait_until{condition=function()
      local line = marked()
      return line and line.bounds.bottom <= sheet_top and line.bounds.top >= 0
    end, timeout=15000, interval=250, label='the marked line sits above where the sheet was'}
    assert_clean_log()
  end)

  run_flow("turn the console off", function() set_console(false) end)

  assert_true(#device:get_network_stub_hits() == 0, "a run reached the backend")
end)
if not ok then error(failure, 0) end
log("console run: pass")
