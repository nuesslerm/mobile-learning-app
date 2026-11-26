local function waitUntil(check, description)
  for i=1,130 do
    if check() then return end
    sleep_ms(250)
  end
  error("Timed out waiting for " .. description)
end
local function green(point)
  local c=device:get_pixel(point)
  return c.g>c.r+45 and c.g>c.b+40
end
waitUntil(function() return green("60,172") end, "paired Feed")
device:tap_on({point="30%,16.8%"})
waitUntil(function() return green("20,160") end, "host")
device:tap_on({point="27%,79.3%"})
waitUntil(function()
 local c=device:get_pixel("10%,92%")
 return c.r<48 and c.g<48 and c.b<48
end, "chat add button")
device:tap_on({point="10%,92%"})
sleep_ms(850)
device:tap_on({point="27%,92%"})
sleep_ms(2000)
device:take_screenshot(env.out .. "/menu.png")
