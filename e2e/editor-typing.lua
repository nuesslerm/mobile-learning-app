--[[---
platform: browser
against: browser=export
timeout: 180
units: problem=valid-palindrome part=1; problem=valid-palindrome part=2; problem=valid-palindrome part=3; problem=two-sum part=1; problem=two-sum part=2
focus: true
features: leetcode
---]]
-- device-lab editor typing: two solutions typed into the CodeMirror editor key by key, soft keyboard up, rail above it.
-- wf android test e2e/editor-typing.lua --env out=<dir> --env base=exp://u.expo.dev/<projectId>/group/<group>/-- [--env golden=off: no result golden]
-- Gates: keyboard shown, EditorHelperRails fully above the keyboard, every typed line present, Run Local passes,
-- zero JS warnings per problem. The run itself is recorded by the harness runner.
local android = device.platform == "android"
local out = env.out or (".agents/tmp/runs/editor-typing-" .. device.platform)
local base = env.base or (not android and "http://127.0.0.1:8800/mla-tamagui" or nil)
assert_true(base, "pass --env base=<update link>; wf test run does")
os.execute("mkdir -p " .. out)
-- Reset code goes through the header menu, Done lowers the keyboard (the console's rail covers the language row) and the last part ends with a console Submit
-- read from the Run Result tab.
local READY = 'ProblemEditorScreen'
local function choose_autocomplete_off()
  device:open_link(base .. "/")
  device:assert_visible({id='LeetcodeListRunnerRow'}, {timeout=60000})
  device:extended_wait_until{condition=function()
    for _, l in ipairs(device:get_logs{level='info'}) do if l:find('route.change', 1, true) then return true end end
  end, timeout=60000, interval=250, label='the page hydrated'}
  device:scroll_until_visible{element={id='LeetcodeListAutocompleteOff'}, direction='DOWN', timeout=30000}
  device:clear_logs()
  -- a tap before the page's scripts have hydrated its buttons is lost: it is repeated until the screen logs it
  for _ = 1, 10 do
    device:tap_on({id='LeetcodeListAutocompleteOff'})
    if device:extended_wait_until{condition=function() for _, l in ipairs(device:get_logs{level='info'}) do if l:find('leetcode.list.autocomplete_tap', 1, true) then return true end end end, timeout=2000, interval=250, optional=true} then return end
  end
  error("the suggestions choice never reached the screen")
end
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

-- Expo Go bundles the route on first load; a fresh install also shows its one-time developer-menu intro, which a user dismisses with Continue
local function wait_route(sel, secs)
  local t0 = os.time()
  while os.time() - t0 < secs do
    if device:extended_wait_until{visible=sel, timeout=8000, optional=true} then return end
    if device:get_element({text='.*Continue.*'}) then device:tap_on({text='.*Continue.*'}) end
  end
  device:assert_visible(sel, {timeout=5000, label='route ready'})
end

-- The screen in pixels (540x960 on the first device, 480x1040 now, a Galaxy S25 in dp): positions measured on the first one are scaled. The width is
-- 360 dp at any density, so a length scales by SW / 540.
local SW, SH = 540, 960
if android then
  local s = device:get_device_state('screen'); SW, SH = s.width, s.height
end
local K = SW / 540
local function across(x) return math.floor(x * K + 0.5) end
local function from_top(y) return math.floor(y * K + 0.5) end
local function from_middle(y) return math.floor(SH / 2 + (y - 480) * K + 0.5) end

local SOL = {
  { id = "valid-palindrome", title = "Valid Palindrome", head = "class Solution:\n    def isPalindrome(self, s: str) -> bool:", lines = {
    { 8, "left, right = 0, len(s) - 1" }, { 8, "while left < right:" }, { 12, "if not s[left].isalnum():" }, { 16, "left += 1" },
    { 12, "elif not s[right].isalnum():" }, { 16, "right -= 1" }, { 12, "elif s[left].lower() != s[right].lower():" },
    { 16, "return False" }, { 12, "else:" }, { 16, "left += 1" }, { 16, "right -= 1" }, { 8, "return True" } } },
  { id = "two-sum", title = "Two Sum", head = "class Solution:\n    def twoSum(self, nums: List[int], target: int) -> List[int]:", lines = {
    { 8, "seen = {}" }, { 8, "for i, n in enumerate(nums):" }, { 12, "if target - n in seen:" },
    { 16, "return [seen[target - n], i]" }, { 12, "seen[n] = i" }, { 8, "return []" } } },
}
-- a unit runs one problem (`--env problem=<id>`), and with `--env part=N` only its Nth run of LINES_PER_PART lines: a later part
-- opens the editor seeded with the lines before it, so every unit stays under a minute; the last part runs Run Local
local LINES_PER_PART = 4
local part = tonumber(env.part)
-- a deep link on Android drops raw newlines from a query value, so the seed travels as base64url
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
if env.problem then
  local only = {}
  for _, p in ipairs(SOL) do if p.id == env.problem then only[#only + 1] = p end end
  assert_true(#only == 1, "no problem " .. env.problem)
  SOL = only
end
local SYM = { [":"] = "EditorSyntax-Colon", ["("] = "EditorSyntax-Paren", ["["] = "EditorSyntax-Bracket",
              ["="] = "EditorSyntax-Equals", ["_"] = "EditorSyntax-Underscore", ["."] = "EditorSyntax-Dot" }
-- the bracket keys insert a pair, and Android's keyboard does not type over the closing one: brackets go on the keyboard there
if android then SYM["("], SYM["["] = nil, nil end

-- Android reads the app's own copy of the document (CodeEditorText). The web view's accessibility text lagged, stayed stale and was
-- absent for whole units (runs 44047720113, 44618453035, 44907690601), which skipped an Enter and pressed others three times.
local function buffer() local e = device:get_element({id = android and 'CodeEditorText' or 'CodeEditorInput'}) return e and e.text end
local function text_of(id) local e = device:get_element({id=id}) return e and e.text end
local function indent_now()
  local b = buffer()
  assert_true(b, "the editor's text is not readable")
  local last = b:match("([^\n]*)$") or ""
  return #last - #(last:gsub("%s+$", "")) -- trailing spaces on the current last line
end
-- The toolbar is wider than the screen: a user swipes it until the key is on screen, then taps it.
-- The first row's keys in the app's order (toolbarKeys.ts): Android drops a key that is scrolled off screen from its
-- accessibility tree, so the swipe direction comes from where the key sits relative to the last key used.
local ROW1 = { ["EditorSyntax-Paren"] = 3, ["EditorSyntax-Bracket"] = 4, ["EditorSyntax-Colon"] = 8,
               ["EditorSyntax-Equals"] = 9, ["EditorSyntax-Underscore"] = 10, ["EditorSyntax-Dot"] = 11 }
local row1_at = 1 -- index of the key last brought into view; a fresh editor shows the row from its start
local missed = 0
local function bring_into_view(id)
  local want = ROW1[id] or row1_at
  local el = device:scroll_until_visible{element={id=id}, direction=want >= row1_at and 'RIGHT' or 'LEFT', from={id='EditorSyntaxToolbar'}, timeout=6000, optional=true}
  if el then row1_at = want; return true end
  missed = missed + 1
  if missed <= 3 then log(("toolbar key %s not on screen after 6 s of swipes; typed on the keyboard"):format(id)) end
  return false
end
local toolbar = { taps = 0, typed = 0 }
local function count_of(text, c)
  return select(2, text:gsub(c:gsub("%p", "%%%0"), ""))
end
local function sym(c)
  local before = count_of(buffer() or "", c)
  local ok = bring_into_view(SYM[c]) and device:tap_on({id=SYM[c]}, {optional=true}) ~= nil
  -- the editor applies a toolbar key asynchronously, over the WebView bridge: a person waits to see that very character before typing
  -- on (the text only growing says nothing: other input may be what grew it, and the key then lands after the text typed past it)
  if ok then
    device:extended_wait_until{condition=function() return count_of(buffer() or '', c) > before end, timeout=8000, label=("the toolbar key %s landed"):format(c)}
  end
  if ok then toolbar.taps = toolbar.taps + 1 else toolbar.typed = toolbar.typed + 1 end
  return ok
end
local function keyboard_down() return not device:get_device_state('keyboard').visible end
local function api_calls()
  local n = 0
  for _, l in ipairs(device:get_logs{level='debug'}) do if l:find('editor.api.call', 1, true) then n = n + 1 end end
  return n
end
local function done() -- the only way a user dismisses the keyboard: Done at the end of the toolbar
  -- a tap on a row still moving after its swipe stops the row instead of pressing Done: a user taps again
  for _ = 1, 3 do
    device:scroll_until_visible{element={id='EditorAction-Done'}, direction='RIGHT', from={id='EditorActionToolbar'}}
    device:tap_on({id='EditorAction-Done'})
    if not android or device:extended_wait_until{condition=keyboard_down, timeout=2000, optional=true} then break end
  end
  if android then
    device:extended_wait_until{condition=keyboard_down, timeout=3000, label='keyboard down after Done'}
  else
    device:assert_not_visible({id='EditorHelperRails'})
  end
end
-- One press, then the wait for the document to gain its line: a key the editor lost fails here, named, and is never pressed again
local function newlines(text) return select(2, text:gsub("\n", "")) end
local function enter()
  local before = newlines(assert(buffer(), "the editor's text is not readable"))
  device:press_key('Enter')
  device:extended_wait_until{condition=function() return newlines(buffer() or "") > before end, timeout=8000, label='the Enter reached the editor'}
end
local keys = 0
local used_keys = {}
local function typ(s) keys = keys + #s; device:input_text(s) end
local warned = false

local function line(problem, i, indent, content, mistake)
  for _ = 1, 40 do
    local c = indent_now()
    if c == indent then break end
    if c > indent then device:press_key('Backspace') else typ(string.rep(" ", indent - c)) end
    -- keys that do not reach the end of the buffer would be retyped 40 times: stop at once instead
    device:extended_wait_until{condition=function() return indent_now() ~= c end, timeout=2000,
      label=("%s line %d: the indent leaves %d after a key (the caret is at the end of the buffer)"):format(problem, i, c)}
  end
  if mistake then typ("q"); device:take_screenshot(("%s/%s-typo.png"):format(out, problem)); device:press_key('Backspace') end
  local chunk = ""
  for ch in content:gmatch(".") do
    -- each toolbar key once per problem: a person who has found a key types its next uses on the keyboard (the browser
    -- editor types over an auto-closed bracket, so brackets work from the keyboard there)
    if SYM[ch] and not used_keys[ch] then
      if chunk ~= "" then typ(chunk); chunk = "" end
      used_keys[ch] = true
      if not sym(ch) then typ(ch) end
    else
      chunk = chunk .. ch
    end
  end
  if chunk ~= "" then typ(chunk) end
  if env.shots == "all" then device:take_screenshot(("%s/%s-line%02d.png"):format(out, problem, i)) end
  if not warned and i % 3 == 0 then
    local w = device:get_logs()
    if #w > 0 then
      warned = true
      log(("first JS warning, after %s line %d: %s"):format(problem, i, w[1]))
      if android then local l = device:get_logs{level='debug'}; log(table.concat(l, '\n', math.max(1, #l - 39))) end -- the component stack, for the gap file
    end
  end
end

-- The lines the document does not hold yet (each trimmed: the indent is checked as it is typed). Nothing is typed again for a missing one.
local function missing_lines(p, last, b)
  local got = {}
  for l in (b .. "\n"):gmatch("([^\n]*)\n") do got[#got + 1] = l:match("^%s*(.-)%s*$") end
  local missing = {}
  for n = 1, last do
    local want, found = p.lines[n][2], false
    for _, g in ipairs(got) do if g == want then found = true break end end
    if not found then missing[#missing + 1] = want end
  end
  return missing
end
local function read_back(p, last)
  local b, missing = "", {}
  local function read() b = buffer() or ""; missing = missing_lines(p, last, b); return #missing == 0 end
  device:extended_wait_until{condition=read, timeout=5000, optional=true, label=p.id .. ': typed lines read back'}
  read()
  return b, missing
end

local result = {}
for _, p in ipairs(SOL) do
  row1_at = 1
  used_keys = {}
  local r = {}
  result[p.id] = r
  device:clear_logs()
  local first, last = 1, #p.lines
  if part then first, last = (part - 1) * LINES_PER_PART + 1, math.min(part * LINES_PER_PART, #p.lines) end
  assert_true(first <= last, ("%s has no part %s"):format(p.id, tostring(part)))
  r.final = last == #p.lines
  local seeded = first > 1
  -- a reused app may still show this screen: a link to it remounts it a moment later, so the first wait would pass
  -- on the old one (18 Oct); the app leaves for Settings first (a link to "/" does not navigate a running Expo Go)
  if android and device:get_element({id='ProblemEditorScreen'}) then
    device:open_link(base .. "/settings")
    device:assert_not_visible({id='ProblemEditorScreen'})
  end
  -- suggestions are on by default and this script types key by key without them (browser only: the Android unit cannot be verified here)
  if not android then run_flow("turn suggestions off from the list", function() choose_autocomplete_off() end) end
  -- the screen opens by deep link, seeded or not, on every platform (owner, 16 Oct); the app may be the one already running
  run_flow("open route", function()
    local link = base .. "/leetcode/" .. p.id
    if seeded then
      local typed = { p.head }
      for n = 1, first - 1 do typed[#typed + 1] = string.rep(" ", p.lines[n][1]) .. p.lines[n][2] end
      link = link .. "?seed64=" .. base64url(table.concat(typed, "\n"))
    end
    device:open_link(link)
    wait_route({id=READY}, 240)
    -- the static page paints its buttons before its scripts have hydrated them, and a tap before that is lost: wait for the first route.change
    if not android then
      device:extended_wait_until{condition=function()
        for _, l in ipairs(device:get_logs{level='info'}) do
          if l:find('route.change', 1, true) then return true end
        end
      end, timeout=60000, interval=250, label='the page hydrated'}
    end
  end)
  -- the cold start of this unit's app start, printed and never judged: a hosted Android job may not carry the app's log, so a missing line is no failure
  device:extended_wait_until{condition=function()
    for _, l in ipairs(device:get_logs{level='info'}) do if l:find('editor.cold_start', 1, true) then return true end end
  end, timeout=android and 3000 or 15000, optional=true, label=p.id .. ': the editor.cold_start line'}
  for _, l in ipairs(device:get_logs{level='info'}) do
    if l:find('editor.cold_start', 1, true) or l:find('pyodide_runner.load', 1, true) then log(p.id .. " cold start: " .. l) end
  end
  run_flow("choose the classic rail", choose_classic_rail)
  -- the web editor autofocuses on open and its keyboard covers Reset, so an unseeded run lowers it; the Android one waits for a tap
  local focused = not android and device:extended_wait_until{visible={id='EditorHelperRails'}, timeout=3000, optional=true}
  if focused and not seeded then
    -- the console's rail covers the language row: Done, at the end of the action toolbar, lowers the keyboard
    done()
    device:assert_visible({id='ConsoleDock'}, {timeout=10000})
    focused = false
  end
  if not seeded then
    device:tap_on({id='ProblemEditorMenuButton'})
    device:extended_wait_until{visible={id='ProblemEditorMenuResetCode'}, timeout=10000, label='the menu'}
    device:tap_on({id='ProblemEditorMenuResetCode'})
    -- the dialog is not in Android's accessibility tree (known gap): there the button is tapped where the dialog draws it
    if android and not device:extended_wait_until{visible={id='ResetCodeDialog-Confirm'}, timeout=3000, optional=true} then
      -- read its Reset button's blue off the screen: tap only once it is drawn, then see it gone (a slow first draw ate the tap)
      local reset = ('%d,%d'):format(across(356), from_middle(578))
      local function blue(c) return c.b > 200 and c.r < 80 and c.g < 170 end
      local function drawn() return blue(device:get_pixel(reset)) end
      device:extended_wait_until{condition=drawn, timeout=10000, label=p.id .. ': Reset dialog drawn'}
      for _ = 1, 3 do
        device:tap_on({point=('%d,%d'):format(across(400), from_middle(578))})
        if not drawn() then break end
      end
      assert_true(not drawn(), p.id .. ": the Reset dialog is still open")
    else
      device:tap_on({id='ResetCodeDialog-Confirm'})
      device:assert_not_visible({id='ResetCodeDialog-Confirm'})
    end
  end
  if android then
    for _ = 1, 3 do -- the first tap can land while the dialog is still leaving
      device:tap_on({point=('%d,%d'):format(across(270), from_top(330))})
      if device:extended_wait_until{visible={id='EditorHelperRails'}, timeout=4000, optional=true} then break end
    end
    device:assert_visible({id='EditorHelperRails'})
    device:press_key('Ctrl+End') -- the tap lands mid-document
  else
    -- a seeded editor is still focused from opening: a tap there could set its caret after Ctrl+End (18 Oct, two-sum part 2)
    if not focused then
      device:tap_on({css='.cm-line:last-child'}) -- focusing the editor raises the keyboard and with it the toolbar rail
      device:assert_visible({id='EditorHelperRails'})
    end
    device:press_key('Ctrl+End') -- typing goes on at the end of the buffer
  end
  local b0 = assert(buffer(), p.id .. ": the editor's text is not readable")
  if (b0:match("([^\n]*)$") or ""):match("%S") then enter() end

  device:take_screenshot(("%s/%s-start.png"):format(out, p.id))
  local rail = device:get_element({id='EditorHelperRails'})
  local rb = rail and rail.bounds
  r.rail_bounds = rb and { rb.left, rb.top, rb.right, rb.bottom } or nil
  -- the keyboard where it is drawn: Android's IME window, the browser frame's keyboard model
  local kb = device:get_device_state('keyboard')
  r.keyboard_at_start = kb.visible
  r.ime_top = kb.bounds and kb.bounds.top
  r.toolbar_above_keyboard = rb ~= nil and kb.bounds ~= nil and rb.bottom <= kb.bounds.top + 2
  device:take_screenshot(("%s/%s-toolbar-visible.png"):format(out, p.id))

  for n = first, last do
    if n > first then enter() end
    line(p.id, n - 1, p.lines[n][1], p.lines[n][2], n == 1)
  end
  r.keyboard_while_typing_end = device:get_device_state('keyboard').visible
  local b, missing = read_back(p, last)
  r.buffer_tail = b:sub(-160)
  r.lines_match = #missing == 0
  r.missing = missing
  r.field_read_empty = b == ""
  -- the toolbar's Undo and Redo reach the page as calls of the typed editor API (issue 86, B1): the text goes back, returns, and each call is logged
  if r.lines_match then
    local typed_text = buffer() or ""
    local typed_length = #typed_text
    local api_before = api_calls()
    device:scroll_until_visible{element={id='EditorAction-Undo'}, direction='LEFT', from={id='EditorActionToolbar'}, timeout=6000, optional=true}
    device:tap_on({id='EditorAction-Undo'})
    device:extended_wait_until{condition=function() return #(buffer() or "") < typed_length end, timeout=8000, label=p.id .. ': the undo reached the page'}
    device:tap_on({id='EditorAction-Redo'})
    -- the exact text returns, not a text of the same length
    device:extended_wait_until{condition=function() return (buffer() or "") == typed_text end, timeout=8000, label=p.id .. ': the redo restored the exact text'}
    -- the app's debug log is read on the web; the phone's console is not a promised channel
    if not android then
      device:extended_wait_until{condition=function() return api_calls() >= api_before + 2 end, timeout=5000, interval=250, label=p.id .. ': both calls were logged as typed API calls'}
    end
  end
  done()
  if android then r.keyboard_after_done = device:get_device_state('keyboard').visible end
  device:take_screenshot(("%s/%s-keyboard-down.png"):format(out, p.id))
  -- Run Local answers on the device (issue 86, B5), so no backend runs for this test; a part whose lines did not read back is not run,
  -- because its outcome would hide the line that went missing (reported below)
  if r.final and r.lines_match then
    local totals_id = 'ConsoleResultCounts'
    run_flow("run local", function()
      device:tap_on({id='ConsoleBarSubmit'})
      -- the first run of a launch loads the interpreter
      device:extended_wait_until{visible={id=totals_id}, timeout=90000, label=p.id .. ': the run outcome'}
    end)
    r.run_local = text_of(totals_id)
    device:take_screenshot(("%s/%s-result.png"):format(out, p.id))
  end
  r.warnings = device:get_logs()
end
result.input = { keys = keys, toolbar_taps = toolbar.taps, toolbar_typed_on_keyboard = toolbar.typed }

local f = assert(io.open(out .. "/result.json", "w")); f:write(json.encode(result)); f:close()
for _, p in ipairs(SOL) do
  local r = result[p.id]
  assert_true(r.keyboard_at_start, p.id .. ": keyboard not shown")
  assert_true(r.toolbar_above_keyboard, p.id .. ": toolbar not above the keyboard")
  assert_true(r.lines_match, ("%s: typed code does not match: missing %s; the field read %s"):format(p.id, json.encode(r.missing),
    r.field_read_empty and "empty" or "..." .. json.encode(r.buffer_tail)))
  if r.final then
    local got, of = (r.run_local or ""):match("(%d+) / (%d+)")
    assert_true(got and got == of, p.id .. ": Run Local did not pass every case: " .. tostring(r.run_local))
  end
  assert_true(#r.warnings == 0, p.id .. ": JS warnings: " .. tostring(r.warnings[1]))
end
log(json.encode(result.input))
log("editor typing: pass")
