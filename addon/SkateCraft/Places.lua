-- SkateCraft places: teleport to interesting spots (capitals, raid entrances) with a small picture and the name, and
-- remember / return to your own spot. Needs a game master account on the server (it sends .tele / .go xyz).
-- Gamepad: L3 + D-pad up = remember this place, down = return to it, right / left = next / previous destination.
local FONT = STANDARD_TEXT_FONT or "Fonts\\FRIZQT__.TTF"
local ICON = "Interface\\Icons\\"
local LFG = "Interface\\LFGFrame\\"
local MAP1, MAP2 = ICON .. "INV_Misc_Note_01", ICON .. "INV_Misc_Map02"

-- { game_tele name, title, subtitle, texture }
local PLACES = {
    { "Stormwind", "Stormwind City", "Alliance capital", ICON .. "Spell_Arcane_TeleportStormwind" },
    { "Ironforge", "Ironforge", "Alliance capital", ICON .. "Spell_Arcane_TeleportIronForge" },
    { "Darnassus", "Darnassus", "Alliance capital", ICON .. "Spell_Arcane_TeleportDarnassus" },
    { "TheExodar", "The Exodar", "Alliance capital", ICON .. "Spell_Arcane_TeleportExodar" },
    { "Orgrimmar", "Orgrimmar", "Horde capital", ICON .. "Spell_Arcane_TeleportOrgrimmar" },
    { "ThunderBluff", "Thunder Bluff", "Horde capital", ICON .. "Spell_Arcane_TeleportThunderBluff" },
    { "Undercity", "Undercity", "Horde capital", ICON .. "Spell_Arcane_TeleportUnderCity" },
    { "SilvermoonCity", "Silvermoon City", "Horde capital", ICON .. "Spell_Arcane_TeleportSilvermoon" },
    { "Shattrath", "Shattrath City", "Outland capital", ICON .. "Spell_Arcane_TeleportShattrath" },
    { "Dalaran", "Dalaran", "Northrend capital", ICON .. "Spell_Arcane_TeleportDalaran" },
    { "BootyBay", "Booty Bay", "Stranglethorn port", MAP1 },
    { "Gadgetzan", "Gadgetzan", "Tanaris", MAP2 },
    { "Everlook", "Everlook", "Winterspring", MAP1 },
    { "Moonglade", "Moonglade", "Druid sanctuary", MAP2 },
    { "Ratchet", "Ratchet", "The Barrens", MAP1 },
    { "WintergraspFortress", "Wintergrasp Fortress", "Northrend", MAP2 },
    { "StormPeaks", "The Storm Peaks", "Northrend", ICON .. "Achievement_Zone_StormPeaks_01" },
    { "Icecrown", "Icecrown", "Northrend", ICON .. "Achievement_Zone_Icecrown_01" },
    { "Naxxramas", "Naxxramas", "Wrath raid (outside)", LFG .. "LFGIcon-Naxxramas" },
    { "Ulduar", "Ulduar", "Wrath raid (outside)", LFG .. "LFGIcon-Ulduar" },
    { "IcecrownCitadelRaid", "Icecrown Citadel", "Wrath raid (outside)", LFG .. "LFGIcon-IcecrownCitadel" },
    { "TrialOfTheCrusader", "Trial of the Crusader", "Wrath raid (outside)", MAP1 },
    { "TheEyeOfEternity", "The Eye of Eternity", "Wrath raid (outside)", MAP2 },
    { "VaultOfArchavon", "Vault of Archavon", "Wrath raid (outside)", LFG .. "LFGIcon-VaultOfArchavon" },
    { "ObsidianDragonshrine", "Obsidian Sanctum", "Wrath raid (outside)", MAP1 },
    { "RubyDragonshrine", "Ruby Sanctum", "Wrath raid (outside)", LFG .. "LFGIcon-RubySanctum" },
    { "Karazhan", "Karazhan", "Burning Crusade raid", LFG .. "LFGIcon-Karazhan" },
    { "BlackTemple", "Black Temple", "Burning Crusade raid", LFG .. "LFGIcon-BlackTemple" },
    { "SunwellPlateau", "Sunwell Plateau", "Burning Crusade raid", MAP2 },
    { "TempestKeep", "Tempest Keep", "Burning Crusade raid", LFG .. "LFGIcon-TempestKeep" },
    { "GruulsLair", "Gruul's Lair", "Burning Crusade raid", LFG .. "LFGIcon-GruulsLair" },
    { "ZulAman", "Zul'Aman", "Burning Crusade raid", LFG .. "LFGIcon-ZulAman" },
    { "HyjalSummit", "Hyjal Summit", "Burning Crusade raid", MAP1 },
    { "ZulGurub", "Zul'Gurub", "Classic raid", LFG .. "LFGIcon-ZulGurub" },
    { "BlackrockMountain", "Blackrock Mountain", "Molten Core, Blackwing Lair", LFG .. "LFGIcon-MoltenCore" },
    { "TheTempleOfAhnQiraj", "Temple of Ahn'Qiraj", "Classic raid", MAP2 },
    { "TheRuinsOfAhnQiraj", "Ruins of Ahn'Qiraj", "Classic raid", MAP1 },
}

