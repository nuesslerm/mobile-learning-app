--[[---
platform: android, ios
against: update
timeout: 180
units: case=home; case=settings; case=pairing; case=skin
focus: true
features: coding-agent
---]]
-- The coding agent on a phone with no desktop paired: the home and its tabs, its settings, the way to the pairing code, and the choice of skin.
-- wf android|ios test e2e/rca-native.lua --device <id> --env base=exp://u.expo.dev/<projectId>/group/<group>/-- --env case=<case> [--env relaunch=1]
local open_library = dofile("e2e/support/native-learner.lua")
local function show(id, timeout) device:assert_visible({id=id}, {timeout=timeout or 12000}) end
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
cases.pairing = function()
  home()
  device:tap_on({text="Pair desktop"})
  -- a phone that already let the camera in opens on the scanner, whose way to the code reads differently
  local paste = "(Paste code instead|Or paste pairing code)"
  text(paste, 30000)
  device:tap_on({text=paste})
  text("Paste pairing code")
  show("MobilePairCode")
  device:tap_on({text="Cancel"})
  text(paste)
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
  chosen("Orca", "Chat text size")
end

run_flow("Given a phone with no desktop paired, when the user opens " .. env.case .. ", then the coding agent shows it", function()
  -- the stage starts every case from a stopped app; a run by hand on an app left open passes --env relaunch=1
  open_library(device, env.base, env.relaunch == "1")
  assert(cases[env.case], "unknown coding agent case")()
end)
