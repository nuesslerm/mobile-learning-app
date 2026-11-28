--[[---
platform: android, ios
against: update
timeout: 180
units: case=search; case=neetcode; case=python; case=javascript; case=custom-input
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
  -- on the simulator a tap straight after typing landed on the "j" key (the field read 27j)
  input.tap_above_keyboard({id="ConsoleFooterRun"})
  device:assert_visible({text="Output"}, {timeout=60000})
  device:assert_visible({id="ConsoleResultOutput(-Toggle)?", text="27"})
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
run_flow("Given isolated learner data, when the learner completes " .. env.case .. ", then its outcome is visible", function()
  open_library(device, env.base)
  assert(cases[env.case], "unknown LeetCode case")()
end)
