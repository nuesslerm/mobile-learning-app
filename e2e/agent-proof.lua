-- Agent proof, the scripted floor: the task `wf bench agent-proof` hands an agent, written by hand.
-- wf browser test e2e/agent-proof.lua --env base=http://127.0.0.1:8810/mla-tamagui
-- Submit runs every case on the console and its Run Result tab holds the tally.
local base = env.base or "http://127.0.0.1:8810/mla-tamagui"

local function logged(needle)
  for _, l in ipairs(device:get_logs{level='info'}) do
    if l:find(needle, 1, true) then return true end
  end
  return false
end


device:open_link(base .. "/leetcode/two-sum")
device:assert_visible({id='ProblemEditorScreen'}, {timeout=60000})
-- the editor autofocuses and its keyboard covers the console dock: Done ends the toolbar's action row, off screen until it scrolls
if device:extended_wait_until{visible={id='EditorHelperRails'}, timeout=3000, optional=true} then
  device:scroll_until_visible{element={id='EditorAction-Done'}, direction='RIGHT', from={id='EditorActionToolbar'}}
  device:tap_on({id='EditorAction-Done'})
  device:assert_not_visible({id='EditorHelperRails'})
end
device:tap_on({id='ConsoleBarSubmit'})
-- the run executes on the device: the first one loads the interpreter
local totals = device:assert_visible({id='ConsoleResultCounts'}, {timeout=90000}).text or ""
log(totals)
-- every case, not just a totals line: "0 / 12 test cases passed" contains the word too
local got, of = totals:match("(%d+) / (%d+)")
assert_true(got and got == of, "the outcome totals do not show every case passed: " .. totals)
