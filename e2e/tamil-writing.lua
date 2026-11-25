--[[---
platform: browser
against: export
timeout: 180
units: part=vowels letters=1-2; part=vowels letters=3-4; part=vowels letters=5-6; part=consonant; part=write; part=words word=word-uyir-a; part=words word=word-uyir-aa; part=words word=word-uyir-i
focus: true
features: tamil-writing
---]]
-- device-lab Tamil writing: traced letters (guide), a write path (template) and three words with local self-check.
-- wf android|browser test e2e/tamil-writing.lua --env out=<dir> [--env base=<exp://u.expo.dev/<projectId>/group/<group>/-- | http://127.0.0.1:8800/mla-tamagui>] [--env package=host.exp.exponent]
-- Gates: every traced letter at least 90% on the line, write path and words show "Practice complete" after self-check, zero JS warnings
-- per lesson and word. Input is touch only (the strokes follow the pad's own guide/template). The test records itself.
local android = device.platform == "android"
local out = env.out or (".agents/tmp/runs/tamil-writing-" .. device.platform)
local base = env.base or (not android and "http://127.0.0.1:8800/mla-tamagui" or nil)
assert_true(base, "pass --env base=<update link>; wf test run does")
local pkg = env.package or "host.exp.exponent"
os.execute("mkdir -p " .. out)
-- The screen in pixels (540x960 on the first device, 480x1040 now, a Galaxy S25 in dp). The width is 360 dp at any density, so a page scroll measured
-- on the first device (605 px) scales by SW / 540.
local SW, SH = 540, 960
if android then
  local s = device:get_device_state('screen'); SW, SH = s.width, s.height
end
local H = tonumber(env.height or SH)
-- a screen taller than 2.2:1 (the earlier 432x1200) shows the score badge, the pad and its buttons at once: no scrolling per letter
local tall = SH / SW > 2.2
local down_y = math.floor(H * 0.86)
local up_y = down_y - math.floor(605 * SW / 540 + 0.5)

-- the browser viewport shows the whole pad; only the phone needs scrolling past the keyboard-sized header
local function scroll_down() if android then device:swipe(('%d,%d'):format(9, down_y), ('%d,%d'):format(9, up_y), {duration=600}) end end
local function scroll_up() if android then device:swipe(('%d,%d'):format(9, up_y), ('%d,%d'):format(9, down_y), {duration=600}) end end
local function nudge_down() if not tall then scroll_down() end end
local function nudge_up() if not tall then scroll_up() end end

local function text_of(id) local e = device:get_element({id=id}) return e and e.text end
-- Expo Go opens its developer menu over the app now and then (first launch of a project); a user closes it with Back
local function close_dev_menu()
  -- a look, not a wait: the menu is already up or it is not, and a wait here cost 1 s before every stroke (18 Oct)
  if android and device:get_element({text='.*Toggle performance monitor.*'}) ~= nil then device:back() end
end

local steps, failures = {}, {}
local function rec(name, value, ok)
  steps[#steps + 1] = { step = name, value = value, ok = ok }
  log(name, tostring(value), ok and "ok" or "FAIL")
  if not ok then failures[#failures + 1] = name .. ": " .. tostring(value) end
end
local function warnings_gate(name)
  local w = device:get_logs()
  rec(name .. "/warnings", #w == 0 and "none" or w[1], #w == 0)
  device:clear_logs()
end
-- the screen opens by deep link on every platform (owner, 16 Oct); the app may be the one already running
local function open_route(path, wait_for, timeout) run_flow("open route", function()
  -- a unit that ended on Enter or Ctrl+End left the display out of touch mode, and a screen that mounts before the next touch takes focus on a control
  -- lower down and scrolls to it (CI run 44932009867: three lessons opened scrolled): the first act on a phone is a touch, here on the header's empty top edge
  if android then device:tap_on({point='50%,4.5%'}) end
  -- a reused app may still show this kind of screen from the last unit, and a link to it remounts it a moment later: the
  -- trace then follows the old template (18 Oct: word-uyir-aa drew அம்மா). The app leaves for Settings first.
  if android and device:get_element(wait_for) then
    device:open_link(base .. "/settings")
    device:assert_not_visible(wait_for)
  end
  -- Expo Go bundles the route on first load; a link that lands on the launcher (seen on the fifth cold start) is opened again
  for attempt = 1, 3 do
    device:open_link(base .. path)
    if device:extended_wait_until{visible=wait_for, timeout=timeout * 1000 // 3, optional=true} then break end
    if android then
      local top = tostring(device:run_shell("dumpsys activity activities | grep -m1 mResumedActivity")):gsub("%s+", " ")
      log(("%s not shown after %d s (attempt %d); resumed:%s"):format(path, timeout // 3, attempt, top))
    end
  end
  device:assert_visible(wait_for, {timeout=30000})
  close_dev_menu()
end) end

local function lesson(route, first, last)
  device:clear_logs()
  open_route("/tamil/" .. route, {id='TamilPracticeCanvasSurface'}, 240)
  -- a lesson opens at its top; one that opened scrolled fails here by name, not at the first Next or golden
  if android then device:assert_visible({id='TamilPracticePosition'}, {timeout=5000}) end
  -- a swipe flings to a different offset each run, so the golden is taken before it
  if android and first == 1 then device:assert_screenshot("tamil-" .. route .. "-open", { ignore = { { 0, 0, SW, math.floor(40 * SW / 540) } } }) end
  -- a later unit starts where a person would after the earlier letters: Next until its first letter is up
  for _ = 2, first do
    device:tap_on({id='TamilPracticeNext'})
    close_dev_menu(); device:assert_visible({id='TamilPracticeCanvasSurface'})
  end
  scroll_down()
  local function trace_and_score(letter)
    -- the coach's "Stroke 2 of 3", a Text on both platforms, changes in the render that shows the next stroke's guide; after a
    -- wait that saw no change it is read afresh, so a change that came late is not taken for the next stroke's
    local step
    for attempt = 1, 6 do
      close_dev_menu()
      step = step or text_of("TamilPracticeStrokeStep")
      local drew = device:draw_strokes_on({id='TamilPracticeCanvasSurface'}, {source='guide', human=true, optional=true}) ~= nil
      if not drew then device:extended_wait_until{visible={id='TamilPracticeCanvasActiveGuide'}, timeout=3000, optional=true} end -- the next letter's guide is still being drawn
      -- the score badge sits above the pad: look for it after every attempt, drawn or not, or a finished letter reads "no score"
      nudge_up()
      -- a letter with more strokes left shows no badge: a user draws on as soon as the next stroke's guide shows
      local now
      local seen = device:extended_wait_until{condition=function()
        now = text_of("TamilPracticeStrokeStep") -- read before the badge: a last stroke's new step always finds its badge
        if device:get_element({id='TamilPracticeStrokeScore'}) then return "score" end
        return step ~= nil and now ~= nil and now ~= step and "next"
      end, timeout=2000, interval=100, optional=true}
      if seen == "score" then break end
      log(("%s letter %d attempt %d: drew=%s, %s"):format(route, letter, attempt, tostring(drew), seen and ("next guide: " .. now) or "no score badge yet"))
      step = seen and now
      nudge_down()
    end
    local score = text_of("TamilPracticeStrokeScore") or ""
    return score, tonumber(score:match("(%d+)%% ON THE LINE"))
  end
  for letter = first, last do
    local score, pct = trace_and_score(letter)
    if pct == nil or pct < 90 then
      -- a person who lands under the line clears the pad and traces once more; the gate stays 90%
      rec(("%s/%d/attempt1"):format(route, letter), score ~= "" and score or "no score", false)
      -- Clear removes the strokes and the score; a retrace read before the old badge is gone would read the old score
      for _ = 1, 3 do
        nudge_down()
        if device:get_element({id='TamilPracticeClear'}) then device:tap_on({id='TamilPracticeClear'}) end
        nudge_up()
        if not device:extended_wait_until{visible={id='TamilPracticeStrokeScore'}, timeout=1000, optional=true} then break end
      end
      nudge_down()
      score, pct = trace_and_score(letter)
    end
    device:take_screenshot(("%s/%s-%d-score.png"):format(out, route, letter))
    rec(("%s/%d"):format(route, letter), score ~= "" and score or "no score", pct ~= nil and pct >= 90)
    if not tall then
      scroll_down()
      device:take_screenshot(("%s/%s-%d.png"):format(out, route, letter))
      scroll_up()
    end
    if letter < last then
      if device:get_element({id='TamilPracticeNext'}) then device:tap_on({id='TamilPracticeNext'}) end
      close_dev_menu(); device:assert_visible({id='TamilPracticeCanvasSurface'}); scroll_down()
    end
  end
  warnings_gate(route)
end

local function writepad(name)
  device:clear_logs()
  open_route("/tamil/write/" .. name, {id='TamilWriteCanvasSurface'}, 240)
  device:assert_visible({id='TamilWriteDone'}, {timeout=30000})
  device:draw_strokes_on({id='TamilWriteCanvasSurface'}, {source='template', human=true})
  device:take_screenshot(("%s/%s-ink.png"):format(out, name))
  device:tap_on({id='TamilWriteDone'})
  device:assert_visible({id='TamilWriteVerdict'}, {timeout=10000})
  device:take_screenshot(("%s/%s-complete.png"):format(out, name))
  local result = text_of("TamilWriteVerdict")
  rec(name .. "/final", result or "no result", result == "Practice complete")
  warnings_gate(name)
  return result == "Practice complete"
end

-- a unit runs one part (`--env part=vowels|consonant|write|words`), the vowels a few letters at a time (`--env letters=3-4`)
-- and the words one at a time (`--env word=<id>`), so each unit stays under a minute
local part = env.part
local function runs(name) return part == nil or part == name end
local from, to = tostring(env.letters or "1-6"):match("^(%d+)-(%d+)$")
assert_true(from ~= nil, "letters is a range like 3-4, not " .. tostring(env.letters))
if runs("vowels") then lesson("uyir-1", tonumber(from), tonumber(to)) end
if runs("consonant") then lesson("uyirmei-k", 1, 1) end
local ok_write = not runs("write") or writepad("uyir-a")
local ok_words = true
if runs("words") then
  local written = 0
  for _, w in ipairs({ "word-uyir-a", "word-uyir-aa", "word-uyir-i" }) do
    if env.word == nil or env.word == w then ok_words = writepad(w) and ok_words; written = written + 1 end
  end
  assert_true(written > 0, "no word " .. tostring(env.word))
end

local f = assert(io.open(out .. "/result.json", "w")); f:write(json.encode({ steps = steps, failures = failures })); f:close()
-- an early attempt may fail as long as a later one passes: the final verdict of each pad and the gates decide
local hard = {}
for _, s in ipairs(steps) do
  if not s.ok and not s.step:match("/attempt%d$") then hard[#hard + 1] = s.step .. " " .. tostring(s.value) end
end
assert_true(#hard == 0, "failed steps: " .. table.concat(hard, "; "))
assert_true(ok_write and ok_words, "a write-path self-check never reached Practice complete")
log("tamil writing: pass")