device:assert_visible("Copy code")
device:assert_not_visible("Wrap lines")
device:take_screenshot(env.out.."/settings.png")
device:tap_on("Copy code")
