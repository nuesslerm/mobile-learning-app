return function(device, base, relaunch)
  device:open_link((base:gsub("/%-%-$", "")), {relaunch=relaunch or false})
  local home = {id="LibraryHomeMenuButton"}
  if device.platform == "android" then
    device:extended_wait_until{condition=function()
      if device:get_element({text="This is the developer menu.*"}) then
        device:tap_on({text="Continue"})
      end
      if device:get_element({text=".*Toggle performance monitor.*"}) or device:get_element({text="Reload"}) then
        device:back()
      end
      return device:get_element(home) ~= nil
    end, timeout=90000, label="Expo Go opens the learner library"}
  else
    device:assert_visible(home, {timeout=90000})
  end
end
