--[[---
platform: browser
against: browser=export
timeout: 180
features: leetcode
---]]
-- The cursor handle on the phone editor: with the setting on, the action row above the keyboard starts with a pad, a drag on it moves the caret
-- (left and right by characters, up and down by lines) and the keyboard stays up, and the row still scrolls to its far keys with the pad in it.
-- The setting is chosen on the problem list, as a person does, and put back off at the end, so a later unit on the same phone finds the default.
-- Where the caret went is read from the buffer by typing a marker after each drag, and only its direction and order are asserted, never how
-- far: exact counts belong to the unit and flow tests and survive a retune of the feel. On the phone this is the one automated check that a
-- finger on a native view over the soft keyboard leaves the editor focused (the keyboard stays up through the drag), and that the pad is in the
-- accessibility tree (a screen reader reaches it).
-- wf browser test e2e/editor-cursor.lua --env base=http://127.0.0.1:8800/mla-tamagui
-- wf android test e2e/editor-cursor.lua --env base=exp://u.expo.dev/<projectId>/group/<group>/--
local android = device.platform == "android"
local base = env.base or (not android and "http://127.0.0.1:8800/mla-tamagui" or nil)
assert_true(base, "pass --env base=<update link>; wf test run does")
local out = env.out or (".agents/tmp/runs/editor-cursor-" .. device.platform)
os.execute("mkdir -p " .. out)

-- Android reads the app's own copy of the document (CodeEditorText), as editor-typing does
local function buffer() return (device:get_element({id = android and 'CodeEditorText' or 'CodeEditorInput'}) or {}).text or "" end
local function bounds(sel) local el = device:get_element(sel); return el and el.bounds or nil end

local function logged(needle)
  local n = 0
  for _, l in ipairs(device:get_logs{level='debug'}) do
    if l:find(needle, 1, true) then n = n + 1 end
  end
  return n
end

-- The screen is 360 dp wide at any density, and a drag is read in dp: a length in dp is that many device pixels times SW / 360 (1 in the browser)
local SW = 360
if android then SW = device:get_device_state('screen').width end
local DP = SW / 360
local function px(dp) return math.floor(dp * DP + 0.5) end

-- Expo Go bundles a route on first load; a link that lands elsewhere is opened again
local function open(path, id, secs)
  local t0 = os.time()
  while os.time() - t0 < (secs or 120) do
    device:open_link(base .. path)
    if device:extended_wait_until{visible={id=id}, timeout=15000, optional=true} then return end
    -- a fresh install shows Expo Go's one-time developer-menu intro, which a person dismisses with Continue
    if device:get_element({text='.*Continue.*'}) then device:tap_on({text='.*Continue.*'}) end
  end
  device:assert_visible({id=id}, {timeout=5000, label='route ready'})
end

-- a link to the route the app already shows remounts it a moment later, and a link to "/" does not leave a running Expo Go's editor: the app leaves for Settings first
local function leave_editor()
  if android and device:get_element({id='ProblemEditorRunLocalButton'}) then
    device:open_link(base .. "/settings")
    device:assert_not_visible({id='ProblemEditorRunLocalButton'})
  end
end

-- a route by link: Expo Go bundles it on first load and opens it again if the link landed elsewhere; the browser loads it once
local function go(path, id, secs)
  if android then
    open(path, id, secs)
  else
    device:open_link(base .. path)
    device:assert_visible({id=id}, {timeout=60000})
  end
end

local function choose(id, tap_log)
  leave_editor()
  device:clear_logs()
  go("/", 'LeetcodeListRunnerRow')
  if not android then
    -- the static page paints its buttons before its scripts have hydrated them, and a tap before that is lost: wait for the first route.change
    device:extended_wait_until{condition=function() return logged('route.change') > 0 end, timeout=60000, interval=250, label='the page hydrated'}
  end
  device:scroll_until_visible{element={id=id}, direction='DOWN'}
  local before = logged(tap_log)
  device:tap_on({id=id})
  -- the screen's own record that the press reached it: the button is there before and after the tap, so seeing it says nothing; the phone's
  -- console is not a promised channel, so there the next flow's handle is the proof
  if not android then
    device:extended_wait_until{condition=function() return logged(tap_log) > before end, timeout=15000, interval=250, label='the tap reached the screen'}
  end
