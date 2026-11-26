-- Start on the paired host list with Project selected and the checkout sections expanded.
local function tap(x,y)
  device:tap_on({point=x..'%,'..y..'%'})
  sleep_ms(850)
end
tap(7.5,94)
tap(38,39)
tap(89,36.5)
device:take_screenshot(env.out..'/'..env.client..'-expanded.png')
tap(18,21.3)
device:take_screenshot(env.out..'/'..env.client..'-collapsed.png')
tap(18,21.3)
