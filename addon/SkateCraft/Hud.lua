-- SkateCraft score HUD: the trick name, the running points and the multiplier while a trick sequence is going,
-- a "landed" line with the points when it is put down, "bail" when it is not, and the session total.
-- The numbers come from the Skate engine through the client plugin (SkateCraft_State).
local FONT = STANDARD_TEXT_FONT or "Fonts\\FRIZQT__.TTF"

local hud = CreateFrame("Frame", "SkateCraftHud", UIParent)
hud:SetAllPoints(UIParent)
hud:SetFrameStrata("HIGH")

local function text(size, r, g, b, point, x, y, justify)
    local fs = hud:CreateFontString(nil, "OVERLAY")
    fs:SetFont(FONT, size, "THICKOUTLINE")
    fs:SetTextColor(r, g, b)
    fs:SetPoint(point, UIParent, point, x, y)
    fs:SetJustifyH(justify or "CENTER")
    fs:SetShadowOffset(2, -2)
    fs:SetShadowColor(0, 0, 0, 0.8)
    fs:Hide()
    return fs
end

local trickText = text(30, 1.0, 0.82, 0.0, "TOP", 0, -150)
local pointsText = text(24, 1.0, 1.0, 1.0, "TOP", 0, -190)
local landedText = text(34, 0.25, 1.0, 0.25, "TOP", 0, -150)
local landedTrick = text(18, 0.9, 0.9, 0.9, "TOP", 0, -192)
local bailText = text(40, 1.0, 0.15, 0.1, "TOP", 0, -150)
local totalText = text(18, 1.0, 0.82, 0.0, "RIGHT", -40, 120, "RIGHT")

local function commas(n)
    local s = tostring(math.floor(n + 0.5))
    local out = s
    while true do
        local k
        out, k = string.gsub(out, "^(-?%d+)(%d%d%d)", "%1,%2")
        if k == 0 then break end
    end
    return out
end

local landedAt, bailAt
local lastLanded, lastBail, lastReq
local acc = 0

hud:SetScript("OnUpdate", function(_, dt)
    acc = acc + dt
    if acc < 0.03 then return end
    local step = acc
    acc = 0
    if not SkateCraft_State then return end
    local flags, seq, line, mult, lineTime, landedPts, total, speed, landedSeq, bailSeq, reqSeq, reqKind, trick, landed = SkateCraft_State()
    if lastLanded == nil then
        lastLanded, lastBail, lastReq = landedSeq, bailSeq, reqSeq
    end

    -- pad requests (L3 + D-pad) go to the places module
    if reqSeq ~= lastReq then
        lastReq = reqSeq
        if SkateCraftPlaces and SkateCraftAPI.isOn() then SkateCraftPlaces.padRequest(reqKind) end
    end

    local on = SkateCraftAPI and SkateCraftAPI.isOn()
    if not on then
        trickText:Hide(); pointsText:Hide(); landedText:Hide(); landedTrick:Hide(); bailText:Hide(); totalText:Hide()
        return
    end

    if landedSeq ~= lastLanded then
        lastLanded = landedSeq
        landedAt = 0
        landedText:SetText("+" .. commas(landedPts))
        landedTrick:SetText(landed or "")
    end
    if bailSeq ~= lastBail then
        lastBail = bailSeq
        bailAt = 0
        landedAt = nil
        bailText:SetText("BAIL")
    end

    local active = bit.band(flags, 1) ~= 0
    local bailing = bit.band(flags, 8) ~= 0
    if active and not bailing and trick and trick ~= "" then
        trickText:SetText(trick)
        local p = commas(seq)
        if mult and mult > 1.01 then p = p .. "  x" .. string.format("%.0f", mult) end
        pointsText:SetText(p)
        if bit.band(flags, 4) ~= 0 then pointsText:SetTextColor(1.0, 0.5, 0.1)
        elseif bit.band(flags, 2) ~= 0 then pointsText:SetTextColor(0.5, 1.0, 0.5)
        else pointsText:SetTextColor(1, 1, 1) end
        trickText:SetAlpha(1); pointsText:SetAlpha(1)
        trickText:Show(); pointsText:Show()
    else
        trickText:Hide(); pointsText:Hide()
    end

    if landedAt then
        landedAt = landedAt + step
        local a = landedAt < 1.6 and 1 or math.max(0, 1 - (landedAt - 1.6) / 0.8)
        if a <= 0 then
            landedAt = nil
            landedText:Hide(); landedTrick:Hide()
        else
            landedText:SetAlpha(a); landedTrick:SetAlpha(a)
            landedText:Show(); landedTrick:Show()
            trickText:Hide(); pointsText:Hide()
        end
    end
    if bailAt then
        bailAt = bailAt + step
        local a = bailAt < 1.0 and 1 or math.max(0, 1 - (bailAt - 1.0) / 0.6)
        if a <= 0 then
            bailAt = nil
            bailText:Hide()
        else
            bailText:SetAlpha(a)
            bailText:Show()
            trickText:Hide(); pointsText:Hide()
        end
    end

    if total and total > 0 then
        totalText:SetText("Score  " .. commas(total))
        totalText:Show()
    else
        totalText:Hide()
    end
end)
