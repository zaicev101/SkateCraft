-- SkateCraft: a button and /skate to switch skate mode on and off.
-- The addon cannot talk to the client plugin directly, so it sends the addon message
-- "SKATECRAFT\tON" / "SKATECRAFT\tOFF" to the player's own name: the plugin sees it in the client's
-- network send. While skate mode is on the message is repeated every 2 seconds, so a plugin that
-- (re)loads or a logout switches it off by itself.
local PREFIX = "SKATECRAFT"
local on = false
local elapsed = 0

local function send(state)
    local me = UnitName("player")
    if me then
        SendAddonMessage(PREFIX, state and "ON" or "OFF", "WHISPER", me)
    end
end

local button = CreateFrame("Button", "SkateCraftButton", UIParent, "UIPanelButtonTemplate")
button:SetWidth(96)
button:SetHeight(24)
button:SetPoint("TOP", UIParent, "TOP", 0, -90)
button:SetMovable(true)
button:EnableMouse(true)
button:RegisterForDrag("RightButton")
button:SetScript("OnDragStart", function(self) self:StartMoving() end)
button:SetScript("OnDragStop", function(self) self:StopMovingOrSizing() end)

-- Weapons are not part of skating: put them in the backpack while skate mode is on, and back on afterwards.
local WEAPON_SLOTS = { 16, 17, 18 } -- main hand, off hand, ranged
local stowed = {}

local function stow()
    for _, slot in ipairs(WEAPON_SLOTS) do
        local link = GetInventoryItemLink("player", slot)
        if link and not CursorHasItem() then
            PickupInventoryItem(slot)
            if CursorHasItem() then
                PutItemInBackpack()
                if CursorHasItem() then
                    ClearCursor() -- no room in the backpack: leave it equipped
                else
                    stowed[slot] = link
                end
            end
        end
    end
end

local function unstow()
    for slot, link in pairs(stowed) do
        EquipItemByName(link, slot)
        stowed[slot] = nil
    end
end

-- The Skate engine runs in lockstep with the client's frames; a fixed frame rate keeps both even.
local SKATE_FPS = "60"
local savedFps

local function capFps()
    -- vertical sync: every client frame lands on a display refresh, the Skate engine follows those frames
    if GetCVar("gxVSync") ~= "1" then
        SetCVar("gxVSync", "1")
        RestartGx()
    end
    if not savedFps then
        savedFps = { GetCVar("maxfps"), GetCVar("maxfpsbk") }
    end
    SetCVar("maxfps", SKATE_FPS)
    SetCVar("maxfpsbk", SKATE_FPS)
end

local function restoreFps()
    if savedFps then
        SetCVar("maxfps", savedFps[1])
        SetCVar("maxfpsbk", savedFps[2])
        savedFps = nil
    end
end

local function refresh()
    button:SetText(on and "Skate: ON" or "Skate: off")
end

local function set(state, keepGear)
    local was = on
    on = state and true or false
    elapsed = 0
    if not keepGear then
        if on and not was then stow() capFps() elseif was and not on then unstow() restoreFps() end
    end
    send(on)
    refresh()
    if not keepGear then
        DEFAULT_CHAT_FRAME:AddMessage("|cff66ccffSkateCraft|r: skate mode " .. (on and "ON" or "off"))
    end
end

button:SetScript("OnClick", function() set(not on) end)
button:SetScript("OnEnter", function(self)
    GameTooltip:SetOwner(self, "ANCHOR_BOTTOM")
    GameTooltip:SetText("SkateCraft", 1, 1, 1)
    GameTooltip:AddLine("Click or /skate: switch skate mode on or off. Right-drag moves the button.", 0.8, 0.82, 0.88, true)
    GameTooltip:Show()
end)
button:SetScript("OnLeave", function() GameTooltip:Hide() end)

button:SetScript("OnUpdate", function(_, dt)
    if not on then return end
    elapsed = elapsed + dt
    if elapsed >= 2 then
        elapsed = 0
        send(true)
    end
end)

SLASH_SKATECRAFT1 = "/skate"
SlashCmdList["SKATECRAFT"] = function(msg)
    msg = string.lower(msg or "")
    local word, rest = string.match(msg, "^(%S*)%s*(.-)$")
    if word == "on" then set(true)
    elseif word == "off" then set(false)
    elseif word == "go" or word == "set" or word == "return" or word == "places" then
        if SkateCraftPlaces then SkateCraftPlaces.command(word, rest) end
    else set(not on) end
end

-- for Hud.lua and Places.lua
SkateCraftAPI = {
    isOn = function() return on end,
    set = set,
    anchor = button,
}

local f = CreateFrame("Frame")
f:RegisterEvent("PLAYER_LOGOUT")
f:SetScript("OnEvent", function() if on then send(false) end restoreFps() end)

refresh()
