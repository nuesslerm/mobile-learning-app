-- Start on the settled list of 22 checkouts on the paired rca iPhone.
local function hold(point)
  device:swipe(point, point, {duration=800})
  sleep_ms(600)
end
local function tap(point)
  device:tap_on({point=point})
end
hold(env.rowPoint)
device:take_screenshot(env.out .. "/expo-actions.png")
tap("20%,87.6%")
sleep_ms(1300)
device:take_screenshot(env.out .. "/expo-rename.png")
-- The existing name is selected. Enter pre on the English iOS keyboard.
for _, point in ipairs({"95%,71.4%", "35%,71.4%", "25%,71.4%"}) do
  tap(point)
  sleep_ms(200)
end
sleep_ms(600)
device:take_screenshot(env.out .. "/expo-editing.png")
tap("89%,63%")
sleep_ms(900)
device:take_screenshot(env.out .. "/expo-saved.png")
hold(env.renamedRowPoint)
tap("20%,92%")
sleep_ms(900)
device:take_screenshot(env.out .. "/expo-pinned.png")
