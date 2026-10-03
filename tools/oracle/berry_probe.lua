-- Yoshi's berry check in Mesen 2, as `examples/contact_probe.rs berry` runs
-- it in Kobo's machine: a stunned baby Yoshi (sprite 2D, which checks the
-- tile at its centre for a berry, as Yoshi's tongue does) held on a tile,
-- with each frame's berry, tile, sprites, and the frame counter logged. A
-- frame counter ($13) that stops is a game that hangs. For checking what a
-- ROM's code does there on an emulator other than Kobo's, from memory
-- effects alone (docs/testing.md).
--
-- Usage, through dump_levels.lua, which enters the level:
--   KOBO_ORACLE_LEVELS=F7 KOBO_ORACLE_OUT=dir \
--   KOBO_ORACLE_PROBE=tools/oracle/berry_probe.lua \
--   BERRY_X=4 BERRY_Y=14 BERRY_TILE=245 [BERRY_LAYER=2] [BERRY_PLAYER_X=D8] \
--   [BERRY_FRAMES=16] [BERRY_PLACE=0] \
--     Mesen --testRunner tools/oracle/dump_levels.lua rom.sfc --timeout=200
-- writes dir/oracle.log. X and Y are in blocks on the layer, the tile and
-- the player's X (held there every frame) in hex. BERRY_PLACE is the frame
-- the tile goes in on, BERRY_FRAMES the frames run. With BERRY_LAYER=2,
-- Yoshi is held where layer 2's (X, Y) is on layer 1. A vanilla-layout
-- LoROM ROM only.

local x = tonumber(os.getenv("BERRY_X"))
local y = tonumber(os.getenv("BERRY_Y"))
local tile = tonumber(os.getenv("BERRY_TILE") or "45", 16)
local layer2 = os.getenv("BERRY_LAYER") == "2"
local player_x = os.getenv("BERRY_PLAYER_X") and tonumber(os.getenv("BERRY_PLAYER_X"), 16)
local frames = tonumber(os.getenv("BERRY_FRAMES") or "16")
local place = tonumber(os.getenv("BERRY_PLACE") or "0")
local cell

-- The address of the tile at block (x, y) of a layer, as the game's block
-- lookups find it: a screen's start from DATA_00BA60 and the tables after
-- it, by layer and by whether the layer is vertical ($5B bits 0 and 1).
local function find_cell(api)
  local vertical = api.peek(0x7E005B) & (layer2 and 2 or 1) ~= 0
  local tables = vertical and { 0x00BA80, 0x00BABC, 0x00BA8E, 0x00BACA }
    or { 0x00BA60, 0x00BA9C, 0x00BA70, 0x00BAAC }
  local low, high = tables[1], tables[2]
  if layer2 then
    low, high = tables[3], tables[4]
  end
  local screen, row = x >> 4, y >> 4
  if vertical then
    screen, row = y >> 4, x >> 4
  end
  local rom = emu.memType.snesMemory
  local start = emu.read(high + screen, rom) << 8 | emu.read(low + screen, rom)
  return 0x7E0000 + start + (y & 15) * 16 + (x & 15) + (row << 8)
end

local function word(api, a)
  return api.peek(a) | api.peek(a + 1) << 8
end

local function signed(v)
  return v >= 0x8000 and v - 0x10000 or v
end

return function(frame, api)
  if frame == 0 then
    cell = find_cell(api)
    for i = 0, 11 do
      api.poke(0x7E14C8 + i, 0)
    end
    api.poke(0x7E009E, 0x2D)
    api.poke(0x7E14C8, 0x09)
    -- What InitSpriteTables would load: its tweaker bytes.
    local tweakers = { { 0x1656, 0x07F26C }, { 0x1662, 0x07F335 }, { 0x166E, 0x07F3FE },
      { 0x167A, 0x07F4C7 }, { 0x1686, 0x07F590 }, { 0x190F, 0x07F659 } }
    for _, t in ipairs(tweakers) do
      api.poke(0x7E0000 + t[1], emu.read(t[2] + 0x2D, emu.memType.snesMemory))
    end
    api.poke(0x7E15F6, emu.read(0x07F3FE + 0x2D, emu.memType.snesMemory) & 0x0F)
    api.poke(0x7E18D6, 0)
  end
  if frame == place then
    api.poke(cell, tile & 0xFF)
    api.poke(cell + 0x10000, tile >> 8)
  end
  if player_x then
    api.poke(0x7E0094, player_x & 0xFF)
    api.poke(0x7E0095, player_x >> 8)
  end
  -- Held where its centre is in the tile.
  local dx, dy = 0, 0
  if layer2 then
    dx, dy = signed(word(api, 0x7E0026)), signed(word(api, 0x7E0028))
  end
  local yx, yy = (x * 16 - dx) & 0xFFFF, (y * 16 - dy) & 0xFFFF
  api.poke(0x7E00E4, yx & 0xFF)
  api.poke(0x7E14E0, yx >> 8)
  api.poke(0x7E00D8, yy & 0xFF)
  api.poke(0x7E14D4, yy >> 8)
  local sprites = {}
  for i = 0, 11 do
    local status = api.peek(0x7E14C8 + i)
    if status ~= 0 then
      sprites[#sprites + 1] = string.format("%02X:%02X@%04X,%04X", api.peek(0x7E009E + i), status,
        api.peek(0x7E00E4 + i) | api.peek(0x7E14E0 + i) << 8,
        api.peek(0x7E00D8 + i) | api.peek(0x7E14D4 + i) << 8)
    end
  end
  api.log("frame %d: $13 %02X berry %02X tile %02X%02X $1693 %02X $1933 %02X $26 %04X $28 %04X sprites %s",
    frame, api.peek(0x7E0013), api.peek(0x7E18D6), api.peek(cell + 0x10000), api.peek(cell),
    api.peek(0x7E1693), api.peek(0x7E1933), word(api, 0x7E0026), word(api, 0x7E0028),
    table.concat(sprites, " "))
  return frame >= frames
end