end

run_flow("turn the cursor handle on from the list", function()
  choose('LeetcodeListCursorHandleOn', 'leetcode.list.cursor_handle_tap')
end)

local function keyboard_up() return device:get_device_state('keyboard').visible end

run_flow("open the editor and bring the keyboard up", function()
  device:clear_logs()
  go("/leetcode/two-sum", 'ProblemEditorRunLocalButton', 240)
  device:extended_wait_until{condition=function() return buffer():find("def twoSum", 1, true) ~= nil end, timeout=30000, label='the starter'}
  for _, marker in ipairs({'Q', 'W', 'X', 'Z', 'U', 'D'}) do
    assert_true(not buffer():find(marker, 1, true), "the starter already holds the marker " .. marker)
  end
  if android then
    -- the editor opens unfocused on the phone: a tap in it raises the keyboard and the rail, and lands mid-document
    for _ = 1, 3 do
      device:tap_on({point=('%d,%d'):format(px(180), px(220))})
      if device:extended_wait_until{visible={id='EditorHelperRails'}, timeout=4000, optional=true} then break end
    end
    device:press_key('Ctrl+End')
  else
    device:tap_on({css='.cm-line:last-child'})
  end
  device:extended_wait_until{condition=keyboard_up, timeout=15000, label='the keyboard is up'}
  device:assert_visible({id='EditorCursorHandle'}, {timeout=15000})
  device:take_screenshot(out .. "/rail-with-pad.png")
end)

-- Types one marker at the caret and waits to see that very character in the buffer.
local function mark(marker)
  device:input_text(marker)
  device:extended_wait_until{condition=function() return buffer():find(marker, 1, true) ~= nil end, timeout=10000, label='the marker ' .. marker}
  assert_true(keyboard_up(), "the keyboard went down after typing " .. marker)
end

-- One finger from the pad by (dx, dy) dp: the drag has ended, and its last position reached the editor, when the screen logs it. The pad sits at the
-- screen's left edge, so a drag to the left starts at its right end and a short one, where a drag to the right and the vertical ones start at its centre.
local function drag_pad(dx, dy, label)
  local pad = bounds({id='EditorCursorHandle'})
  assert_true(pad ~= nil, "the handle has no bounds")
  local x, y = (pad.left + pad.right) // 2, (pad.top + pad.bottom) // 2
  if dx < 0 then x = pad.right - px(4) end
  assert_true(x + px(dx) > 0, label .. ": the drag would leave the screen")
  local before = logged('editor.caret_drag')
  device:swipe(('%d,%d'):format(x, y), ('%d,%d'):format(x + px(dx), y + px(dy)), {duration=400})
  device:extended_wait_until{condition=function() return logged('editor.caret_drag') > before end, timeout=15000, interval=250, label=label .. ': the drag ended'}
  assert_true(keyboard_up(), label .. ": the keyboard went down with the drag")
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
  if android then
    -- a tap on a row still moving after its swipe stops the row instead of pressing Done: a user taps again
    for _ = 1, 3 do
      device:tap_on({id='EditorAction-Done'})
      if device:extended_wait_until{condition=function() return not keyboard_up() end, timeout=3000, optional=true} then break end
    end
    assert_true(not keyboard_up(), "Done did not put the keyboard away")
  else
    device:tap_on({id='EditorAction-Done'})
    device:assert_not_visible({id='EditorHelperRails'})
  end
end)

run_flow("turn the cursor handle back off", function()
  choose('LeetcodeListCursorHandleOff', 'leetcode.list.cursor_handle_tap')
end)
