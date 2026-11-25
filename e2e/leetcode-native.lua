--[[---
platform: android, ios
against: update
timeout: 180
units: case=search; case=neetcode; case=one-editor; case=python; case=javascript; case=custom-input; case=repair-error; case=draft; case=symbol; case=keyboard; case=settings; case=completion; case=blocks; case=two-drafts; case=voice
focus: true
features: leetcode
---]]
local open_library = dofile("e2e/support/native-learner.lua")
local function visible(id, timeout)
  return device:assert_visible({id=id}, {timeout=timeout or 12000})
end
local function tap(id) device:tap_on({id=id}) end
local function open(route, ready)
  device:open_link(env.base .. route)
  visible(ready, 30000)
end
local function base64url(s)
  local bytes, alphabet = {}, "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
  for i = 1, #s, 3 do
    local a, b, c = s:byte(i, i+2)
    local n = a*65536 + (b or 0)*256 + (c or 0)
    for j = 1, 2 + (b and 1 or 0) + (c and 1 or 0) do
      local k = math.floor(n / 64^(4-j)) % 64 + 1
      bytes[#bytes + 1] = alphabet:sub(k,k)
    end
  end
  return table.concat(bytes)
end
local py = "class Solution:\n    def twoSum(self, nums, target): return "
local function editor(code, lang)
  open("/leetcode/two-sum?lang=" .. (lang or "python") .. "&seed64=" .. base64url(code), "ConsoleBarRun")
end
local function value_editor(expression)
  editor("v=" .. expression .. "\n" .. py .. "v")
end
local input = dofile("e2e/support/native-editor.lua")
local function output(expected)
  local control = device:get_element({id="ConsoleFooterRun"}) and "ConsoleFooterRun" or "ConsoleBarRun"
  tap(control)
  device:extended_wait_until{condition=function()
    return device:get_element({text="(Output|Compile Error|Runtime Error)"}) ~= nil
  end, timeout=60000}
  device:assert_visible({id="ConsoleResultOutput(-Toggle)?", text=expected})
end
local function search(term, problem)
  tap("AlgorithmsSearch")
  input.keyboard(true)
  device:input_text(term)
  tap("AlgorithmsProblem-" .. problem)
  visible("ProblemOpenEditor")
end

local cases = {}
cases.search = function()
  open("/leetcode/library/collection", "AlgorithmsSearch")
  search("two sum", "two-sum")
  device:assert_visible({text=".*Two Sum.*"})
  visible("ProblemInfoOpen-topics")
  device:assert_visible({text=".*Given an array of integers.*"})
end
cases.neetcode = function()
  local category = assert(device:get_element({id="LibraryCategory-All Topics"}), "category row missing").bounds
  local screen = device:get_device_state("screen")
  device:swipe(("%d,%d"):format(math.floor(screen.width*.9), category.cy), ("%d,%d"):format(math.floor(screen.width*.15),category.cy), {duration=400})
  tap("LibraryCategory-NeetCode 150")
  visible("AlgorithmsSearch")
  search("car fleet", "car-fleet")
  device:assert_visible({text="853\\. Car Fleet"})
  visible("ProblemOpenLesson")
  device:assert_visible({text=".*There are n cars at given miles.*"})
end
cases["one-editor"] = function()
  open("/leetcode/two-sum/problem", "ProblemOpenEditor")
  local box = device:get_element({id="ProblemOpenEditor"}).bounds
  local point = {point=("%d,%d"):format(box.cx, box.cy)}
  device:tap_on(point)
  device:tap_on(point)
  visible("ConsoleBarRun")
  if device.platform == "ios" then
    local bar = device:assert_visible({id="RNSScreen"}).bounds
    device:tap_on({point=("%d,%d"):format(bar.left+30,bar.cy)})
  else
    tap("ProblemEditorHeaderBack")
  end
  visible("ProblemOpenEditor")
end
cases.python = function()
  value_editor("4")
  input.append("2")
  output("42")
end
cases.javascript = function()
  editor(py .. "4")
  if device.platform == "ios" then
    local header = device:assert_visible({id="RNSScreen"}).bounds
    device:tap_on({point=("%d,%d"):format(math.floor(header.right*.72), header.cy)})
    device:tap_on({text="JavaScript"})
  else
    tap("ProblemEditorLanguagePicker-Trigger")
    visible("WheelItemContainer-0")
    local wheel = assert(device:get_element({id="WheelItemContainer-0"})).bounds
    device:swipe(("%d,%d"):format(wheel.cx, wheel.cy), ("%d,%d"):format(wheel.cx, wheel.top-14), {duration=350})
    device:assert_visible({id="ProblemEditorLanguagePicker-Trigger", text="JavaScript"})
  end
  input.type_in_starter("return 37", 8)
  output("37")
end
cases["custom-input"] = function()
  local code = "class Solution:\n    def climbStairs(self, n): return n"
  open("/leetcode/climbing-stairs?lang=python&seed64=" .. base64url(code), "ConsoleBarRun")
  output("2")
  device:tap_on({text="Testcase"})
  tap("ConsoleChipAdd")
  tap("ConsoleField-n")
  input.keyboard(true)
  if device.platform == "android" then
    device:input_text("27", {replace=true})
    device:assert_visible({id="ConsoleField-n", text="27"})
    device:tap_on({text="Testcase"})
  else
    device:press_key("Ctrl+End")
    device:input_text("7")
  end
  tap("ConsoleFooterRun")
  device:assert_visible({text="Output"}, {timeout=60000})
  device:assert_visible({id="ConsoleResultOutput(-Toggle)?", text="27"})
end
cases["repair-error"] = function()
  editor("v=Tru\n" .. py .. "53 if v else 0")
  tap("ConsoleBarRun")
  device:assert_visible({text="(Show line 1 in the editor|Line 1.*)"}, {timeout=60000})
  tap("ConsoleSheetHandle")
  visible("ConsoleBarRun")
  input.focus_end(true)
  device:input_text("e")
  input.hide_keyboard()
  output("53")
  device:assert_not_visible({id="ConsoleResultLine"})
end
cases.draft = function()
  open("/leetcode/two-sum", "ConsoleBarRun")
  input.type_in_starter("return 64", 4)
  output("64")
  open_library(device, env.base, true)
  open("/leetcode/two-sum", "ConsoleBarRun")
  output("64")
end
cases.symbol = function()
  value_editor("8")
  if device.platform == "ios" then
    device:extended_wait_until{condition=function() return device:get_element({id="ProblemEditorStartupInput"}) == nil end, timeout=12000}
  end
  input.focus_end(true)
  local key = {id="SymbolBarKey-g4-0"}
  local group = {id="SymbolBarGroup-g4", text="Show symbols .*"}
  if device.platform == "ios" then
    device:tap_on(group)
    visible("EditorExpandedSymbols")
    device:tap_on(key)
  else
    if device:get_element(group) then device:tap_on(group) end
    if device:get_element(key) then device:tap_on(key) else device:tap_on({text="(Type )?\\+"}) end
    device:assert_visible({id="CodeEditorText", text="v=8\\+.*"})
    input.focus_end(true)
  end
  device:input_text("5")
  input.hide_keyboard()
  output("13")
end
cases.keyboard = function()
  value_editor("1")
  input.append("2")
  output("12")
  tap("ConsoleSheetHandle")
  visible("ConsoleBarRun")
  input.append("3")
  output("123")
end
cases.settings = function()
  local function open_settings()
    if device.platform == "ios" then
      local bar = device:assert_visible({id="RNSScreen"}).bounds
      device:tap_on({point=("%d,%d"):format(bar.right-28,bar.cy)})
    else
      tap("EditorSettingsButton")
    end
  end
  open("/leetcode/two-sum", "ConsoleBarRun")
  open_settings()
  local setting = {text="Font size: [0-9]+px"}
  device:assert_visible(setting)
  local current = assert(device:get_element(setting)).text
  local chosen = current == "Font size: 18px" and "16px" or "18px"
  device:tap_on(setting)
  device:tap_on({text=chosen})
  device:extended_wait_until{not_visible={text=chosen}}
  open_settings()
  device:assert_visible({text="Font size: " .. chosen})
  open_library(device, env.base, true)
  open("/leetcode/two-sum", "ConsoleBarRun")
  open_settings()
  device:assert_visible({text="Font size: " .. chosen})
end
local function enable_list_setting(id)
  open("/blind75", "LeetcodeListMenuButton(Compact)?")
  local screen = device:get_device_state("screen")
  local x, y = math.floor(screen.width*.5), math.floor(screen.height*.52)
  local step = math.floor(screen.height*.18)
  for _ = 1, 8 do
    local target = device:get_element({id=id})
    -- The Android navigation bar can cover the bottom of an accessible radio button.
    if target and target.bounds.bottom <= screen.height*.90 then break end
    device:swipe(("%d,%d"):format(x, y+step), ("%d,%d"):format(x, y), {duration=500})
  end
  local last_y, steady_since = nil, 0
  device:extended_wait_until{condition=function()
    local target = device:get_element({id=id})
    if not target then return false end
    if target.bounds.cy ~= last_y then
      last_y, steady_since = target.bounds.cy, now_ms()
    end
    return now_ms() - steady_since >= 250
  end, label="The setting has stopped scrolling"}
  tap(id)
end
cases.completion = function()
  enable_list_setting("LeetcodeListAutocompleteOn")
  editor("const v=fal\nclass Solution { twoSum(nums, target) { return v === false ? 79 : 0; } }", "javascript")
  if device.platform == "ios" then
    device:extended_wait_until{condition=function() return device:get_element({id="ProblemEditorStartupInput"}) == nil end, timeout=15000}
  end
  input.focus_end(true)
  device:input_text("s")
  if device.platform == "android" then device:assert_visible({text="false"}) end
  device:tap_on({text="Tab"})
  device:assert_visible({text="const v=false.*"})
  input.hide_keyboard()
  output("79")
end
cases.blocks = function()
  open("/leetcode/two-sum", "ConsoleBarRun")
  input.type_in_starter("return 64", 4)
  enable_list_setting("LeetcodeListBlockViewOn")
  device:open_link(env.base .. "/leetcode/two-sum")
  device:assert_visible({text=".*return 64.*"})
  device:tap_on({text="Blocks"})
  device:assert_visible({text=".*return 64.*"})
  device:tap_on({text="Code"})
  device:assert_not_visible({id="BlockRow-0"})
  visible("ConsoleBarRun")
  output("64")
end
cases["two-drafts"] = function()
  local function open_draft(slug, signature)
    open("/leetcode/library/collection", "AlgorithmsSearch")
    open("/leetcode/" .. slug, "ConsoleBarRun")
    if device.platform == "ios" then
      device:extended_wait_until{not_visible={id="ProblemEditorStartupInput"}}
    end
    device:assert_visible({text=".*def " .. signature .. ".*"})
  end
  local function expect_body(signature, statement)
    device:assert_visible({text=".*def " .. signature .. ".*:\\s*" .. statement .. ".*"})
  end
  open_draft("two-sum", "twoSum")
  input.type_in_starter("pass", 4)
  expect_body("twoSum", "pass")
  open_draft("contains-duplicate", "containsDuplicate")
  input.type_in_starter("return", 4)
  expect_body("containsDuplicate", "return")
  open_draft("two-sum", "twoSum")
  expect_body("twoSum", "pass")
  open_draft("contains-duplicate", "containsDuplicate")
  expect_body("containsDuplicate", "return")
end
cases.voice = function()
  local code = "class Solution {\n    twoSum(nums, target) {\n        return nums.indexOf(target - nums[0]);\n    }\n}"
  open("/leetcode/two-sum?lang=javascript&voiceReplay=1&seed64=" .. base64url(code), "ConsoleBarRun")
  device:scroll_until_visible{element={id="ProblemEditorVoiceStartButton"}, direction="DOWN"}
  tap("ProblemEditorVoiceStartButton")
  tap("ProblemEditorVoiceToolbar-Send")
  device:assert_visible({text="rename nums to values and return early when target is found"})
  tap("ProblemEditorVoiceReviewSheet-Send")
  device:assert_visible({text=".*twoSum\\(values, target\\).*return values\\.indexOf.*"})
  output("1")
end
run_flow("Given isolated learner data, when the learner completes " .. env.case .. ", then its outcome is visible", function()
  open_library(device, env.base)
  assert(cases[env.case], "unknown LeetCode case")()
end)
