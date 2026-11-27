--[[---
platform: browser
against: export
timeout: 900
focus: true
features: *
---]]
-- The skin gallery (issue #106): every entry of the skin contract with fixed props, drawn by each skin in light and dark. A golden per
-- section pins both looks, so a change to one skin shows up as that skin's diff and nothing else. The skin is chosen where a person
-- chooses it (Settings, Appearance, Design); the gallery route pins the scheme. Waits are on elements and on bounds that hold still.
-- wf test run skin-gallery   (wf browser test e2e/skin-gallery.lua --env base=http://127.0.0.1:8800/mla-tamagui)
local base = env.base or "http://127.0.0.1:8800/mla-tamagui"

local SECTIONS = {
  "settings", "home", "unpaired", "onboarding", "pairing", "chat", "suggestions", "workspaces", "source-control", "tab-controls", "layout", "controls", "chat-settings",
  "sheet-home-actions", "sheet-new-tab", "sheet-session-tabs", "sheet-composer-options",
  "sheet-workspace-actions", "sheet-workspace-options", "sheet-plain", "sheet-options", "image-viewer",
}
-- These two focus a field, which raises the keyboard, and the browser frame keeps it up for every later page: they run last.
local WITH_KEYBOARD = { "search", "sheet-pairing-code" }

-- A sheet slides in: the screen is ready when five points across it show the same colours three looks in a row.
local function settled()
  local screen = device:get_device_state("screen")
  local last, steady = nil, 0
  device:extended_wait_until{condition=function()
    local seen = {}
    for _, at in ipairs({{.5, .2}, {.5, .5}, {.2, .7}, {.8, .7}, {.5, .9}}) do
      local p = device:get_pixel(("%d,%d"):format(math.floor(screen.width * at[1]), math.floor(screen.height * at[2])))
      seen[#seen + 1] = ("%d.%d.%d"):format(p.r, p.g, p.b)
    end
    local now = table.concat(seen, " ")
    steady = now == last and steady + 1 or 0
    last = now
    return steady >= 2
  end, timeout=15000, interval=150, label='the screen holds still'}
end

local function choose_skin(skin)
  device:open_link(base .. "/settings/appearance")
  device:assert_visible({id='AppearanceSettingsScreen-DesignControl-' .. skin}, {timeout=60000})
  settled()
  device:tap_on({id='AppearanceSettingsScreen-DesignControl-' .. skin})
end

local function capture(skin, sections)
  run_flow("Given the " .. skin .. " skin is chosen in Settings", function() choose_skin(skin) end)
  for _, scheme in ipairs({"light", "dark"}) do
    for _, section in ipairs(sections) do
      run_flow(("When the %s section opens in %s, then the %s skin draws it as its golden"):format(section, scheme, skin), function()
        device:open_link(("%s/gallery?section=%s&scheme=%s"):format(base, section, scheme))
        device:assert_visible({id='SkinGallery-' .. section}, {timeout=60000})
        settled()
        device:assert_screenshot(("gallery-%s-%s-%s"):format(skin, scheme, section))
      end)
    end
  end
end

for _, skin in ipairs({"tamagui", "orca"}) do capture(skin, SECTIONS) end
for _, skin in ipairs({"tamagui", "orca"}) do capture(skin, WITH_KEYBOARD) end
