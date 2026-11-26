device:tap_on('Open Orca parity')
device:assert_visible('Search workspaces')
device:tap_on('Search workspaces')
device:input_text('native-fixture')
device:press_key('Enter')
device:assert_visible('native-fixture')
local function tapSettled(label)
  local previous
  local stable=0
  for i=1,20 do
    local bounds=device:get_element(label).bounds
    stable = previous==bounds.cy and stable+1 or 0
    -- Wait through the 250 ms web sheet transition before placing a touch.
    if stable>=3 and bounds.cy<840 then
      device:tap_on({point=tostring(math.floor(bounds.cx))..','..tostring(math.floor(bounds.cy))})
      return
    end
    previous=bounds.cy
    sleep_ms(100)
  end
  error('Action did not settle: '..label)
end
local function open()
  local row = device:get_element('native-fixture')
  local p = tostring(math.floor(row.bounds.cx))..','..tostring(math.floor(row.bounds.cy))
  device:swipe(p,p,{duration=850})
  device:assert_visible('Move to Status')
  tapSettled('Move to Status')
  device:assert_visible('In review')
  device:assert_visible('Todo')
  device:assert_visible('Done')
end
open()
device:take_screenshot(env.out..'/status.png')
tapSettled('In review')
device:assert_not_visible('Move to Status')
open()
device:take_screenshot(env.out..'/selected.png')
tapSettled('In progress')
device:assert_not_visible('Move to Status')
