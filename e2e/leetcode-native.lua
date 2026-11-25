--[[---
platform: android, ios
against: update
timeout: 180
units: case=search; case=neetcode; case=one-editor; case=python; case=javascript; case=custom-input; case=repair-error; case=draft; case=symbol; case=keyboard
focus: true
features: leetcode
---]]
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
local js = "var twoSum = (nums, target) => "
local function editor(code, lang)
  open("/leetcode/two-sum?lang=" .. (lang or "python") .. "&seed64=" .. base64url(code), "ConsoleBarRun")
end
local function keyboard(want)
  device:extended_wait_until{condition=function() return device:get_device_state("keyboard").visible == want end,
    timeout=10000, label=want and "system keyboard opened" or "system keyboard dismissed"}
end
local function focus_end()
  local box = device:get_element({id="ProblemEditorInput"})
  if box then
    device:tap_on({point=("%d,%d"):format(box.bounds.cx, math.floor(box.bounds.top + 25))})
  else
    local bar = assert(device:get_element({id="RNSScreen"}), "native editor header missing").bounds
    device:tap_on({point=("%d,%d"):format(bar.cx, bar.bottom + 12)})
  end
  keyboard(true)
  device:press_key("Ctrl+End")
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
  device:assert_visible({text="Output"}, {timeout=60000})
  device:assert_visible({id="ConsoleResultOutput(-Toggle)?", text=expected})
end
local function append(text)
  focus_end()
  device:input_text(text)
  hide_keyboard()
end
local function replace_program(code)
  focus_end()
  device:press_key("Select all")
  device:input_text(code)
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
end
cases.neetcode = function()
  local category = assert(device:get_element({id="LibraryCategory-All Topics"}), "category row missing").bounds
  local screen = device:get_device_state("screen")
  device:swipe(("%d,%d"):format(math.floor(screen.width*.9), category.cy), ("%d,%d"):format(math.floor(screen.width*.15),category.cy), {duration=400})
  tap("LibraryCategory-NeetCode 150")
  visible("AlgorithmsSearch")
  search("car fleet", "car-fleet")
  tap("ProblemOpenLesson")
  visible("NeetCodeLesson")
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
  editor(py .. "4")
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
    tap("ProblemEditorLanguagePicker-JavaScript")
  end
  replace_program(js .. "37")
  output("37")
end
cases["custom-input"] = function()
  editor(py .. "target")
  output("9")
  device:tap_on({text="Testcase"})
  tap("ConsoleChipAdd")
  tap("ConsoleField-target")
  keyboard(true)
  device:press_key("Select all")
  device:input_text("47")
  tap("ConsoleFooterRun")
  device:assert_visible({text="Output"}, {timeout=60000})
  device:assert_visible({id="ConsoleResultOutput(-Toggle)?", text="47"})
end
cases["repair-error"] = function()
  editor(py .. "missing")
  tap("ConsoleBarRun")
  device:assert_visible({id="ConsoleResultLine", text="(Show line 2 in the editor|Line 2.*)"}, {timeout=60000})
  tap("ConsoleResultLine")
  focus_end()
  for _ = 1, #"missing" do device:press_key("Backspace") end
  device:input_text("53")
  hide_keyboard()
  output("53")
  device:assert_not_visible({id="ConsoleResultLine"})
end
cases.draft = function()
  open("/leetcode/two-sum", "ConsoleBarRun")
  replace_program(py .. "64")
  output("64")
  open("/leetcode/library/collection", "AlgorithmsSearch")
  open("/leetcode/two-sum", "ConsoleBarRun")
  output("64")
end
cases.symbol = function()
  editor(py .. "8")
  focus_end()
  if device:get_element({id="SymbolBarGroup-g4"}) then tap("SymbolBarGroup-g4") end
  tap("SymbolBarKey-g4-0")
  device:input_text("5")
  hide_keyboard()
  output("13")
end
cases.keyboard = function()
  editor(py .. "1")
  append("2")
  output("12")
  tap("ConsoleSheetHandle")
  append("3")
  output("123")
end
run_flow("Given isolated learner data, when the learner completes " .. env.case .. ", then its outcome is visible", function()
  device:open_link((env.base:gsub("/%-%-$", "")))
  visible("LibraryHomeMenuButton", 90000)
  assert(cases[env.case], "unknown LeetCode case")()
end)
