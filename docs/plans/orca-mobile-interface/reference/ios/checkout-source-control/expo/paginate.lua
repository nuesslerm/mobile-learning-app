for i=1,6 do
  device:swipe('60%,86%','60%,23%',{duration=460})
  sleep_ms(400)
end
sleep_ms(900)
device:take_screenshot(env.out..'/paged.png')
