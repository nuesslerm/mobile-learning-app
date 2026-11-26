local out = env.out
device:tap_on("Workspace options")
device:assert_visible("Synced across your devices")
sleep_ms(500) -- The native sheet exposes its final accessibility bounds during its slide.
if env.restoreOnly == "true" then
  device:assert_visible("Card layout, " .. env.otherLayout)
  device:tap_on("Card layout, " .. env.otherLayout)
  device:tap_on(env.originalLayout)
  device:tap_on("Back to Workspace options")
  device:assert_visible("Card layout, " .. env.originalLayout)
  device:tap_on("Done")
  return
end

device:assert_visible("Hide sleeping")
device:take_screenshot(out .. "/expo-options.png")
for _, page in ipairs({{"Sort by","Agent Activity","sort"},{"Project order","Manual","project-order"},
                       {"Show properties","Agent activity layout","properties"}}) do
  device:tap_on(page[1] .. ".*")
  device:assert_visible(page[2])
  device:take_screenshot(out .. "/expo-" .. page[3] .. ".png")
  device:tap_on("Back to Workspace options")
end
device:tap_on("Card layout, " .. env.originalLayout)
device:assert_visible(env.otherLayout)
device:take_screenshot(out .. "/expo-card-layout.png")
device:tap_on(env.otherLayout)
device:tap_on("Back to Workspace options")
device:assert_visible("Card layout, " .. env.otherLayout)
device:tap_on("Done")
