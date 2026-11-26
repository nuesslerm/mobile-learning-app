local function tapStable(label)
  local previousX, previousY, stable = nil, nil, 0
  for i=1,25 do
    local bounds = device:get_element(label).bounds
    stable = previousX == bounds.cx and previousY == bounds.cy and stable+1 or 0
    if stable >= 4 then
      device:tap_on({point=tostring(math.floor(bounds.cx))..','..tostring(math.floor(bounds.cy))})
      return
    end
    previousX, previousY = bounds.cx, bounds.cy
    sleep_ms(100)
  end
  error(label..' did not settle')
end
device:tap_on('Open Orca parity')
device:assert_visible('Search workspaces')
tapStable('Workspace options')
device:assert_visible('Group by')
sleep_ms(400)
device:tap_on('Status')
device:assert_not_visible('Project order')
sleep_ms(400)
device:tap_on('Done')
device:assert_visible('In progress '..env.count)
device:take_screenshot(env.out..'/expanded.png')
device:tap_on('In progress '..env.count)
device:assert_not_visible('native-fixture')
device:take_screenshot(env.out..'/collapsed.png')
device:tap_on('In progress '..env.count)
device:tap_on('Search workspaces')
device:input_text('native-fixture')
device:press_key('Enter')
device:assert_visible('In progress 1')
device:assert_visible('native-fixture')
device:take_screenshot(env.out..'/search.png')
tapStable('Workspace options')
device:assert_visible('Group by')
sleep_ms(400)
device:tap_on('Project')
device:assert_visible('Project order')
sleep_ms(400)
device:tap_on('Done')
device:assert_visible(env.project..' 1')
