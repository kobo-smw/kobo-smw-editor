# SA-1

What an SA-1 ROM changes for the renderer, and how it is modelled. An SA-1 cartridge running
SMW is SA-1 Pack (Vitor Vilela): the game does not run on the chip without it. Its source is
the reference (`~/.local/share/kobo/docs/sa1pack/`, a clone of `VitorVilela7/SA1-Pack`, not
committed); `docs/memory-map-summary.md`, `docs/Sprite-Remap.md`, and `docs/maxtile.md` in it
are the ones to read.

## Recognising one

- Map mode `$23` at `$00FFD5` gives `Mapping::Sa1Rom`, or `Mapping::BigSa1Rom` for an image
  over 4 MiB, and `RamMap::of` takes either to mean `RamMap::Sa1Pack`. Nothing looks for
  SA-1 Pack itself.
- The Super MMC (`$2220`-`$2223`) picks the 1 MiB block behind each quarter of the HiROM
  view (`$C0`-`$FF`) and, with bit 7, of the LoROM view (`addr::SuperMmc`). SA-1 Pack
  writes the table at `$008A60` to the registers at start-up and never again, `00 01 02 03`
  for an image up to 4 MiB (both views show the same 4 MiB) and `04 05 06 07` with `6mb.asm`
  or `8mb.asm` (the first 4 MiB in the LoROM view alone, the rest in the HiROM view alone).
  The second is Asar's `bigsa1rom`, and `Mapping::BigSa1Rom` here. A `Mapping` is that
  assignment, which is what everything reading a ROM's tables uses; the bus follows the
  registers once the game has written them, so code that switches banks as it runs reads
  what the cartridge would give it. No ROM in the corpus does.
- A reference ROM is vanilla with SA-1 Pack applied, which should load every level as vanilla
  does: copy the headerless vanilla ROM and run `asar sa1.asm rom.sfc` in the source's `asm/`
  (Asar 1.91 works). It lives in `~/.local/share/kobo/roms/sa1/` and, like any ROM, is never
  committed. SA-1 Pack's guide says to save a level in Lunar Magic afterwards; the reference
  ROM has not had that done. A Kobo project with `[rom] sa1 = true` and nothing else builds
  the same bytes.

## The two processors

- The SA-1 is a second 65816 with its own view of the bus: I-RAM at `$0000`-`$07FF` and
  `$3000`-`$37FF`, BW-RAM in banks `$40`-`$4F` and through a window at `$6000`-`$7FFF`
  (`$2224` for the S-CPU, `$2225` for the SA-1, whose bit 7 selects the bitmap view), BW-RAM
  as 2- or 4-bit cells in banks `$60`-`$6F`, the ROM, its registers, and nothing of the
  console: no work RAM, no PPU. It takes its reset and IRQ vectors from `$2203`/`$2207`, and
  can replace the S-CPU's IRQ and NMI vectors (`$220E` and `$220C`, selected by `$2209`
  bits 6 and 4). SA-1 Pack does both: its S-CPU IRQ handler is in work RAM at `$1D00`, and
  the NMI vector is the cartridge's own again. Both handlers begin and end in SA-1 Pack's
  code (`snes_nmi`, `snes_nmi_end`), which saves another set of registers than vanilla's
  and writes `$4200` last, from X.
