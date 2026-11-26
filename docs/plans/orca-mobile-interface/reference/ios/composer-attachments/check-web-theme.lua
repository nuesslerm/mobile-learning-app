-- Given a paired browser using the requested system appearance.
local expected = {dark = {23, 10, 31}, light = {250, 255, 255}}
local surfaces = assert(expected[env.scheme], "Select dark or light")

-- When the exported chat is opened directly with a saved conversation.
device:assert_visible("Add to message")
device:assert_visible(".*RCA_ORCA_COLD_FOLLOW_FINAL.*")

-- Then the header, transcript and composer all use the requested appearance.
for i, point in ipairs({"48%,2%", "97%,47%", "70%,93%"}) do
  local pixel = device:get_pixel(point)
  local value = surfaces[i]
  assert(pixel.r == value and pixel.g == value and pixel.b == value,
    "Unexpected " .. env.scheme .. " surface at " .. point)
end
device:take_screenshot(env.out .. "/" .. env.scheme .. ".png")
