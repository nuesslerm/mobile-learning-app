-- Start on the settled checkout list. The official iOS build exposes these rows visually.
device:take_screenshot(env.out .. "/orca-host.png")
device:swipe(env.rowPoint, env.rowPoint, {duration=800})
sleep_ms(600)
device:take_screenshot(env.out .. "/orca-actions.png")
device:tap_on({point=env.renamePoint})
sleep_ms(1300)
device:take_screenshot(env.out .. "/orca-rename.png")
