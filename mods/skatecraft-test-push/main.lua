-- One short push (A held for 0.25 s at t = 4 s), then log speed/position twice a second for 40 s.
local t = 0
local logt = 0
return {
  on_update = function(event)
    local dt = (event and tonumber(event.dt)) or 0
    t = t + dt
    logt = logt + dt
    if t > 4 and t < 4.25 then
      sdk.input.override_action("a", 1)
    else
      sdk.input.override_action("a", nil)
    end
    if logt >= 0.5 and t < 45 then
      logt = 0
      local p = sdk.player.read()
      if p then
        sdk.log(string.format("TESTPUSH t=%.1f speed=%.2f pos=(%.2f, %.2f) mode=%s on_board=%s",
          t, p.speed or -1, p.position[1], p.position[3], tostring(p.mode), tostring(p.on_board)))
      end
    end
  end,
}
