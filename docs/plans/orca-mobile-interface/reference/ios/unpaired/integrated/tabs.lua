local tabs = {
  {"feed", "10%,94%", "Connect your desktop"},
  {"Agents", "30%,94%", "No agents running"},
  {"Usage", "50%,94%", "Usage"},
  {"Automations", "70%,94%", "Schedule agent work"},
  {"Settings", "90%,94%", "Appearance"}
}
for _, tab in ipairs(tabs) do
  device:tap_on({point=tab[2]})
  device:assert_visible(tab[3])
  device:take_screenshot(env.out .. "/" .. tab[1] .. ".png")
end
