--[[---
platform: browser
against: export
timeout: 180
focus: true
features: leetcode
---]]
-- The cursor handle on the phone editor: with the setting on, the action row above the keyboard starts with a pad, a drag on it moves the caret
-- (left and right by characters, up and down by lines) and the keyboard stays up, and the row still scrolls to its far keys with the pad in it.
-- The setting is chosen on the problem list, as a person does. Where the caret went is read from the buffer by typing a marker after each drag, and
-- only its direction and order are asserted, never how far: exact counts belong to the unit and flow tests and survive a retune of the feel.
-- wf browser test e2e/editor-cursor.lua --env base=http://127.0.0.1:8800/mla-tamagui
local base = env.base or "http://127.0.0.1:8800/mla-tamagui"
local out = env.out or ".agents/tmp/runs/editor-cursor"
os.execute("mkdir -p " .. out)
local function buffer() return (device:get_element({id='CodeEditorInput'}) or {}).text or "" end
local function bounds(sel) local el = device:get_element(sel); return el and el.bounds or nil end

local function logged(needle)
  local n = 0
  for _, l in ipairs(device:get_logs{level='info'}) do
    if l:find(needle, 1, true) then n = n + 1 end
  end
  return n
end

local function choose(id, tap_log)
  -- the static page paints its buttons before its scripts have hydrated them, and a tap before that is lost: wait for the first route.change
  device:extended_wait_until{condition=function() return logged('route.change') > 0 end, timeout=60000, interval=250, label='the page hydrated'}
  device:scroll_until_visible{element={id=id}, direction='DOWN', timeout=30000}
  local before = logged(tap_log)
  device:tap_on({id=id})
  -- the screen's own record that the press reached it: the button is there before and after the tap, so seeing it says nothing
  device:extended_wait_until{condition=function() return logged(tap_log) > before end, timeout=15000, interval=250, label='the tap reached the screen'}
end

run_flow("turn the cursor handle on from the list", function()
  device:clear_logs()
  device:open_link(base .. "/")
  choose('LeetcodeListCursorHandleOn', 'leetcode.list.cursor_handle_tap')
end)

run_flow("open the editor and bring the keyboard up", function()
  device:clear_logs()
  device:open_link(base .. "/leetcode/two-sum")
  device:assert_visible({id='ProblemEditorRunLocalButton'}, {timeout=60000})
  device:extended_wait_until{condition=function() return buffer():find("def twoSum", 1, true) ~= nil end, timeout=30000, label='the starter'}
  for _, marker in ipairs({'Q', 'W', 'X', 'Z', 'U', 'D'}) do
    assert_true(not buffer():find(marker, 1, true), "the starter already holds the marker " .. marker)
  end
  device:tap_on({css='.cm-line:last-child'})
  device:extended_wait_until{condition=function() return device:get_device_state('keyboard').visible end, timeout=15000, label='the keyboard is up'}
  device:assert_visible({id='EditorCursorHandle'}, {timeout=15000})
  device:take_screenshot(out .. "/rail-with-pad.png")
end)

-- Types one marker at the caret and waits to see that very character in the buffer.
local function mark(marker)
  device:input_text(marker)
  device:extended_wait_until{condition=function() return buffer():find(marker, 1, true) ~= nil end, timeout=10000, label='the marker ' .. marker}
end

-- One finger from the pad by (dx, dy): the drag has ended, and its last position reached the editor, when the screen logs it. The pad sits at the
-- screen's left edge, so a drag to the left starts at its right end and a short one, where a drag to the right and the vertical ones start at its centre.
local function drag_pad(dx, dy, label)
  local pad = bounds({id='EditorCursorHandle'})
  assert_true(pad ~= nil, "the handle has no bounds")
  local x, y = (pad.left + pad.right) // 2, (pad.top + pad.bottom) // 2
  if dx < 0 then x = pad.right - 4 end
  assert_true(x + dx > 0, label .. ": the drag would leave the screen")
  local before = logged('editor.caret_drag')
  device:swipe(('%d,%d'):format(x, y), ('%d,%d'):format(x + dx, y + dy), {duration=400})
  device:extended_wait_until{condition=function() return logged('editor.caret_drag') > before end, timeout=15000, interval=250, label=label .. ': the drag ended'}
  assert_true(device:get_device_state('keyboard').visible, label .. ": the keyboard went down with the drag")
end

local function at(marker) return buffer():find(marker, 1, true) end
local function line_of(marker)
  local b, i = buffer(), at(marker)
  return select(2, b:sub(1, i - 1):gsub("\n", ""))
end

run_flow("a drag to the left puts the caret before the last character, and a longer run of drags keeps going left", function()
  mark('Q')
  drag_pad(-40, 0, 'left once')
  mark('W')
  assert_true(at('W') < at('Q'), "the caret did not move left of Q")
  drag_pad(-40, 0, 'left twice')
  mark('X')
  assert_true(at('X') < at('W'), "the second drag did not carry the caret farther left")
  device:take_screenshot(out .. "/after-left.png")
end)

run_flow("a drag to the right puts the caret after the last marker", function()
  drag_pad(80, 0, 'right')
  mark('Z')
  assert_true(at('Z') > at('X'), "the caret did not move right of X")
end)

run_flow("a drag up goes to an earlier line and a drag down to a later one", function()
  local here = line_of('Z')
  drag_pad(0, -50, 'up')
  mark('U')
  assert_true(line_of('U') < here, ("the caret stayed on line %d after a drag up"):format(here))
  drag_pad(0, 25, 'down')
  mark('D')
  assert_true(line_of('D') > line_of('U'), "the caret did not go down from the line of U")
  device:take_screenshot(out .. "/after-lines.png")
end)

run_flow("the row still scrolls to its last key with the pad in it, and the swipe moves no caret", function()
  local drags, text = logged('editor.caret_drag'), buffer()
  device:scroll_until_visible{element={id='EditorAction-Done'}, direction='RIGHT', from={id='EditorActionToolbar'}}
  assert_true(logged('editor.caret_drag') == drags, "a swipe along the row started a caret drag")
  assert_true(buffer() == text, "a swipe along the row changed the document")
  device:assert_visible({id='EditorCursorHandle'})
  device:tap_on({id='EditorAction-Done'})
  device:assert_not_visible({id='EditorHelperRails'})
end)

run_flow("turn the cursor handle back off", function()
  device:clear_logs()
  device:open_link(base .. "/")
  choose('LeetcodeListCursorHandleOff', 'leetcode.list.cursor_handle_tap')
end)