local P = {}
SkateCraftPlaces = P
P.list = PLACES

local current = 0 -- index of the last destination (for next / previous)

----------------------------------------------------------------------------------------------------------------- toast
local toast = CreateFrame("Frame", "SkateCraftToast", UIParent)
toast:SetWidth(330)
toast:SetHeight(86)
toast:SetPoint("TOP", UIParent, "TOP", 0, -70)
toast:SetFrameStrata("DIALOG")
toast:SetBackdrop({
    bgFile = "Interface\\DialogFrame\\UI-DialogBox-Background",
    edgeFile = "Interface\\DialogFrame\\UI-DialogBox-Border",
    tile = true, tileSize = 32, edgeSize = 28,
    insets = { left = 8, right = 8, top = 8, bottom = 8 },
})
toast:Hide()
local toastIcon = toast:CreateTexture(nil, "ARTWORK")
toastIcon:SetWidth(60)
toastIcon:SetHeight(60)
toastIcon:SetPoint("LEFT", toast, "LEFT", 14, 0)
toastIcon:SetTexCoord(0.07, 0.93, 0.07, 0.93)
local toastTitle = toast:CreateFontString(nil, "OVERLAY")
toastTitle:SetFont(FONT, 22, "OUTLINE")
toastTitle:SetTextColor(1, 0.82, 0)
toastTitle:SetPoint("TOPLEFT", toastIcon, "TOPRIGHT", 12, -6)
toastTitle:SetPoint("RIGHT", toast, "RIGHT", -14, 0)
toastTitle:SetJustifyH("LEFT")
local toastSub = toast:CreateFontString(nil, "OVERLAY")
toastSub:SetFont(FONT, 14, "")
toastSub:SetTextColor(0.8, 0.82, 0.88)
toastSub:SetPoint("TOPLEFT", toastTitle, "BOTTOMLEFT", 0, -6)
toastSub:SetPoint("RIGHT", toast, "RIGHT", -14, 0)
toastSub:SetJustifyH("LEFT")

local toastAge
local function showToast(icon, title, sub)
    toastIcon:SetTexture(icon or MAP1)
    toastTitle:SetText(title or "")
    toastSub:SetText(sub or "")
    toastAge = 0
    toast:SetAlpha(0)
    toast:Show()
end

toast:SetScript("OnUpdate", function(self, dt)
    if not toastAge then return end
    toastAge = toastAge + dt
    local a
    if toastAge < 0.3 then a = toastAge / 0.3
    elseif toastAge < 3.6 then a = 1
    else a = 1 - (toastAge - 3.6) / 0.7 end
    if a <= 0 and toastAge > 3.6 then
        toastAge = nil
        self:Hide()
        return
    end
    self:SetAlpha(math.max(0, math.min(1, a)))
end)

