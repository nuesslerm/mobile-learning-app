local out = env.out
device:tap_on({point="7.5%,94%"})
device:assert_visible("Workspace options")
device:take_screenshot(out .. "/options.png")
local choices = {
  {"45.5", "Sort by", "sort"},
  {"50.4", "Project order", "project-order"},
  {"55.6", "Card layout", "card-layout"},
  {"60.6", "Show properties", "properties"},
}
for _, choice in ipairs(choices) do
  device:tap_on({point="80%," .. choice[1] .. "%"})
  device:assert_visible(choice[2])
  device:take_screenshot(out .. "/" .. choice[3] .. ".png")
  device:tap_on({point="8%,31.4%"})
  device:assert_visible("Workspace options")
end