- SA-1 Pack's calls are IRQs with a mailbox. S-CPU to SA-1: pointer in `$3180`-`$3182`,
  `JSR $1E80` (work RAM) writes `#$80` to `$2200` and spins on `$3189`; the SA-1's handler
  does `JML [$3180]` and increments `$0189` (the same byte: I-RAM is at both). SA-1 to
  S-CPU, used where the SA-1 needs the console (`stripe_help`, `score_stuff` in
  `boost/level_mode.asm`): pointer in `$0183`, `#$D0` to `$2209`, spin on `$018A`. Code
  tells which processor it is on from the stack page (`$37` is the SA-1's).
- Only one processor runs at a time (`cpu::smw_bus`): the SA-1 gets a turn when the S-CPU
  stops to wait, and runs until it waits itself. `Cpu::run` recognises a wait as `WAI` or as
  two passes round a loop with the registers and the CPU's own write count unchanged. The
  turn must not come when the S-CPU *triggers* the SA-1: `snes_init` releases the SA-1 from
  reset and then clears `$3189`, relying on the SA-1's start-up taking longer than that,
  and a SA-1 run at the trigger has already answered by then.
- An S-CPU IRQ needs interrupts enabled, so `Machine` enters every routine with `I` clear,
  as game-mode code runs. The direct page is `$3000` (`RamMap::direct_page`).
- Boot goes through the reset vector, in emulation mode. SA-1 Pack points the vector at
  `snes_init`, which sets the SA-1 up; entered in native mode it takes itself for a second
  boot on a swapped ROM image and flips the Super MMC bank bits.
- A fault on the SA-1 (`BRK`, `STP`, the step limit) stops it, and the S-CPU's wait for it
  then fails; `Machine` reports the SA-1's error (`CpuError::Sa1`) instead of the wait.
  The SA-1's state is part of `Ram` so that restoring a snapshot after a failed pass brings
  back a SA-1 that is idle, not one stuck in the middle of a handler.

## What SA-1 Pack moves

- RAM (`ram::RamMap::Sa1Pack`): `$7E0000`-`$7E00FF` to I-RAM `$3000`, `$7E0100`-`$7E1FFF` to
  `$400100`, the tile grid `$7EC800`/`$7FC800` to `$40C800`/`$41C800`, Wiggler segments to
  `$418800`, the sprite load flags `$1938` to `$418A00` (255 of them). The rest of work RAM
  stays, for the S-CPU alone. The per-slot sprite tables are packed at `$3200` and `$74C8`
  with 22 slots each; `$9E`, `$D8`, and `$E4` left the direct page, and `$AA` and `$C2` moved
  into the space (`$9E` and `$D8`).
- Every vanilla level's sprite memory setting is rewritten to `$08` in the ROM
  (`remap/sprite_memory.asm`), bar the boss and Wiggler ones, so slots are given out
  differently from vanilla.
- Graphics are decompressed by the SA-1 into BW-RAM at `$410000`, then copied into work RAM
  by the S-CPU with a DMA to the work RAM port (`$2180`-`$2183`), which the bus therefore
  models. (LoROM hacks use the port too: Invictus's custom sprite `D1` draws nothing
  without it.)
- `ConsolidateOAM` (`$008494`, where every drawing pass ends) becomes MaxTile's
  `oam_compress`, run on the SA-1: it rebuilds all of `$0200`-`$03FF` from four priority
  buffers, packed down from `$03FC`. After a frame no object is where the game drew it, so
  whatever is found by its OAM index (the player in objects 64-71, the candle flames in
  124-127) is read when the frame gets to `$008494`, with sizes from the unpacked table at
  `$0420` (`expand::oam::draw_frame`). MaxTile also changes which sprite is in front of
  which, deliberately.
- The OAM upload (now from `$006200`, the BW-RAM window) no longer applies `$3F`
  (`org $00846A : RTS`), so the PPU draws from object 0; MaxTile puts the objects from
  `$3F` on in its first buffer instead. The renderer runs the ROM's upload and reads what
  the PPU was sent (see [smw.md](smw.md)), so the order is the ROM's either way; objects
  the game draws at fixed indices are picked out before `oam_compress` and found again
  afterwards by what they are (`expand::oam::Frame::uploaded_from`).
- SA-1 DMA (`$2230`-`$2239`) copies between ROM, BW-RAM, and I-RAM, starts when the last
  byte of the destination address is written (`$2236` for I-RAM, `$2237` for BW-RAM), and
  ends with an IRQ to the SA-1 (`$2301` bit 5), whose handler sets `$018C` for the code
  that waits on it. SA-1 Pack itself does not use it outside character conversion; hacks
  do (QLDC 2021 `24_HD_DankBaron` level `10B`). It takes no time here.
- Character conversion, first type (`DCNT` = `$B0`), is how SA-1 Pack uploads dynamic
  sprites: code queues up to ten slots in the table at `$3190` (count in `$317F`), and
  `snes_nmi` has the SA-1 turn the conversion on (message 1 to `$2200`; the handler writes
  `$2230` and sets `$318D`), then for each slot writes `CDMA` (`$2231`: bits 1-0 the depth,
  8, 4, or 2 bits to a pixel, bits 4-2 the bitmap's width as a power of two in characters),
  the bitmap's BW-RAM address to `SDA` and to the DMA channel, and the I-RAM buffer
  `$3700` to `DDA`, all from the S-CPU. Writing `$2236` starts it and raises the S-CPU's
  IRQ (`$2300` bit 5, enabled by `$2201` bit 5, cleared through `$2202`), which
  `snes_irq` passes on in `$318D`, where the NMI is waiting with interrupts on. The S-CPU's
  DMA then reads the bitmap's address and gets the PPU's planar characters instead, 64, 32,
  or 16 bytes each, the first pixel of a bitmap byte in its low bits
  (`cpu::sa1::Conversion`): every read of banks `$40`-`$4F` by the S-CPU is converted until
  `CDMA` bit 7 or `DCNT` ends it. The I-RAM buffer the chip converts through is not
  written. `24_HD_DankBaron` level `10B` is the one user in the corpus (four slots at
  `$402000`, 4 bits, four characters wide: the player's flying machine, 32x32, into sprite
  tiles `1C0` on), and its tiles come out whole; nothing compares them to an emulator.

## What it changes in the vanilla levels

Checked against Mesen 2 (`docs/testing.md`): the reference ROM's tile grids (512 levels),
layer 3 tilemaps, and entrance-screen sprite slots match the emulator's, and its pictures
match the emulator's frames as closely as vanilla's do. Against the vanilla ROM it renders
487 of 512 levels identically. The rest have the same objects overlapping in another order
(MaxTile's priorities), or differ because a sprite's slot differs, and the game looks at
the slot:

- The line-guided rope (`64`) has nine segments instead of five in a slot from 6 up when
  the sprite memory setting is not zero (`CODE_01DC54`). Every level has setting `$08` now
  and its loader hands slots out from 19 down, so every rope is long
  (`00F`, `0DD`, `12A`, `12C`; Mesen shows the long rope in `0DD`).
- Animation frames and palettes taken from the slot number: the Yoshi's House birds'
  colours (`104`), another frame in `0FC` and `1DD`.
- Sprites vanilla had no slot for: the fifth Eerie of a generator group (`DE`) in `11D`,
  `1E8`, and `1E9`, where setting `$0B` allows five slots.
- Boss arenas (`098`, `0D9`, `198`) differ in their flames.

## Lunar Magic's layout on SA-1

Kobo's code for Lunar Magic's layout (`asm/lunar-magic/`) runs on SA-1 Pack 1.40 as on
LoROM, with the game's variables where SA-1 Pack keeps them (`memory.asm`), its uploads on
SA-1 Pack's DMA channel, its sprite loader on SA-1 Pack's loop, and what the SA-1 runs
asking the S-CPU for work RAM. How Lunar Magic's own install looks on SA-1, and what Kobo's
does differently there, is in
[lunar-magic-install.md](lunar-magic-install.md#on-an-sa-1-rom).

## Not modelled

Timers, the second type of character conversion (the SA-1 feeding bitmap lines through
registers), the variable-length bit reader, write protection, and the SA-1's NMI. Across
every level of the 39 SA-1 hacks in the corpus, the only registers written that are not
modelled are the write protection ones (`$2226`-`$222A`) and the SA-1's NMI vector
(`$2205`-`$2206`), both set once by SA-1 Pack's start-up, and `$2306`, which is read-only.
The bus accepts those writes without effect, so they are not reported as unmodelled
hardware (`cpu::access`); a write to any other SA-1 register outside the model is.
