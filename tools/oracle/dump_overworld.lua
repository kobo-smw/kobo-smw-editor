-- Kobo overworld oracle: boots SMW in Mesen 2, starts a new game on the
-- file select, and dumps the overworld as the game loaded it: the memory
-- effects a ROM's overworld code leaves, the yardstick for Kobo's own
-- (docs/lunar-magic-install.md, "The overworld").
--
-- Usage (see dump_overworld.sh):
--   KOBO_ORACLE_OUT=/some/dir Mesen --testRunner dump_overworld.lua rom.sfc --timeout=120
--
-- It writes, on the first frame of game mode $0E (the overworld running):
--   overworld.wram.bin   $7E0000-$7FFFFF, the stack page ($0110-$01FF) zeroed
--   overworld.vram.bin   64 KiB of VRAM
--   overworld.cgram.bin  512 bytes of CGRAM
--   overworld.ppm        the screen, KOBO_ORACLE_FRAMES frames later (default 60)
--
-- How: Start on the title screen, A twice on the file and player select,
-- and $0109 (the overworld override, which a new game sets to the intro
-- level) held at 0 until the overworld loads, so the game goes there
-- straight. No callback stops at an address: what runs is only watched
-- through RAM, so nothing of a Lunar Magic-saved ROM's code is seen
-- (docs/clean-room.md). A vanilla-layout SA-1 ROM is not handled.

local mem = emu.memType.snesMemory
local outdir = os.getenv("KOBO_ORACLE_OUT") or "."
local after = tonumber(os.getenv("KOBO_ORACLE_FRAMES") or "60")
local log = assert(io.open(outdir .. "/oracle.log", "a"))

local function logf(fmt, ...)
  log:write(string.format(fmt, ...), "\n")
  log:flush()
end

local function peek(addr)
  return emu.read(addr, mem)
end

local function poke(addr, value)
  emu.write(addr, value, mem)
end

local function read_range(base, len, memtype)
  local parts = {}
  for i = 0, len - 1 do
    parts[#parts + 1] = string.char(emu.read(base + i, memtype))
  end
  return table.concat(parts)
end

local function write_file(name, data)
  local f = assert(io.open(outdir .. "/" .. name, "wb"))
  f:write(data)
  f:close()
end

local function write_frame(name)
  local pixels = {}
  for _, rgb in ipairs(emu.getScreenBuffer()) do
    pixels[#pixels + 1] = string.char((rgb >> 16) & 255, (rgb >> 8) & 255, rgb & 255)
  end
  local size = emu.getScreenSize()
  write_file(name, string.format("P6\n%d %d\n255\n", size.width, size.height) .. table.concat(pixels))
end

local stage = "boot"
local frames = 0
local dumped_at = nil
local buttons = {}

local function tap(name)
  if frames % 8 == 0 then
    buttons[name] = true
  end
end

local function dump()
  local wram = {}
  for addr = 0x7E0000, 0x7FFFFF do
    local stack = addr >= 0x7E0110 and addr < 0x7E0200
    wram[#wram + 1] = string.char(stack and 0 or peek(addr))
  end
  write_file("overworld.wram.bin", table.concat(wram))
  write_file("overworld.vram.bin", read_range(0, 0x10000, emu.memType.snesVideoRam))
  write_file("overworld.cgram.bin", read_range(0, 512, emu.memType.snesCgRam))
end

local function on_frame()
  frames = frames + 1
  if frames > 3000 then
    logf("FAIL: stuck in %s (game mode $%02X)", stage, peek(0x7E0100))
    write_frame("stuck.ppm")
    log:close()
    emu.stop(2)
    return
  end
  local mode = peek(0x7E0100)
  buttons = {}
  if stage == "boot" and mode == 0x00 then
    stage = "title"
  end
  if stage == "title" then
    if mode == 0x07 then
      tap("start")
    elseif mode == 0x08 then
      stage = "file"
    end
  elseif stage == "file" then
    if mode == 0x08 or mode == 0x0A then
      tap("a")
    elseif mode > 0x0A then
      stage = "going"
    end
  end
  if stage == "going" or stage == "file" then
    -- The new game's intro level is chosen through $0109; going to the
    -- overworld instead is $0109 = 0.
    if mode >= 0x0A and mode < 0x0E then
      poke(0x7E0109, 0)
    end
    if mode == 0x0E then
      dump()
      logf("dumped at frame %d", frames)
      stage, dumped_at = "after", frames
    elseif mode >= 0x0F then
      logf("FAIL: game mode $%02X before the overworld", mode)
      log:close()
      emu.stop(3)
      return
    end
  elseif stage == "after" and frames - dumped_at >= after then
    write_frame("overworld.ppm")
    log:close()
    emu.stop(0)
  end
end

local function on_input()
  emu.setInput(buttons, 0)
end

emu.addEventCallback(on_frame, emu.eventType.endFrame)
emu.addEventCallback(on_input, emu.eventType.inputPolled)
