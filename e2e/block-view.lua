--[[---
platform: browser
against: export
timeout: 420
focus: true
features: leetcode
---]]
-- The block view with a real touch (issue 86, F3.10): turn "Blocks (experimental)" on from the problem list, open two-sum, open its Blocks tab with a body that has an if holding two statements, and drag with a
-- touch swipe: the grip of the second statement to the boundary after the if (it leaves the if), the "while" chip to the end of the def body (a new statement lands there), and a block to a
-- boundary that would leave it where it is (nothing changes). Every wait is on a log line or a read, never on a clock: leetcode.block_view.set, route.change, leetcode.blocks.read,
-- leetcode.blocks.drop, leetcode.blocks.drag_end. The run records itself (a video and the swipes, published by `wf test publish`); no run reaches a backend.
-- wf test run --platform browser block-view   (wf browser test e2e/block-view.lua --env base=http://127.0.0.1:8800/mla-tamagui)
local base = env.base or "http://127.0.0.1:8800/mla-tamagui"
local out = env.out or ".agents/tmp/runs/block-view"
os.execute("mkdir -p " .. out)

-- the layout of domain/blocks/blockLayout.ts: what is drawn and what the planner reads are one geometry
local ROW_HEIGHT, INDENT_STEP, GUTTER, PALETTE_HEIGHT, CHIP_WIDTH = 44, 24, 44, 44, 72

local function base64url(s)
  local abc, out_ = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_", {}
  for i = 1, #s, 3 do
    local a, b, c = s:byte(i, i + 2)
    local n = a * 65536 + (b or 0) * 256 + (c or 0)
    for k = 1, 2 + (b and 1 or 0) + (c and 1 or 0) do
      local idx = math.floor(n / 64 ^ (4 - k)) % 64
      out_[#out_ + 1] = abc:sub(idx + 1, idx + 1)
    end
  end
  return table.concat(out_)
end

-- rows: class 0, def 1, if 2, a 3, b 4, return 5
local CODE = table.concat({
  "class Solution:",
  "    def twoSum(self, nums, target):",
  "        if nums:",
  "            a = 1",
  "            b = 2",
  "        return a",
  "",
}, "\n")

local function logged(needle, level)
  for _, l in ipairs(device:get_logs{level=level or 'info'}) do
    if l:find(needle, 1, true) then return true end
  end
  return false
end

local function count_logged(needle)
  local n = 0
  for _, l in ipairs(device:get_logs{level='info'}) do
    if l:find(needle, 1, true) then n = n + 1 end
  end
  return n
end

-- the editor is on the Code tab: read it there and come back to Blocks (never inside a wait condition)
local function buffer()
  device:tap_on({id='ProblemWorkspaceTab-code'})
  device:assert_visible({id='CodeEditorInput'}, {timeout=15000})
  local text = (device:get_element({id='CodeEditorInput'}) or {}).text or ""
  device:tap_on({id='ProblemWorkspaceTab-blocks'})
  device:assert_visible({id='BlockRow-1'}, {timeout=15000})
  return text
end
local function bounds(id) local e = device:get_element({id=id}); return e and e.bounds or nil end

local function choose_blocks()
  device:clear_logs()
  device:open_link(base .. "/blind75")
  -- the static page paints its buttons before its scripts have hydrated them, and a tap before that is lost: the first route.change is awaited
  device:extended_wait_until{condition=function() return logged('route.change') end, timeout=60000, interval=250, label='the page hydrated'}
  -- the setting rows run below the first screen of the list
  device:scroll_until_visible{element={id='LeetcodeListBlockViewOn'}, direction='DOWN', timeout=30000}
  device:tap_on({id='LeetcodeListBlockViewOn'})
  device:extended_wait_until{condition=function() return logged('leetcode.block_view.set') end, timeout=15000, interval=250, label='the setting reached the store'}
end

-- the web editor autofocuses and its keyboard covers the screen: the rail's Done key lowers it, on the Code tab
local function lower_keyboard()
  device:extended_wait_until{visible={id='EditorHelperRails'}, timeout=15000, optional=true}
  if device:get_element({id='EditorHelperRails'}) then
    device:scroll_until_visible{element={id='EditorAction-Done'}, direction='RIGHT', from={id='EditorActionToolbar'}}
    device:tap_on({id='EditorAction-Done'})
    device:assert_not_visible({id='EditorHelperRails'})
  end
  assert_true(not device:get_device_state('keyboard').visible, "the keyboard is still up")
end

-- opens two-sum with `code` in the editor, the rows drawn, the keyboard lowered and the log empty
local function open_with(code, last_row)
  if device:get_element({id='ProblemEditorScreen'}) then
    device:open_link(base .. "/settings")
    device:assert_not_visible({id='ProblemEditorScreen'})
  end
  device:clear_logs()
  device:open_link(base .. "/leetcode/two-sum?seed64=" .. base64url(code))
  device:assert_visible({id='ProblemEditorScreen'}, {timeout=60000})
  device:extended_wait_until{condition=function() return logged('route.change') end, timeout=60000, interval=250, label='the page hydrated'}
  lower_keyboard()
  -- the block view is the Blocks tab of the workspace: the editor stays mounted on the Code tab
  device:tap_on({id='ProblemWorkspaceTab-blocks'})
  device:extended_wait_until{condition=function() return device:get_element({id='BlockRow-' .. last_row}) ~= nil end, timeout=60000, label='the rows are drawn'}
  -- a long list scrolls inside the tab: it is scrolled until the last row is on screen
  device:scroll_until_visible{element={id='BlockRow-' .. last_row}, direction='DOWN', timeout=30000}
  device:clear_logs()
end

-- the screen px of one css px: the rows are ROW_HEIGHT high in css px, whatever the device frame scales them by
local function scale()
  local r0, r1 = bounds('BlockRow-0'), bounds('BlockRow-1')
  assert_true(r0 and r1, "the first two rows have no bounds")
  return (r1.top - r0.top) / ROW_HEIGHT
end

-- a point of the list, in css px from its top left corner, as the screen sees it
local function list_point(x, y)
  local top = bounds('BlockRow-0')
  local s = scale()
  return ('%d,%d'):format(math.floor(top.left + x * s + 0.5), math.floor(top.top + y * s + 0.5))
end

local function center_of(id)
  local b = bounds(id)
  assert_true(b, id .. " has no bounds")
  return ('%d,%d'):format((b.left + b.right) // 2, (b.top + b.bottom) // 2)
end

-- where the finger is when it is at the slot of (gap, depth): the row boundary and the indent the planner names
local function slot_point(gap, depth) return list_point(GUTTER + depth * INDENT_STEP, gap * ROW_HEIGHT) end

local function assert_clean_log()
  local warnings = device:get_logs()
  assert_true(#warnings == 0, "JS warnings: " .. table.concat(warnings, " | "))
end

local ok, failure = pcall(function()
  run_flow("turn Blocks on from the problem list", choose_blocks)

  run_flow("the rows are as tall as the plan says", function()
    open_with(CODE, 5)
    local r0, r1, r5 = bounds('BlockRow-0'), bounds('BlockRow-1'), bounds('BlockRow-5')
    assert_true(r0 and r1 and r5, "a row has no bounds")
    local step = (r5.top - r0.top) / 5
    assert_true(math.abs(step - (r1.top - r0.top)) < 1.5, ("the rows are not evenly spaced: %.1f then %.1f"):format(r1.top - r0.top, step))
    -- the plan's geometry is css px and this frame draws them one to one: a row that wrapped or was scaled would move every slot the planner names
    assert_true(math.abs(step - ROW_HEIGHT) < 1.5, ("a row is %.1f px high, the plan says %d"):format(step, ROW_HEIGHT))
    log(json.encode({ scale = scale(), step = step }))
    device:take_screenshot(out .. "/rows.png")
  end)

  run_flow("a touch swipe on a grip moves the statement out of the if", function()
    -- b = 2 is row 4; the boundary after the if is below row 4 (gap 5), at the depth of the if (2)
    device:swipe(center_of('BlockHandle-4'), slot_point(5, 2), {duration=900})
    device:extended_wait_until{condition=function() return logged('leetcode.blocks.drag_begin') end, timeout=10000, interval=250, label='the pan took the touch'}
    device:extended_wait_until{condition=function() return logged('leetcode.blocks.drop') end, timeout=20000, interval=250, label='the drop was made'}
    assert_true(logged('outcome: "moved"'), "the drop did not move")
    device:extended_wait_until{condition=function() return count_logged('leetcode.blocks.read') >= 1 end, timeout=15000, interval=250, label='the rows were read again'}
    local moved = buffer()
    assert_true(moved:find("\n        b = 2", 1, true) ~= nil, "b = 2 is not at the if's indent: " .. moved)
    assert_true(moved:find("\n            b = 2", 1, true) == nil, "b = 2 is still inside the if")
    device:take_screenshot(out .. "/moved.png")
    assert_clean_log()
  end)

  run_flow("a touch swipe on a chip adds a statement at the end of the def body", function()
    -- the while chip is the third of the palette; the end of the def body is below the last row, at the depth of the def's statements (2)
    local rows_before = 6
    device:swipe(center_of('BlockPaletteItem-while'), slot_point(rows_before, 2), {duration=900})
    device:extended_wait_until{visible={id='BlockRow-7'}, timeout=15000, label='the two new rows are drawn'}
    local inserted = buffer()
    assert_true(inserted:find("\n        while condition:\n            pass", 1, true) ~= nil, "the snippet is not at the def body's indent: " .. inserted)
    device:take_screenshot(out .. "/inserted.png")
    device:assert_screenshot("block-view")
    assert_clean_log()
  end)

  run_flow("a swipe to a boundary that would leave the block where it is changes nothing", function()
    local before = buffer()
    device:clear_logs()
    -- b = 2 is row 4 again (a, then b, now at the def's depth): the boundary right below it is its own place
    device:swipe(center_of('BlockHandle-4'), slot_point(5, 2), {duration=900})
    device:extended_wait_until{condition=function() return logged('leetcode.blocks.drag_end') end, timeout=20000, interval=250, label='the drag ended'}
    assert_true(not logged('leetcode.blocks.drop'), "a drop was made")
    assert_true(buffer() == before, "the buffer changed")
    assert_clean_log()
  end)
end)

if not ok then error(failure, 0) end
log("block view: pass")
