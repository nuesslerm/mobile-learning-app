device:assert_visible('Search workspaces')
local function menu(label)
  device:assert_visible('native-fixture')
  local row=device:get_element('native-fixture')
  local lastY=-1
  for i=1,15 do
    if lastY==row.bounds.cy then break end
    lastY=row.bounds.cy
    sleep_ms(100)
    row=device:get_element('native-fixture')
  end
  local p=tostring(math.floor(row.bounds.cx))..','..tostring(math.floor(row.bounds.cy))
  device:swipe(p,p,{duration=900})
  device:assert_visible(label)
  local previous=-1
  for i=1,20 do
    local y=device:get_element(label).bounds.cy
    if y==previous and y<tonumber(env.height) then return end
    previous=y
    sleep_ms(100)
  end
  error('Workspace menu did not settle in the viewport')
end
if env.initialUnread=='true' then
  menu('Mark Read')
  device:tap_on('Mark Read')
  device:assert_not_visible('Mark Read')
end
menu('Mark Unread')
device:tap_on('Mark Unread')
device:assert_not_visible('Mark Unread')
device:take_screenshot(env.out..'/unread.png')
menu('Mark Read')
device:take_screenshot(env.out..'/menu.png')
device:tap_on('Mark Read')
device:assert_not_visible('Mark Read')
menu('Mark Unread')
device:tap_on('Dismiss')
