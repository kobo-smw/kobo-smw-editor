-- The taller levels' RAM in Mesen 2, every frame from boot, for checking the
-- paths that load the game's own data over a level (the castle "No Yoshi"
-- intro, Choc Island 2's rooms, the credits) against Lunar Magic's code,
-- from memory effects alone (docs/lunar-magic-install.md, "Taller levels").
--
-- Usage, through dump_levels.lua, which enters the level:
--   KOBO_ORACLE_LEVELS=101 KOBO_ORACLE_OUT=dir \
--   KOBO_ORACLE_PROBE=tools/oracle/tables_probe.lua [PROBE_FRAMES=400] \
--   [PROBE_CI2=1] [PROBE_CREDITS=1] [PROBE_RIGHT=1] \
--     Mesen --testRunner tools/oracle/dump_levels.lua rom.sfc --timeout=300
-- writes dir/tables.log: in game modes $11-$14, a line whenever game mode,
-- $13D7, $1936, $0BF5, a sum of the tables $0BF6-$0CF5 and their first
-- entries, $5B, layer 1's Y, the player's Y, or $1931 change; and
-- dir/intro_tables.txt: the pointers $65-$6A and $CE-$D0 and the tables on
-- the first frame of mode $14 (the intro's, if the level has one).
-- PROBE_CI2 makes the load Choc Island 2's (translevel $24, a sublevel
-- past the first); PROBE_CREDITS goes to the credits' enemy list once the
-- level runs, logging a sum of the tile grid on each list screen;
-- PROBE_RIGHT carries the player right two pixels a frame, logging where
-- he is every 20 frames. A vanilla-layout LoROM ROM's addresses.

local frames = tonumber(os.getenv("PROBE_FRAMES") or "400")
local mem = emu.memType.snesMemory
local function rd(a) return emu.read(a, mem) end
local function w(a) return rd(a) | rd(a + 1) << 8 end
local last = ""
local n = 0
local dumped = false
local ci2 = false
local credits = false
local vram_logged = nil
emu.addEventCallback(function()
  n = n + 1
  local mode = rd(0x7E0100)
  if os.getenv("PROBE_CI2") and mode == 0x11 and not ci2 then
    -- Choc Island 2's rooms: translevel $24, a sublevel past the first.
    emu.write(0x7E13BF, 0x24, mem)
    emu.write(0x7E141A, 1, mem)
    ci2 = true
  end
  if os.getenv("PROBE_CREDITS") and mode == 0x14 and not credits then
    credits = true
    emu.write(0x7E0100, 0x20, mem)
    return
  end
  if (mode == 0x25 or mode == 0x23) and vram_logged ~= mode then
    vram_logged = mode
    local vsum = 0
    for a = 0x7EC800, 0x7EFFFF do vsum = (vsum * 31 + rd(a)) & 0xFFFFFF end
    for a = 0x7FC800, 0x7FFFFF do vsum = (vsum * 31 + rd(a)) & 0xFFFFFF end
    local f = assert(io.open(os.getenv("KOBO_ORACLE_OUT") .. "/tables.log", "a"))
    f:write(string.format("%6d mode %02X grid %06X 13D7 %04X 0BF5 %02X\n", n, mode, vsum, w(0x7E13D7), rd(0x7E0BF5)))
    f:close()
  end
  if mode < 0x11 or mode > 0x14 then return end
  local sum = 0
  for a = 0x7E0BF6, 0x7E0CF5 do sum = (sum + rd(a)) & 0xFFFF end
  local line = string.format("mode %02X 13D7 %04X 1936 %04X 0BF5 %02X tables %04X [%04X %02X %04X %02X] 5B %02X 1C %04X 96 %04X 1931 %02X",
    mode, w(0x7E13D7), w(0x7E1936), rd(0x7E0BF5), sum, w(0x7E0BF6), rd(0x7E0BF8), w(0x7E0BF9), rd(0x7E0BFB), rd(0x7E005B), w(0x7E001C), w(0x7E0096), rd(0x7E1931))
  if mode == 0x14 and not dumped then
    dumped = true
    local f = assert(io.open(os.getenv("KOBO_ORACLE_OUT") .. "/intro_tables.txt", "w"))
    f:write(string.format("ptrs 65-6A %02X %02X %02X %02X %02X %02X CE-D0 %02X %02X %02X 1692 %02X 190E %02X 13BF %02X\n", rd(0x7E0065), rd(0x7E0066), rd(0x7E0067), rd(0x7E0068), rd(0x7E0069), rd(0x7E006A), rd(0x7E00CE), rd(0x7E00CF), rd(0x7E00D0), rd(0x7E1692), rd(0x7E190E), rd(0x7E13BF)))
    for a = 0x7E0BF6, 0x7E0CF5 do f:write(string.format("%02X ", rd(a))) if (a - 0x7E0BF6) % 32 == 31 then f:write("\n") end end
    f:close()
  end
  if line ~= last then
    local f = assert(io.open(os.getenv("KOBO_ORACLE_OUT") .. "/tables.log", "a"))
    f:write(string.format("%6d %s\n", n, line))
    f:close()
    last = line
  end
end, emu.eventType.endFrame)
return function(frame, api)
  if os.getenv("PROBE_RIGHT") then
    -- Carry the player right two pixels a frame; log where he stands.
    local x = api.peek(0x7E0094) | api.peek(0x7E0095) << 8
    x = x + 2
    api.poke(0x7E0094, x & 0xFF)
    api.poke(0x7E0095, x >> 8)
    api.poke(0x7E1497, 0x7F)
    if frame % 20 == 0 then
      api.log("frame %d x %04X y %04X air %02X", frame, x, api.peek(0x7E0096) | api.peek(0x7E0097) << 8, api.peek(0x7E0072))
    end
  end
  return frame >= frames
end
