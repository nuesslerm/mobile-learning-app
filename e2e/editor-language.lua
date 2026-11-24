--[[---
platform: browser
against: export
timeout: 180
focus: true
features: leetcode
---]]
-- The editor follows the screen's language: the problem is switched to JavaScript through the picker, the lines a person types in the method
-- body read back whole, and the toolbar's Comment key writes the JavaScript marker (`//`, where Python's key writes `#`). That key reads the
-- language the screen gave the toolbar, so a marker of the right kind is the proof that the language reached the page and the toolbar.
-- wf browser test e2e/editor-language.lua --env base=http://127.0.0.1:8800/mla-tamagui
local base = env.base or "http://127.0.0.1:8800/mla-tamagui"
local out = env.out or ".agents/tmp/runs/editor-language"
os.execute("mkdir -p " .. out)
local function buffer() return (device:get_element({id='CodeEditorInput'}) or {}).text or "" end
local READY = 'ProblemEditorScreen'

run_flow("open the editor", function()
  device:open_link(base .. "/leetcode/two-sum")
  device:assert_visible({id=READY}, {timeout=60000})
  device:extended_wait_until{condition=function() return buffer():find("def twoSum", 1, true) ~= nil end, timeout=30000, label='the Python starter'}
  -- the static page paints its buttons before its scripts have hydrated them, and a tap before that is lost: wait for the first route.change
  device:extended_wait_until{condition=function()
    for _, l in ipairs(device:get_logs{level='info'}) do
      if l:find('route.change', 1, true) then return true end
    end
  end, timeout=60000, interval=250, label='the page hydrated'}
end)

-- the symbols rail is the default and this script presses the classic character keys: Classic is chosen in the Symbol Bar sheet, as a person does
local function choose_classic_rail()
  device:clear_logs()
  device:tap_on({id='EditorSettingsButton'})
  device:assert_visible({id='EditorSettingsButtonContent'}, {timeout=10000})
  device:tap_on({id='EditorSettingsSymbolBar'})
  device:assert_visible({id='SymbolBarSheetHeader'}, {timeout=10000})
  device:scroll_until_visible{element={id='SymbolBarRailStyle-classic'}, direction='DOWN', timeout=10000, optional=true}
  device:tap_on({id='SymbolBarRailStyle-classic'})
  device:extended_wait_until{condition=function()
    for _, l in ipairs(device:get_logs{level='info'}) do
      if l:find('leetcode.symbol_bar.rail_style_tap', 1, true) then return true end
    end
  end, timeout=15000, interval=250, label='the Classic tap reached the screen'}
  device:tap_on({id='SymbolBarDone'})
  device:extended_wait_until{not_visible={id='SymbolBarSheetHeader'}, timeout=10000, label='the sheet is gone'}
end

run_flow("choose the classic rail", choose_classic_rail)

-- the web editor autofocuses and its keyboard rises over the language row, which sits just above the rail: a person lowers it with Done first
run_flow("lower the keyboard the editor opened with", function()
  if device:extended_wait_until{condition=function() return device:get_device_state('keyboard').visible end, timeout=8000, optional=true} then
    device:scroll_until_visible{element={id='EditorAction-Done'}, direction='RIGHT', from={id='EditorActionToolbar'}}
    device:tap_on({id='EditorAction-Done'})
    device:extended_wait_until{condition=function() return not device:get_device_state('keyboard').visible end, timeout=10000, label='the keyboard is down'}
  end
end)

run_flow("switch to JavaScript", function()
  device:tap_on({id='ProblemEditorLanguagePicker-Trigger'})
  device:assert_visible({id='ProblemEditorLanguagePicker-JavaScript'}, {timeout=10000})
  device:tap_on({id='ProblemEditorLanguagePicker-JavaScript'})
  device:extended_wait_until{condition=function() return buffer():find("twoSum(nums, target) {", 1, true) ~= nil end, timeout=30000, label='the JavaScript starter'}
  device:take_screenshot(out .. "/javascript-starter.png")
end)

-- single-statement lines: an auto-closed brace below the caret would break the line model of the typing script
local LINES = { "let seen = 0;", "seen += nums.length;", "return [seen, target];" }
run_flow("type three lines in the method body", function()
  -- the blank line inside the method: the eighth of the starter
  device:tap_on({css='.cm-line:nth-child(8)'})
  device:extended_wait_until{condition=function() return device:get_device_state('keyboard').visible end, timeout=15000, label='the keyboard is up'}
  for i, line in ipairs(LINES) do
    device:input_text(line)
    if i < #LINES then device:press_key('Enter') end
  end
  device:extended_wait_until{condition=function() return buffer():find(LINES[#LINES], 1, true) ~= nil end, timeout=15000, label='the typed lines'}
  local text = buffer()
  for _, line in ipairs(LINES) do
    assert_true(text:find(line, 1, true) ~= nil, "the line did not read back: " .. line)
  end
  -- Enter inside the method body kept its indentation
  assert_true(text:find("        seen += nums.length;", 1, true) ~= nil, "the second line lost its indentation")
end)

run_flow("the Comment key writes the JavaScript marker", function()
  -- the rail is wider than the screen: the key is scrolled into view, whichever side it is on
  local key = device:scroll_until_visible{element={id='EditorSyntax-Comment'}, direction='RIGHT', from={id='EditorSyntaxToolbar'}, timeout=8000, optional=true}
    or device:scroll_until_visible{element={id='EditorSyntax-Comment'}, direction='LEFT', from={id='EditorSyntaxToolbar'}, timeout=8000}
  assert_true(key ~= nil, "the Comment key is not on the toolbar")
  device:tap_on({id='EditorSyntax-Comment'})
  device:extended_wait_until{condition=function() return buffer():find("// return [seen, target];", 1, true) ~= nil end, timeout=15000, label='the line is commented with //'}
  assert_true(buffer():find("# return", 1, true) == nil, "the key wrote Python's marker")
  device:take_screenshot(out .. "/javascript-commented.png")
end)

-- the app's console: nothing but the test's own navigation
local warnings = {}
for _, line in ipairs(device:get_logs()) do
  if not (line:find("Response body loading was aborted", 1, true) or line:find("falling back to ArrayBuffer instantiation", 1, true)) then warnings[#warnings + 1] = line end
end
assert_true(#warnings == 0, "JS warning: " .. tostring(warnings[1]))