---------------------------------------------------------------------------------------------------------- teleporting
local job -- { phase, t, cmd, wasOn, map, x, y, title, sub, icon }
local runner = CreateFrame("Frame")

local function dist(x1, y1, x2, y2) return math.sqrt((x1 - x2) ^ 2 + (y1 - y2) ^ 2) end

runner:SetScript("OnUpdate", function(_, dt)
    if not job then return end
    job.t = job.t + dt
    if job.phase == "off" then
        if job.t >= 0.4 then
            SendChatMessage(job.cmd, "SAY")
            job.phase, job.t = "wait", 0
        end
    elseif job.phase == "wait" then
        local map, x, y = SkateCraft_Pos()
        if map ~= job.map or dist(x, y, job.x, job.y) > 15 then
            job.phase, job.t = "settle", 0
        elseif job.t > 8 then
            job.phase, job.t = "settle", 0 -- nothing moved (not a game master?): carry on anyway
        end
    elseif job.phase == "settle" then
        if job.t >= 2.0 then
            if job.wasOn then SkateCraftAPI.set(true, true) end
            if job.title then showToast(job.icon, job.title, job.sub) end
            job = nil
        end
    end
end)

local function go(cmd, title, sub, icon)
    if job then return end
    if not SkateCraft_Pos then
        DEFAULT_CHAT_FRAME:AddMessage("|cff66ccffSkateCraft|r: the client plugin is not loaded")
        return
    end
    local map, x, y = SkateCraft_Pos()
    local wasOn = SkateCraftAPI.isOn()
    if wasOn then SkateCraftAPI.set(false, true) end
    job = { phase = "off", t = 0, cmd = cmd, wasOn = wasOn, map = map, x = x, y = y, title = title, sub = sub, icon = icon }
end

function P.teleport(i)
    local p = PLACES[i]
    if not p then return end
    current = i
    go(".tele " .. p[1], p[2], p[3], p[4])
end

function P.save()
    if not SkateCraft_Pos then return end
    local map, x, y, z, o = SkateCraft_Pos()
    if not map or (x == 0 and y == 0) then return end
    SkateCraftDB = SkateCraftDB or {}
    SkateCraftDB.place = { map = map, x = x, y = y, z = z, o = o }
    showToast(MAP2, "Place saved", string.format("%.0f, %.0f  (map %d)", x, y, map))
end

function P.back()
    local s = SkateCraftDB and SkateCraftDB.place
    if not s then
        showToast(MAP1, "No saved place", "Save one first: L3 + D-pad up, or the Places window")
        return
    end
    go(string.format(".go xyz %.2f %.2f %.2f %d %.3f", s.x, s.y, s.z + 1.0, s.map, s.o or 0),
        "Back at your place", string.format("%.0f, %.0f  (map %d)", s.x, s.y, s.map), MAP2)
end

