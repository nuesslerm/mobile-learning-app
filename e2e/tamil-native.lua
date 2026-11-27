--[[---
platform: android, ios
against: update
timeout: 180
units: case=library; case=tiles; case=typed; case=resume; case=trace
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
  -- the label changes when the recording ends: on the hosted emulator both flash cases met it 0.7 s after the default 10 s (run 46037745674)
  device:assert_visible({id=id, text=label .. ", recording played"}, {timeout=30000})
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
  if device.platform == "android" then
    device:tap_on({text=headings.translate})
    device:extended_wait_until{condition=function() return not device:get_device_state("keyboard").visible end}
  end
  tap("TamilCheck")
  text("Correct")
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
local function scroll_to(id)
  device:scroll_until_visible{element={id=id}, direction="DOWN"}
end
cases.library = function()
  open("/tamil", "TamilAllTopics")
  tap("TamilAllTopics")
  show("TamilTaskSearch")
  tap("TamilTaskSearch")
  local query = "your number"
  device:input_text(query)
  device:assert_visible({id="TamilTaskSearch", text=query})
  tap("TamilTask-" .. deck.id)
  tap("TamilStartPractice")
  text(headings.fill)
  text(deck.exercises[1].english)
end
cases.trace = function() trace(false) end

run_flow("Given a Tamil learner, when practicing " .. env.case .. ", then progress and feedback match their actions", function()
  open_library(device, env.base)
  assert(cases[env.case], "unknown Tamil case")()
end)
