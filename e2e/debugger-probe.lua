--[[---
platform: browser
against: export
features: leetcode
---]]
-- Debugger probe: a user types one line into the Two Sum editor, then resets it, with breakpoints on the Reset request and on
-- the confirm handler. The run passes when the typed line is gone and the editor is back at its starter code; its report and
-- debug.jsonl show where the app's code stopped, at original lines. Screenshots: edited, before-reset, reset-dialog, after-reset.
-- wf browser test e2e/debugger-probe.lua --env out=<dir> \
--   --logpoint features/leetcode/ui/ProblemEditorScreen.tsx:504 --logpoint features/leetcode/ui/ProblemEditorScreen.tsx:513
local out = env.out or ".agents/tmp/runs/debugger-probe-browser"
local base = env.base or "http://127.0.0.1:8800/mla-tamagui"
local EDIT = "# probe edit"
os.execute("mkdir -p " .. out)

local function buffer() local e = device:get_element({id='CodeEditorInput'}) return e and e.text or "" end

device:open_link(base .. "/")
device:assert_visible({id='LeetcodeListItem'})
device:scroll_until_visible{element={text='.*Two Sum.*'}, direction='DOWN'}
device:tap_on({text='.*Two Sum.*'})
device:assert_visible({id='ProblemEditorRunLocalButton'}, {timeout=240000})

-- the only way a user lowers the keyboard: Done at the end of the toolbar rail
local function done()
  device:scroll_until_visible{element={id='EditorAction-Done'}, direction='RIGHT', from={id='EditorActionToolbar'}}
  device:tap_on({id='EditorAction-Done'})
  device:assert_not_visible({id='EditorHelperRails'})
end

-- a user puts the caret at the end of the stub and types a line
device:tap_on({css='.cm-line:last-child'}) -- the editor opens focused with the caret at the top; a user taps the last line
device:assert_visible({id='EditorHelperRails'})
local stub = buffer()
assert_true(stub ~= "", "the editor is empty before the edit")
if (stub:match("([^\n]*)$") or ""):match("%S") then device:press_key('Enter') end
local before = #buffer()
device:input_text(EDIT)
device:extended_wait_until{condition=function() return #buffer() > before end, label='typed line landed'}
assert_true(buffer():find(EDIT, 1, true) ~= nil, "the typed line did not reach the editor")
device:take_screenshot(out .. "/edited.png")
done()

device:clear_logs()
device:take_screenshot(out .. "/before-reset.png")
device:tap_on({id='ProblemEditorResetButton'})
device:assert_visible({id='ResetCodeDialog-Confirm'})
device:take_screenshot(out .. "/reset-dialog.png")
device:tap_on({id='ResetCodeDialog-Confirm'})
device:assert_not_visible({id='ResetCodeDialog-Confirm'})
device:extended_wait_until{condition=function() return not buffer():find(EDIT, 1, true) end, timeout=3000, label='typed line gone'}
device:take_screenshot(out .. "/after-reset.png")
local after = buffer()
assert_true(after:find(EDIT, 1, true) == nil, "the typed line survived the reset")
assert_true(after ~= "" and #after <= #stub, "the editor is not back at its starter code")

local hits = device:get_debug_hits()
log(("debugger hits: %d"):format(#hits))
for _, h in ipairs(hits) do
  local top = h.frames and h.frames[1] or {}
  log(("hit %s:%s in %s at %s:%s"):format(h.file, h.line, tostring(h["function"]), tostring(top.file), tostring(top.line)))
end
assert_true(#device:get_logs() == 0, "JS warnings during the reset")
