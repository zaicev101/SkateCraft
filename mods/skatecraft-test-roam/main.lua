-- Test only: push (A for 0.3 s) every 1.5 s, steer right for 1.2 s at the end of each 10 s cycle.
local t = 0
local logt = 0
return {
  on_update = function(event)
    local dt = (event and tonumber(event.dt)) or 0
    t = t + dt
    logt = logt + dt
    if t > 6 then
      local c = (t - 6) % 10
      if (c % 1.5) < 0.3 then sdk.input.override_action("a", 1) else sdk.input.override_action("a", nil) end
      if c > 8.8 then sdk.input.override_action("left_stick_x", 1) else sdk.input.override_action("left_stick_x", nil) end
    end
    if logt >= 1.0 then
      logt = 0
      local p = sdk.player.read()
      if p then
        sdk.log(string.format("ROAM t=%.0f speed=%.2f pos=(%.2f, %.2f, %.2f) mode=%s", t, p.speed or -1,
          p.position[1], p.position[2], p.position[3], tostring(p.mode)))
      end
    end
  end,
}
