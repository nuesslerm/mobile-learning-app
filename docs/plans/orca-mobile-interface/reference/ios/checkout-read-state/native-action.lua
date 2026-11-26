-- Confirm the checkout name and the action in the captured menu before selecting its point.
device:tap_on({point=env.actionPoint})
sleep_ms(1300)
device:take_screenshot(env.out..'/result.png')
