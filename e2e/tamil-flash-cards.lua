--[[---
platform: android, browser
against: android=update, browser=export
timeout: 120
focus: true
features: tamil-writing
---]]
run_flow("Given a Tamil task, when a phrase is revealed and rated, then the next card is face down", function()
  device:open_link(env.base .. "/tamil/lessons/greetings")
  local entry = {id="TamilFlashCards-greetings-3129c99db4dff1b721e1"}
  device:assert_visible(entry, {timeout=60000})
  device:tap_on(entry)
  device:assert_visible({id="TamilFlashReveal"})
  device:tap_on({id="TamilFlashReveal"})
  device:assert_visible({id="TamilFlashAudio"})
  device:take_screenshot(env.out .. "/revealed.png")
  device:tap_on({id="TamilFlashRate-Good"})
  device:assert_visible({id="TamilFlashReveal"})
  device:assert_not_visible({id="TamilFlashRate-Good"})
end)
