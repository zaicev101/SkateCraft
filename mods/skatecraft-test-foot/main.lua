-- Test only: dismount at t = 6 s, then walk/run forward with a slow turn.
local t = 0
local logt = 0
return {
  on_update = function(event)
    local dt = (event and tonumber(event.dt)) or 0
    t = t + dt
    logt = logt + dt
    if t > 6 and t < 6.3 then sdk.input.override_action("b", 1) else sdk.input.override_action("b", nil) end
    if t > 8 then
      sdk.input.override_action("left_stick_y", 1)
      sdk.input.override_action("left_stick_x", math.sin((t - 8) * 0.5) * 0.4)
    end
    if logt >= 1.0 then
      logt = 0
      local p = sdk.player.read()
      if p then sdk.log(string.format("FOOT t=%.0f mode=%s on_board=%s speed=%.2f", t, tostring(p.mode), tostring(p.on_board), p.speed or -1)) end
    end
  end,
}
