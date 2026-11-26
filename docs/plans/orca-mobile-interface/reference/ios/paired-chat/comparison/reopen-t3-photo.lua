local out = env.out
local tools = "Read 2 files and ran 2 commands"
device:assert_visible(tools)
device:assert_visible(env.firstImage)
device:take_screenshot(out .. "/mla-t3-photo-thread.png")
device:tap_on(tools)
device:take_screenshot(out .. "/mla-t3-tool-details.png")
device:tap_on(tools)
device:tap_on(env.firstImage)
device:assert_visible("Close image")
device:take_screenshot(out .. "/mla-t3-image-viewer.png")
device:tap_on("Close image")
