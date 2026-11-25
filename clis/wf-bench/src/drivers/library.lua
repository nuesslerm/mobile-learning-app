if env.url then device:open_link(env.url) end
local function show(id) device:assert_visible({id=id}, {timeout=15000}) end
local function tap(id) device:tap_on({id=id}) end
show("TamilCollection-first-steps")
device:take_screenshot(env.out.."/start.png")
tap("TamilCollection-first-steps")
show("TamilPractice")
tap("TamilPractice")
show("TamilStartPractice")
device:take_screenshot(env.out.."/lesson.png")
device:tap_on({text="Back"})
show("TamilPractice")
device:tap_on({text="Back"})
show("TamilCollection-first-steps")
device:take_screenshot(env.out.."/finish.png")
