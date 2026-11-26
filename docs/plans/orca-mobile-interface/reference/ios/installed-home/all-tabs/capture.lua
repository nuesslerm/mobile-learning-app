local tabs = {{"feed","10%"},{"bots","30%"},{"agents","50%"},{"usage","70%"},{"automations","90%"},{"return","10%"}}
for _, tab in ipairs(tabs) do
  device:tap_on({point=tab[2]..",93.6%"})
  sleep_ms(1100)
  device:take_screenshot(env.out.."/"..tab[1]..".png")
end
