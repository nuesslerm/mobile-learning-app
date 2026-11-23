--[[---
platform: browser
against: browser=export
timeout: 120
features: leetcode
---]]
-- Paused: voice and AI edit entry points are flagged off (domain/featureFlags.ts). Add focus: true and bake EXPO_PUBLIC_LEETCODE_AI_EDIT=1 into the export or update to run it again.
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

-- the editor may open focused with its keyboard over the action row, and a slow page focuses it late (the hosted browser run tapped the
-- sheet's button under a keyboard that rose a moment after the check): the web editor always autofocuses, so it is waited for, then
-- lowered as a person would, with a tap on the page outside it, aimed at the part of the language row the rail does not cover
if android then
  if device:get_device_state('keyboard').visible then
    device:scroll_until_visible{element={id='EditorAction-Done'}, direction='RIGHT', from={id='EditorActionToolbar'}}
    device:tap_on({id='EditorAction-Done'})
    device:assert_not_visible({id='EditorHelperRails'})
  end
elseif device:extended_wait_until{visible={id='EditorHelperRails'}, timeout=20000, optional=true} then
  local row, rail = device:get_element({id='ProblemEditorDiffRow'}).bounds, device:get_element({id='EditorHelperRails'}).bounds
  device:tap_on({point=('%d,%d'):format((row.left + row.right) // 2, (row.top + math.min(row.bottom, rail.top - 4)) // 2)})
  device:assert_not_visible({id='EditorHelperRails'})
end
device:clear_logs()

run_flow("type an edit", function()
  device:tap_on({id='ProblemEditorTextEditStartButton'})
  device:extended_wait_until{condition=function() return device:get_element({id='ProblemEditorVoiceReviewSheet-TranscriptInput'}) ~= nil end, timeout=15000, label='the sheet opened'}
  -- the sheet takes the keyboard down when it opens; a person taps the field to type, and taps it again when the first tap did not take (a tap
  -- on a sheet that is still settling can land beside the field: one hosted browser run never got its keyboard)
  local function keyboard_up() local k = device:get_device_state('keyboard') return k.visible and k.bounds ~= nil end
  for _ = 1, 4 do
    device:tap_on({id='ProblemEditorVoiceReviewSheet-TranscriptInput'})
    -- Android's AOSP keyboard asks once, the first time it serves a text field, to read the contacts, and the keyboard stays down until the
    -- dialog is answered; a person answers it, and the field is tapped again
    if android and device:extended_wait_until{visible={text='.*access your contacts.*'}, timeout=2000, optional=true} then
      device:tap_on({text='.*DON.T ALLOW.*'})
      device:tap_on({id='ProblemEditorVoiceReviewSheet-TranscriptInput'})
    end
    -- the state flips before the keyboard is drawn, so its rectangle is waited for too
    if device:extended_wait_until{condition=keyboard_up, timeout=3000, optional=true} then break end
  end
  device:extended_wait_until{condition=keyboard_up, timeout=8000, label='the keyboard rose over the sheet'}
end)

local ok, failure = pcall(function()
  -- everything the person needs to finish the edit is above the keyboard
  local kb = device:get_device_state('keyboard')
  device:take_screenshot(out .. "/sheet-with-keyboard.png")
  assert_true(kb.bounds ~= nil, "the keyboard reports no bounds")
  local function above(id)
    local e = device:get_element({id=id})
    assert_true(e ~= nil, id .. " is not on screen")
    return e.bounds.bottom <= kb.bounds.top + 2, e.bounds.bottom
  end
  -- the sheet itself has risen above the keyboard: the design-system Sheet lifts itself by the keyboard's height (the same code on web and on a
  -- phone), so a sheet that stops doing it leaves its bottom edge behind the keyboard here, whatever room the content leaves above it
  if not android then
    local function sheet_bottom() local s = device:get_element({id='ProblemEditorVoiceReviewSheet'}) return s and s.bounds.bottom end
    device:extended_wait_until{condition=function() local b = sheet_bottom() return b ~= nil and b <= kb.bounds.top + 2 end, timeout=5000, optional=true, label='the sheet rose above the keyboard'}
    local b = sheet_bottom()
    assert_true(b ~= nil and b <= kb.bounds.top + 2, ("the sheet did not rise above the keyboard (bottom %s, keyboard top %d)"):format(tostring(b), kb.bounds.top))
  end
  device:take_screenshot(out .. "/sheet-lifted.png")
  local ok_input, input_bottom = above('ProblemEditorVoiceReviewSheet-TranscriptInput')
  local ok_send, send_bottom = above('ProblemEditorVoiceReviewSheet-Send')
  log(("keyboard top %d, transcript input bottom %d, Send bottom %d"):format(kb.bounds.top, input_bottom, send_bottom))
  assert_true(ok_input, "the transcript field is behind the keyboard")
    assert_true(ok_send, ("the Send button is behind the keyboard (bottom %d, keyboard top %d)"):format(send_bottom, kb.bounds.top))
  -- and the sheet's own top is on screen: a sheet lifted by the keyboard's height must not put its handle and its Close button above the screen
  local close = device:get_element({id='ProblemEditorVoiceReviewSheet-Close'})
  assert_true(close ~= nil and close.bounds.top >= 0, "the sheet's Close button is not on screen once the sheet is lifted: " .. tostring(close and close.bounds.top))
  -- a warning in the page's console fails every browser run (gorhom's own input logged one on every focus in react-native-web)
  local warnings = device:get_logs()
  assert_true(#warnings == 0, "JS warnings with the sheet open: " .. tostring(warnings[1]))
  log("review sheet keyboard: pass")
end)
-- the next test on this device finds the app as it was: the sheet closed and the keyboard down (the keyboard this test raised was left up, and
-- the stubbed-run-local that follows it on the hosted Android shard then saw the rail of a keyboard that was gone)
pcall(function()
  if device:get_element({id='ProblemEditorVoiceReviewSheet-Close'}) then device:tap_on({id='ProblemEditorVoiceReviewSheet-Close'}) end
  device:extended_wait_until{condition=function() return not device:get_device_state('keyboard').visible end, timeout=5000, optional=true}
  if android and device:get_device_state('keyboard').visible then device:press_key('Back') end
end)
if not ok then error(failure, 0) end
