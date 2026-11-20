--[[---
platform: android, browser
against: android=update, browser=export
timeout: 420
focus: true
features: leetcode
---]]
-- A5 spike: the Pyodide bundled with the app boots in a hidden WebView over @lib/webview-rpc, prints 1 + 1 and runs a loop; the
-- screen prints the numbers (strategy, bridge round trip, import and load times, run times, memory) and the test logs them. No network is needed.
-- wf browser test e2e/pyodide-spike.lua --env base=http://127.0.0.1:8800/mla-tamagui
-- wf android test e2e/pyodide-spike.lua --env base=exp://u.expo.dev/<projectId>/group/<group>/--
local android = device.platform == "android"
local out = env.out or (".agents/tmp/runs/pyodide-spike-" .. device.platform)
local base = env.base or (not android and "http://127.0.0.1:8800/mla-tamagui" or nil)
assert_true(base, "pass --env base=<update link>; wf test run does")
os.execute("mkdir -p " .. out)

local function text_of(id) local e = device:get_element({id=id}) return e and e.text or "" end

run_flow("open route", function()
  -- Expo Go bundles the route on first load; a link that lands on the launcher is opened again
  for _ = 1, 3 do
    device:open_link(base .. "/leetcode/pyodide-spike")
    if device:extended_wait_until{visible={id='PyodideSpikeScreen'}, timeout=60000, optional=true} then break end
  end
  device:assert_visible({id='PyodideSpikeScreen'}, {timeout=30000})
end)

run_flow("boot and run", function()
  device:extended_wait_until{condition=function() return text_of('PyodideSpikeResult') ~= "" end, timeout=360000, interval=1000, label='Pyodide ran'}
end)

local result = text_of('PyodideSpikeResult')
log("pyodide spike: " .. result)
local file = io.open(out .. "/pyodide-spike.json", "w")
if file then file:write(result); file:close() end
device:take_screenshot(out .. "/pyodide-spike.png")
assert_true(not result:find('"error":"', 1, true), "the run reported an error: " .. result)
assert_true(result:find('"stdout":"2"', 1, true) ~= nil, "print(1 + 1) did not print 2: " .. result)
log("pyodide spike: pass")
