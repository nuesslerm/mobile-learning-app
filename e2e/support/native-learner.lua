return function(device, base, relaunch)
  device:open_link((base:gsub("/%-%-$", "")), {relaunch=relaunch or false})
  local home = {id="LibraryHomeMenuButton"}
  local intro, menu = {text="This is the developer menu.*"}, {text="Reload"}
  device:extended_wait_until{condition=function()
    return device:get_element(home) ~= nil or device:get_element(intro) ~= nil or device:get_element(menu) ~= nil
  end, timeout=90000, label="Expo Go starts the learner library"}
  if device:get_element(intro) then
    device:tap_on({text="Continue"})
    device:extended_wait_until{condition=function()
      return device:get_element(menu) ~= nil or device:get_element(home) ~= nil
    end}
  end
  if device:get_element(menu) then
    if device.platform == "ios" then device:tap_on({text="Close"}) else device:back() end
  end
  device:assert_visible(home, {timeout=90000})
end
