-- Expo Go's cold start against the EAS Update, as its own run: `wf test run` runs this with --cold-start once
-- per job, after a unit that did not pass and when the target moved, so the units reuse the running app. No header: it is not a test.
-- Besides "first bundle" (open to first frame), it splits the start-up from the device log: the process start, the
-- manifest and bundle until React Native runs the app, then the app's own start until its first frame.
local base = env.base
assert_true(base, "pass --env base=<update link>; wf test run does")
-- what the start opens: the app's root under base, unless the caller names the link (an EAS Update opens bare: its
-- manifest answers 404 with a /--/ path after it, CI run 39593152492)
local link = env.open or (base .. "/")
local out = env.out or ".agents/tmp/warm-app"
os.execute("mkdir -p " .. out)

-- Expo Go's own error screen ends the start at once, with what its error log says; its "reload the project" line must not
-- read as the Reload dialog below (CI run 39585958890 pressed back on it, then waited out the 300 s)
local function expo_go_failed()
  if not device:get_element({text='Something went wrong.*'}) then return end
  device:take_screenshot(out .. "/expo-go-error.png")
  -- the screen says only "Failed to download remote update"; the loader's own lines and their cause are in logcat
  local logs = tostring(device:run_shell("logcat -d -t 5000 -v epoch -b main,system,crash | grep -iE 'expo|updates|exception|caused by|ssl|http' | tail -n 200"))
  local f = assert(io.open(out .. "/expo-go-logcat.txt", "w")); f:write(logs); f:close()
  local cause
  for line in logs:gmatch("[^\n]+") do cause = line:match("Caused by: (.+)") or cause end
  local said = "no error log"
  if device:tap_on({text='View error log'}, {optional=true}) then
    -- an error word with at least a phrase after it: the "View error log" button itself does not match
    local line = device:extended_wait_until{visible={text='.*(error|failed|unable|exception|could not|cannot).{8,}'}, timeout=5000, optional=true}
    device:take_screenshot(out .. "/expo-go-error-log.png")
    said = line and line.text or "an error log with no error line"
  end
  assert_true(false, "Expo Go could not open " .. link .. ": " .. said .. (cause and (" (logcat: " .. cause .. ")") or ""))
end

device:clear_logs()
local opened = now_ms()
run_flow("first bundle", function()
  device:open_link(link)
  local deadline = os.time() + 300
  -- the wait returns as the frame shows; a dialog that holds the start is answered between probes
  while not device:extended_wait_until{visible={id='DebugFab'}, timeout=3000, optional=true} do
    expo_go_failed()
    if device:get_element({text='.*Continue.*'}) then device:tap_on({text='.*Continue.*'}) end
    if device:get_element({text='.*Reload.*'}) then device:back() end
    assert_true(os.time() < deadline, "the first bundle did not reach the app within 300 s")
  end
end)
local framed = now_ms()
device:take_screenshot(out .. "/first-frame.png")

-- `logcat -v epoch` lines start with seconds since the epoch: the first line that matches each mark, in milliseconds
local lines = tostring(device:run_shell("logcat -d -v epoch -b main,system,events | grep -E 'Start proc [0-9]+:host.exp.exponent|Running \"'"))
local function mark(pattern)
  for line in lines:gmatch("[^\n]+") do
    if line:find(pattern) then
      local secs = line:match("^%s*(%d+%.%d+)")
      if secs then return math.floor(tonumber(secs) * 1000 + 0.5) end
    end
  end
end
local proc, running = mark("Start proc %d+:host%.exp%.exponent"), mark('Running "')
local marks = { opened = opened, process = proc, running = running, framed = framed }
if proc then add_timing_span("process start", opened, proc) end
if proc and running then add_timing_span("manifest and bundle", proc, running) end
if running then add_timing_span("app start to first frame", running, framed) end
local f = assert(io.open(out .. "/startup.json", "w")); f:write(json.encode(marks)); f:close()
-- the device health the job checked, published with this bundle so no goal needs a device up to read it
os.execute("wf android list-devices | jq '.result.devices[0]' > " .. out .. "/device-features.json")
log(("cold start %.1f s: process %s, manifest and bundle %s, app to first frame %s"):format((framed - opened) / 1000,
  proc and ("%.1f s"):format((proc - opened) / 1000) or "unmarked",
  (proc and running) and ("%.1f s"):format((running - proc) / 1000) or "unmarked",
  running and ("%.1f s"):format((framed - running) / 1000) or "unmarked"))
