device:tap_on({point="18%,88%"})
sleep_ms(1100)
for _, key in ipairs({"35%,71.5%","25%,71.5%","20%,77.2%","65%,71.5%","79%,83.2%","25%,71.5%"}) do
  device:tap_on({point=key})
  sleep_ms(110)
end
sleep_ms(450)
device:take_screenshot(env.out .. "/official-typed.png")
