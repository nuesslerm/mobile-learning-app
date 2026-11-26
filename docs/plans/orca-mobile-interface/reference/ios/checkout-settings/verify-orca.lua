-- Start on the settled checkout list in Orca after pinning the test checkout in Expo.
device:take_screenshot(env.out .. "/orca-pinned.png")
device:swipe("26%,25%", "26%,25%", {duration=800})
sleep_ms(650)
device:take_screenshot(env.out .. "/orca-unpin-actions.png")
device:tap_on("Unpin")
sleep_ms(800)
