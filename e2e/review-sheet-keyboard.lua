--[[---
platform: android, browser
against: android=update, browser=export
timeout: 120
focus: true
features: leetcode
---]]
-- The voice-edit review sheet keeps its Send button above the keyboard. Typing an edit raises the keyboard over a sheet that sits at the bottom
-- of the screen: a Send that ends up behind the keyboard cannot be tapped (the voice-edit run on the CI emulator lost a chunk to exactly that:
-- the transcript took focus and the keyboard covered the sheet's lower half). No backend is needed: the text mode of the sheet is typed, not
-- recorded and transcribed. The browser's keyboard model (`wf browser`: the viewport shrinks on Android, an overlay on iOS) reproduces it.
-- wf browser test e2e/review-sheet-keyboard.lua --env base=http://127.0.0.1:8800/mla-tamagui
-- wf android test e2e/review-sheet-keyboard.lua --env base=exp://u.expo.dev/<projectId>/group/<group>/--
local android = device.platform == "android"
local base = env.base or (not android and "http://127.0.0.1:8800/mla-tamagui" or nil)
assert_true(base, "pass --env base=<update link>; wf test run does")
local out = env.out or (".agents/tmp/runs/review-sheet-keyboard-" .. device.platform)
os.execute("mkdir -p " .. out)

-- a reused app may still show this screen: a link to it remounts it a moment later, so the app leaves for Settings first
if android and device:get_element({id='ProblemEditorTextEditStartButton'}) then
  device:open_link(base .. "/settings")
  device:assert_not_visible({id='ProblemEditorTextEditStartButton'})
end
run_flow("open the editor", function()
  for _ = 1, 3 do
    device:open_link(base .. "/leetcode/two-sum")
    if device:extended_wait_until{visible={id='ProblemEditorTextEditStartButton'}, timeout=60000, optional=true} then break end
  end
  device:assert_visible({id='ProblemEditorTextEditStartButton'}, {timeout=5000})
end)

-- the editor may open focused with its keyboard over the action row: Done at the end of the toolbar lowers it, as it does for a person
if device:get_element({id='EditorHelperRails'}) and (not android or device:get_device_state('keyboard').visible) then
  device:scroll_until_visible{element={id='EditorAction-Done'}, direction='RIGHT', from={id='EditorActionToolbar'}}
  device:tap_on({id='EditorAction-Done'})
  device:assert_not_visible({id='EditorHelperRails'})
end

run_flow("type an edit", function()
  device:tap_on({id='ProblemEditorTextEditStartButton'})
  device:extended_wait_until{condition=function() return device:get_element({id='ProblemEditorVoiceReviewSheet-TranscriptInput'}) ~= nil end, timeout=15000, label='the sheet opened'}
  -- the sheet takes the keyboard down when it opens; a person taps the field to type
  device:tap_on({id='ProblemEditorVoiceReviewSheet-TranscriptInput'})
  device:extended_wait_until{condition=function() return device:get_device_state('keyboard').visible end, timeout=8000, label='the keyboard rose over the sheet'}
end)

-- everything the person needs to finish the edit is above the keyboard
local kb = device:get_device_state('keyboard')
device:take_screenshot(out .. "/sheet-with-keyboard.png")
assert_true(kb.bounds ~= nil, "the keyboard reports no bounds")
local function above(id)
  local e = device:get_element({id=id})
  assert_true(e ~= nil, id .. " is not on screen")
  return e.bounds.bottom <= kb.bounds.top + 2, e.bounds.bottom
end
local ok_input, input_bottom = above('ProblemEditorVoiceReviewSheet-TranscriptInput')
local ok_send, send_bottom = above('ProblemEditorVoiceReviewSheet-Send')
log(("keyboard top %d, transcript input bottom %d, Send bottom %d"):format(kb.bounds.top, input_bottom, send_bottom))
assert_true(ok_input, "the transcript field is behind the keyboard")
assert_true(ok_send, ("the Send button is behind the keyboard (bottom %d, keyboard top %d)"):format(send_bottom, kb.bounds.top))
log("review sheet keyboard: pass")
