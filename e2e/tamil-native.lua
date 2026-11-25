--[[---
platform: android, ios
against: update
timeout: 180
units: case=resume; case=tiles; case=typed; case=listen; case=fill; case=review; case=trace; case=redraw; case=flash-english; case=flash-tamil; case=word; case=weak-letter; case=skip; case=library; case=double-rate
focus: true
features: tamil-writing
---]]
local open_library = dofile("e2e/support/native-learner.lua")
local file = assert(io.open("frontend/mla-tamagui/features/tamil-writing/data/lessons/course.json"))
local curriculum = json.decode(file:read("*a"))
file:close()
local function task_in(topic_id, task_id)
  for _, section in ipairs(curriculum.sections) do
    for _, topic in ipairs(section.topics) do
      if topic.id == topic_id then
        for _, task in ipairs(topic.tasks) do
          if task.id == task_id then return task end
        end
      end
    end
  end
  error("Bundled task missing: " .. task_id)
end
local topic = "romance"
local short = task_in(topic, "romance-acf7f2191b1e3a111fd2")
local function show(id, timeout) device:assert_visible({id=id}, {timeout=timeout or 12000}) end
local function tap(id) device:tap_on({id=id}) end
local function text(label) device:assert_visible({text=label}) end
local function open(route, ready)
  device:open_link(env.base .. route)
  show(ready, 30000)
end
local headings = {translate="Translate into Tamil", fill="Complete the sentence", listen="Tap what you hear"}
local function begin(topic_id, task)
  open("/tamil/lessons/" .. topic_id, "TamilTask-" .. task.id)
  tap("TamilTask-" .. task.id)
  tap("TamilStartPractice")
  text(headings[task.exercises[1].kind])
end
local function correct(exercise)
  local count = exercise.kind == "fill" and 1 or #exercise.words
  for i = 0, count - 1 do tap("TamilTile-bank-" .. i) end
  tap("TamilCheck")
  text("Correct")
end
local function wrong(exercise)
  local offset = exercise.kind == "fill" and 1 or #exercise.words
  tap("TamilTile-bank-" .. offset)
  tap("TamilCheck")
  text("Incorrect")
end
local deck = task_in(topic, "romance-5c682b9c7c2fa684d69d")
local function flash()
  local entry = "TamilFlashCards-" .. deck.id
  open("/tamil/lessons/" .. topic, entry)
  tap(entry)
  show("TamilFlashReveal")
end
local function played(id, label)
  tap(id)
  device:assert_visible({id=id, text=label .. ", recording played"})
end

