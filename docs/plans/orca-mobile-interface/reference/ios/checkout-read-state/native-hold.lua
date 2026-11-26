-- Start with the fixture row centered at rowPoint on the connected host list.
device:take_screenshot(env.out..'/list.png')
device:swipe(env.rowPoint, env.rowPoint, {duration=800})
sleep_ms(650)
device:take_screenshot(env.out..'/menu.png')
