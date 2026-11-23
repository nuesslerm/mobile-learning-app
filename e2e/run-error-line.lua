--[[---
platform: browser
against: export
timeout: 420
focus: true
features: leetcode
---]]
-- A failing run marks the line it reports (issue 86, E2): with "This device" chosen on the problem list, a solution that raises on one input at
-- line 44 (below the editor's first screen) shows the error on the card, and the editor draws that line red and scrolls it into the editor zone,
-- inside the screen. Typing a character takes the mark and the card away together. A syntax error marks the broken def line. The run's own
-- record, editor_v2.run_error.shown, is read from the app log, and no run reaches the backend (a stub counts the requests that would).
-- wf test run --platform browser run-error-line   (wf browser test e2e/run-error-line.lua --env base=http://127.0.0.1:8800/mla-tamagui)
-- --env console=1 runs the same flows through the console (Submit runs every case, the Run Result tab holds the error and its Line button; its goldens carry a -console suffix):
-- wf test run run-error-line --platform browser -- --env console=1
local base = env.base or "http://127.0.0.1:8800/mla-tamagui"
local console = env.console == "1"
local READY = console and 'ProblemEditorScreen' or 'ProblemEditorRunLocalButton'
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

-- 40 comment lines push the statement that raises to line 44: the editor opens showing the first lines only
local HEAD = "class Solution:\n    def twoSum(self, nums: List[int], target: int) -> List[int]:\n"
local NOTES = {}
for n = 1, 40 do NOTES[n] = ("        # note %d\n"):format(n) end
local BODY = "        seen = {}\n        for i, n in enumerate(nums):\n            if target - n in seen:\n                return [seen[target - n], i]\n            seen[n] = i\n        return []\n"
local RAISING = HEAD .. table.concat(NOTES) .. "        if nums == [3, 2, 4]:\n            raise IndexError('boom')\n" .. BODY
local BROKEN = "class Solution:\n    def twoSum(self, nums, target)\n        return []\n"

local function logged(needle)
  for _, l in ipairs(device:get_logs{level='info'}) do
    if l:find(needle, 1, true) then return true end
  end
  return false
end

local function buffer() local e = device:get_element({id='CodeEditorInput'}) return e and e.text or "" end

local function choose_device()
  device:clear_logs()
  device:open_link(base .. "/")
  device:assert_visible({id='LeetcodeListRunnerRow'}, {timeout=60000})
  -- the static page paints its buttons before its scripts have hydrated them, and a tap before that is lost: the first route.change is awaited
  device:extended_wait_until{condition=function() return logged('route.change') end, timeout=60000, interval=250, label='the page hydrated'}
  device:tap_on({id='LeetcodeListRunnerDevice'})
  -- the screen's own record that the press reached it: the button is there before and after the tap
  device:extended_wait_until{condition=function() return logged('leetcode.list.runner_tap') end, timeout=15000, interval=250, label='the tap reached the screen'}
end

local function set_console(on)
  local id = on and 'LeetcodeListConsoleOn' or 'LeetcodeListConsoleOff'
  device:open_link(base .. "/")
  device:assert_visible({id='LeetcodeListConsoleRow'}, {timeout=60000})
  device:scroll_until_visible{element={id=id}, direction='DOWN', timeout=30000}
  device:clear_logs()
  for _ = 1, 10 do
    device:tap_on({id=id})
    if device:extended_wait_until{condition=function() return logged('leetcode.list.console_tap') end, timeout=2000, interval=250, optional=true} then return end
  end
  error("the console choice never reached the screen")
end

-- the web editor autofocuses and its keyboard covers Run Local (a tap there would type the key under it): a tap on the page outside the
-- editor lowers it, aimed at the part of the language row the rail does not cover
local function lower_keyboard()
  device:extended_wait_until{visible={id='EditorHelperRails'}, timeout=15000, optional=true}
  if device:get_element({id='EditorHelperRails'}) then
    local row, rail = device:get_element({id='ProblemEditorDiffRow'}).bounds, device:get_element({id='EditorHelperRails'}).bounds
    device:tap_on({point=('%d,%d'):format((row.left + row.right) // 2, (row.top + math.min(row.bottom, rail.top - 4)) // 2)})
    device:assert_not_visible({id='EditorHelperRails'})
  end
  assert_true(not device:get_device_state('keyboard').visible, "the keyboard is still up")
end

-- opens two-sum with `code` in the editor, hydrated, keyboard lowered and the log empty
local function open_editor(code, first_line)
  -- a link to the route the app already shows remounts it a moment later: the app leaves first
  if device:get_element({id=READY}) or device:get_element({id='ConsoleSheet'}) then
    device:open_link(base .. "/settings")
    device:assert_not_visible({id=READY})
  end
  device:clear_logs()
  device:open_link(base .. "/leetcode/two-sum?seed64=" .. base64url(code))
  device:assert_visible({id=READY}, {timeout=60000})
  device:extended_wait_until{condition=function() return logged('route.change') end, timeout=60000, interval=250, label='the page hydrated'}
  -- the editor draws only the lines near its viewport, so its text is read as far as it goes: the seed's first lines are there
  device:extended_wait_until{condition=function() return buffer():find(first_line, 1, true) ~= nil end, timeout=30000, label='the seed in the editor'}
  lower_keyboard()
  -- the dock shows while the keyboard is down
  if console then device:assert_visible({id='ConsoleDock'}, {timeout=10000}) end
  -- the cold deep link's own hydration warning is not this test's: the log is read from here on
  device:clear_logs()
end

local function marked() return device:get_element({css='.cm-run-error-line'}) end
local function no_mark() return marked() == nil and device:get_element({css='.cm-run-error'}) == nil end

local function run(code, first_line, card_id, card_text, secs, console_button)
  open_editor(code, first_line)
  assert_true(no_mark(), "a line is marked before any run")
  device:tap_on({id=console and console_button or 'ProblemEditorRunLocalButton'})
  device:extended_wait_until{visible={id=card_id, text=card_text}, timeout=secs * 1000, label='the card'}
  device:extended_wait_until{condition=function() return marked() ~= nil end, timeout=10000, label='the failing line is marked'}
end

local function assert_clean_log()
  local warnings = device:get_logs()
  assert_true(#warnings == 0, "JS warnings after the run: " .. table.concat(warnings, " | "))
end

local ok, failure = pcall(function()
  run_flow("choose this device", function()
    choose_device()
    if console then set_console(true) end
  end)

  if console then
    run_flow("a case that raises at line 44 marks it inside the editor", function()
      -- the first run loads the interpreter
      run(RAISING, '# note 5', 'ConsoleResultErrorText', '.*IndexError.*', 240, 'ConsoleBarSubmit')
      device:assert_visible({id='ConsoleResultVerdict', text='Runtime Error'}, {timeout=10000})
      device:assert_visible({id='ConsoleResultLine', text='Line 44.*'}, {timeout=10000})
      local line, zone, screen = marked(), device:get_element({id='ProblemEditorEditorZone'}), device:get_device_state('screen')
      assert_true(line and zone, "the marked line or the editor zone has no bounds")
      local b, z = line.bounds, zone.bounds
      log(json.encode({ line = b, zone = z, screen = screen }))
      assert_true(line.text:find("raise IndexError('boom')", 1, true), "the marked line is not the raising statement: " .. tostring(line.text))
      assert_true(b.bottom - b.top >= 12, ("the marked line is cut to %d px"):format(b.bottom - b.top))
      assert_true(b.top >= z.top and b.bottom <= z.bottom, ("the marked line (%d to %d) is outside the editor zone (%d to %d)"):format(b.top, b.bottom, z.top, z.bottom))
      assert_true(b.top >= 0 and b.bottom <= screen.height, ("the marked line (%d to %d) is outside the screen (%d high)"):format(b.top, b.bottom, screen.height))
      assert_true(device:get_element({css='.cm-run-error'}) ~= nil, "the line's text is not underlined")
      device:extended_wait_until{condition=function() return logged('editor_v2.run_error.shown') end, timeout=10000, interval=250, label='the run logged the jump'}
      assert_clean_log()
    end)

    run_flow("typing a character takes the mark and the result away", function()
      device:tap_on({css='.cm-run-error-line'})
      device:extended_wait_until{condition=function() return device:get_device_state('keyboard').visible end, timeout=15000, label='the keyboard is up'}
      assert_true(marked() ~= nil, "focusing the editor took the mark away")
      local before = buffer()
      device:input_text('x')
      device:extended_wait_until{condition=function() return buffer() ~= before end, timeout=10000, label='the character is in the buffer'}
      device:extended_wait_until{condition=no_mark, timeout=10000, label='the mark is gone'}
      lower_keyboard()
      device:tap_on({id='ConsoleBarOpen'})
      device:extended_wait_until{visible={id='ConsoleSheet'}, timeout=15000, label='the sheet'}
      device:tap_on({id='ConsoleTabs-result'})
      device:extended_wait_until{visible={id='ConsoleResultEmpty'}, timeout=15000, label='the result is gone'}
      device:assert_not_visible({id='ConsoleResultVerdict'})
      assert_clean_log()
    end)

    run_flow("a syntax error marks the broken def line", function()
      run(BROKEN, 'def twoSum(self, nums, target)', 'ConsoleResultErrorText', '.*CompileError.*', 120, 'ConsoleBarRun')
      device:assert_visible({id='ConsoleResultLine', text='Line 2.*'}, {timeout=10000})
      local line = marked()
      assert_true(line.text:find("def twoSum(self, nums, target)", 1, true), "the marked line is not the broken def: " .. tostring(line.text))
      assert_clean_log()
    end)

    run_flow("turn the console off", function() set_console(false) end)
    assert_true(#device:get_network_stub_hits() == 0, "a run reached the backend")
    return
  end

  run_flow("a case that raises at line 44 marks it inside the editor", function()
    -- the first run loads the interpreter
    run(RAISING, '# note 5', 'ProblemEditorRunOutcomeRuntimeError', '.*IndexError.*', 240)
    device:assert_visible({id='ProblemEditorRunOutcomeLine', text='Line 44'}, {timeout=10000})
    local line, zone, screen = marked(), device:get_element({id='ProblemEditorEditorZone'}), device:get_device_state('screen')
    assert_true(line and zone, "the marked line or the editor zone has no bounds")
    local b, z = line.bounds, zone.bounds
    log(json.encode({ line = b, zone = z, screen = screen }))
    assert_true(line.text:find("raise IndexError('boom')", 1, true), "the marked line is not the raising statement: " .. tostring(line.text))
    assert_true(b.bottom - b.top >= 12, ("the marked line is cut to %d px"):format(b.bottom - b.top))
    assert_true(b.top >= z.top and b.bottom <= z.bottom, ("the marked line (%d to %d) is outside the editor zone (%d to %d)"):format(b.top, b.bottom, z.top, z.bottom))
    assert_true(b.top >= 0 and b.bottom <= screen.height, ("the marked line (%d to %d) is outside the screen (%d high)"):format(b.top, b.bottom, screen.height))
    assert_true(device:get_element({css='.cm-run-error'}) ~= nil, "the line's text is not underlined")
    device:extended_wait_until{condition=function() return logged('editor_v2.run_error.shown') end, timeout=10000, interval=250, label='the run logged the jump'}
    -- the toast that announces the result comes and goes by itself: the shot is of the settled screen, after it has gone
    device:extended_wait_until{visible={text='Tests failed'}, timeout=10000, label='the toast'}
    device:extended_wait_until{not_visible={text='Tests failed'}, timeout=15000, label='the toast is gone'}
    device:extended_wait_until{condition=function() return device:get_element({text='Tests failed'}) == nil end, timeout=10000, label='the toast is removed'}
    device:assert_screenshot("run-error-line")
    assert_clean_log()
  end)

  run_flow("typing a character takes the mark and the card away", function()
    device:tap_on({css='.cm-run-error-line'})
    device:extended_wait_until{condition=function() return device:get_device_state('keyboard').visible end, timeout=15000, label='the keyboard is up'}
    assert_true(marked() ~= nil and device:get_element({id='ProblemEditorRunOutcomeStatus'}) ~= nil, "focusing the editor took the mark or the card away")
    local before = buffer()
    device:input_text('x')
    device:extended_wait_until{condition=function() return buffer() ~= before end, timeout=10000, label='the character is in the buffer'}
    device:extended_wait_until{condition=no_mark, timeout=10000, label='the mark is gone'}
    device:assert_not_visible({id='ProblemEditorRunOutcomeStatus'})
    assert_clean_log()
    lower_keyboard()
  end)

  run_flow("a syntax error marks the broken def line", function()
    run(BROKEN, 'def twoSum(self, nums, target)', 'ProblemEditorRunOutcomeCompileError', '.*CompileError.*', 120)
    device:assert_visible({id='ProblemEditorRunOutcomeLine', text='Line 2'}, {timeout=10000})
    local line = marked()
    assert_true(line.text:find("def twoSum(self, nums, target)", 1, true), "the marked line is not the broken def: " .. tostring(line.text))
    assert_clean_log()
  end)

  assert_true(#device:get_network_stub_hits() == 0, "a run reached the backend")
end)
if not ok then error(failure, 0) end
log("run error line: pass")
