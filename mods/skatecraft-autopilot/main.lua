-- Test autopilot: scripted pushing and a slow turn instead of a gamepad.
-- Starts OFF; F9 toggles it. Every override is released when the autopilot is switched off.
local on = false
local started = false
local held_f9 = false
local t = 0

local function release()
  sdk.input.override_action("a", nil)
  sdk.input.override_action("left_stick_x", nil)
  sdk.input.override_action("left_stick_y", nil)
end

return {
  on_update = function(event)
    local dt = (event and tonumber(event.dt)) or 0
    if not started then
      started = true
      on = false  -- off by default: press F9 to start the test autopilot
    end
    local f9 = sdk.input.down("F9") == true
    if f9 and not held_f9 then
      on = not on
      if not on then release() end
      sdk.ui.text("autopilot", on and "autopilot ON" or "autopilot OFF")
    end
    held_f9 = f9
    if not on then return end
    t = t + dt
    -- push with the right foot every 0.7 s (hold A for 0.25 s), lean the stick forward, turn slowly
    local phase = t % 0.7
    sdk.input.override_action("a", phase < 0.25 and 1 or 0)
    sdk.input.override_action("left_stick_y", 0.9)
    sdk.input.override_action("left_stick_x", 0.18)
  end,
}
