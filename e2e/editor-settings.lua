--[[---
platform: browser
against: export
timeout: 300
focus: true
features: leetcode
---]]
-- The editor settings menu through the in-place MenuPanel (the web and Android renderer; iOS draws the native menu, which only a phone shows): Line wrap
-- and Show Whitespace toggle with the menu kept open, Font size 16px returns to the root and the lines grow, Symbol Bar opens its sheet and Done closes it,
-- Format Code asks first and then writes what the format route answered (a stub: no backend), Tabs 2 indents the line after a colon by two. Waits are
-- on elements, bounds, the buffer and log lines, never on a clock.
-- wf test run --platform browser editor-settings   (wf browser test e2e/editor-settings.lua --env base=http://127.0.0.1:8800/mla-tamagui)
local base = env.base or "http://127.0.0.1:8800/mla-tamagui"
device:add_network_stub("POST", "*/api/leetcode/format", { body_file = "e2e/stubs/leetcode-format-two-sum.json", headers = { ["content-type"] = "application/json" } })

local function base64url(s)
  local abc, out = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_", {}
  for i = 1, #s, 3 do
    local a, b, c = s:byte(i, i + 2)
    local n = a * 65536 + (b or 0) * 256 + (c or 0)
    for k = 1, 2 + (b and 1 or 0) + (c and 1 or 0) do
      local idx = math.floor(n / 64 ^ (4 - k)) % 64
      out[#out + 1] = abc:sub(idx + 1, idx + 1)
    end
  end
  return table.concat(out)
end

local function buffer() return (device:get_element({id='CodeEditorInput'}) or {}).text or "" end
local function logged(needle)
  for _, l in ipairs(device:get_logs{level='info'}) do
    if l:find(needle, 1, true) then return true end
  end
  return false
end
local function present(css) return device:get_element({css=css}) ~= nil end
local function line_height() local e = device:get_element({css='.cm-line'}); return e and (e.bounds.bottom - e.bounds.top) or 0 end

-- only the page's first load hydrates (route.change); later links are client-side navigations
local first_open = true
local function open_editor(path, wait_for)
  device:clear_logs()
  device:open_link(base .. path)
  device:assert_visible({id='ProblemEditorScreen'}, {timeout=60000})
  device:extended_wait_until{condition=function() return buffer():find(wait_for, 1, true) ~= nil end, timeout=30000, label='the starter'}
  local first = first_open
  if first_open then
    device:extended_wait_until{condition=function() return logged('route.change') end, timeout=60000, interval=250, label='the page hydrated'}
    first_open = false
  end
  -- the web editor autofocuses and its keyboard rises over the header row: a person lowers it with Done first
  -- only the first load autofocuses the editor; later opens are client-side navigations and the menu button sits above any keyboard
  if first and device:extended_wait_until{condition=function() return device:get_device_state('keyboard').visible end, timeout=8000, optional=true} then
    device:scroll_until_visible{element={id='EditorAction-Done'}, direction='RIGHT', from={id='EditorActionToolbar'}}
    device:tap_on({id='EditorAction-Done'})
    device:extended_wait_until{condition=function() return not device:get_device_state('keyboard').visible end, timeout=10000, label='the keyboard is down'}
  end
end

local function open_menu()
  device:tap_on({id='EditorSettingsButton'})
  device:assert_visible({id='EditorSettingsButtonContent'}, {timeout=10000})
end

run_flow("open the editor and the menu", function()
  open_editor("/leetcode/two-sum", "def twoSum")
  open_menu()
  for _, id in ipairs({'EditorSettingsFontSize', 'EditorSettingsWhitespace', 'EditorSettingsWrap', 'EditorSettingsTabs', 'EditorSettingsSymbolBar', 'EditorSettingsFormat'}) do
    device:assert_visible({id=id}, {timeout=5000})
  end
  device:assert_screenshot("editor-settings-menu")
end)

run_flow("Line wrap and Show Whitespace keep the menu open", function()
  assert_true(present('.cm-lineWrapping'), "line wrap is on by default")
  device:tap_on({id='EditorSettingsWrap'})
  device:extended_wait_until{condition=function() return not present('.cm-lineWrapping') end, timeout=10000, label='line wrap is off'}
  device:assert_visible({id='EditorSettingsButtonContent'})
  assert_true(not present('.cm-highlightSpace'), "whitespace marks before the toggle")
  device:tap_on({id='EditorSettingsWhitespace'})
  device:extended_wait_until{condition=function() return present('.cm-highlightSpace') end, timeout=10000, label='whitespace marks'}
  device:assert_visible({id='EditorSettingsButtonContent'})
end)

run_flow("Font size 16px returns to the root and the lines grow", function()
  local before = line_height()
  device:tap_on({id='EditorSettingsFontSize'})
  device:tap_on({id='EditorSettingsFontSize-16'})
  device:assert_visible({id='EditorSettingsWrap'}, {timeout=10000})
  device:assert_not_visible({id='EditorSettingsFontSize-16'})
  device:extended_wait_until{condition=function() return line_height() > before end, timeout=10000, label='the lines grew'}
end)

run_flow("Symbol Bar opens its sheet and Done closes it", function()
  device:tap_on({id='EditorSettingsSymbolBar'})
  device:assert_visible({id='SymbolBarSheetHeader'}, {timeout=10000})
  device:tap_on({id='SymbolBarSwitch'})
  device:tap_on({id='SymbolBarDone'})
  device:extended_wait_until{not_visible={id='SymbolBarSheetHeader'}, timeout=10000, label='the sheet is gone'}
end)

run_flow("Format Code asks first, then writes the formatted text", function()
  open_editor("/leetcode/two-sum", "def twoSum")
  open_menu()
  device:tap_on({id='EditorSettingsFormat'})
  device:assert_visible({id='EditorSettingsFormatCard'}, {timeout=10000})
  assert_true(buffer():find("formatted by the stub", 1, true) == nil, "formatted before the confirmation")
  device:tap_on({id='EditorSettingsFormatConfirm'})
  device:extended_wait_until{condition=function() return logged('editor_v2.format.result') end, timeout=15000, label='the format result'}
  device:extended_wait_until{condition=function() return buffer():find("# formatted by the stub", 1, true) ~= nil end, timeout=15000, label='the formatted text'}
end)

run_flow("Tabs 2 indents the line after a colon by two", function()
  open_editor("/leetcode/two-sum?seed64=" .. base64url("if x:"), "if x:")
  open_menu()
  device:tap_on({id='EditorSettingsTabs'})
  device:tap_on({id='EditorSettingsTabs-2'})
  device:extended_wait_until{condition=function() return logged('leetcode.editor_prefs.set') end, timeout=10000, label='the preference was set'}
  device:tap_on({id='EditorSettingsButton'})
  device:extended_wait_until{not_visible={id='EditorSettingsButtonContent'}, timeout=10000, label='the menu is closed'}
  -- a tap on the line's right edge puts the caret at its end
  local line = device:get_element({css='.cm-line'})
  device:tap_on({point=('%d,%d'):format(line.bounds.right - 4, (line.bounds.top + line.bounds.bottom) // 2)})
  device:extended_wait_until{condition=function() return device:get_device_state('keyboard').visible end, timeout=15000, label='the keyboard is up'}
  device:press_key('Enter')
  device:input_text("y")
  device:extended_wait_until{condition=function() return buffer():find("if x:\n  y", 1, true) ~= nil end, timeout=15000, label='the line is indented by two'}
  assert_true(buffer():find("if x:\n    y", 1, true) == nil, "indented by four")
end)

local warnings = {}
for _, line in ipairs(device:get_logs()) do
  if not (line:find("Response body loading was aborted", 1, true) or line:find("falling back to ArrayBuffer instantiation", 1, true)) then warnings[#warnings + 1] = line end
end
assert_true(#warnings == 0, "JS warning: " .. tostring(warnings[1]))
