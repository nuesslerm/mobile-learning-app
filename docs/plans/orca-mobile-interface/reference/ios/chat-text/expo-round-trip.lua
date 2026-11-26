local function tap(point)
  device:tap_on({point=point})
  sleep_ms(600)
end
local function shot(name)
  device:take_screenshot(env.out .. "/" .. name .. ".png")
end
local function openText()
  tap("94%,9.2%")
  tap("57%,18.8%")
end
local function defaultSizeVisible()
  local color=device:get_pixel("20%,51.52%")
  return color.b>200 and color.r<45 and color.g<160
end
local function resetText()
  tap("24%,91.2%")
  for i=1,16 do
    if defaultSizeVisible() then return end
    sleep_ms(180)
  end
  error("Reset did not restore the default slider and preview")
end
resetText()
shot("settings-default")
tap("7%,9.2%")
shot("chat-default")
openText()
shot("sheet-default")
tap("35%,92.2%")
tap("72%,57.2%")
shot("fonts")
tap("19%,84.9%")
tap("21%,66.1%")
tap("70%,51.5%")
shot("settings-edited")
tap("7%,9.2%")
shot("chat-edited")
openText()
shot("sheet-edited")
tap("35%,92.2%")
device:swipe("50%,87%","51%,36%",{duration=500})
sleep_ms(1800)
shot("reset-row")
resetText()
tap("7%,9.2%")
shot("chat-restored")