local cases = {}
cases.resume = function()
  begin(topic, short)
  correct(short.exercises[1])
  tap("TamilContinue")
  text("2/" .. #short.exercises)
  local entry = "TamilTask-" .. short.id
  open("/tamil/lessons/" .. topic, entry)
  tap(entry)
  text("2/" .. #short.exercises)
  text(headings[short.exercises[2].kind])
end
cases.tiles = function()
  begin(topic, short)
  correct(short.exercises[1])
end
cases.typed = function()
  begin(topic, short)
  tap("TamilSwitchAnswer")
  tap("TamilTypedAnswer")
  device:extended_wait_until{condition=function() return device:get_device_state("keyboard").visible end}
  local answer = short.exercises[1].tamil
  device:input_text(answer, {replace=device.platform == "android"})
  device:assert_visible({id="TamilTypedAnswer", text=answer})
  device:tap_on({text=headings.translate})
  device:extended_wait_until{condition=function() return not device:get_device_state("keyboard").visible end}
  tap("TamilCheck")
  text("Correct")
end
cases.listen = function()
  local task = task_in("colors", "colors-e64ac07e1c54ea9701d8")
  begin("colors", task)
  played("TamilPlayAudio", "Play Tamil")
  played("TamilSlowAudio", "Slow")
  correct(task.exercises[1])
end
cases.fill = function()
  local task = task_in(topic, "romance-5c682b9c7c2fa684d69d")
  begin(topic, task)
  local exercise = task.exercises[1]
  wrong(exercise)
  text("Missing word · " .. exercise.words[exercise.blank+1])
end
cases.review = function()
  begin(topic, short)
  for i, exercise in ipairs(short.exercises) do
    if i == 1 then wrong(exercise) else correct(exercise) end
    tap("TamilContinue")
  end
  text("1 missed · 0 skipped")
  device:tap_on({text="Review missed and skipped"})
  text(short.exercises[1].english)
  correct(short.exercises[1])
  tap("TamilContinue")
  text("0 missed · 0 skipped")
end
local function trace(redraw)
  local pad = {id="TamilPracticeCanvasSurface"}
  open("/tamil/uyir-1", device.platform == "android" and "TamilPracticePosition" or "TamilPracticeUndo")
  if device.platform == "android" then
    local screen = device:get_device_state("screen")
    device:swipe(("9,%d"):format(math.floor(screen.height*.86)), ("9,%d"):format(math.floor(screen.height*.31)), {duration=600})
  end
  show("TamilPracticeUndo")
  if redraw then
    local box = assert(device:get_element(pad), "tracing surface missing").bounds
    device:swipe(("%d,%d"):format(box.left+8, box.top+8), ("%d,%d"):format(box.left+26, box.top+8), {duration=200})
    text("Stroke 1 of 2")
    device:assert_not_visible({text="[0-9]+% ON THE LINE"})
    tap("TamilPracticeClear")
  end
  device:draw_strokes_on(pad, {source="guide", human=true})
  text("Stroke 2 of 2")
  device:draw_strokes_on(pad, {source="guide", human=true})
  text("[0-9]+% ON THE LINE")
  local score = assert(device:get_element({text="[0-9]+% ON THE LINE"}).text):match("(%d+)%%")
  assert_true(tonumber(score) and tonumber(score) >= 90, "tracing score must reach 90%")
end
cases.word = function()
  open("/tamil/write/word-uyir-a", "TamilWriteCanvasSurface")
  device:assert_visible({text=".*அம்மா.*"})
  device:draw_strokes_on({id="TamilWriteCanvasSurface"}, {source="template", human=true})
  device:scroll_until_visible{element={id="TamilWriteDone"}, direction="DOWN"}
  tap("TamilWriteDone")
  text("Practice complete")
end
local function scroll_to(id)
  device:scroll_until_visible{element={id=id}, direction="DOWN"}
end
cases["weak-letter"] = function()
  open("/tamil/uyir-1", device.platform == "android" and "TamilPracticePosition" or "TamilPracticeUndo")
  scroll_to("TamilPracticeGotIt")
  tap("TamilPracticeGotIt")
  open("/tamil/alphabet", "TamilCourseReview")
  tap("TamilCourseReview")
  show("TamilReviewReveal")
  tap("TamilReviewReveal")
  text("ஆ")
  tap("TamilReviewReveal")
  tap("TamilReviewAgain")
  text("1 AGAIN")
end
cases.skip = function()
  local task = task_in("colors", "colors-e64ac07e1c54ea9701d8")
  begin("colors", task)
  tap("TamilSkipListening")
  for i = 2, #task.exercises do
    correct(task.exercises[i])
    tap("TamilContinue")
  end
  text("0 missed · 1 skipped")
  device:tap_on({text="Review missed and skipped"})
  text(headings.listen)
  correct(task.exercises[1])
  tap("TamilContinue")
  text("0 missed · 0 skipped")
end
cases.library = function()
  open("/tamil/handwriting", "TamilHandwritingSearch")
  tap("TamilHandwritingSearch")
  device:input_text("ஆ")
  tap("TamilCharacter-uyir-aa")
  text("ஆ · ā")
  show("TamilWriteCanvasSurface")
end
cases["double-rate"] = function()
  flash()
  tap("TamilFlashReveal")
  show("TamilFlashRate-Good")
  local box = assert(device:get_element({id="TamilFlashRate-Good"})).bounds
  local point = {point=("%d,%d"):format(box.cx, box.cy)}
  device:tap_on(point)
  device:tap_on(point)
  text(deck.exercises[2].english)
  open_library(device, env.base, true)
  flash()
  text("0 due · " .. (#deck.exercises - 1) .. " new")
  text(deck.exercises[2].english)
end
cases.trace = function() trace(false) end
cases.redraw = function() trace(true) end
cases["flash-english"] = function()
  flash()
  local first = deck.exercises[1]
  text(first.english)
  device:assert_not_visible({id="TamilFlashAudio"})
  device:assert_not_visible({text=first.tamil})
  tap("TamilFlashReveal")
  text(first.tamil)
  text(first.transliteration)
  played("TamilFlashAudio", "Play Tamil")
  tap("TamilFlashRate-Good")
  show("TamilFlashReveal")
  text(deck.exercises[2].english)
end
cases["flash-tamil"] = function()
  flash()
  tap("TamilFlash-tamil-first")
  for _, exercise in ipairs(deck.exercises) do
    text(exercise.tamil)
    show("TamilFlashAudio")
    device:assert_not_visible({text=exercise.english})
    tap("TamilFlashReveal")
    text(exercise.english)
    tap("TamilFlashRate-Easy")
  end
  local next_review = {text="Next review · .*"}
  device:assert_visible(next_review)
  local due = assert(device:get_element(next_review)).text
  open_library(device, env.base, true)
  local entry = "TamilFlashCards-" .. deck.id
  open("/tamil/lessons/" .. topic, entry)
  tap(entry)
  text(due)
  tap("TamilFlash-english-first")
  text("0 due · " .. #deck.exercises .. " new")
  text(deck.exercises[1].english)
end

run_flow("Given a Tamil learner, when practicing " .. env.case .. ", then progress and feedback match their actions", function()
  open_library(device, env.base)
  assert(cases[env.case], "unknown Tamil case")()
end)
