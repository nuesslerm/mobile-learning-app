local function keyboard(want)
  device:extended_wait_until{condition=function() return device:get_device_state("keyboard").visible == want end,
    timeout=10000, label=want and "system keyboard opened" or "system keyboard dismissed"}
end
local function focus_end(first_line)
  local box = device.platform == "android" and device:assert_visible({id="CodeEditorInput"})
  if box then
    device:tap_on({point=("%d,%d"):format(box.bounds.cx, math.floor(box.bounds.top + 25))})
  else
    local bar = device:assert_visible({id="RNSScreen"}).bounds
    device:tap_on({point=("%d,%d"):format(bar.cx, bar.bottom + 12)})
  end
  keyboard(true)
  if device.platform == "ios" then
    local bar = device:assert_visible({id="RNSScreen"}).bounds
    local screen = device:get_device_state("screen")
    local startup = device:get_element({id="ProblemEditorStartupInput"})
    local y = first_line and (bar.bottom + (startup and 12 or 2)) or math.floor(screen.height*.29)
    device:tap_on({point=("%d,%d"):format(math.floor(screen.width*.83), y)})
  elseif not first_line then
    device:press_key("Ctrl+End")
  elseif box then
    device:tap_on({point=("%d,%d"):format(box.bounds.right-16, box.bounds.top+15)})
  end
end
local function hide_keyboard()
  if not device:get_device_state("keyboard").visible then return end
  local hide = {text="Hide [Kk]eyboard"}
  if device:get_element(hide) then device:tap_on(hide)
  elseif device.platform == "android" then device:tap_on({id="com.android.systemui:id/back"})
  elseif device:get_element({id="ProblemEditorTapAway"}) then device:tap_on({id="ProblemEditorTapAway"})
  else
    device:scroll_until_visible{element={id="EditorAction-Done"}, direction="RIGHT", from={id="EditorActionToolbar"}}
    device:tap_on({id="EditorAction-Done"})
  end
  keyboard(false)
end
local function append(text)
  focus_end(true)
  device:input_text(text)
  hide_keyboard()
end
local function type_in_starter(text, row)
  focus_end(false)
  if device.platform == "ios" then
    local header = device:assert_visible({id="RNSScreen"}).bounds
    device:tap_on({point=("%d,%d"):format(header.cx, header.bottom+12+(row-1)*21)})
  elseif row == 8 then
    device:press_key("Remote Dpad Up")
    device:press_key("Remote Dpad Up")
  end
  device:input_text(text)
  if device.platform == "android" then device:assert_visible({id="CodeEditorText", text=".*" .. text .. ".*"}) end
  hide_keyboard()
end
-- Typing on a simulator brings the app back to the front and the keyboard rises again: a tap before the control has moved above it
-- lands on a key (Tamil's Check became a typed "t"). The simulator gives its keyboard no frame, so the top row of keys stands in.
local function tap_above_keyboard(sel)
  device:extended_wait_until{condition=function()
    local control = device:get_element(sel)
    if not control then return false end
    local shown = device:get_device_state("keyboard")
    if not shown.visible then return true end
    local key = device:get_element({text="[qQ]"})
    local top = shown.bounds and shown.bounds.top or key and key.bounds.top
    return top == nil or control.bounds.bottom <= top + 2
  end, timeout=10000, label="the control sits above the keyboard"}
  device:tap_on(sel)
end
return {keyboard=keyboard, focus_end=focus_end, hide_keyboard=hide_keyboard, append=append, type_in_starter=type_in_starter, tap_above_keyboard=tap_above_keyboard}
