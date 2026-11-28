--[[---
platform: android, ios
against: update
timeout: 240
units: case=home; case=settings; case=skin; case=paired
focus: true
features: coding-agent
---]]
-- The coding agent on a phone: with no desktop paired its home and tabs, its settings and the choice of skin; then the journey that matters most,
-- pairing with a desktop by its code and chatting with an agent in one of its worktrees.
-- wf android|ios test e2e/rca-native.lua --device <id> --env base=exp://u.expo.dev/<projectId>/group/<group>/-- --env case=<case> [--env relaunch=1]
local open_library = dofile("e2e/support/native-learner.lua")
local input = dofile("e2e/support/native-editor.lua")
local function tap(id) device:tap_on({id=id}) end
local function text(label, timeout) device:assert_visible({text=label}, {timeout=timeout or 12000}) end
local function open(route)
  device:open_link(env.base .. route)
end
local function home()
  open("/coding-agent")
  text("Connect your desktop", 30000)
end

local cases = {}
cases.home = function()
  home()
  tap("HomeTab-agents")
  text("No agents running")
  tap("HomeTab-usage")
  text("No usage yet")
  tap("HomeTab-feed")
  text("Connect your desktop")
end
cases.settings = function()
  open("/coding-agent/settings")
  text("Chat UI", 30000)
  device:tap_on({text="Chat UI"})
  text("Why is the resume test flaky\\?")
  text("Line spacing")
end
-- needs a desktop this device reaches: the stage's wrapper mints pair_url on the one `wf test backends up --only agent-desktop` started
cases.paired = function()
  assert(env.pair_url, "the paired case needs --env pair_url=<a pairing link of a desktop this device reaches>")
  open("/coding-agent")
  -- a simulator keeps a pairing in its keychain when the app's data is cleared: a phone that holds one offers the next under its list
  local pair = "(Pair desktop|Pair a desktop)"
  text(pair, 30000)
  device:tap_on({text=pair})
  -- a phone that already let the camera in opens on the scanner, whose way to the code reads differently
  local paste = "(Paste code instead|Or paste pairing code)"
  text(paste, 30000)
  device:tap_on({text=paste})
  tap("MobilePairCode")
  input.keyboard(true)
  device:input_text(env.pair_url)
  input.tap_above_keyboard({text="Save"})
  -- the desktop's row reads as its action in one skin and as its name in the other
  local desktop = "(Open )?" .. (env.desktop or "rca")
  text(desktop, 30000)
  device:tap_on({text=desktop})
  text("Search workspaces", 30000)
  device:tap_on({text=".*main.*"})
  device:tap_on({text="New Tab"})
  device:tap_on({text="Claude"})
  device:tap_on({text="Ask anything.*"})
  input.keyboard(true)
  device:input_text("Say hello")
  input.tap_above_keyboard({text="Send message"})
  text("Say hello", 30000)
  text(env.reply or "Hello from the scripted agent", 60000)
end
-- the Design control's segments reach the simulator's tree as their labels only
local function choose(label)
  open("/settings/appearance")
  text("Design", 30000)
  device:tap_on({text=label})
end
cases.skin = function()
  -- the two skins draw the text size differently: steps in the app's own, a slider in Orca's.
  -- A tap while the Appearance screen is still sliding in misses the control, so the choice is repeated until the chat settings show it.
  local function chosen(label, sign)
    for _ = 1, 3 do
      choose(label)
      open("/coding-agent/native-chat-settings")
      text("Chat UI", 30000)
      -- on a short screen the control sits under the preview
      if device:scroll_until_visible{element={text=sign}, direction='DOWN', timeout=15000, optional=true} then return end
    end
    text(sign)
  end
  chosen("App", "Larger text")
  -- the emulator reads the slider as its name and its value
  chosen("Orca", "Chat text size.*")
end

run_flow("Given a phone with no desktop paired, when the user goes through " .. env.case .. ", then the coding agent shows it", function()
  -- the stage starts every case from a stopped app; a run by hand on an app left open passes --env relaunch=1
  open_library(device, env.base, env.relaunch == "1")
  assert(cases[env.case], "unknown coding agent case")()
end)
