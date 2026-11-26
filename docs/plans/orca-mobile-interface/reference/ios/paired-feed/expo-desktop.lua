local function connected()
  local dot = device:get_pixel("60,172")
  return dot.g > dot.r + 45 and dot.g > dot.b + 40
end
local function inHost()
  local dot = device:get_pixel("20,160")
  return dot.g > dot.r + 45 and dot.g > dot.b + 40
end
local function waitFor(screen, description)
  for i = 1, 45 do
    if screen() then return end
    sleep_ms(300)
  end
  error(description)
end
waitFor(connected, "Expected the paired desktop")
device:take_screenshot(env.out .. "/expo-feed.png")
device:tap_on({point="30%,16.8%"})
waitFor(inHost, "Expected the desktop workspace list")
sleep_ms(350)
device:take_screenshot(env.out .. "/expo-host.png")
device:tap_on({point="7%,9.2%"})
waitFor(connected, "Expected Back to return to the same desktop")
sleep_ms(350)
device:take_screenshot(env.out .. "/expo-return.png")
