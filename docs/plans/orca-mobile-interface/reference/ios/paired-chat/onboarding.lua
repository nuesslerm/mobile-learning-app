local out = env.out
device:assert_visible("Open sessions in Chat UI")
device:take_screenshot(out .. "/open-sessions.png")
device:tap_on("Open sessions in Chat UI")
device:assert_visible("Skip notifications for now")
device:take_screenshot(out .. "/notifications.png")
device:tap_on("Skip notifications for now")
