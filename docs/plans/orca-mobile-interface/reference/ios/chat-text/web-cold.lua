device:assert_visible("Font, Monospace")
device:take_screenshot(env.out .. "/web-cold.png")
device:tap_on("Reset text settings")
device:assert_visible("Font, System")
