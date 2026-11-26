local function tap(x,y)
  device:tap_on({point=x..'%,'..y..'%'})
end
local function capture(name)
  device:take_screenshot(env.out..'/'..name..'.png')
end
local function check_sheet(name, x, y, outsideY)
  for n=1,2 do
    tap(x,y)
    sleep_ms(950)
    capture(name..'-open-'..n)
    tap(48,outsideY)
    sleep_ms(500)
    capture(name..'-closed-'..n)
  end
end
check_sheet('workspace-options',7.5,94,13)
tap(21,79)
sleep_ms(2200)
check_sheet('composer-add',7,92.2,30)
tap(83,9.2)
sleep_ms(1500)
check_sheet('switch-branch',12,20.9,11)
