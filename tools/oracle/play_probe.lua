-- A probe for dump_levels.lua (KOBO_ORACLE_PROBE): plays a level with
-- buttons held, for comparing two ROMs' play side by side, from memory
-- effects alone (it reads memory and takes screenshots, so it may run on a
-- ROM Lunar Magic saved).
--
--   PROBE_PLACE=x,y,cx,cy  the player and the camera (pixels), set as the
--                          level's preparation starts (the first frame of
--                          game mode $12), so that the game draws it there
--   PROBE_SCRIPT="f:buttons,..."  from frame f of the level on, hold these
--                          (Mesen's names joined by +: "b", "left+b", or
--                          none), e.g. "20:b,40:,70:left,134:"
--   PROBE_EVERY=n, PROBE_FRAMES=n  a screenshot (shot-NNNN.png) and a log
--                          line (the player, the ON/OFF flag, layer 1's X,
--                          and the OAM's objects) every n frames, up to n
--   PROBE_VRAM=1           and VRAM (vram-NNNN.bin) with each
--
-- Into KOBO_ORACLE_OUT. A vanilla-layout LoROM ROM's addresses.
local mem = emu.memType.snesMemory
local every = tonumber(os.getenv("PROBE_EVERY") or "20")
local frames = tonumber(os.getenv("PROBE_FRAMES") or "200")
local out = os.getenv("KOBO_ORACLE_OUT") or "."
local steps = {}
for f, b in (os.getenv("PROBE_SCRIPT") or ""):gmatch("(%d+):([%w+]*)") do
  steps[#steps + 1] = { tonumber(f), b }
end

local place = os.getenv("PROBE_PLACE")
if place then
  local px, py, cx, cy = place:match("(%d+),(%d+),(%d+),(%d+)")
  px, py, cx, cy = tonumber(px), tonumber(py), tonumber(cx), tonumber(cy)
  local words = { [0x94] = px, [0x96] = py, [0xD1] = px, [0xD3] = py,
    [0x1A] = cx, [0x1C] = cy, [0x1462] = cx, [0x1464] = cy }
  local done = false
  emu.addEventCallback(function()
    if done or emu.read(0x7E0100, mem) ~= 0x12 then return end
    done = true
    for a, v in pairs(words) do
      emu.write(0x7E0000 + a, v & 0xFF, mem)
      emu.write(0x7E0001 + a, v >> 8, mem)
    end
  end, emu.eventType.endFrame)
end

local held = nil
emu.addEventCallback(function()
  if not held then return end
  local input = {}
  for b in held:gmatch("%w+") do input[b] = true end
  emu.setInput(input, 0)
end, emu.eventType.inputPolled)

return function(frame, api)
  held = ""
  for _, s in ipairs(steps) do
    if frame >= s[1] then held = s[2] end
  end
  if frame % every == 0 then
    local f = assert(io.open(string.format("%s/shot-%04d.png", out, frame), "wb"))
    f:write(emu.takeScreenshot())
    f:close()
    if os.getenv("PROBE_VRAM") then
      local bytes = {}
      for a = 0, 0xFFFF do bytes[#bytes + 1] = string.char(emu.read(a, emu.memType.snesVideoRam)) end
      local v = assert(io.open(string.format("%s/vram-%04d.bin", out, frame), "wb"))
      v:write(table.concat(bytes))
      v:close()
    end
    local oam = {}
    for i = 0, 127 do
      local b = 4 * i
      local y = emu.read(b + 1, emu.memType.snesSpriteRam)
      if y < 0xE0 then
        local hi = emu.read(512 + (i >> 2), emu.memType.snesSpriteRam) >> ((i & 3) * 2) & 3
        oam[#oam + 1] = string.format("%03X,%02X,%02X%02X%s",
          emu.read(b, emu.memType.snesSpriteRam) | (hi & 1) << 8, y,
          emu.read(b + 3, emu.memType.snesSpriteRam), emu.read(b + 2, emu.memType.snesSpriteRam),
          (hi & 2) ~= 0 and "L" or "")
      end
    end
    api.log(string.format("frame %d x %04X y %04X onoff %02X 1A %04X oam %s", frame,
      api.peek(0x7E0094) | api.peek(0x7E0095) << 8, api.peek(0x7E0096) | api.peek(0x7E0097) << 8,
      api.peek(0x7E14AF), api.peek(0x7E001A) | api.peek(0x7E001B) << 8, table.concat(oam, " ")))
  end
  return frame >= frames
end
