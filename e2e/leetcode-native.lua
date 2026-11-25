--[[---
platform: android, ios
against: update
timeout: 180
units: case=search; case=neetcode; case=one-editor; case=python; case=javascript; case=custom-input; case=repair-error; case=draft; case=symbol; case=keyboard
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
local function keyboard(want)
  device:extended_wait_until{condition=function() return device:get_device_state("keyboard").visible == want end,
    timeout=10000, label=want and "system keyboard opened" or "system keyboard dismissed"}
end
local function focus_end(first_line)
  local box = device:get_element({id="ProblemEditorInput"})
  if box then
    device:tap_on({point=("%d,%d"):format(box.bounds.cx, math.floor(box.bounds.top + 25))})
  else
    local bar = assert(device:get_element({id="RNSScreen"}), "native editor header missing").bounds
    device:tap_on({point=("%d,%d"):format(bar.cx, bar.bottom + 12)})
  end
  keyboard(true)
  if device.platform == "ios" then
    local bar = assert(device:get_element({id="RNSScreen"})).bounds
    local screen = device:get_device_state("screen")
    local startup = device:get_element({id="ProblemEditorStartupInput"})
    local y = first_line and (bar.bottom + (startup and 12 or 2)) or math.floor(screen.height*.29)
    device:tap_on({point=("%d,%d"):format(math.floor(screen.width*.83), y)})
  elseif not first_line then
    device:press_key("Ctrl+End")
  elseif box then
    device:tap_on({point=("%d,%d"):format(box.bounds.right-16, box.bounds.top+15)})
  end
end
local function hide_keyboard()
  local hide = {text="Hide keyboard"}
  if device:get_element(hide) then device:tap_on(hide) else
    device:scroll_until_visible{element={id="EditorAction-Done"}, direction="RIGHT", from={id="EditorActionToolbar"}}
    tap("EditorAction-Done")
  end
  keyboard(false)
end
local function output(expected)
  local control = device:get_element({id="ConsoleFooterRun"}) and "ConsoleFooterRun" or "ConsoleBarRun"
  tap(control)
  device:extended_wait_until{condition=function()
    return device:get_element({text="(Output|Compile Error|Runtime Error)"}) ~= nil
  end, timeout=60000}
  device:assert_visible({id="ConsoleResultOutput(-Toggle)?", text=expected})
end
local function append(text)
  focus_end(true)
  device:input_text(text)
  hide_keyboard()
end
local function type_in_starter(text, row)
  focus_end(false)
  if device.platform == "ios" then
    local header = assert(device:get_element({id="RNSScreen"})).bounds
    device:tap_on({point=("%d,%d"):format(header.cx, header.bottom+12+(row-1)*21)})
  elseif row == 8 then
    device:press_key("Remote Dpad Up")
    device:press_key("Remote Dpad Up")
  end
  device:input_text(text)
  hide_keyboard()
end
local function search(term, problem)
  tap("AlgorithmsSearch")
  keyboard(true)
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
  tap("ProblemOpenLesson")
  device:assert_visible({text="Car Fleet"})
  device:assert_visible({text="1. Stack"})
end
cases["one-editor"] = function()
  open("/leetcode/two-sum/problem", "ProblemOpenEditor")
  local box = device:get_element({id="ProblemOpenEditor"}).bounds
  local point = {point=("%d,%d"):format(box.cx, box.cy)}
  device:tap_on(point)
  device:tap_on(point)
  visible("ConsoleBarRun")
  if device.platform == "ios" then
    local bar = assert(device:get_element({id="RNSScreen"})).bounds
    device:tap_on({point=("%d,%d"):format(bar.left+30,bar.cy)})
  else
    tap("ProblemEditorHeaderBack")
  end
  visible("ProblemOpenEditor")
end
cases.python = function()
  value_editor("4")
  append("2")
  output("42")
end
cases.javascript = function()
  editor(py .. "4")
  if device.platform == "ios" then
    local header = assert(device:get_element({id="RNSScreen"})).bounds
    device:tap_on({point=("%d,%d"):format(math.floor(header.right*.72), header.cy)})
    device:tap_on({text="JavaScript"})
  else
    tap("ProblemEditorLanguagePicker-Trigger")
    local wheel = assert(device:get_element({id="WheelItemContainer-0"})).bounds
    device:swipe(("%d,%d"):format(wheel.cx, wheel.cy), ("%d,%d"):format(wheel.cx, wheel.top-14), {duration=350})
    device:assert_visible({id="ProblemEditorLanguagePicker-Trigger", text="JavaScript"})
  end
  type_in_starter("return 37", 8)
  output("37")
end
cases["custom-input"] = function()
  editor(py .. "target")
  output("9")
  device:tap_on({text="Testcase"})
  tap("ConsoleChipAdd")
  tap("ConsoleField-target")
  keyboard(true)
  if device.platform == "android" then
    device:input_text("97", {replace=true})
    device:tap_on({text="Testcase"})
  else
    device:press_key("Ctrl+End")
    device:input_text("7")
  end
  tap("ConsoleFooterRun")
  device:assert_visible({text="Output"}, {timeout=60000})
  device:assert_visible({id="ConsoleResultOutput(-Toggle)?", text="97"})
end
cases["repair-error"] = function()
  editor("v=Tru\n" .. py .. "53 if v else 0")
  tap("ConsoleBarRun")
  device:assert_visible({text="(Show line 1 in the editor|Line 1.*)"}, {timeout=60000})
  tap("ConsoleSheetHandle")
  focus_end(true)
  device:input_text("e")
  hide_keyboard()
  output("53")
  device:assert_not_visible({id="ConsoleResultLine"})
end
cases.draft = function()
  open("/leetcode/two-sum", "ConsoleBarRun")
  type_in_starter("return 64", 4)
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
  focus_end(true)
  local collapsed = {id="SymbolBarGroup-g4", text="Show symbols .*"}
  if device:get_element(collapsed) then device:tap_on(collapsed) end
  local key = {id="SymbolBarKey-g4-0"}
  if device:get_element(key) then device:tap_on(key) else device:tap_on({text="(Type )?\\+"}) end
  device:input_text("5")
  hide_keyboard()
  output("13")
end
cases.keyboard = function()
  value_editor("1")
  append("2")
  output("12")
  tap("ConsoleSheetHandle")
  append("3")
  output("123")
end
run_flow("Given isolated learner data, when the learner completes " .. env.case .. ", then its outcome is visible", function()
  open_library(device, env.base)
  assert(cases[env.case], "unknown LeetCode case")()
end)
