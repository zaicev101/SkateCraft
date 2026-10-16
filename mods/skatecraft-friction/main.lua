-- Rolling friction crutch for SkateCraft: WoW's terrain is never perfectly flat, so without this the
-- skater keeps rolling on the slightest slope. When nobody pushes, a drag impulse (horizontal,
-- proportional to speed, strong when slow) is applied to the board and the skater's parts.
local K_SLOW = 4.0     -- 1/s drag below 1.2 m/s (a creep on a slope stops, a push still carries)
local K_FAST = 0.3     -- 1/s drag above 2.5 m/s (coasting after a push stays natural)
local parts = nil
local logt = 0

local function pushing()
  local function v(name) return math.abs(sdk.input.action(name) or 0) end
  return v("a") > 0.1 or v("right_stick_y") > 0.25 or v("right_stick_x") > 0.25 or v("right_trigger") > 0.1
end

local function drag(body, k, dt)
  if not body or not body.inverse_mass or body.inverse_mass <= 0 then return end
  local vel = body.velocity
  if not vel then return end
  local m = 1 / body.inverse_mass
  local f = -k * m * dt
  sdk.bodies.impulse(body, {vel[1] * f, 0, vel[3] * f}, body.position)
end

return {
  on_update = function(event)
    local dt = (event and tonumber(event.dt)) or 0
    if dt <= 0 or dt > 0.1 then return end
    local p = sdk.player.read()
    if not p or p.mode ~= "ground" or p.bailing or pushing() then return end
    local s = p.speed or 0
    if s < 0.05 then return end
    local k
    if s <= 1.2 then k = K_SLOW elseif s >= 2.5 then k = K_FAST else k = K_SLOW + (K_FAST - K_SLOW) * (s - 1.2) / 1.3 end
    if not parts then
      parts = {}
      local rig = sdk.rig.read({"parts"})
      for _, part in ipairs((rig and rig.parts) or {}) do parts[#parts + 1] = part.index end
    end
    for i = 0, 5 do drag(sdk.bodies.read({kind = "board", index = i}), k, dt) end
    for _, idx in ipairs(parts) do drag(sdk.bodies.read({kind = "skater", index = idx}), k, dt) end
    logt = logt + dt
    if logt > 2 then
      logt = 0
      sdk.log(string.format("FRICTION speed=%.2f k=%.2f", s, k))
    end
  end,
}