function P.padRequest(kind)
    if kind == 1 then P.save()
    elseif kind == 2 then P.back()
    elseif kind == 3 then P.teleport(current % #PLACES + 1)
    elseif kind == 4 then P.teleport((current - 2) % #PLACES + 1) end
end

---------------------------------------------------------------------------------------------------------------- window
local win = CreateFrame("Frame", "SkateCraftPlacesFrame", UIParent)
local COLS, W, H = 3, 196, 30
local rows = math.ceil(#PLACES / COLS)
win:SetWidth(COLS * W + 28)
win:SetHeight(rows * H + 96)
win:SetPoint("CENTER")
win:SetFrameStrata("DIALOG")
win:SetMovable(true)
win:EnableMouse(true)
win:RegisterForDrag("LeftButton")
win:SetScript("OnDragStart", function(self) self:StartMoving() end)
win:SetScript("OnDragStop", function(self) self:StopMovingOrSizing() end)
win:SetBackdrop({
    bgFile = "Interface\\DialogFrame\\UI-DialogBox-Background",
    edgeFile = "Interface\\DialogFrame\\UI-DialogBox-Border",
    tile = true, tileSize = 32, edgeSize = 32,
    insets = { left = 11, right = 12, top = 12, bottom = 11 },
})
win:Hide()
tinsert(UISpecialFrames, "SkateCraftPlacesFrame")

local title = win:CreateFontString(nil, "OVERLAY", "GameFontNormalLarge")
title:SetPoint("TOP", win, "TOP", 0, -16)
title:SetText("SkateCraft places")

for i, p in ipairs(PLACES) do
    local b = CreateFrame("Button", nil, win)
    b:SetWidth(W - 4)
    b:SetHeight(H - 2)
    local col, row = (i - 1) % COLS, math.floor((i - 1) / COLS)
    b:SetPoint("TOPLEFT", win, "TOPLEFT", 14 + col * W, -42 - row * H)
    local ic = b:CreateTexture(nil, "ARTWORK")
    ic:SetWidth(24)
    ic:SetHeight(24)
    ic:SetPoint("LEFT", b, "LEFT", 2, 0)
    ic:SetTexture(p[4])
    ic:SetTexCoord(0.07, 0.93, 0.07, 0.93)
    local label = b:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    label:SetPoint("LEFT", ic, "RIGHT", 6, 0)
    label:SetPoint("RIGHT", b, "RIGHT", -2, 0)
    label:SetJustifyH("LEFT")
    label:SetText(p[2])
    b:SetHighlightTexture("Interface\\QuestFrame\\UI-QuestTitleHighlight")
    b:SetScript("OnClick", function()
        win:Hide()
        P.teleport(i)
    end)
    b:SetScript("OnEnter", function(self)
        GameTooltip:SetOwner(self, "ANCHOR_RIGHT")
        GameTooltip:SetText(p[2], 1, 0.82, 0)
        GameTooltip:AddLine(p[3], 0.8, 0.82, 0.88)
        GameTooltip:Show()
    end)
    b:SetScript("OnLeave", function() GameTooltip:Hide() end)
end

local save = CreateFrame("Button", nil, win, "UIPanelButtonTemplate")
save:SetWidth(170)
save:SetHeight(24)
save:SetPoint("BOTTOMLEFT", win, "BOTTOMLEFT", 20, 18)
save:SetText("Save this place")
save:SetScript("OnClick", function() P.save() end)

local back = CreateFrame("Button", nil, win, "UIPanelButtonTemplate")
back:SetWidth(170)
back:SetHeight(24)
back:SetPoint("LEFT", save, "RIGHT", 8, 0)
back:SetText("Return to saved place")
back:SetScript("OnClick", function()
    win:Hide()
    P.back()
end)

local hint = win:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
hint:SetPoint("LEFT", back, "RIGHT", 12, 0)
hint:SetText("Gamepad: L3 + D-pad  up = save, down = return, right / left = next / previous")

local close = CreateFrame("Button", nil, win, "UIPanelCloseButton")
close:SetPoint("TOPRIGHT", win, "TOPRIGHT", -4, -4)

local open = CreateFrame("Button", "SkateCraftPlacesButton", UIParent, "UIPanelButtonTemplate")
open:SetWidth(70)
open:SetHeight(24)
open:SetPoint("LEFT", SkateCraftAPI.anchor, "RIGHT", 4, 0)
open:SetText("Places")
open:SetScript("OnClick", function()
    if win:IsShown() then win:Hide() else win:Show() end
end)

------------------------------------------------------------------------------------------------------------- commands
function P.command(word, rest)
    if word == "places" then
        if win:IsShown() then win:Hide() else win:Show() end
    elseif word == "set" then P.save()
    elseif word == "return" then P.back()
    elseif word == "go" then
        rest = string.lower(rest or "")
        for i, p in ipairs(PLACES) do
            if rest ~= "" and (string.find(string.lower(p[2]), rest, 1, true) or string.find(string.lower(p[1]), rest, 1, true)) then
                P.teleport(i)
                return
            end
        end
        DEFAULT_CHAT_FRAME:AddMessage("|cff66ccffSkateCraft|r: no place matches '" .. rest .. "' (/skate places)")
    end
end
