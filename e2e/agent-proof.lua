-- Agent proof, the scripted floor: the task `wf bench agent-proof` hands an agent, written by hand.
-- wf browser test e2e/agent-proof.lua --env base=http://127.0.0.1:8810/mla-tamagui
-- --env console=1 runs it with the console on (turned on from the list; Submit runs every case and the Run Result tab holds the tally).
local base = env.base or "http://127.0.0.1:8810/mla-tamagui"
local console = env.console == "1"

local function logged(needle)
  for _, l in ipairs(device:get_logs{level='info'}) do
    if l:find(needle, 1, true) then return true end
  end
  return false
end

local function choose_console(id)
  device:open_link(base .. "/")
  device:assert_visible({id='LeetcodeListConsoleRow'}, {timeout=60000})
  device:extended_wait_until{condition=function() return logged('route.change') end, timeout=60000, interval=250, label='the page hydrated'}
  device:scroll_until_visible{element={id=id}, direction='DOWN', timeout=30000}
  device:clear_logs()
  -- a tap before the page's scripts have hydrated its buttons is lost: it is repeated until the screen logs it
  for _ = 1, 10 do
    device:tap_on({id=id})
    if device:extended_wait_until{condition=function() return logged('leetcode.list.console_tap') end, timeout=2000, interval=250, optional=true} then return end
  end
  error("the console choice never reached the screen")
end

if console then choose_console('LeetcodeListConsoleOn') end

device:open_link(base .. "/leetcode/two-sum")
device:assert_visible({id=console and 'ProblemEditorScreen' or 'ProblemEditorRunLocalButton'}, {timeout=60000})
-- the editor autofocuses and its keyboard covers Run Local: Done ends the toolbar's action row, off screen until it scrolls
if device:extended_wait_until{visible={id='EditorHelperRails'}, timeout=3000, optional=true} then
  device:scroll_until_visible{element={id='EditorAction-Done'}, direction='RIGHT', from={id='EditorActionToolbar'}}
  device:tap_on({id='EditorAction-Done'})
  device:assert_not_visible({id='EditorHelperRails'})
end
device:tap_on({id=console and 'ConsoleBarSubmit' or 'ProblemEditorRunLocalButton'})
-- the run executes on the device: the first one loads the interpreter
local totals = device:assert_visible({id=console and 'ConsoleResultCounts' or 'ProblemEditorRunOutcomeTotals'}, {timeout=90000}).text or ""
log(totals)
-- every case, not just a totals line: "0 / 12 test cases passed" contains the word too
local got, of = totals:match("(%d+) / (%d+)")
assert_true(got and got == of, "the outcome totals do not show every case passed: " .. totals)
if console then choose_console('LeetcodeListConsoleOff') end
