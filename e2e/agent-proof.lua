-- Agent proof, the scripted floor: the task `wf bench agent-proof` hands an agent, written by hand.
-- wf browser test e2e/agent-proof.lua --env base=http://127.0.0.1:8810/mla-tamagui
local base = env.base or "http://127.0.0.1:8810/mla-tamagui"

device:open_link(base .. "/leetcode/two-sum")
device:assert_visible({id='ProblemEditorRunLocalButton'})
-- the editor autofocuses and its keyboard covers Run Local: Done ends the toolbar's action row, off screen until it scrolls
if device:extended_wait_until{visible={id='EditorHelperRails'}, timeout=3000, optional=true} then
  device:scroll_until_visible{element={id='EditorAction-Done'}, direction='RIGHT', from={id='EditorActionToolbar'}}
  device:tap_on({id='EditorAction-Done'})
  device:assert_not_visible({id='EditorHelperRails'})
end
device:tap_on({id='ProblemEditorRunLocalButton'})
local totals = device:assert_visible({id='ProblemEditorRunOutcomeTotals'}).text or ""
log(totals)
assert_true(totals:find("passed", 1, true), "the outcome totals do not say passed: " .. totals)
