--[[---
platform: android, browser
against: android=update, browser=export
timeout: 180
units: chunk=1; chunk=2; chunk=3; chunk=4; chunk=5; chunk=6; chunk=7; chunk=8
focus: true
features: leetcode, coding-agent
---]]
-- device-lab voice edit: a problem solved by short spoken edits, one small change per turn. A prepared speech clip is the
-- microphone; the backend transcribes and plans for real. Deterministic mode takes the instructions from the list below.
-- With --env agent=1 a local agent picks each next small edit from the current code instead (non-deterministic mode); the
-- script keeps the guard (at most 18 words, no whole-solution ask), speaks it, applies it and gates every turn.
-- wf android|browser test e2e/voice-edit.lua --env out=<dir> --env problem=valid-palindrome [--env agent=1]
local android = device.platform == "android"
local out = env.out or (".agents/tmp/runs/voice-edit-" .. device.platform)
local base = env.base or (not android and "http://127.0.0.1:8800/mla-tamagui" or nil)
assert_true(base, "pass --env base=<update link>; wf test run does")
local problem = env.problem or "valid-palindrome"
-- a unit runs one chunk of a problem's turns (`--env chunk=N`, TURNS_PER_CHUNK each); a later chunk opens the editor on the
-- code the earlier turns leave (SEED: what the plan's edits make, as the passing run of 16 Oct did), so every unit stays short
local TURNS_PER_CHUNK = 1
local chunk = tonumber(env.chunk)
local function code(lines) return "class Solution:\n    def isPalindrome(self, s: str) -> bool:\n" .. table.concat(lines, "\n") end
local VP = {
  '        s = "".join(c.lower() for c in s if c.isalnum())',
  "        left, right = 0, len(s) - 1",
  "        while left < right:",
  "            if s[left] != s[right]:",
  "                return False",
  "            left += 1",
  "            right -= 1",
  "        return True",
}
local function upto(...) local l = {} for _, i in ipairs({ ... }) do l[#l + 1] = VP[i] end return code(l) end
local SEED = { ["valid-palindrome"] = {
  [2] = upto(1), [3] = upto(1, 2), [4] = upto(1, 2, 3) .. "\n            pass", [5] = upto(1, 2, 3, 4, 5),
  [6] = upto(1, 2, 3, 4, 5, 6), [7] = upto(1, 2, 3, 4, 5, 6, 7), [8] = upto(1, 2, 3, 4, 5, 6, 7, 8),
} }
-- a deep link on Android drops raw newlines from a query value, so the seed travels as base64url
local function base64url(s)
  local abc, out = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_", {}
  for i = 1, #s, 3 do
    local a, b, c = s:byte(i, i + 2)
    local n = a * 65536 + (b or 0) * 256 + (c or 0)
    for k = 1, 2 + (b and 1 or 0) + (c and 1 or 0) do
      local idx = math.floor(n / 64 ^ (4 - k)) % 64
      out[#out + 1] = abc:sub(idx + 1, idx + 1)
    end
  end
  return table.concat(out)
end
local use_agent = env.agent == "1"
local function exists(path) local f = io.open(path, "rb"); if f then f:close() end return f ~= nil end
-- the sandbox keeps the speech tool in its scratch folder, the VM worker on the shared workspace volume
local piper = env.piper or (exists(".agents/tmp/voice/tts-venv/bin/piper") and ".agents/tmp/voice/tts-venv/bin/piper" or "/workspace/home/voice/piper")
local model = env.model or (exists(".agents/tmp/voice/models/en_US-lessac-medium.onnx") and ".agents/tmp/voice/models/en_US-lessac-medium.onnx" or "/workspace/home/voice/models/en_US-lessac-medium.onnx")
os.execute("mkdir -p " .. out)
-- Expo Go bundles the route on first load; a fresh install also shows its one-time developer-menu intro, which a user dismisses with Continue
local function wait_route(sel, secs)
  local t0 = os.time()
  while os.time() - t0 < secs do
    if device:extended_wait_until{visible=sel, timeout=8000, optional=true} then return end
    if device:get_element({text='.*Continue.*'}) then device:tap_on({text='.*Continue.*'}) end
  end
  device:assert_visible(sel, {timeout=5000, label='route ready'})
end

-- The screen in pixels (540x960 on the first device, 480x1040 now, a Galaxy S25 in dp): positions measured on the first one are scaled. The width is
-- 360 dp at any density, so a length scales by SW / 540; things anchored to the bottom (keyboard, toolbar, buttons) are placed from there.
local SW, SH = 540, 960
if android then
  local s = device:get_device_state('screen'); SW, SH = s.width, s.height
end
local K = SW / 540
local function across(x) return math.floor(x * K + 0.5) end
local function from_top(y) return math.floor(y * K + 0.5) end
local function from_bottom(y) return SH - math.floor((960 - y) * K + 0.5) end
local function from_middle(y) return math.floor(SH / 2 + (y - 480) * K + 0.5) end
local PLAN = {
  ["valid-palindrome"] = { "set s to only its alphanumeric characters in lowercase", "after that line, declare left as zero and right as the last index of s",
    "add a while loop that runs while left is less than right, with a pass body", "replace pass with a return false if s at left differs from s at right",
    "after that if, add one to left", "then subtract one from right", "at the very end of the method, after the loop, add return true", "rename the variable left to lo" },
  ["two-sum"] = { "declare an empty dictionary called visited", "loop over the numbers with enumerate, calling the index i and the value n",
    "inside the loop, check whether target minus n is in visited", "inside that check, return visited at target minus n and i as a list",
    "after the check, set the entry of visited for the key n to i", "rename the variable visited to lookup" },
  ["container-with-most-water"] = { "declare left at zero, right at the last index, and best at zero", "add a while loop that runs while left is less than right",
    "inside the loop, compute the width as right minus left", "then compute the height as the smaller of the two heights",
    "then set best to the max of best and width times height", "then move the pointer on the shorter side inward", "return best after the loop", "rename the variable best to area" },
}

local function guard(say)
  local words = 0
  for _ in say:gmatch("%S+") do words = words + 1 end
  assert_true(words <= 18 and not say:lower():match("solution") and not say:lower():match("solve"), "voice turn is not a small edit: " .. say)
end
local function text_of(id) local e = device:get_element({id=id}) return e and e.text end
local function buffer() local e = device:get_element({id='CodeEditorInput'}) return e and e.text end
local function speak(say, n)
  -- each turn's clip is committed under e2e/voice, named by its words, so a runner needs no speech tool
  local fixture = "e2e/voice/" .. say:lower():gsub("[^a-z0-9]+", "-"):gsub("^-+", ""):gsub("-+$", "") .. ".wav"
  if exists(fixture) then return fixture end
  local wav = ("%s/%s-%d.wav"):format(out, problem, n)
  local p = io.popen(("printf '%%s' %q | %s -m %s -f %s >/dev/null 2>&1; echo $?"):format(say, piper, model, wav))
  local code = p:read("*l"); p:close()
  assert_true(code == "0", "piper failed for: " .. say)
  return wav
end

device:clear_logs()
local seeded = chunk and chunk > 1
-- the screen opens by deep link, seeded or not, on every platform (owner, 16 Oct); the app may be the one already running
-- a reused app may still show this screen: a link to it remounts it a moment later, so the first wait would pass on the
-- old one (18 Oct); the app leaves for Settings first (a link to "/" does not navigate a running Expo Go)
if android and device:get_element({id='ProblemEditorRunLocalButton'}) then
  device:open_link(base .. "/settings")
  device:assert_not_visible({id='ProblemEditorRunLocalButton'})
end
run_flow("open route", function()
  local link = base .. "/leetcode/" .. problem
  if seeded then
    local seed = SEED[problem] and SEED[problem][chunk]
    assert_true(seed ~= nil, ("no seed for %s chunk %d"):format(problem, chunk))
    link = link .. "?seed64=" .. base64url(seed)
  end
  device:open_link(link)
  wait_route({id='ProblemEditorRunLocalButton'}, 240)
end)
local autofocused = not android and device:extended_wait_until{visible={id='EditorHelperRails'}, timeout=3000, optional=true} -- the web editor autofocuses on open and its keyboard covers Reset
-- The toolbar is wider than the screen: a user swipes it until the key is on screen, then taps it.
local function done() -- the only way a user dismisses the keyboard: Done at the end of the toolbar
  -- a user swipes the row again until Done is on screen (18 Oct: one swipe left it at x 436 of 393)
  device:scroll_until_visible{element={id='EditorAction-Done'}, direction='RIGHT', from={id='EditorActionToolbar'}}
  device:tap_on({id='EditorAction-Done'})
  device:assert_not_visible({id='EditorHelperRails'})
end
if autofocused then done() end
if not seeded then
device:tap_on({id='ProblemEditorResetButton'})
-- the dialog is not in Android's accessibility tree (known gap): there the button is tapped where the dialog draws it
if android and not device:extended_wait_until{visible={id='ResetCodeDialog-Confirm'}, timeout=3000, optional=true} then
  -- read its Reset button's blue off the screen: a tap before the dialog is drawn lands on the editor and raises the keyboard
  local function blue(c) return c.b > 200 and c.r < 80 and c.g < 170 end
  local pt = ('%d,%d'):format(across(356), from_middle(578))
  local function drawn() return blue(device:get_pixel(pt)) end
  device:extended_wait_until{condition=drawn, timeout=10000, label='Reset dialog drawn'}
  for _ = 1, 3 do
    device:tap_on({point=('%d,%d'):format(across(400), from_middle(578))})
    if not drawn() then break end
  end
  assert_true(not drawn(), "the Reset dialog is still open")
else
  device:tap_on({id='ResetCodeDialog-Confirm'})
  device:assert_not_visible({id='ResetCodeDialog-Confirm'})
end
end

local turns = {}
local function next_instruction(n)
  if not use_agent then return PLAN[problem][n] end
  local r = agent.run{
    prompt = "You are testing a voice code editor on the problem '" .. problem .. "'. Current code:\n\n" .. (buffer() or "") ..
      "\n\nReply with ONE JSON line {\"say\": \"<one small edit, at most 16 words, never the whole solution>\"} that moves this code one step closer to a correct solution, or {\"say\": \"\"} when the solution is complete. Print nothing else.",
    json = true, timeout = 120 }
  assert_true(r.ok and r.result, "agent gave no instruction: " .. tostring(r.text):sub(1, 200))
  return r.result.say ~= "" and r.result.say or nil
end

local first, last = 1, 12
if chunk then first, last = (chunk - 1) * TURNS_PER_CHUNK + 1, chunk * TURNS_PER_CHUNK end
local n = first - 1
while true do
  n = n + 1
  local say = n <= last and next_instruction(n) or nil
  if not say then break end
  guard(say)
  device:clear_logs()
  local wav = speak(say, n)
  local fh = assert(io.open(wav, "rb")); local size = fh:seek("end"); fh:close()
  local clip_s = math.max(1, (size - 44) / 44100) -- piper writes 22.05 kHz 16-bit mono
  device:set_microphone_input(wav)
  if device:get_element({id='EditorHelperRails'}) then done() end
  device:tap_on({id='ProblemEditorVoiceStartButton'})
  if not android then device:assert_visible({id='ProblemEditorVoiceToolbar-Send'}) end
  local function mic_took() return device:get_device_state('microphone').consumed end
  -- the first recording asks for the microphone first; "ALLOW" alone would also match "DON'T ALLOW"
  if not device:extended_wait_until{condition=mic_took, timeout=4000, optional=true}
    and android and device:extended_wait_until{visible={text='WHILE USING THE APP'}, timeout=3000, optional=true} then
    device:tap_on({text='WHILE USING THE APP'})
  end
  device:extended_wait_until{condition=mic_took, timeout=10000, label='the recorder took the clip'}
  sleep_ms(clip_s * 1000) -- the clip plays in real time from the recorder's start: stopping earlier would cut the sentence
  if android then
    -- while recording, the waveform keeps uiautomator from going idle: Send is tapped where the toolbar draws it
    for i = 1, 8 do
      local x, y = across(467), from_bottom(i % 2 == 1 and 880 or 884)
      local pt = ('%d,%d'):format(x, y)
      -- a user taps Send only while they see it: once the sheet covers it, a tap there dismisses the sheet (16 Oct, units 2/4/6/7)
      local function covered(c) return c.b - c.r < 80 end -- blue: the Send button (and its icon); white or grey: the sheet is over it
      if covered(device:get_pixel(pt)) then break end
      device:tap_on({point=pt})
      device:extended_wait_until{condition=function() return covered(device:get_pixel(pt)) end, timeout=2000, optional=true}
      -- once stopped, the review sheet slides up over this spot: a second tap would focus its transcript and raise the keyboard
      if device:get_element({id='ProblemEditorVoiceReviewSheet-TranscriptLabel'}) or device:get_element({id='ProblemEditorVoiceToolbar-Send'}) == nil then break end
    end
  else
    -- the recorder can end by itself once the clip is over: then the review is already coming and Send is gone
    if device:get_element({id='ProblemEditorVoiceToolbar-Send'}) then device:tap_on({id='ProblemEditorVoiceToolbar-Send'}, {optional=true}) end
  end
  local heard
  run_flow("transcribe", function()
    device:assert_visible({id='ProblemEditorVoiceReviewSheet-TranscriptInput'}, {timeout=60000, label='transcribed'}) -- the transcription is a real backend call
    heard = text_of("ProblemEditorVoiceReviewSheet-TranscriptInput")
    -- the sheet slides up: its input is in the tree before Send is on screen
    device:assert_visible({id='ProblemEditorVoiceReviewSheet-Send'}, {timeout=30000})
  end)
  device:take_screenshot(("%s/%s-%d-review.png"):format(out, problem, n))
  device:tap_on({id='ProblemEditorVoiceReviewSheet-Send'})
  local outcome = "applied"
  run_flow("plan", function()
    -- planning is a real backend call too
    if not device:extended_wait_until{not_visible={id='ProblemEditorVoiceReviewSheet-TranscriptInput'}, timeout=60000, optional=true} then outcome = "timeout" end
    -- the sheet closes when planning starts; the edit lands when the "Thinking" strip goes (the planner takes 10 to 25 s)
    if device:extended_wait_until{visible={id='ProblemEditorVoiceProcessing'}, timeout=4000, optional=true}
      and not device:extended_wait_until{not_visible={id='ProblemEditorVoiceProcessing'}, timeout=120000, optional=true} then outcome = "timeout" end
  end)
  if device:get_element({id='ProblemEditorAlternativePicker-Close'}) then
    device:tap_on({id='ProblemEditorAlternativePicker-Close'}); outcome = "picker-closed"
  end
  device:take_screenshot(("%s/%s-%d-after.png"):format(out, problem, n))
  local w = device:get_logs()
  turns[#turns + 1] = { n = n, say = say, heard = heard, outcome = outcome, warnings = #w, buffer = buffer() or "" }
  assert_true(outcome == "applied", ("turn %d: outcome %s"):format(n, outcome))
  assert_true(#w == 0, ("turn %d: JS warning: %s"):format(n, tostring(w[1])))
  -- BetterStack holds these lines for the session: a turn that applied must have logged its transcription and its plan (a rename plans locally)
  local function info_log() return table.concat(device:get_logs{level='info'}, "\n") end
  local function planned() local t = info_log() return t:find("voice_edit.plan.response", 1, true) or t:find("voice_edit.plan.local", 1, true) end
  -- the strip can go before the planner answers: its line is awaited, not read once
  device:extended_wait_until{condition=planned, timeout=30000, optional=true}
  local info = info_log()
  assert_true(info:find("voice_edit.transcribe.response", 1, true), ("turn %d: no transcribe.response line in the app log"):format(n))
  assert_true(planned(), ("turn %d: no plan.response or plan.local line in the app log"):format(n))
end

local final = not chunk or last >= #(PLAN[problem] or {})
if not final then
  local f = assert(io.open(out .. "/result.json", "w")); f:write(json.encode({ problem = problem, chunk = chunk, turns = turns, agent = use_agent })); f:close()
  log(("voice edit %s chunk %d: pass"):format(problem, chunk))
  return
end
if device:get_element({id='EditorHelperRails'}) then done() end -- the keyboard covers the run buttons
run_flow("run local", function()
device:tap_on({id='ProblemEditorRunLocalButton'})
device:assert_visible({id='ProblemEditorRunOutcomeTotals'}, {timeout=120000}) -- the local run executes the code in the page and may load a runtime first
end)
local totals = text_of("ProblemEditorRunOutcomeTotals")
device:take_screenshot(("%s/%s-final.png"):format(out, problem))
local f = assert(io.open(out .. "/result.json", "w")); f:write(json.encode({ problem = problem, turns = turns, run_local = totals, agent = use_agent })); f:close()
assert_true(totals and totals:match("passed"), "Run Local did not pass: " .. tostring(totals))
log("voice edit " .. problem .. ": pass", totals)
