--[[---
platform: browser
against: export
timeout: 180
focus: true
features: leetcode
---]]
-- Suggestions on the phone editor: with the setting on, typing `ret` opens a list beside the caret line in rows a finger can hit, a tap on the row
-- takes it, the Indent key takes it too, and the keyboard stays up through both. The setting is chosen on the problem list, as a person does,
-- and is read back from there when the editor opens. editor-typing keeps running with the setting off, which is its regression guard: a
-- binding that took Return would eat the newlines it types.
-- wf browser test e2e/editor-completion.lua --env base=http://127.0.0.1:8800/mla-tamagui
local base = env.base or "http://127.0.0.1:8800/mla-tamagui"
local out = env.out or ".agents/tmp/runs/editor-completion"
os.execute("mkdir -p " .. out)
local function buffer() return (device:get_element({id='CodeEditorInput'}) or {}).text or "" end
local function bounds(sel) local el = device:get_element(sel); return el and el.bounds or nil end

local function logged(level, needle)
  for _, l in ipairs(device:get_logs{level=level}) do
    if l:find(needle, 1, true) then return true end
  end
  return false
end

run_flow("turn suggestions on from the list", function()
  device:open_link(base .. "/")
  device:assert_visible({id='LeetcodeListAutocompleteOn'}, {timeout=60000})
  -- the static page paints its buttons before its scripts have hydrated them, and a tap before that is lost: wait for the first route.change
  device:extended_wait_until{condition=function() return logged('info', 'route.change') end, timeout=60000, interval=250, label='the page hydrated'}
  device:tap_on({id='LeetcodeListAutocompleteOn'})
  -- the screen's own record that the press reached it: the button is there before and after the tap, so seeing it says nothing
  device:extended_wait_until{condition=function() return logged('info', 'leetcode.list.autocomplete_tap') end, timeout=15000, interval=250, label='the tap reached the screen'}
end)

run_flow("open the editor and place the caret in the method body", function()
  device:open_link(base .. "/leetcode/two-sum")
  device:assert_visible({id='ProblemEditorRunLocalButton'}, {timeout=60000})
  device:extended_wait_until{condition=function() return buffer():find("def twoSum", 1, true) ~= nil end, timeout=30000, label='the starter'}
  device:tap_on({css='.cm-line:last-child'})
  device:extended_wait_until{condition=function() return device:get_device_state('keyboard').visible end, timeout=15000, label='the keyboard is up'}
  -- the page is told the setting after it has hydrated, and the list only opens for a key typed once it knows: its own mark that it does
  device:extended_wait_until{condition=function() return device:get_element({css='.cm-content[aria-autocomplete]'}) ~= nil end, timeout=15000, interval=250, label='the editor has suggestions on'}
end)

local function type_prefix()
  device:input_text('ret')
  device:extended_wait_until{condition=function() return device:get_element({css='.cm-tooltip-autocomplete'}) ~= nil end, timeout=15000, label='the suggestion list'}
end

run_flow("the list opens clear of the caret line in rows a finger can hit", function()
  type_prefix()
  device:take_screenshot(out .. "/list-open.png")
  local popup = bounds({css='.cm-tooltip-autocomplete'})
  local row = bounds({css='.cm-tooltip-autocomplete li'})
  local line = bounds({css='.cm-line:last-child'})
  local rail = bounds({id='EditorSyntaxToolbar'})
  assert_true(popup ~= nil and row ~= nil and line ~= nil, "the list, its first row or the caret line has no bounds")
  assert_true(row.bottom - row.top >= 40, ("a row is %d px high, a finger needs 40"):format(row.bottom - row.top))
  -- above the caret when the box has room above it, below when the caret sits near the top of the box (two-sum's does): either way the line being typed stays visible
  assert_true(popup.bottom <= line.top + 4 or popup.top >= line.bottom - 4, ("the list (%d to %d) covers the caret line (%d to %d)"):format(popup.top, popup.bottom, line.top, line.bottom))
  assert_true(popup.left >= 0, "the list starts left of the screen")
  if rail then assert_true(popup.bottom <= rail.top, "the list overlaps the toolbar") end
end)

run_flow("a tap on the return row takes it and the keyboard stays up", function()
  device:tap_on({css='.cm-tooltip-autocomplete li:first-child'})
  device:extended_wait_until{condition=function() return buffer():match("return%s*$") ~= nil end, timeout=15000, label='return in the buffer'}
  assert_true(device:get_device_state('keyboard').visible, "the keyboard went down with the tap")
  device:extended_wait_until{condition=function() return device:get_element({css='.cm-tooltip-autocomplete'}) == nil end, timeout=10000, label='the list is gone'}
  device:take_screenshot(out .. "/after-tap.png")
end)

run_flow("the Indent key takes a suggestion too", function()
  for _ = 1, 6 do device:press_key('Backspace') end
  device:extended_wait_until{condition=function() return buffer():match("return%s*$") == nil end, timeout=10000, label='return deleted'}
  type_prefix()
  local key = device:scroll_until_visible{element={id='EditorSyntax-Tab'}, direction='RIGHT', from={id='EditorSyntaxToolbar'}, timeout=8000, optional=true}
    or device:scroll_until_visible{element={id='EditorSyntax-Tab'}, direction='LEFT', from={id='EditorSyntaxToolbar'}, timeout=8000}
  assert_true(key ~= nil, "the Indent key is not on the toolbar")
  local before = buffer()
  device:tap_on({id='EditorSyntax-Tab'})
  device:extended_wait_until{condition=function() return buffer():match("return%s*$") ~= nil end, timeout=15000, label='return in the buffer after Indent'}
  assert_true(not buffer():match("ret%s+return"), "the key indented as well as taking the suggestion")
  assert_true(#buffer() - #before == 3, ("the key added %d characters, 3 expected (urn)"):format(#buffer() - #before))
end)

local warnings = {}
for _, line in ipairs(device:get_logs()) do
  if not (line:find("Response body loading was aborted", 1, true) or line:find("falling back to ArrayBuffer instantiation", 1, true)) then warnings[#warnings + 1] = line end
end
assert_true(#warnings == 0, "JS warning: " .. tostring(warnings[1]))
