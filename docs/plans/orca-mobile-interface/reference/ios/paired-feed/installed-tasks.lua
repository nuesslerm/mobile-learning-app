sleep_ms(700)
device:take_screenshot(env.out .. "/orca-feed.png")
device:tap_on({point="25%,35.8%"})
sleep_ms(900)
device:take_screenshot(env.out .. "/orca-tasks-picker.png")
device:tap_on({point="37%,91.3%"})
sleep_ms(600)
device:take_screenshot(env.out .. "/orca-tasks-open.png")
local function hasUpdateNotice()
  return device:get_pixel("159,520").r < 80
end
for i = 1, 65 do
  if hasUpdateNotice() then break end
  sleep_ms(1000)
end
assert(hasUpdateNotice(), "Expected the unsupported-host notice")
device:take_screenshot(env.out .. "/orca-tasks-result.png")
device:tap_on({point="7%,9.2%"})
sleep_ms(800)
device:take_screenshot(env.out .. "/orca-return.png")
