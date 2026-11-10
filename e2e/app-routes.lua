--[[---
platform: browser
against: export
timeout: 180
focus: true
features: *
---]]
-- The app's main routes render from a web export of the pushed commit: each opens by deep link and shows its own screen,
-- with no JS warning. wf test run serves the export and passes its base; by hand, serve one at 8800.
local base = env.base or "http://127.0.0.1:8800/mla-tamagui"
local out = env.out or ".agents/tmp/app-routes"
os.execute("mkdir -p " .. out)
local ROUTES = {
  { path = "/", sel = {id='LeetcodeListScreen'} },
  { path = "/leetcode/two-sum", sel = {id='ProblemEditorScreen'} },
  { path = "/tamil", sel = {id='TamilCourseScreen'} },
  -- dynamic routes the export holds a page per id for (generateStaticParams), and one id it has none for
  { path = "/tamil/uyir-1", sel = {id='TamilLessonScreen'} },
  { path = "/tamil/tracks/vowels", sel = {id='TamilTrackScreen'} },
  { path = "/tamil/write/uyir-a", sel = {id='TamilWriteScreen'} },
  { path = "/tamil/no-such-lesson", sel = {id='TamilLessonMissing'} },
  { path = "/leetcode/two-sum/description", sel = {id='ProblemDescriptionSheet'} },
  { path = "/settings", sel = {text='.*Appearance.*'} },
}
for n, r in ipairs(ROUTES) do
  run_flow("open " .. r.path, function()
    device:open_link(base .. r.path)
    device:assert_visible(r.sel, {timeout=60000})
  end)
  device:take_screenshot(("%s/route-%d.png"):format(out, n))
  -- Opening the next link aborts the wasm stream the previous page still loads; that abort is the test's own navigation, not an app warning
  local w = {}
  for _, line in ipairs(device:get_logs()) do
    if not (line:find("Response body loading was aborted", 1, true) or line:find("falling back to ArrayBuffer instantiation", 1, true)) then w[#w + 1] = line end
  end
  assert_true(#w == 0, ("%s: JS warning: %s"):format(r.path, tostring(w[1])))
  -- the landing, as BetterStack holds it for the session: a route that rendered logged route.change (the echo flushes about once a second)
  local landed = device:extended_wait_until{condition=function()
    for _, line in ipairs(device:get_logs{level='info'}) do
      if line:find('route.change', 1, true) then return true end
    end
  end, timeout=5000, interval=250, optional=true}
  assert_true(landed, r.path .. ": no route.change line in the app log")
  device:clear_logs()
end
log("app routes: " .. #ROUTES .. " rendered")
-- a stale or mistyped link lands on expo-router's Unmatched Route screen, and the app says which path it could not find
device:open_link(base .. "/no-such-route")
local unmatched = device:extended_wait_until{condition=function()
  for _, line in ipairs(device:get_logs{level='warn'}) do
    if line:find('nav.unmatched', 1, true) then return true end
  end
end, timeout=30000, interval=250, optional=true}
assert_true(unmatched, "/no-such-route: no nav.unmatched line in the app log")
