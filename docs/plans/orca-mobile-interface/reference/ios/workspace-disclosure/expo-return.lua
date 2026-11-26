device:tap_on({point="18%,21.3%"})
sleep_ms(800)
device:tap_on({point="7%,9.3%"})
sleep_ms(950)
device:tap_on({point="26%,17%"})
sleep_ms(1700)

device:take_screenshot(env.out..'/verified-expo-fold.png')
