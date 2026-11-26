-- Start on the expanded Status list.
local function tap(x,y)
  device:tap_on({point=x..'%,'..y..'%'})
  sleep_ms(900)
end
tap(7.5,94)
tap(85,44)
tap(89,31.5)
device:take_screenshot(env.out..'/'..env.client..'-restored-project.png')
