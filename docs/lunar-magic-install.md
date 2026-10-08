# Lunar Magic's one-time install

What Lunar Magic 3.70 writes into a ROM only when its install is not there yet, how a
save decides that piece by piece, what each piece replaces in the vanilla game, what it is
for, and what it has to leave behind. A piece whose tables a Kobo build writes has to be
Kobo's, with the check Lunar Magic makes for it met, or a save installs Lunar Magic's
over it and resets the tables; any other piece a save may install
([build.md](build.md#lunar-magics-layout-piece-by-piece)). The rest of Lunar Magic's footprint, and the
spike that found this set, are in [lunar-magic.md](lunar-magic.md); the vanilla code
named here is described in [smw.md](smw.md).

Confidence, per statement: *documented* (Lunar Magic's help, the community's format
documentation, or a tool's source), *observed* (a byte diff's addresses, data tables'
values, or RAM after a level load), *inferred* (from the vanilla code a change covers and
the feature it belongs to). Nothing here comes from reading Lunar Magic's code: every
change to vanilla code is described by the vanilla instructions it covers and the effect
seen, never by what Lunar Magic wrote. Until 2026-10-02 some tools showed more than that
(a decoded byte, an instruction's address in an error, the registers a routine returned);
what was found that way was removed on 2026-10-03
([clean-room-audit.md](clean-room-audit.md)).

The goal is interoperability, not byte-for-byte equivalence (maintainer, 2026-10-03):
Kobo's code leaves the RAM and ROM that Lunar Magic, a tool, or a hack is known to read
as Lunar Magic's does, and is free to differ in what nothing known reads (scratch RAM,
its own state, values like `$54` and `$0BEE`-`$0BEF` below). A difference is matched once
a reader of it is found. Accepted under that rule, each described in its section:

- the VRAM patch's own state (`$0695`-`$06BE`, the upload buffers, `$7FBC00`/`$7FC300`)
  and the row above a level's top, laid out as Kobo's ("Graphics");
- Lunar Magic's game loop hook and stripe upload, which Kobo's patch has no counterpart
  for: stripe images leave the same VRAM ("Graphics", "Game loop, stripe images");
- `$7FC009` after the graphics loader, and the layer 3 tilemap buffer it puts aside in
  VRAM while a `$2000`-byte tilemap loads ("Graphics");
- `$02950B`, the cape's block checks, left as the game has it ("Taller levels");
- the castle intro's, Choc Island 2's rooms', and the credits' `$0CB6` and `$0CD6`,
  which Kobo sets to size 0's ("Taller levels");
- the layer 3 code's state, which Kobo's leaves as Lunar Magic's does anyway, and need
  not ("Layer 3 settings");
- the player's tile words on a level's first frames, which a Kobo build leaves as the
  vanilla ROM does (testing.md);
- on SA-1, the conditional Direct Map16 flags read through SA-1 Pack's call to the
  S-CPU (the one place Kobo's code relies on its inter-processor calls), the uploads on
  DMA channel 2 reading from bank `$00`, and `$187B` not kept by Kobo's loader with PIXI
  on SA-1, where PIXI keeps the goal tape's extra bits itself ("On an SA-1 ROM").

## How it was found

- The set is the spike's: `e0` is the vanilla ROM after `-ImportLevel` of level `105`'s own
  MWL; `e9` is `e0` with every changed range outside the 52 hook sites put back to vanilla
  and the gate left set, saved again. What that save left vanilla is the one-time set: 15
  hooks and 95 ranges (`$695` bytes). Ranges it restored only in part are split here into
  their one-time and restored parts.
- `tools/lunar-magic/sites.py` names the vanilla instructions or data a range covers
  (SMWDisX assembled with `--symbols=wla`, whose addr-to-line map ties each address to a
  source line) and which of their byte offsets changed. Of Lunar Magic's bytes it prints
  only the target of a jump at a hook site, where its change starts (since 2026-09-27;
  before that it also decoded jumps inside regions Lunar Magic rewrote).
- `tools/lunar-magic/ramdiff.py` loads the same levels in two ROMs with `kobo level wram`
  and reports the RAM that differs (the stack blanked, since it holds return addresses);
  `--revert` puts chosen ranges of the second ROM back to the first's bytes, so a hook's
  effect shows as what disappears without it. Levels used: `105` (mode `00`, background),
  `1A` (`02`, layer 2 objects), `E` (`01`), `1CE` (`08`, vertical), `C2` (`0A`, vertical
  with background), `95` (boss), `4` (`0C`), `18` (`0E`), and all 512 for the summary.
- Every ROM of `KOBO_LM_ROMS` (Lunar Magic 1.62 to 3.51) was compared with `e0` range by
  range, which dates some pieces: what matches `e0` only in 3.x ROMs came with 3.00, what
  no corpus ROM has is newer than 3.51.
- Accessible documentation: the help file's technical pages (`info_map16_gameplay`,
  `info_screen_exit_routine`, `info_decompress`, `info_command_line`) and its version
  history (`changes`), the smwspeedruns level format page (saved in
  `~/.local/share/kobo/docs/`), and the sources of PIXI, GPS, UberASM Tool, AddmusicK, and
  SA-1 Pack (not `boost/lz3.asm`).

## How a save decides what to install

Found on Kobo's install (bank `$06` only, `$06F600` set) and on vanilla after one Lunar
Magic save, by copying one ROM's changed ranges onto the other in halves until a save
stopped rewriting a watched site or resetting a watched table
(`tools/lunar-magic/install-gate.py`, which prints addresses only), then trying single
ranges. A `JSL` counts whatever its target, a `JML` or other bytes do not. Every piece
not met is installed by the save, over whatever is at its sites, and its tables reset.

| Piece (sites and tables) | Counted as installed when |
|---|---|
| Map16 routine, acts-like chain, Yoshi's tongue (`$06F540`-`$06F8DE`, `$02BA9E`, `$01A24D`, `$01F58A`, `$02BAE9`) | `$06F600` is not `$FF` |
| Per-level tables `$05DE00`, `$06FC00`-`$06FFFF`; midway points (`$05D9C3`, `$05D9E8`, `$00F2DB`); `$05D718`/`$05D728`; `$02ABF3`; `$00A6CC` and `$05DD00` | a `JSL` at `$05DA17` (a hook a save restores) |
| Sprite data banks `$0EF100` and the `$05D8F5` hook | a `JSL` at `$05D8F5` |
| BG Map16 pointers `$0EFD50` and the `$058DA4` hook | a `JSL` at `$058DA4` |
| Taller levels (`$00BDA8` tables, `$00F478` bounds, `$00A2AF` screen shake) | a `JSL` at `$05DA8A` (restored) |
| The sprite loader (`$02A826` and its group, "Sprites" below) | a `JSL` at `$02AF3D`, whatever it calls |
| The game loop hook (`$008072`) | the restored group checked at `$00A5A2` |
| Extended objects (`$0DA10F`) | a `JSL` at `$0583C7` (restored) |
| The level number hook (`$05D8E2`) | `$0EF550`-`$0EF56B` not all `$FF` |
| The background hook (`$05803B`) and the level flags (`$0EF310`) | the byte at `$0EF519` is `$5C` (`JML`); `$00`, `$01`, `$80`, `$FE` do not count |
| ExAnimation (`$0583AD`, `$0095B5`, `$00A2A5`, `$00A5FD`, `$00A390`, `$008A4E`, `$00A5E1`; the tables through `read3($0583AE)`, "ExAnimation" below) | a `JSL` at `$00A390`, whatever follows; Lunar Magic installs it only with a level's or the global ExAnimation, not on the first save |
| Custom palettes (`$0EF600` and the `$00A5BF` hook) | not bisected: with Kobo's `JSL` at `$00A5BF`, a save kept every `$0EF600` pointer a build wrote; whether it keeps Kobo's hook or points it at `$0EF570` was not checked |

- Hooks into the areas a save always rewrites (`$05D7CE` to `$05DC50`, `$05DBC2` to
  `$03BB00`, `$05DB5B`, `$04E5F1`) are pointed back at Lunar Magic's code by every save.
  A restored hook is retargeted to Lunar Magic's code too, so its `JSL` protects the
  piece's tables but not Kobo's code behind the hook.
- `$0EF310` (flags) and `$06FA00` are written for every level by every save; `$0EF600`
  (custom palettes) was kept in every trial.
- `-ImportAllMap16` rewrites the bank `$06` code (`$06F540`-`$06F643` and most of the
  acts-like code), installs the Yoshi's tongue hooks, and writes `$06F600` = `$EA`, on a
  ROM with Kobo's code there: a Map16 import is Lunar Magic's own operation on bank
  `$06`, whatever the check says. It keeps the tables' contents' layout, so Kobo's data
  reads the same through either code.
- On a Kobo build with only bank `$06`, `-ImportLevel` and `-ImportMultLevels` install
  every other piece of the set (all 15 one-time hooks but those in bank `$06`'s piece).

## What a save does with its install there

Found by saving copies of `e0` with parts of the set changed (level `105` re-imported each
time); the command line reports success in every case.

- Foreign bytes in every one-time code area (`$00BA56`, `$03BCDC`, `$05DD00`, `$06F540`-
  `$06F5FF`, the acts-like code from `$06F65C`, `$0DE1D0`-`$0DE24F`, `$0EFD00`-`$0EFD3F`)
  survive a save, alone or with every one-time change to vanilla code put back to vanilla.
  Nothing of the set is reinstalled. Kobo's code at Lunar Magic's fixed addresses is kept.
- Some areas outside the set are Lunar Magic's on every save: it writes its own code over
  whatever is at `$03BB00`-`$03BB1E`, `$03BCA0`-`$03BCBF`, `$05DD30`-`$05DD7F`, and
  `$0EF510`-`$0EF54F`, and its table pointers into `$05DC81`, `$05DC86`, `$05DC8B`,
  `$0DE191`, `$0DE198`, and `$0DE19F` (the rest of `$05DC50`-`$05DC8E` and
  `$0DE190`-`$0DE1CF`, `$03BCE0`-`$03BD9F`, `$05DCB0`, and `$05DCCC`-`$05DCCF` keep foreign bytes and
  are rewritten only from `$FF`). Code in the first two areas is how
  `kobo_core::clean_room` tells a save whose marker is gone, so Kobo's builds keep them
  `$FF`, as SA-1 Pack and the pinned tools do (`tests/lunar_magic_save.rs`,
  `tests/tool_stages.rs`); `$0EF510` has Kobo's background entry. Two one-time hooks jump into those areas (`$05803B` to
  `$0EF510`, `$05DBC2` to `$03BB00`), so after a save they run Lunar Magic's code whatever
  Kobo put there. Kobo's two hooks can instead jump to Kobo's own blocks, since Lunar
  Magic keeps retargeted hooks; the four areas must then stay free for Lunar Magic.
  `$05DD30`-`$05DD7F` is the exception since 2026-10-01: with `"LM"` at `$05DD7C`, which
  Kobo's builds have (register below), a save leaves it, and its entrance and camera
  sites, as they are.
- `$0FF035` is rewritten by every save, to a value that depends on the state of the
  one-time code (`$D8` after a fresh install; `$00`, `$7F`, or `$FF` with single pieces
  reverted). When the bytes below it are not `$FF` it also rewrites some of them (with all
  one-time code foreign, `$0FEFC8`-`$0FF034`; over AddmusicK's `$55` padding,
  `$0FEF9F`-`$0FF091`). What the byte records is unknown. Most corpus ROMs have `$00`
  there and non-`$FF` bytes below, several `$FF` and nothing below.
- Resaving a 3.31 ROM (`SMW_2022-4-9`) with 3.70 changed 11 items of the set: it added the
  3.50 Choc Island 2 hook and the 3.70 game loop hook and rewrote `$0085D2`,
  `$00A2AF`, and `$0580C0`, so 3.70 upgrades an older install in place. It does not put
  the same pieces back into a 3.70 install they were removed from, whether the marker at
  `$0FF0A0` says 3.70, 3.30, or is absent. What decides the upgrade is unknown; a foreign
  install was not upgraded.

## Bytes Kobo writes because Lunar Magic or other tools check them

Bytes of Lunar Magic's own that Kobo writes only because Lunar Magic, or a patch or tool
that expects its install, checks them before it treats a piece as installed, and only as
many as the check needs ([clean-room.md](clean-room.md)).
Each can tell Lunar Magic more than Kobo means: a feature that goes in after one of these
has its Lunar Magic check run with and without the bytes ([testing.md](testing.md)), and
what Lunar Magic does differently is recorded here.

| Bytes | What checks them | How found | Status |
|---|---|---|---|
| `$03BD9C`-`$03BD9F` = `4C 4D 10 01` (`"LM"` `$10 $01`), a marker in 3.x ROMs, not in 2.5x | Lunar Magic's MWL export and save read a secondary entrance's two further tables through `$05DC86`/`$05DC8B` only with it and the pointers below; without it they write new, empty tables | bisecting its `-ExportLevel` over a Kobo build and a Lunar Magic-saved vanilla ROM, printing addresses only, then taking each byte away from a Kobo build (2026-09-27); in neither the help file nor the open tools' sources | written (`entrance.asm`). With it, a save also writes `$03` rather than `$25` at `$0FEFB4`, in the area it keeps for itself (above); nothing else it writes changes |
| The vanilla entrance tables' pointers, `$05DC81` = `$05FE00`, `$0DE191`/`98`/`9F` = `$05F800`/`FA00`/`FC00`, at the fixed addresses the community's level format page gives for them (`read3($0DE191)` and so on) | the same check: without `$05DC81` or `$0DE191` the further tables are lost, and without `$0DE198` and `$0DE19F` the save reads the main tables from elsewhere and a secondary entrance's Y is lost. The bytes before them, and the rest of `$05DC50`-`$05DC8E` and `$0DE190`-`$0DE1FF`, can stay `$FF` | the addresses from the community's page; that the check needs them, by taking each byte away from a Kobo build (2026-09-27). (The session that did so also printed and identified the byte before each pointer, which no check needs; that is not used: [clean-room-audit.md](clean-room-audit.md)) | written (`entrance.asm`), the pointers alone |
| `$06F5FC`-`$06F5FD` = `4C 4D` (`"LM"`), just before the gate | Lunar Magic's Map16 editor and `-ExportAllMap16` read the acts-like tables through `$06F624`, pages `$10`-`$7F` through their pointers, and page 2 per tileset through `$06F547` and `$06F586` only with it; without it they read all of those from elsewhere, getting other bytes for every tile (pages 0 to `$0F` and the tilesets' pages 0 and 1 read the same either way), and a Map16 save from the editor would write that back | bisecting `-ExportAllMap16` of a Kobo build against the same with Lunar Magic's bytes copied in (a Lunar Magic-saved ROM with Kobo's bank `$06` code swapped in, `swap bank06`), printing addresses only (2026-09-27); in neither the help file nor the open tools' sources. Every build before it had its pages past `$0F` and its acts-like settings shown wrong in Lunar Magic's editor | written (`actslike.asm`). With it, a level save also writes other values at `$0FF05D`-`$0FF05E` and `$0FFFFF`, in the areas it keeps for itself (above); nothing else it writes changes |
| `$0FFFFF` not `$FF` (a build writes `$00`), when a build stores GFX as LC_LZ3 | Lunar Magic reads the compression setting at `$0FFFEB` only then: with `$FF` it exports every LC_LZ3 file as if LC_LZ2, and a save writes `$00` (LC_LZ2) at `$0FFFEB` over the ROM's `$02` | bisecting `-ExportGFX` of an SA-1 Kobo build with LC_LZ3 GFX against the same after Lunar Magic's `-ChangeCompression`, addresses only, then trying values (2026-09-27) | written by `[rom] lz3` builds (`build::Stage::Base`); `tests/lunar_magic_save.rs` checks both ways. What the byte records is not known; Lunar Magic's own saves leave `$00` or `$03` |
| `$06F547` = `$06` to turn page 2 per tileset on | with the marker above, Lunar Magic 3.70's editor and export take page 2 as per tileset only when this byte is `$06`; `$00`-`$05`, `$07`, `$08`, `$0E`, `$16`, `$26`, `$46`, `$80`, `$86`, `$EA`, and `$FF` all read as off (the two Lunar Magic 2.53 ROMs of the corpus have `$06` there). Kobo's Map16 routine takes any value but `$00` as on | trying values with `-ExportAllMap16` (2026-09-27) | written by a build with page 2 per tileset (`build::write_tileset_page2`) |
| `$00AAD8` = `$EA` (4bpp GFX), `$00AA47` = `$EA` and a `JSL` (`$22`) at `$0583B8` (ExGFX and lists), `"LM"` at `$0FF15C` (lists kept) | Lunar Magic reads the GFX files as 4bpp, and its ExGFX export and level export read the ExGFX tables and the level's list, only with the first three; its save keeps the lists and ExGFX tables as they are only with the marker ("Graphics", below) | bisecting `-ExportExGFX`, `-ExportLevel`, and a save over `+ExGFX` with Kobo's graphics code swapped in, printing addresses only (2026-09-28) | written (`graphics.asm`) by a build that uses Lunar Magic's graphics formats. Without the marker a save installs Lunar Magic's ExGFX code over the tables and rewrites the lists and ExGFX pointers; without any one of the other three a save keeps everything (tests/lunar_magic_save.rs) |
| `$0EF519` = `$5C` (a `JML`): Kobo's background entry at `$0EF510` is laid out so that its jump lands there | Lunar Magic's save keeps the background piece and the level flags (`$0EF310`) only with it ("How a save decides what to install") | `install-gate.py`, then trying values: `$00`, `$01`, `$80`, `$FE` do not count (2026-09-26) | written (`background.asm`). Without it a save loses a level's background in Lunar Magic's layout (`tests/lunar_magic_save.rs`) |
| `$0EF550`-`$0EF56B` not all `$FF`: Kobo's level number hook's code is placed there | Lunar Magic's save keeps the level number hook (`$05D8E2`) only with it | `install-gate.py` (2026-09-26) | written (`level.asm`). Without it a save puts its own code there, which the level number hook then reaches; the level reads the same (`tests/lunar_magic_save.rs`) |
| A `JSL` (`$22`) at `$00A390` (ExAnimation) | Lunar Magic's save treats ExAnimation as installed with it, whatever follows the opcode: it keeps every site and reads and writes the level table through `read3(read3($0583AE) + $EA)` and the global list through `+$5B`/`+$65`, as does its level export | ablation of each site from a Kobo ExAnimation install on `vanilla+LM`, then of the byte (a `JML`, `JSR`, `NOP`, or the game's bytes all fail), printing addresses only (2026-09-28) | written (`exanimation.asm`, whose NMI hook it is). Without it a save of a level with ExAnimation installs Lunar Magic's code over every site with new, empty tables (tests/lunar_magic_save.rs) |
| `$03FDFF` = `$00` (ExAnimation settings) | Lunar Magic's save keeps the per-level settings at `$03FE00` only with `$00` there; `$01`, `$80`, `$FE`, `$FF` all make it set every level's again (`$00`, level `104` `$30`), keeping the lists | `install-gate.py` over a Kobo build of Kaizo Kindergarten, then trying values, printing addresses only (2026-09-30) | written (`exanimation.asm`). With it a save also writes other values in the area it keeps for itself (`$0FEFA3`-`$0FF070`, `$0FFFFF`); tests/lunar_magic_save.rs checks both ways |
| `$05DD7C`-`$05DD7D` = `"LM"` (layer 2 scroll settings) | Lunar Magic's save keeps the separate layer 2 scroll settings (`$06FA00`'s `S` and `H`, from 3.40) only with it; without it the save sets every level's `$06FA00` to `$20` (keeping the auto-screens bit) and installs its own code at `$009708`, `$00AF72`, `$00D2B2`, `$00E966`, the camera's `$00F77B`, `$00F79D`, and `$00F871`, `$05BCA5`, `$05D7BA`, `$05D97D`, `$05DA17`, and `$05DD30`-`$05DD75` | `install-gate.py` with `$06FA00`-`$06FBFF` watched found `$05DD30`-`$05DD7F`, then removing Lunar Magic's bytes from a Kobo build one group at a time left these two (2026-10-01) | written (`entrance.asm`). With it a save leaves every one of those sites as it found them: Kobo's entrance code and camera stay, with its own code for the six of the others that change anything, and the game's at two ("The sites a save keeps with the marker" below). `tests/lunar_magic_save.rs` checks both ways |
| `$0FFFE6` = `$01` | Community patches take anything but `$00` and `$FF` there to mean Lunar Magic's VRAM patch is in, and refuse to assemble otherwise (the Romhack Races baserom's `sprite_scroll_fix.asm`) | the patch's source; Lunar Magic's install sets `$01` there ("Game loop, stripe images, and the rest") (2026-10-04) | written (`vram.asm`), in every build that uses Lunar Magic's layout. Lunar Magic's save writes the same; the RHR template's build survives it (`save-check`) |
| `$008072` = `JMP $BA56`, and a `JML` (`$5C`), not a `JSL`, at `$00BA59` | Patches that do the VRAM patch's stripe move themselves take the same hook: the Romhack Races baserom's `vram_optimize.asm` writes `JSR $BA56` at `$008072` and its own code from `$00BA56`, with an `autoclean JSL` at `$00BA59`, which frees the RATS block a `JSL` already there leads to (Asar's `autoclean` acts only on its own opcode) | the patch's source and Asar's (2026-10-08); Lunar Magic 3.70's first save writes `$008072`-`$008074` and `$00BA56`-`$00BA5C` (a byte diff of vanilla saved once) | written (`vram.asm`), as the game loop hook of every build in Lunar Magic's layout; the template's build, with the patch over it, keeps Kobo's block whole (`rats::Snapshot`) and draws as the baserom's own build does. The save over a build is checked by `save-check` |
| `$0FF0A0` not `$FF` (a build writes `$00`) | The retry system (kkevinm's, in the Romhack Races baserom and many hacks) asserts it, as "Lunar Magic has saved this ROM", where Lunar Magic's save writes its version string | the retry system's `check_incompatibilities.asm` (2026-10-04) | written (`level.asm`), one byte, not the string, so nothing reads a build as a version of Lunar Magic (`Rom::lunar_magic_version`); a save writes its string over it, and the RHR template's build survives a save. With and without it (Kaizo Kindergarten's build, 2026-10-05), Lunar Magic 3.70 prints the same messages, exports the same GFX, ExGFX, Map16, shared palette, and all 512 levels, and keeps every level through a level and a Map16 save; the first save writes other values at `$0FF017`-`$0FF018`, in the area it keeps for itself, the same each time for the same ROM. Kept in every build in Lunar Magic's layout, rather than only where a patch checks it (maintainer, 2026-10-05) |
| `$00AACD`-`$00AACE` = `A2 10` (`LDX #$10`) | Patches that replace the GFX upload check for a 4bpp upload there, the game's 3bpp one having `A2 07` (freeplay's level graphics loading optimization: anything else stops it) | the patch's source (2026-10-04) | written (`graphics.asm`) when GFX are stored as 4bpp. Kobo's upload is elsewhere and this is dead code, so such a patch assembles and its hook at `$00AA80` never runs: it changes nothing in a Kobo build. Nothing is lost: what it adds, one DMA per file in place of the CPU loop, is what Kobo's upload (`dma_buffer`) already does, and Kobo's loader stays after a save. Kept so, rather than refusing such a patch (maintainer, 2026-10-05) |
| A `JSL` (`$22`) at `$00A01F` (layer 3 settings) | Lunar Magic's save treats its layer 3 code as installed with it, whatever the operand: it leaves `$00A01F`, `$00A153`, `$0194B6`, and `$05C40C` alone; with a `JML` or the game's bytes it installs its own over all four | bisecting a save of a level with settings over a ROM with Lunar Magic's code, site by site and then byte by byte, printing addresses only (2026-09-30) | written (`layer3.asm`, whose setup hook it is). Without it a save installs Lunar Magic's code and keeps the lists (tests/lunar_magic_save.rs) |

An earlier bisection, which kept each opcode and its operand together, also found the
sprite data bank piece (the `JSL` at `$05D8F5`, and `$0EF100` giving the level's bank)
necessary. Taken away byte by byte from a build that has the marker and the pointers, it
is not: level 105's secondary entrance, with every further byte set, survives a save
whole without it. Kobo installs that piece since 2026-09-28 (`sprite-banks.asm`), and
the entrance checks give what they gave before: with the bytes above a save keeps
everything, without them it loses the secondary entrance's further tables and nothing
else (`tests/lunar_magic_save.rs`, whose level also has its sprite list in a RATS
block).

## The hooks

Each is a `JSL` over the vanilla instructions named; the bytes after the four of the `JSL`
change too. "Target" areas are where Lunar Magic keeps its code at fixed addresses. A
contract is what the vanilla instructions covered leave (inferred) or what a level load
leaves in RAM (observed); none is from Lunar Magic's code.

| Site | Vanilla (SMWDisX) | Target | Feature | Contract | Confidence |
|---|---|---|---|---|---|
| `$058A65`, `$058B45`, `$058C33`, `$058D2A` | `TAY : LDA Map16Pointers,Y` in the layer 1 and 2 row and column uploads (`CODE_0589xx`-`058Dxx`) | `$06F540` | Map16 pages 2-`7F` | A (16-bit) = tile*2 in; out: A = definition address low word, `$0C` = its bank, Y free, X (the buffer index) kept; `[$0A]` then reads the 8 bytes. For pages 0-1 the bank is vanilla's choice, by tileset: `$0D`, or `$05` when `$1931` is `$10` or more, which is the overworld's layer 1 (SMWDisX `bank_05.asm`, the uploads' `LDY #$0D`) | inferred from the vanilla code covered; `$0C` observed; used by `expand` |
| `$00C17A`, `$00C25C` | `REP #$20 : LDA Map16Pointers,Y` in the tile-change stripe builders (`CODE_00C13E`, `CODE_00C222`) | `$06F5D0` | the same, for a tile changed in play | Y = tile*2 in; out: A (16-bit) the address low word, `$06` its bank (vanilla sets `$06 = $0D` just before) | inferred |
| `$04DCFA` | `ASL : ASL : ASL : TAY` in `CODE_04DCB6`, the overworld layer 1 tilemap build | `$06F5E4` | overworld layer 1 16x16 tiles past page 0 (1.90: two pages) | A (16-bit) = tile from `$7EC800`/`$7FC800` in; out: Y indexes `[$65]` for the tile's 4 words | inferred; not observable at level load |
| `$02BA9E` | `INC $07 : LDA [$05]` (tile high byte) in `CODE_02B9FA`, Yoshi's berry check | `$06F820` | acts-like for Yoshi's tongue and berries (3.70, "Yoshi's tongue touched a block") | out: A = high byte of the tile to act as (inferred: the `LDA [$05]` it covers); `$1693` = its low byte | `$1693` observed by ablation ("Yoshi and berries" below); 3.70 only (vanilla in all 47 corpus ROMs) |
| `$05803B` | `CMP #$FF : BNE` on the layer 2 pointer's bank in `CODE_05801E` | `$0EF510` (Lunar Magic's on every save) | per-level background flags and formats | `$7FC00B` = the level's `$0EF310` byte; background decoded to `$7EB900` (low) and `$7EBD00` (high); `$7FBC00`/`$7FC300` filled (below) | observed by ablation |
| `$058DA4` | `STA $0A` of `Map16BGTiles` in the background column upload | `$0EFD00` | background Map16 from `$0EFD50` | `$0A`-`$0C` = the level's BG Map16 table; `$05`-`$06` = bytes per background screen (`$01B0`, `$0200` for 32 rows) | observed (`bg_survey`); the table's place documented (the community's level format page) |
| `$05D7CE` | `BEQ : LDA #$01` choosing the destination's high byte in the screen exit path | `$05DC50` | exits to any level; secondary exits by `$19D8` | takes the exit from `$19D8,X` (`$04` Lunar Magic format, bit 0 destination bit 8, bit 1 secondary, bit 3 water, which reaches `$192A`) | documented (the community's level format page, the flags `0000wush`; UberASM Tool's and GPS's teleport routines store the high byte `ORA #$04` there), observed (`exit_probe`) |
| `$05D8E2` | `LDA $0E : ASL : TAY` before the sprite pointer read | `$0EF550` | full level number for code | out: Y = level*2 (16-bit), as the instructions covered leave it; `$010B` = level (16-bit; PIXI's own `$05D8B9` hook stores the same); `$00FE` = level + 1 | inferred (Y), observed by ablation (RAM) |
| `$05DB5B` | `STA $68 : SEP #$20 : LDA [$CE] : AND #$7F` in `CODE_05DB3E`, Choc Island 2's rooms | `$0DE210` | 24-bit pointers to Choc Island 2's extra rooms (3.50) | the rooms' layer 1, layer 2, and sprite pointers with banks (`$67` = `$06`, `$6A` = `$0C`, `$D0` = `$07`, as the game's own load leaves them, observed in Mesen with level `0CD` moved); `$1692` sprite memory from the header, as the game's own path leaves it (inferred) | documented (changes 3.50), observed; 3.50+ only. Kobo's, at `$05DB4B` (`choc-island.asm`), in builds that write `0CD`-`0CF`; a save adds Lunar Magic's beside it and the rooms load the same |
| `$05DBC2` | `STA $19B8,X : INC $141A` in `CODE_05DBAC`, the bonus game and Yoshi wings exit | `$03BB00` (Lunar Magic's on every save) | the bonus room exit in the screen-exit format; 3.60 fixed it for low screens of tall levels | `$19B8,X` and `$19D8,X` for the screen Mario is on (`$03BCDC`), `$141A` incremented | documented (changes 1.42, 3.60), inferred |
| `$04E5F1` | `CMP #$02 : BNE : INC $1DEA` in `CODE_04E5EE` (secret exit adds 1 to the event) | `$05DCB0` | Secret Exit 2 and 3 (3.00), exits to the overworld from secondary entrances | the event number `$1DEA` for exit modes 2-4 | documented (changes 3.00), inferred |

## The ranges

### Map16 pages and the acts-like chain (bank `$06`)

- `$06F540`-`$06F643`, all vanilla `$FF` fill: the Map16 routine (`$06F540`) and its two
  other entries (`$06F5D0`, `$06F5E4`), the gate, and the acts-like pointers.
  - Fixed operands others read (documented, smwspeedruns): page tables at
    `$06F553`/`$06F557` (pages `02`-`0F`, 16-bit address and bank), `$06F55C`/`$06F560`
    (`10`-`1F`), `$06F567`/`$06F56B` (`20`-`2F`, address + 1), `$06F570`/`$06F574`
    (`30`-`3F`, + 1), `$06F594`/`$06F598`, `$06F59D`/`$06F5A1`, `$06F5A8`/`$06F5AC`,
    `$06F5B1`/`$06F5B5` (`40`-`7F` likewise); `$06F547` non-zero turns on per-tileset
    page 2, whose table is `read3` at `$06F586`/`$06F58A` plus `$1000`, `$800` bytes per
    tileset. Kobo's routine has to hold these values at exactly these addresses, which
    fixes much of its layout. A tile's definition is at its group's pointer (plus 1 for
    the groups kept less one) plus the tile number times 8, in 16 bits (observed: Kaizo
    Kindergarten's pages `02`-`0F` at `$187000`, so tile `$200` is at `$188000`, and
    `10`-`1F` at `$190000`). A pointer outside `$8000`-`$FFFF` is therefore usual, and a
    fresh install's `$00F000` puts pages 2 and up in bank `$00`'s work RAM mirror.
    Page 2 per tileset is at `$06F586`'s pointer plus the tile number times 8 plus `$800`
    per tileset, also in 16 bits, so a tileset whose `$800` bytes would pass `$FFFF`
    wraps to the start of the pointer's bank; where Lunar Magic puts a tileset's page 2
    that would not fit is not known. Lunar Magic's editor takes page 2 as per tileset
    only with `$06` at `$06F547` (register above); a Kobo build writes that, and one
    table of 15 pages (`$7800` bytes) in a RATS block, which Lunar Magic's export reads
    back whole. No corpus ROM has a table for it: the two with `$06` there (2.53) point
    it at bank `$00`, so their page 2 tiles resolve outside the image.
  - `$06F5FC`-`$06F5FD`: `"LM"`, a marker Lunar Magic's editor checks before it reads
    the tables above and the acts-like tables in this layout (register above).
  - `$06F600`: the gate, any byte but `$FF` (3.x writes `$EA`, 1.62 and 2.41 `$68`).
    `$06F602` is the acts-like chain's common exit (GPS jumps to it).
  - `$06F624`: 3-byte pointer to the acts-like table, pages `00`-`3F`, 2 bytes per tile;
    PIXI and GPS refuse `$FFFFFF`. `$06F63A`: pointer, minus `$8000`, to pages `40`-`7F`;
    `$06F63C = $FF` means none (GPS). A fresh install points `$06F624` at a RATS block and
    leaves `$06F63A` with bank `$FF`. Chains resolve until a tile below `$200`. Kobo's
  chain follows up to `$7E00` lookups, the most a chain of distinct tiles can take, so
  only a table that loops ends in its fallback, tile `$130` (cement); what Lunar Magic's
  does with a loop is not known.
- The acts-like entries (hook targets, from the spike's diff; the custom block slots below
  are the help file's): four calls to `RemapBlocks` are
  retargeted, `$00F4DD` (Mario) to `$06F660`, `$019533` (sprites) to `$06F700`,
  `$02961A` (cape) to `$06F760`, `$02A6EB` (fireballs, `CODE_02A611`) to `$06F7A0`; Yoshi's
  `JSL CODE_02B9FA` at `$01A24C` to `$06F845` and at `$01F589` to `$06F840`, and the
  berry's `JSL InitSpriteTables` at `$02BAE8` to `$06F8B0`. Code: `$06F65C`-`$06F67D`,
  `$06F690`-`$06F769`, `$06F780`-`$06F7B3`, `$06F7C0`-`$06F7CF`, `$06F7F0`-`$06F82D`,
  `$06F840`-`$06F8A0`, `$06F8B0`-`$06F8D8` (the last three, and `$02BA9E`, are 3.70's).
  Contract at the retargeted calls: vanilla `RemapBlocks`' (in: A = tile high byte,
  `$1693` = low byte; out: A = the high byte to report, 8-bit, `$1693` the low byte),
  with the tile resolved through the acts-like table first and, for each kind of contact,
  the custom block slots run.
  - Custom block slots (help file): up to three 4-byte `JSL`s at each of `$06F890`
    (Mario below), `$06F8A0` (above), `$06F8B0` (side), `$06F8C0` (top corner),
    `$06F8D0` (body), `$06F8E0` (head), `$06F920` (sprite above/below), `$06F930`
    (sprite side), `$06F980` (cape), `$06F9C0` (fireball), `$06F9F0` (Yoshi's tongue);
    `$06F8F0`, `$06F940`, `$06F950`, `$06F990` reserved. Called with A/X/Y 8-bit, X and Y
    to be preserved, Y = reported high byte and `$1693` the low byte (below `$200`), `$03`
    (16-bit) the last tile of the acts-like chain. `$1933`, `$185E`, `$0F` say which layer.
    After a fresh install `$06F890`-`$06F8A0` and `$06F8B0`-`$06F8D8` have changed and
    `$06F8A1`-`$06F8AF` and `$06F8D9`-`$06F9FF` are still `$FF`, and `$06F8B0`, the side
    slot, is also where the berry's retargeted `JSL` at `$02BAE8` goes. 3.70 does not run
    these slots: with `JSL`s to logging routines written at the head slot's three places
    and the sprite, cape, fireball, and tongue slots (all still `$FF`) of vanilla saved by
    3.70, none ran with the player in tile `$200`, `$025`, or `$130`, where Kobo's chain
    runs every one for each of them (2026-10-02). Block tools for 3.70 use GPS's 16-byte
    entries from `$06F690` instead (below). Kobo's `actslike.asm` fills
    every slot with `NOP`s and a `JMP $F602`, `$06F8B0`-`$06F8DF` among them, so a `JSL`
    a tool writes there runs, which 3.70's would not (kept so on review, 2026-10-04: the help
  file's interface, and the same play with the slots empty); its berry hooks do not
    send the berry's call to `$06F8B0` as 3.70 does (that is Kobo's side slot): all four
    call Kobo's code directly. What a closed-source block tool checks before writing a
    slot remains unknown.
  - GPS (source, `main.asm`) writes 16-byte entries at `$06F690`, `$06F6A0`, `$06F6B0`,
    `$06F6C0`, `$06F6D0`, `$06F6E0`, `$06F720`, `$06F730`, `$06F780`, `$06F7C0`, `$06F7D0`,
    `$06F7E0` (below, above, side, top corner, body, head, sprite vertical, sprite
    horizontal, cape, fireball, wall feet, wall body): `PHB : PHX : REP #$30 : LDA #id*3+1
    : JSL : PLX : PLB : JMP $F602`. So each entry is entered with the data bank and X free
    to push, A/X/Y 8-bit, `$03` holding the tile (it does `LDX $03` and `BIT $03` for pages
    `40`+), and leaves through `$06F602`. It replaces 4 bytes at `$06F67B` with a `JML` whose
    code compares A with `#$39` and `#$EA` (the low byte of the caller's return address,
    for the wall-run calls) and at `$06F717` with `#$82` (sprite horizontal), falling back
    to `$06F602`: at those two addresses Kobo's chain must hold a 4-byte instruction slot
    reached with A = the caller's return address low byte, where falling through means "no
    custom block". GPS treats `$8B` at `$06F690` without its version string as foreign
    block code, so Kobo's entry there must not start with `$8B`.
- Tile generation (`GenerateTile`, `CODE_00C077`/`CODE_00C0C4`): the operand of
  `AND #$FE` at `$00C096` and the opcode of `ORA #$01` at `$00C0E7`, which set the new
  tile's high byte from what was there. With tiles on pages 2 and up, the page has to be
  set outright (0 or 1) rather than bit 0 toggled (inferred). In every corpus ROM from
  1.62 on.

### Custom block actions, observed

What Lunar Magic 3.70's chain runs as the player meets a custom block, found by playing a
Lunar Magic-saved vanilla ROM with a GPS probe block at tile `$200` that logs each action
it is called for with the game's touch position (`$98`-`$9B`) and the player's (`$94`-`$97`)
(`tools/lunar-magic/block-probe`, `examples/contact_probe.rs`; memory effects only). The
touch offset from the player names the interaction point through the game's hitbox tables
(`PlayerXHitboxPoints` `$00E830`, `PlayerYHitboxPoints` `$00E89C`), and the point names the
call site of `$00F44D` whose return address the chain sees (the low byte GPS compares):

| Interaction point (`NormalCollision`) | Call site, return low byte | Offset (small; big) | Action |
|---|---|---|---|
| 0, centre | `$00EBAF`, `$B1` | (8, 24); (8, 18) | body |
| 1, side body | `$00EC24`, `$26` | (14 or 2, 26) | side |
| 2, side head | `$00EC3A`, `$3C` | (14 or 2, 22); (14 or 2, 15) | head |
| 3, head | `$00EC8A`, `$8C` | (8, 16); (8, 8) | below |
| 4, right foot | `$00ED4A`, `$4C` | (11, 32) | above, or top corner |
| 5, left foot | `$00EDE9`, `$EB` | (5, 32) | above, or top corner |
| wall run | `$00EB37`, `$39` | | wall feet (GPS's own check) |
| wall run | `$00EFE8`, `$EA` | | wall body (GPS's own check) |

- A foot point gives "top corner" rather than "above" in some frames: with the left foot
  alone on the block (the right one over air) standing still, and for the foot still on
  the block while walking off either edge; with the right foot alone on the block
  standing still it gives "above". The condition is not yet known.
- The "head" action is the side head point, not the head point (which gives "below").
- Top corner: a foot contact whose touch point's X within its tile (`$9A & $0F`) is 0-2
  or 13-15; any other foot contact is "above". A solid tile beside the block hides the
  corner only because the game's feet check stops at the first foot on solid ground.
- Sprites: "sprite above/below" from the vertical check (`CODE_0192C9`, its call into
  `CODE_019441` returning to `$0192D2`), "sprite side" from the horizontal one
  (`CODE_01928E`, `$019293`); the water check (`$01921E`) runs no action.
- Order: the actions see the tile after the acts-like chain and before the game's
  `RemapBlocks` (a tile acting as a coin is reported as `$02B` to them with the blue
  P-switch running, and is then solid for the player). `$03` is the last tile looked up.
- The default table a fresh install writes: pages 0 and 1 act as themselves, pages 2 to
  `3F` as `$130` (cement); `$06F63A`-`$06F63C` is `$FF8000`, none. Solid matters: the
  boss arenas' floors are sampled with high bytes past 1.
- In the Mode 7 boss battles (`$0D9B` bit 7: Iggy and Larry `$80`, Reznor, Morton, Roy,
  and Ludwig `$C0`, Bowser `$C1`; SMWDisX `rammap.asm`) the
  chain neither follows the table nor runs an action: the tile goes to `RemapBlocks` as
  it was, so a floor sampled as `$3232` stays `$3232` whatever the table says (observed
  with the player: `$0D9B` = `$C0` or `$80` passes tiles through, `$40` or `$00` follows
  the table; a probe block in arena `095` logs nothing).
- Kobo's implementation (`asm/lunar-magic/actslike.asm`) gives the same actions, at the
  same points, with the same `Y`, `$1693`, and `$03`, in every probe scenario: the
  player's, sprites', the cape's spin on both sides, and a fireball in the block (34
  scenarios, 2026-09-26); GPS 1.4.4 inserts into it unchanged. Yoshi's berry check
  (3.70) is Kobo's too (below). `contact_probe stand` drops the player onto chosen
  tiles, in any level and with RAM held at chosen values, and reports whether they land
  and what `$1693` became, which needs no probe block.
- Yoshi and berries (3.70's `$02BA9E`, `$01A24C`, `$01F589`, `$02BAE8`), observed
  2026-09-27 and 2026-10-01 on vanilla saved by Lunar Magic, with acts-like settings
  written into its table, by `contact_probe berry` (a stunned baby Yoshi held on the
  tile, which calls the game's berry check `CODE_02B9FA` at its centre; any level, either
  layer) and `contact_probe tongue` (a level entered with Yoshi, `$0DC1` set, and Y
  pressed at a block of the tile), with each hook put back to the game's bytes in turn,
  and in Mesen 2 (`tools/oracle/berry_probe.lua`). 3.70's changes: "Yoshi's tongue and
  baby Yoshi's mouth can now interact with layer 2 and work on blocks in vertical levels
  (though Yoshi's mouth has not been touched)".
  - `$02BA9E`, the tile's high byte: the check follows the acts-like chain. A tile
    acting like `$45`-`$47`, directly or through another tile, is eaten as that berry
    (`$18D6` its type, a mushroom sprite `74` made, the tile generated as `$49`); any
    other leaves `$1693` the low byte of the tile it resolves to (`$30` for `$130`). The
    game checks the tile number itself.
  - `$01A24C` and `$01F589`, the baby Yoshi's and the tongue's calls: the check reads a
    vertical level as one (the game reads every level's tiles as a horizontal level's),
    and where the level has layer 2 interaction (`$5B` bit 7) layer 2 too, at the point
    moved by layer 2's offset from layer 1 (`$26`, `$28`): layer 1 first, and layer 2
    only if layer 1 has no berry, so one berry a call. With bit 7 clear a berry on layer
    2 stays. `$1933` is 0 after every call, where the game leaves what the player's code
    left.
  - `$02BAE8`, the mushroom's `JSL InitSpriteTables`: a berry on layer 2 makes its
    mushroom where the berry is on layer 1 (its position less `$26`, `$28`); without the
    hook it is at the berry's layer 2 coordinates. The hook also sets `$1B7D` (an
    overworld variable) to 1 until the berry is swallowed; nothing observed reads it,
    and no other byte of work RAM changes with it.
  - Yoshi walking into a berry (`CODE_02D1AD`, the mouth the changes leave alone) is the
    game's: a page 0 berry in his path is eaten that way, by the tile number, before the
    tongue gets to it, which is why the tongue seemed to eat custom berries earlier and
    two at a time in the first probes. With that check's `CMP #$45` made to fail, the
    tongue on `$45` and on a tile acting like it leaves the same work RAM, frame by
    frame.
  - With the layer 2 point outside the level (above it, as level `D4`'s layer 2, `$C0`
    over layer 1, puts it for a sprite near the top; or to the left), a Lunar
    Magic-saved ROM goes wrong: in a vertical level, with the player's X at `$D0` or more
    and a stunned baby Yoshi anywhere, the game hangs (`$13` stops, in Mesen 2 as in
    Kobo's machine), and in level `D4` empty sprite slots' tables and the OAM change, and
    Kobo's machine stops where Mesen 2 has not in 90 frames. Kobo's code bounds both
    points as the game's lookups do.
  - The tongue's slot (`$06F9F0`): a `JSL` to a logging routine there
    (`tools/lunar-magic/block-probe/tongue-slot.asm`), or at every 16 bytes of
    `$06F8E0`-`$06F9F0` (all `$FF` after a fresh install), is called in no scenario, for
    tiles below or past `$200`. How Lunar Magic enables the slot is not known, and no
    tool in the toolchain writes it.
- Kobo's (`actslike.asm`, since 2026-10-01) takes the four sites with `JSL`s to its own
  code. Its check runs the game's lookup for a horizontal layer (`CODE_02BA48`, which
  `exlevel.asm` points at the level's tables) or the one the game has for a vertical
  layer and never calls (from `$02B9FE`), with `$0F` choosing layer 2's tables, for layer
  1 and then layer 2; the tile goes through the same chain as the player's, but for the
  Mode 7 boss battles. It does not write `$1B7D` or call the tongue's slot. Lunar Magic's
  ROM and the same with Kobo's bank `$06` code swapped in give the same berry, tile,
  `$1693`, `$1933`, and sprites in every scenario tried (levels `105`, `F7`, `E7`, `D4`,
  and `1A`, both layers, a berry on each, tiles on page 0 and acting like berries or
  cement, the tongue in level `105`), and Kobo's neither hangs nor crashes where Lunar
  Magic's does. A save keeps Kobo's four hooks, as the gate is set
  (`tests/lunar_magic_save.rs`); `tests/install.rs` checks every layout. The three
  differences (the bounded points, `$1B7D`, the tongue's slot) are kept (maintainer,
  2026-10-03) until a hack or tool is found that relies on `$1B7D` or the slot, or a
  newer Lunar Magic fixes the hang (then check Kobo's still matches it).
- With a hack's own tables: Kaizo Kindergarten, its levels, Map16, graphics, palette,
  and ExAnimation transferred into a Lunar Magic-saved vanilla ROM with Lunar Magic's
  command line, and the same ROM with Kobo's bank `$06` code swapped in and the table
  pointers kept (`swap bank06` (examples/swap.rs)), render all 512 levels the same and
  leave the same RAM after every load but `$0B`, a direct-page byte; no picture and no
  other RAM differs (2026-09-26).

### Placed objects

Lunar Magic's objects `22`, `23`, `27`, and `29` (formats in the smwspeedruns level data
format page) are drawn by code of its own, which each object set's dispatch (the
`ExecutePtrLong` table ten bytes into `OBJTS*`, entry `n - 1` for object `n`) reaches:
a save points those four entries, in all five object sets, at code in bank `$0D`'s free
space (`$0DF08A`-`$0DFF66`). The settings objects (`24`-`26`, `28`) and `2D` are taken
before the dispatch in a Lunar Magic ROM (inferred from its hook in `LoadLevelData`,
`$0586F7`, which comes first).
Lunar Magic's check for the piece is on bytes at `$0DFF50`-`$0DFF66` (bisected; zeros
there do not count), which Kobo does not match, so its first save puts its code in place
of Kobo's.

Observed (examples/lm_objects.rs: every form at several sizes, on horizontal level `105`
and vertical level `1CE`, imported by Lunar Magic into vanilla, and the grid its load
leaves):

- A size is the stored value plus one. `22`/`23` draw one tile of page 0 or 1 (the
  object number's low bit); `29` is `27` with `$4000` added to the base tile.
- A selection is Map16 tiles laid out 16 to a row: the tile at selection column `x`, row
  `y` is the base plus `y * 16`, with `x` added to the low byte alone (a block from `$2FE`
  four wide is `2FE 2FF 200 201`). A rectangle larger than its selection repeats it; a
  smaller one shows its top left.
- Cells are placed from the object's position right and down through the game's own
  steps: past row 26 of a horizontal level a column goes on at the next screen's top
  (the vanilla `AdvanceDownOneTile`). In a vertical level a step right past column 15
  goes into the screen's right half, `$100` bytes on, and a step down past row 15 into the
  next screen, `$200` bytes on, where the game's steps go `$1B0` and `$100` on.
- Conditional objects (help file, "Conditional Direct Map16"): flag `C` is bit `C % 8` of
  `$7FC060 + C / 8`. Without `A`, nothing is drawn unless the flag is set; with `A`, the
  tiles are drawn, `$100` added to each when it is set.
- `26` (music bypass): `$0DDA` = the third byte less one; the second byte's low nibble is
  ignored. `28` (time limit, `N10-TTTT 1000OOOO R---HHHH`: tens, ones, hundreds): the
  load sets the timer (`$0F31`-`$0F33`, hundreds first) to the three digits, each 0 to
  `F` as they stand, with `R` on every entry and without it only on an entry from the
  overworld (sublevel count `$141A` 0), as the header's setting is applied, which it
  replaces; the status bar's copy (`$0F25`-`$0F27`) follows from the game's own code,
  leading zeros blank. Bits 6-4 of the third byte are ignored, and `000` without `R`
  sets `000` too (written into the level data, since Lunar Magic's import drops such an
  object, as its help says the bypass cannot set 0 without the force option). Nothing
  else in work RAM changes (`lm_objects timer`, `ramdiff.py`, 2026-09-27). The format
  page's layout for `28` gives the wrong object number: its second byte is `1000AAAA`.

Kobo's code (`asm/lunar-magic/objects.asm`): the dispatch entries for `22`, `23`, `27`,
`29`, `26`, `28`, and `2D` point at stubs at `$0DFF70`, past Lunar Magic's area and free in every
corpus ROM, which call Kobo's code and return with `RTS`; tiles go through the game's
steps, reached through gates there too, so a tall level's own steps are followed, with
the vertical level steps above done by Kobo's code. It gives the same grid as Lunar
Magic's for every case, with the game's steps as Lunar Magic's install patches them and
as vanilla has them, and for all 512 levels of Kaizo Kindergarten's content (RAM after
load differs at `$5A` in two levels, the last object's number); Kaizo Kindergarten
imported and built by Kobo gives every level's grid as the hack has it (2026-09-26).

### Kobo's set against Lunar Magic's, after a save

The first acceptance check: Kobo's install, saved by Lunar Magic (which, as found later,
installed every piece outside bank `$06` itself, so this checks bank `$06` alone), against
vanilla saved by Lunar Magic, both with level `105` re-imported from its own export;
`render_hashes` and `ramdiff.py --summary` over all 512 levels. With `map16.asm` and `actslike.asm` (2026-09-26): every picture and every level's
data the same; RAM after load differs at `$0B` in 492 levels, at `$1693` in the 18 boss arenas, and
in level `105`'s data pointers (each save put the level elsewhere). Re-measured
2026-09-27, after the boss battles' pass-through below, with `swap bank06` on vanilla saved
by Lunar Magic (level `105` re-imported): RAM after load differs only at `$0B`, in the
same 492 levels. Found on the way:

- A fresh install's page table pointers, before any page has data, are bank `$00`:
  `$00F000` for pages 2-`F` and for page 2 per tileset, `$008000` for `10`-`1F`, `30`-`3F`,
  `50`-`5F`, and `70`-`7F`, `$000000` for `20`-`2F`, `40`-`4F`, and `60`-`6F`. Kobo writes
  the same, so Lunar Magic finds no tables where there are none.
- For a tile on pages `40` and up with no table (`$06F63A` = `$FF8000`), a Lunar
  Magic-saved ROM gives the tile what lies where that documented pointer, plus `$8000`,
  wraps to (inferred from the pointer's format): tile `$40EC` came out solid and `$FFEC`
  not. The game reads such high bytes in the boss
  arenas. Kobo's chain treats a tile past its tables as cement instead; in the boss
  battles, where the game reads them, both chains pass the tile through untouched (below),
  so this leaves no trace in RAM.

### Taller levels (3.00, "ExLevel")

Vanilla finds a horizontal level's screens through fixed tables: `LoadBlkPtrs` (`$00BEA8`)
points per level mode at 3-byte per-screen pointer tables (`Ptrs00BDA8`, `Ptrs00BDE8`,
`Ptrs00BE28`, `Ptrs00BE68`: layer 1 and 2, low and high byte planes), and block
lookups add the split screen offsets `DATA_00BA60`/`BA70` (low) and `BA9C`/`BAAC` (high).
All assume 27 rows (`$1B0` bytes a screen).

The piece's sites, found as the bytes a save rewrites on a Kobo install without the
`JSL` at `$05DA8A` and leaves alone with it (`examples/swap.rs exlevel` lists
them):

- The 9 ranges `$00BDA8`-`$00BEA7`: in all four tables the entries for modes `00`, `01`,
  `02`, `0C`, `0E`, `0F`, `11`, `1E`, `1F` (those that lead to the 27-row horizontal
  tables) point at RAM instead: `$0BF6`, `$0C26`, `$0C56`, `$0C86` (observed; data).
- Lookups of `DATA_00BA60`/`BA70`/`BA9C`/`BAAC` have their operands changed at
  `$00F492`, `$00F49A`, `$00F50D`, `$00F515` (Mario), `$019500`-`$01951B` (sprites),
  `$01D97B`/`$01D981`, `$0292F9`-`$029312`, `$0295EC`-`$029605` (cape), `$02A6BA`-
  `$02A6D3` (fireballs), `$02BA71`-`$02BA8A` (Yoshi), `$02D18C`-`$02D1A5`; the bank stays
  `$00`, consistent with the RAM tables at `$0CB6`/`$0CD6`.
- Height bounds: `CMP #$01B0` at `$00F478`, `$00F4F3` (player block collision),
  `$0194D6` (sprites); `ADC #$01B0` at `$03D793`; the Y high byte checks (`SBC #$00 : CMP
  #$02` and the like) at `$0292D7`, `$0295BE`, `$02BA4E`, `$02D158`; `GenerateTile`'s
  `CPY #$0200` (`$00BEE8`-`$00BEEE`); the high byte of `AND #$01F0` (Y to row) at
  `$00C07B`, `$00C0C8`, `$00C1B3`, `$00C3D5`, `$058A18`, `$058AF4`, `$058BE6`, `$058CD9`;
  the branches after the on-screen checks in `CODE_00C0FB` (`$00C116`, `$00C122`); the
  VRAM address build in `GenerateTile` (`$00BF35`-`$00BF48`, `$00BF81`-`$00BF9A`).
- The object loader's screen stepping: `$0DA963`-`$0DA973` and `CODE_0DA9D6`/`CODE_0DA9EF`
  (`$0DA9D6`-`$0DAA04`: `$1B0` added to or taken from `$6B`/`$6E`).
- The level's setup: `JSL`s at `$05D9A1` (`LDA $5B : AND #$01`, after the primary header
  load), `$05DA8A` (the castle "No Yoshi" intro's `LDA [$CE] : AND #$3F`, its check),
  `$05DB5F` (Choc Island 2's rooms' `LDA [$CE] : AND #$7F`, the same target as `$05DA8A`),
  `$0C9436` (the credits' `STA $65 : SEP #$30`); the operand of `AND #$3F` at `$05D8FB`
  (the sprite memory setting).
- Camera and shake: a `JML` at `$00F70D` (`LDA #$00C0 : JSR CODE_00F7F4`, the lowest camera
  position in a horizontal level); `GrndShakeDispYLo`/`Hi` (`$00A1CE`-`$00A1D5`) and the
  code at `$00A2AF`-`$00A2D4` that applies them to `$1C` and `$1888`.
- A `JML` at `$02950B` (`STZ $0F : JSR CODE_029540`, the cape's block checks): no effect
  found (below).
- Elsewhere, and not part of this piece: the extended objects (`$0DA10F` table: `01`, the
  screen jump, to `$0DE1D0`, `03` to `$0DE1E0`, `02` to `$0DE1B0`; `$0586A1`-`$0586B1`,
  each object's Map16 pointers; `$0DA9BB`-`$0DA9C4`; the restorable hook at `$0583C7`),
  which a save installs even with the `JSL` at `$05DA8A`; sprites below the level and the
  goal tape, in the sprite loader's group ("Sprites"); `Layer1Map16DMAData` (`$008A3E`,
  `$008A4C`, `$2C` to `$40` bytes), the VRAM patch's.

The size table: 512 bytes, one per level, `$240` bytes before the code the `JSL` at
`$05DA8A` calls (3.30 and 3.70; the community write-up's `read3(read3($05D9A2)+70)` does
not hold for either). Lunar Magic's editor, save, and MWL export find it so: a Kobo build
with the table at `$240` before the `$05DA8A` code exports it and keeps it through a save,
with Kobo's code at both hooks, and one with the table elsewhere exports other bytes as
sizes (2026-09-28). How `$240` was first found was not recorded, so it was found again
on 2026-10-03 by a byte diff: Luminescent's level `156` (298 rows) imported by Lunar Magic
3.70 into vanilla as level `105` and, in another copy, as `106`. Of the 13 places where a
byte moves by one between the two copies, the one in the bank of the `$05DA8A` hook's
target is `$10E4C3 + level`, `$240` before that target (`$10E703`), and holds `$1A`, the
size of 298 rows and 3 screens in the documented table (`level::size::SIZES`); the
others are other per-level tables. Without the `JSL` at `$05DA8A`, a save installs Lunar Magic's piece over
every site above and a fresh table. The byte is `TB0MMMMM`; an export rewrites `T` from
the level (set with layer 2 objects, modes `01`, `02`, `08`, and some levels with layer 3
tides), and every tall level of the corpus has in the ROM the `T` its export says.

Observed (`examples/exlevel_probe.rs`: `sizes` writes each size into a level and diffs the
RAM its entrance leaves; `call` runs one routine with chosen RAM; `compare` plays a level
in two ROMs frame by frame; on vanilla saved by Lunar Magic and on hacks moved into such
a ROM with `tools/lunar-magic/transfer`), 2026-09-28:

- What `$05D9A1`'s code sets up, for a horizontal layer 1 (`$5B` bit 0 clear): `$0BF5` the
  size byte, with the sprite header's new sprite system bit (`$20`) in bit 5; `$13D7` the
  height, `$1936` the height less `$10`; `$0BF6` 32 3-byte pointers into `$7EC800`,
  `$0C56` the same into `$7FC800`, `$0CB6` and `$0CD6` their low and high bytes. Entries
  0-15 are screens 0-15; entries 16-31 are layer 2's, from screen 16, or with `T` from the
  screen whose start is nearest the middle of the tile planes (`$1C00` bytes in), the
  lower of two as near, at most 16 (so half way, rounded, for most sizes, 3 of size
  `16`'s 7, 0 of size `1C`'s 1). A screen at `$C800 + n * height` is `$7E0000`/`$7F0000`
  once past `$FFFF`. `$0BF4` is the level's `tTT` (as entrance.asm has it); `$1692` is
  the sprite memory setting without bit 5.
- A vertical level takes no size: `$0BF5` the new sprite system bit only, `$13D7` =
  `$0100`, `$1936` = `$01A0`, the tables with the game's `$1B0` a screen, `$0CB6`-`$0CF5`
  zero, `$0BF4` zero. In a vertical level the object loader's step right goes `$100` on,
  into the screen's right half.
- Bounds, by `call` with chosen positions against every size, compared by the memory
  each call leaves: a block at or below the level's height is out for the player,
  sprites, bounce blocks, the cape, Yoshi's berry check and `CODE_02D149` (the game had
  `Y < $200` for the last four), and for a tile generated in play in a horizontal layer;
  the memory effects of every case match. (`call` also printed the registers a routine
  returned until 2026-10-02; what they showed is not used: clean-room-audit.md.)
- A point of the player's out of the level (below its height, past its screens, outside
  a vertical level's two columns; `CODE_00F4A0` and `CODE_00F4E7`, where every
  out-of-bounds branch of the player's block lookups goes) also leaves tile `$25` in
  `$1693`, where the game leaves the last tile in the level there. Lunar Magic's
  `$00F47E`, `$00F488`, `$00F4A0`-`$00F4A2`, and `$00F4EB` (in every 3.x corpus ROM, in
  none of the 2.x) are its piece for it; the cases above missed it, since a swap of
  Kobo's code into a Lunar Magic-saved ROM kept them. Found 2026-10-04 in the corpus play
  comparison (the player falling below a level: `$1693` `$25` in the 3.70 transfer, `$00`
  in Kobo's build), then by calls of `CODE_00F465` at points in and out of horizontal,
  vertical, and layer 2 levels at the game's height and at 896 rows, 168 and 164 cases:
  the same memory with Kobo's `exlevel.asm` (a `JML` at `CODE_00F4E7` and a `BRA` to it
  at `CODE_00F4A0`) as with Lunar Magic's, but for four points of layer 2 interaction
  forced on in a 896-row level without layer 2 objects, where the two read different
  tiles (not followed: no level of the corpus has it). `tests/install.rs`
  (`a_point_out_of_the_level_touches_air`).
- The camera's lowest position: the height less `$F0`, or with `B` less `$E1`, low enough
  to show the last row whole (the screen's first line shows the line below the
  camera's). The entrance places layer 1 a pixel higher with `B` when it sets it
  relative to the player, and the background at the camera's bottom `$0F` lower (`$10`
  with no vertical scrolling). The background's position fits layer 1's position and the
  bottom each scaled by the vertical scroll rate, unsigned, and then subtracted, where the
  game scales their difference: the two agree but for a size whose bottom is above 0
  (`1F`, `$8000` apart; with `B` the bottom keeps its sign). Fitted to the positions
  observed for every size, rate, and `B`.
- Screen shake: offsets -2, 0, 2, 0 as the game's, but -1 for the first with the camera
  on the level's first row (`$1C` 0 to `$0F`, every size), and -4, -2, 0, -2 with `B`
  elsewhere.
- The castle intro, Choc Island 2's rooms, and the credits load the game's own 27-row
  data: the screen pointers for size 0 (observed in Mesen, 2026-10-01, with
  `tools/oracle/dump_levels.lua` and a probe logging `$0BF6`-`$0CF5`: castle `101` and
  Choc Island 2's target `024` at size `01`, and the credits' enemy list after `024`).
  The pointer tables are the same as Kobo's; `$0CB6`/`$0CD6`, their low and high bytes,
  Lunar Magic leaves at the level's own size in the intro and the rooms, where Kobo's
  gives them size 0's to match the pointers (review.md). The credits' tile grid is the
  same after a taller level as after a level of the game's height, under either code.
  Choc Island 2's rooms take their banks from the level the screen exit led to, which
  the game keeps in bank `$06`: a build that writes `0CD`-`0CF` elsewhere crashed the
  game in the rooms (found here; `choc-island.asm` since, as the hook table has it,
  installed by any build that writes them, kept so on review, 2026-10-04).
- `$02950B`'s rewrite: the cape's block checks with every `$5B`, size, and position tried
  leave the same memory as the game's code; Kobo leaves the game's.
- `$0BE7` (`$40` or `$00`) comes from another piece: bit 6 is set by the entrance code
  and cleared by the code after the player's collision with layer 2 ("The sites a save
  keeps with the marker").

Kobo's (`asm/lunar-magic/exlevel.asm`, in every build that uses Lunar Magic's layout)
takes every site above but `$02950B` and the VRAM address and view checks, which Kobo's
VRAM patch answers for its 64x32 tilemaps (`vram.asm`'s `$00BF36`). Its screen jumps
with a vertical part (`$0DA10F` entries `01` and `03` to stubs at `$0DFFC0`, and each
object's Map16 pointers at `$0586A1`) keep the part in `$0BDD` while the objects load and
clear it after. Its size table is where Lunar Magic's layout has it, with the heights of
the 32 sizes in the `$40` bytes after it. `entrance.asm` takes the level's height from
`$13D7` and `B` from `$0BF5` for the layers' positions and for vertical scrolling at
will, and `exits.asm` now leaves an exit's entrance and flags in `$0BDA`-`$0BDC`, since
the tables fill `$0BF6` before the entrance code reads them. Checked against Lunar
Magic's (2026-09-28): with `swap exlevel` on vanilla saved by Lunar Magic and on
Luminescent, ValuableAndBeautiful, Akogare2, SMW_2022-4-9, SMW_2021-5-1, Sakaya
Sanctuary, and Advanced Shells moved into it (Kobo's sprite loader and VRAM patch
swapped into both sides), every level leaves the same RAM after its load and draws the
same picture, and the tall levels play the same along ten paths (carried, running and
jumping, spinning the cape, throwing fireballs, with vertical scrolling at will),
through the ground shaking and the camera at every vertical scroll setting; every
value of every entrance byte and of the size byte, with and without `B` and a relative
camera, leaves the same RAM at the end of the entrance code as Lunar Magic's but for
Lunar Magic's added layer 2 scroll rates (below), and in one corner, size `1E` with `B`
at the slowest rate, which placed the background a pixel off and no longer does
(2026-10-04: every settings byte over size `1E` with `B` and a relative camera,
`entry_probe compare` with `KOBO_ENTRY_BASE`).
Luminescent's and ValuableAndBeautiful's levels imported and built by Kobo resolve the
same grid as the hacks moved into Lunar Magic 3.70 (a few cells differ from the hacks'
own ROMs, saved by older versions, which draw some of Lunar Magic's objects
differently), and a Lunar Magic save of the build keeps every level and size.

A level's header may count more screens than its size leaves room for (layer 1's
screens before layer 2's in a level with layer 2 objects, layer 2 the rest): QLDC 2021
`22_FerpyMcFrosting` has seven at 15 screens where 37 rows leave 12, and with Kobo's
code swapped in (`swap exlevel`) they load and play as under Lunar Magic's along
three camera paths (2026-10-02). Objects on screens past that room are another matter:
the screen pointers there are 0, so they would be written over low work RAM. Builds
refuse objects past the room and build a screen count past it. No level of the corpus
has any but in locked ROMs, whose added objects run past the level's last screen
([lunar-magic.md](lunar-magic.md)). The 74 levels of three SA-1 hacks once counted here,
and a crash of Heraga's level `1E3` under Kobo's code, came from a misread size table
(below): under their own code every one of those levels has the game's 27 rows.

On an SA-1 ROM the `JSL` at `$05DA8A` may name a bank from `$80` up, which is not a mirror
of `$00`-`$3F` there but the ROM's third and fourth megabytes (Super MMC's reset): QLDC
2022 `26_Heraga` (3 MiB, Lunar Magic 3.30) has `JSL $80DEF1`, its table in the third
megabyte. Taking `$80` as a mirror, as Kobo did until 2026-10-02, read 512 bytes of
SA-1 Pack's bank `$00` as sizes; writing a size into those bytes changes nothing under
the hack's code, and into the table the hook names, the level's height (`exlevel_probe
ram`). `level::size::table` reads the pointer through the ROM's mapping.

### Backgrounds and the level load's uploads

- `$058DB1`-`$058DBB` (`ADC #$01B0`, twice), `$058DCA` (`STY $0C` of the BG Map16 bank),
  `$058E12` (`CMP #$01B0`) in the background column upload change too, and the upload's
  stride and bank follow what the `$058DA4` hook leaves in `$05` and `$0C` (inferred from
  the offsets changed, and `bg_survey` below).
- `$0EFD00`-`$0EFD3F`: the `$058DA4` hook's code. `$0EFD50`-
  `$0EFD7F`: 16 3-byte pointers, 16 BG Map16 pages each; a fresh install sets the first
  to `$0D9100` (vanilla's) and the rest to `$000000` (observed; documented).
- The initial tilemap upload: in `CODE_0580BD` the three `JSL`s (`$0580BF`, `$0580C3`,
  `$0580C7`) are retargeted to Lunar Magic blocks (the VRAM patch, "Graphics" below), and
  `LDA $47` at `$0580D3` jumps to
  `$0580FB`, past the vertical-pipe re-pointing (smw.md; an address-only diff shows a
  3-byte change starting there in 43 of the 45 LoROM corpus ROMs, and at `$058A65` in all
  45, 2026-10-03; the targets are the jumps' at those hook sites); `$05879D`-`$0587A1` does the
  same in the scroll setup `CODE_05877E` (inferred). The NMI's `JSL UploadOneMap16Strip`
  at `$008209` is retargeted too. Observed: without the `$0580BF` retargets the load
  leaves the vanilla state in `$1BE6`-`$1DE7` (the layer VRAM buffers), `$0695`-`$06B6`
  (VRAM addresses and buffer pointers, inside `DynPaletteTable`), `$7F819F`, and nothing
  in `$7FBC00`/`$7FC300`.

Observed (examples/bg_survey.rs over five hacks and a Lunar Magic-saved Kaizo Kindergarten,
the hook's outputs as the ROM's code leaves them) and implemented by Kobo
(`asm/lunar-magic/background.asm`, `level.asm`):

- The `$058DA4` hook: `$0A`-`$0C` = pointer `n` of `$0EFD50`, where `n` is the flags' high
  nibble for Lunar Magic's own format with `F` (`C` and `F`) and 0 for any other background
  (flags `08`, `18`, `00`, and `C` alone: Kaizo Mario World 3's, flags `$32` and `$12`,
  read their tiles from pointer 0 under the hack's own code, and Lunar Magic's MWL export
  gives them table 0 with the nibble in each tile's high byte); `$05` = `$0200` with `F`,
  else `$01B0`. Kobo's builds write `C` only with `F`. A background's tile numbers count
  from its table's pointer. Kobo's routine at `$0EFD00`, the hook's target, keeps the
  caller's register sizes, as any routine at a fixed entry point Kobo's code does not
  alone call should.
- The `$05803B` hook: `$7FC00B` = the level's flags; `C` with `F` decodes the stream
  (2048 bytes: low bytes of both halves, then high bytes) to `$7EB900`; `C` alone and `V`
  decode 864 low bytes there, and every tile's high byte (`$7EBD00`) is the flags' high
  nibble; neither is layer 2 objects. Kobo's decodes through the game's own decoder (from
  `$058064`). The level number comes from `$010B`, which the `$05D8E2` hook sets before
  the background load runs (`$010B` = level, `$00FE` = level + 1; Y = level * 2 and A, X,
  and Y 16-bit as the instructions it covers leave them; Kobo's is at `$0EF550`, where
  Lunar Magic's layout has it).
- A save rewrites `$0EF510`-`$0EF54F` with its own code and frees the RATS block that the
  `JML` at `$0EF519` leads to (seen with Kobo's own block there). Kobo's `$05803B` jumps to
  `$0EF510` as Lunar Magic's does, and its entry there has its `JML` at `$0EF519` to a block
  holding only the background code, which Lunar Magic's replaces after a save.
- BG Map16 tables, like the foreground's, end at the last tile used: Kaizo
  Kindergarten's table 1 is `$950` bytes, page 0 and 42 tiles of page 1.
- Kaizo Kindergarten's content through Kobo's code draws all 512 levels as through Lunar
  Magic's; imported and built by Kobo, with its backgrounds and 16 BG Map16 pages, every
  level resolves the same grid, background tilemap, Map16, and BG Map16 as the hack
  (examples/tiles_diff.rs), before and after Lunar Magic saves the build (2026-09-26).

### Custom palettes

- A level's palette at `$0EF600` (docs/lunar-magic.md) is loaded by the `JSL CODE_05BE8A` at
  `$00A5BF`, in the level's setup just after the game's `LoadPalette`, retargeted to
  `$0EF570`: found by putting Lunar Magic's changes back to vanilla in halves until Kaizo
  Kindergarten's level `001` lost its colours. The palette goes over `$0701` (back area)
  and `$0703` (the 256 colours) before the game uploads them. The hook also clears
  `$00FE`-`$00FF` (the level number plus one, from the `$05D8E2` hook) wherever it runs,
  palette or not; boss arenas and a few special levels (`198`-`19B`, `1C7`, `1DE`, `1EB`,
  `1F6` in Kaizo Kindergarten) never reach it and keep the value.
- Kobo's (`asm/lunar-magic/palette.asm`, at the same site, its code elsewhere) gives the
  same pictures, VRAM, CGRAM, and RAM but for direct-page scratch on all 512 levels of
  Kaizo Kindergarten's content. A save kept every `$0EF600` pointer a build wrote.
- Levels with ExAnimation change colours across Lunar Magic's first save of a build
  without it; builds now carry it ("ExAnimation" below).
- The palette's fade at a level's end takes colours 8 to 15 of rows 0 to 7 too, but for
  layer 3's: `palette.asm` has it ("The sites a save keeps with the marker").

### Graphics: GFX files, ExGFX, per-level lists, and the VRAM patch

Found by importing an MWL with one graphics slot changed into copies of a Lunar
Magic-saved ROM and diffing (data only), following pointers to what changed, loading
levels whose lists name ExGFX files made for the purpose (every 16-byte unit of file `f`
starts `A5 5A f j`, `j` the unit's number) and finding where each unit lands in VRAM and
RAM, playing levels along set paths and recording which tilemap words each frame
changes (examples/gfx_probe.rs), and bisecting which bytes Lunar Magic checks
(scripts that print addresses only). `vanilla+LM` below is the vanilla ROM after one
save (level `105` re-imported); `+4bpp` is that after `-ExportGFX` and `-ImportGFX`;
`+ExGFX` is that after `-ImportExGFX`.

Three pieces, each with a check of its own, which Lunar Magic installs at different
times:

| Piece | Installed by | Sites | Counted as installed when |
|---|---|---|---|
| The VRAM patch | the first save (a restorable group) | `$0081E2`, `$008209`, `$0085D2`-`$0085DE`, `$00A5A2`, `$008072` and `$00BA56`, `$00F6E4`, `$0580A9`, `$0580BF`-`$0580CA`, `$0586F7`; its code in one `$30C0`-byte RATS block | a `JML` (`$5C`) at `$00A5A2`, whatever its target; a `JSL`, `JSR`, `JMP`, the game's own bytes, `$EA` or `$00` there do not count |
| 4bpp GFX files | `-ImportGFX` (the GUI's "use 4bpp" option, on by default) | `$0093F7`, `$0095E9`, `$00A82F`, `$00AA8C`-`$00AB3B` (`UploadGFXFile`'s conversion), `$00B89F`-`$00B8A9` (`CODE_00B888`, GFX32 and GFX33), `$03DDC8` (the Mode 7 bosses' file), `$048000`-`$0480D0` (the overworld's layer 1 tile pointers and loop), `$04F2B6`-`$04F3CD` (the switch palace blocks' RAM, moved clear of the larger buffer), code at `$0EFC00`-`$0EFCAB` and in a RATS block | `$00AAD8` = `$EA` (one byte of `STA GfxBppConvertBuffer,X` in the game) |
| ExGFX, per-level lists, and objects `24`/`25` | `-ImportExGFX` (or the bypass dialogs) | `JSL`s at `$009471` (the castle cutscenes' graphics), `$00A140` (the overworld's), `$049DFD`, `$0583B8` (the level header load), `$00AA6B` (`UploadGFXFile`'s decompression), `$00AA47`-`$00AA54` in `UploadSpriteGFX`'s FG/BG loop; objects `24` and `25`'s entries in every object set's dispatch (`$0DA4BE`, `$0DC203`, `$0DCE03`, `$0DDA03`, `$0DE903`) to code at `$0DF0E0`; tables and code at `$0FF15C`-`$0FFECB` | reading ExGFX (`-ExportExGFX`): `$00AA47` = `$EA` (the `BEQ` that skips a slot already loaded), a `JSL` (`$22`) at `$0583B8`, and the pointer at `$0FF873`; reading a level's list (`-ExportLevel`): the `JSL` at `$0583B8`; a save keeping the lists as they are: `"LM"` (`4C 4D`) at `$0FF15C` |

- With `$00A5A2` not a `JML`, a save installs the VRAM patch anew: a new copy of its
  code in a fresh block, the other eight sites pointed at it, the old block left where it
  was. It keeps the ExGFX pointers, the lists, and the GFX files.
- With the three ExGFX bytes, the `JSL` at `$0583B8`, the marker, and the pointers, and
  none of Lunar Magic's ExGFX code (a Kobo build's case), a save changes nothing but its
  usual `$0FEFC9`-`$0FF044`, keeps every list and pointer, and exports the level's list as
  imported. Without the marker it installs its ExGFX code over the bank `$0F` area and
  the hooks above, and rewrites one word of every level's list (`+$10`, the SP4 slot, from
  `$FFFF`); without `$00AA47` it warns that the ROM has no ExGFX list and uses the
  standard files on import, and keeps everything; without the `JSL` at `$0583B8` it keeps
  everything but exports the level's list as if there were none.
- The pointers at `$00B88B` and `$00B8D8` (the operands of `LDY #GFX33` and `LDA #GFX32`)
  are where Lunar Magic reads and writes those two files' addresses; a 4bpp insert moves
  both files.

#### The VRAM layout

After a level load (vanilla+LM; the registers the game writes):

| VRAM (words) | Holds |
|---|---|
| `$0000`, `$0800`, `$1000`, `$1800` | FG1, FG2, BG1, FG3 (as vanilla), 4bpp |
| `$2000`, `$2800` | BG2, BG3: the two extra slots (vanilla has layer 1's tilemap here) |
| `$3000`-`$37FF` | layer 1's tilemap, 64x32 (`BG1SC` = `$31`) |
| `$3800`-`$3FFF` | layer 2's tilemap, 64x32 (`BG2SC` = `$39`) |
| `$4000`, `$4400`, `$4800`, `$4C00` | LG1-LG4, layer 3's 2bpp files (as vanilla) |
| `$5000`-`$5FFF` | layer 3's tilemap (`BG3SC` = `$53`, as vanilla) |
| `$6000`, `$6800`, `$7000`, `$7800` | SP1-SP4 (as vanilla, `OBSEL` = `$03`) |

Without a list (vanilla+LM), BG2 and BG3 are never written. A level's tile numbers
reach them through the tiles' bit 9, as the game's own reach FG1-FG3 and BG1.

#### Tilemap streaming

A 64x32 tilemap holds 32 Map16 columns and 16 rows, wrapping both ways (column `c` at
tile column `2c mod 64`, row `r` at tile row `2r mod 32`). What the patch keeps there,
found by comparing the tilemaps frame by frame with Kobo's version of it swapped in
(`gfx_probe play`, `scroll`, `loads`; examples/swap.rs vram) on every vanilla
level along several camera paths. A layer's camera cell is `cx` = X / 16 and `cy` =
(Y + 1) / 16: the screen's first line shows the line below the layer's vertical position,
and the rows follow what is on screen.

- At load, a layer that scrolls horizontally (the level modes the game's own column
  builders serve) holds Map16 columns `cx`..`cx+16` of rows `cy-1`..`cy+14`; a vertical
  level's layers and a background hold all 32 columns of those rows. At the level's top
  (`cy` = 0) row -1, which no camera shows, holds other words under Lunar Magic's patch
  (seen in VRAM); Kobo's leaves that row as it was.
- Each frame, for each layer, when the camera has moved (its position against the
  frame before's, in pixels) a column goes up, `cx+16` moving right and `cx` moving
  left, rows `cy`..`cy+14`; and a row, `cy+14` moving down and `cy` moving up, columns
  `cx`..`cx+16` (all 32 for a layer whose width fits). Each is left out when it is the
  column or row that layer last had built, so a camera that keeps moving one way
  builds one as it crosses into a cell, and one that turns within a cell builds the
  one on its other edge. The frame that crosses uploads it, whatever the game mode is
  once the frame ends (a frame in which the player dies still uploads).
- A camera that skips columns (as when a player is put somewhere else in the level)
  gets the one column at its edge; the columns skipped are not built, and show what was
  there before until they leave the view and come into it again at an edge.
- The first frame after the load builds the right column `cx+16` and the bottom row
  `cy+14`, as if the camera had come right and down, though it has not moved (its
  position is the load's all along); a later frame where it has not moved builds
  nothing. It shows where the column is past the level's end, which the load leaves
  unwritten (levels `0D0`, `0D1`, `0F5`, `0F6`, entered on their last screen), and
  where that frame's camera also skipped columns.
- Layer 1's position is the one shown: with the ground shaking (`$1887`), which the
  game adds to `$1C` after the frame's builds and the NMI adds to the scroll register
  (`$1888`), the rows follow the shaken position, so a shake that takes the camera
  across a row boundary builds the row it reveals and, as it settles, the one at the
  other edge. Kobo's patch works out the frame's shake before the shake code runs,
  as `exlevel.asm`'s shake will apply it (seen in level `0E8`, 2026-10-03).
- Found 2026-10-03 by putting the player at x 0 and at points either side of the
  entrance, on the first frame and later ones (`exlevel_probe cells ... from=N`, level
  `1E2`), and by watching when each word of a column (`0ED`) and of a row (`105`) is
  written along paths that turn (`KOBO_VRAM_WATCH`, `gfx_probe watch`, whose `scroll`
  dumps a frame's VRAM with `KOBO_PROBE_DUMP`), and on every vanilla level along the
  paths below and three that jump: Kobo's patch writes them on the same frames since.
- Rows of a 27-row level past its last row are left as they were.
- The vertical pipes (`133`-`13A`) take the definitions for their screen (every 16
  columns, or rows in a vertical level; `MAP16AppTable`), and the game's Map16 pointers
  for them (`$1224`-`$1233`) are left at the set for the last column (row, in a vertical
  level) the patch built for layer 1, which tile changes then read.
- A tile changed in play shows in these tilemaps when it is in the rows the frame's
  builds keep, `cy`..`cy+14`, and on a layer that scrolls horizontally in the game's
  own window of columns, `X/16-8`..`X/16+23` (the 32 the tilemap holds, of which only
  `cx`..`cx+16` are kept current). The game queues the stripe for the game's window
  (rows `Y/16-8`..`Y/16+23` too) at the game's own address (layer 1 at `$2000`); Lunar
  Magic's patch puts the ones in those rows at their place at `$3000` and leaves the
  rest out, not before another frame of the game loop has run (so, it seems, at its
  game loop hook, `$008072`). Found 2026-10-03 by
  calling the game's `GenerateTile` around the camera after frames of play in a
  horizontal, a vertical, and a layer 2 objects level, with the camera held at
  positions either side of a row, and reading the tile's place in VRAM after the next
  frame (`exlevel_probe call ... frames=N path=... vramw=... then=frame`). Kobo's does
  the same since 2026-10-08: `GenerateTile` queues the game's address for the game's
  window, and Kobo's code at the game loop hook (`$008072`: `JMP $BA56`; `$00BA56`:
  `JSR RunGameMode : JML`, Lunar Magic's 3.70 bytes there by a save's byte diff) moves
  each stripe for `$2000`-`$3FFF` to its place, the address's low 11 bits at `$3000`
  (layer 1) or `$3800` (layer 2), or takes it out of the buffer when its Map16 row (mod
  32, bit 11 being the row's bit 4 in the game's 64x64 tilemaps) is not one of
  `cy`..`cy+14`, in game modes `$05`, `$07`, `$13`, and `$14`, the ones
  `vram_optimize.asm` runs its move in (its hook sites and stated behaviour are evidence:
  clean-room.md, maintainer, 2026-10-08). Until then
  Kobo's queued its own address and decided from the camera when the tile was made,
  kept so on review (2026-10-04) until a hack's code read the stripe buffer: the Romhack
  Races baserom's `vram_optimize.asm` (kkevinm's) does, taking that hook over to move
  the stripes itself at the end of the game loop, so in a Kobo build it took Kobo's
  layer 1 addresses for layer 2's and every tile changed in play went to layer 2's
  tilemap (a block hit showed nothing on layer 1, and its new tile showed in the
  background, scrolling with it). With it, a build of the template now leaves layer 1's
  and 2's tilemaps as the baserom's own Lunar Magic build does, frame by frame through
  an ON/OFF block's bounce and a custom block's change (Mesen 2, level `13B`,
  2026-10-08; testing-log.md).
- The load leaves the patch's own state in `$0695`-`$06BE`, `$06DD`, `$7F8183`-`$7F819F`,
  the vanilla layer 1 and 2 buffers `$1BE6`-`$1DE7`, and for a background
  `$7FBC00`-`$7FBF5F` and `$7FC300`-`$7FC65F` (per cell, a 16-bit address in the BG Map16
  table, for 27-row backgrounds; other values for 32-row ones). With them laid out
  otherwise (Kobo's layout) no picture and no other RAM changed. Kobo's keeps its own state in the same places, in its own
  layout (the background as 16-bit tile numbers), so they differ after a load.
- The patch also changes how the NMI uploads the player's tiles: vanilla+LM leaves some
  of the player's tile words in VRAM (`$6061`-`$6187`) different from vanilla's along the
  same path, words that come from RAM the patch's state overlaps. Not looked into.

Kobo's version (`asm/lunar-magic/vram.asm`) hooks the level load's tilemap upload
(`$0580BF`'s three `JSL`s, `$0580D3`), the scroll setup (`$05879D`), the frame's builds
(`$0586F7`), the NMI's upload (`$008209`), the camera (`$00F6E4`, which PIXI checks), and
`GenerateTile`'s address (`$00BF36`), the NMI's lag path (`$0081E2`), and the game loop
(`$008072` and `$00BA56`, where a tile changed in play goes to its place), and leaves
`$00A5A2`, `$0085D2`, and `$0580A9` as the game has them. Lunar Magic's first save of a build puts
its own patch over all of them (its taller levels piece over `$0580D3`, `$05879D`, and
`$00BF36`). Against Lunar Magic's on every vanilla level (2026-09-28): the same tilemaps
after every load but for row -1 above, and no visible cell different for more than a
frame along seven camera paths (slow and fast, both ways, diagonal), nor for a coin
collected in a horizontal and a vertical level; since 2026-10-04 also held still, along
three paths that jump, and with the ground shaking (the rules above found then). Kaizo Kindergarten's content moved into that ROM by Lunar Magic's command line
(tools/lunar-magic/transfer) gives the same tilemaps after every
load with Kobo's patch swapped in, but for row -1. Lunar Magic's own graphics loader
then leaves BG2 and BG3 unloaded: it loads them only with its VRAM patch in place, so
the comparison of pictures swaps Kobo's loader in too.

Lag frames and stripe images (2026-10-02, `expand::play_game_loop_lagging`, which runs an
NMI before the game loop has ended the frame; `exlevel_probe compare ... lag=N`):

- On a frame that lags in a level that is not special, Lunar Magic's NMI (`$0081E2`)
  leaves layer 1's and layer 2's scroll registers as the last frame set them, at the
  camera its tilemaps were built for, where the game's writes the camera as the
  unfinished frame has it. Kobo's (`vram.asm`) does the same: with every other frame
  lagging, the scroll registers and tilemaps at every vertical blank are Lunar Magic's on
  levels `105`, `1CF`, `0F7`, `0E7`, `002`, `0C2`, `1E2` and the boss arenas `1C7`, `1F6`,
  `0CC` but for row -1 (`tests/lunar_magic_save.rs`, LoROM and SA-1). In the arenas
  the patch changes nothing seen.
- The game loop hook (`$008072`) changes the player's tile words on a level's frames
  (within `$6040`-`$6187` and `$67F0`), from scratch RAM a Lunar Magic ROM leaves
  otherwise, and Lunar Magic's own RAM; a Kobo build leaves them as the vanilla ROM does
  (review.md).
- A message box (`$1426`) and the level's end into a cutscene (`$1493` with `$13C6`)
  upload stripe images; with Kobo's patch and the game's upload (`$0085D2`) VRAM is the
  same as with Lunar Magic's, lagging or not, but for the player's tile words. Lunar
  Magic's patch with the game's upload in its place draws the message box blank: the two
  belong together, which a save keeps (it installs the group whole).

#### Per-level graphics lists

- The 3-byte pointer at `$0FF7FF` leads to level `000`'s list, then one list of 16 words
  per level, in the MWL file's slot order (AN2, LT3, BG3, BG2, FG3, BG1, FG2, FG1, SP4,
  SP3, SP2, SP1, LG4, LG3, LG2, LG1). A word's low 12 bits are a file (`00`-`33` the
  game's, `80`-`FFF` ExGFX, `7F` none), its high nibble settings (the community's
  level format page): AN2's bit 15 (`G`) turns the list on ("Super GFX Bypass"), bit 14
  (`3`) layer 3's files, bit 13 (`T`) layer 3's tilemap; the high nibbles of LT3, BG3,
  SP1-SP3, and LG1-LG4 are layer 3 settings. Corpus lists often have `$F` in SP4's high
  nibble; Lunar Magic 3.70's save of a level clears its bit 12 (`$F020` becomes `$E020`,
  lunar-magic.md), and what bits 12-15 of SP4 do is not known. A list is `$FFFF` words for a level that has
  none. A list naming files the ROM does not have (vanilla, with ExGFX `80`) is not stored
  by an import into that ROM; a hack's list may still name one (nine corpus hacks, ExGFX
  `FF` in level `105` of several), and its slot then loads nothing, as with `7F`: VRAM
  after the load is the same either way (SMW_2021-5-8, 2026-10-02), and so it is with
  Kobo's loader, which builds such a list as it is (kept so on review, 2026-10-04: an
  import notes each, and `kobo build` warns of each, `build::warnings`).
- The block (`$6E00` bytes in Kaizo Kindergarten, Invictus, and `+ExGFX`) starts with
  the ExGFX `100`-`FFF` pointers (`$2D00` bytes), and the lists follow: `read3($0FF7FF)`
  is `read3($0FF873)` plus `$2D00`. `$0FF873` and `$0FF937` both hold the block's start.
- With `G` set, the level's slots are loaded in this order, each decompressed into the
  buffer at `$7EAD00` and uploaded whole: SP1-SP4, then FG1, FG2, BG1, FG3, BG2, BG3. A
  file of `7F` is not decompressed, and its slot gets whatever the buffer holds: a slot
  after one with a file gets that file again, SP1 gets what was there before. With the
  4bpp piece the files are uploaded as they are, `$1000` bytes each. LG1-LG4 go to
  `$4000`-`$4C00` only with `3` set. AN2 is decompressed into `$7EAD00` last (up to
  `$1A00` bytes; the animated tile frames read it there); with AN2 `7F` the buffer keeps
  BG3's file.
- The load leaves `$0101`-`$0108` (the files the game believes are loaded) at the
  tileset's own files, as vanilla does, whatever the list loaded.

#### ExGFX and GFX files

- ExGFX `80`-`FF`: 3-byte pointers at `$0FF600`, 128 of them, each to a RATS-tagged file
  compressed as the ROM's GFX are (LC_LZ2, or as `$0FFFEB` says). `100`-`FFF`: 3-byte
  pointers from `read3($0FF873)`, `(n - $100) * 3` in; `$FFFFFF` for a file not there.
  Both tables are Lunar Magic's own layout; Kaizo Kindergarten has no file past `FF`.
- GFX files `00`-`33` stay in the game's pointer tables (and `$00B88B`, `$00B8D8` for
  GFX32 and GFX33). With the 4bpp piece every file the game keeps as 3bpp is stored as
  4bpp instead, `$1000` bytes for 128 tiles (the help file's chart), in place where it
  fits and in RATS blocks where not. A hack's are often 4bpp (Kaizo Kindergarten: 47 of
  52).

#### Layer 3 in the lists, and the older lists

Found the same way, from ExGFX files made for the purpose in lists and in objects `24`
and `25` imported into copies of `+ExGFX` (x2-x7 in the scratch notes of 2026-09-28),
comparing VRAM and RAM after every load.

- `3` (AN2 bit 14) loads LG1-LG4 to `$4000`-`$4C00`, `$800` bytes each, after the other
  slots. Without it, once any level's list in the ROM has `3`, every level loads the
  game's own layer 3 files (`28`-`2B`) there again, since another level may have left
  its own. What tells Lunar Magic's loader that some list has `3` is set by a save:
  `3` written straight into a list of `+ExGFX` (no save) loaded no layer 3 files at all
  (2026-09-30). Kobo's loader looks through the lists instead.
- `T` (AN2 bit 13) loads LT3 as layer 3's tilemap, before layer 3's files. LT3's high
  nibble is `DDFF`: `FF` the size (0 and 3 `$2000` bytes, 1 `$1000`, 2 `$800`), `DD`
  where it goes: 0 under the status bar (`$50A0`, the file's first `$140` bytes left
  out), 2 from its last row (`$5080`, `$100` left out), 3 the bottom half (`$5800`, all
  of it), 1 the tilemap's start (`$5000`, all of it; settled 2026-09-30 from files
  tagged by unit, the status bar's counters written over some words after). A
  `$2000`-byte file runs past the buffer at `$7EAD00` into `$7EBD00`-`$7ECCFF`, which
  comes out of a Lunar Magic ROM's load as it went in, with a copy of it left in VRAM at
  `$4000`-`$47FF` (seen in RAM and VRAM): layer 3's tiles `00`-`7F` stay garbage unless layer 3's
  files are loaded after (with `3`, or the game's own when some level's list has `3`).
  Kobo's loader reloaded `28` and `29` there before 2026-09-30, and parked only size 0
  (an index bug); it now does as Lunar Magic's. Size 3 (`FF` = 3), which the dialog does
  not offer, leaves `$4000`-`$4FFF` otherwise than size 0, not found how; builds refuse
  it (kept so on review, 2026-10-04, until a hack needs it; so too a file too large for
  its slot, a file `32`-`7E` in a list, and objects `24` and `25` on layer 2).
- The layer 3 settings in the high nibbles of BG3, SP1-SP3, and LG1-LG4 are Lunar
  Magic's layer 3 code's ("Layer 3 settings", below).
- A list word of `$FFFF` is an empty slot in lists older versions of Lunar Magic wrote
  (Kaizo Kindergarten's FG and BG slots in levels that never set `G`); 3.70 writes `7F`
  there. A save of a level with `G` set writes `$EFFF` for SP4's `$FFFF`.
- Lunar Magic leaves `$7FC006`-`$7FC008` pointing at the level's list after every level's
  header load, boss arenas included, once the ROM has lists; in boss arenas it also leaves
  `$41` at `$7FC009`, which nothing was seen to read.
- The older lists (objects `24` and `25`, the bypass before the per-level lists) are 4
  bytes each at `$0FF200`, 256 of them, the files stored FG3/SP4, BG1/SP3, FG2/SP2,
  FG1/SP1. Object `24`'s position bits (`YYYY` low nibble and `XXXX`) give its sprite
  list plus one and its third byte its FG/BG list plus one (0: the tileset's own); object
  `25`'s third byte gives the animated tiles' file plus one. The loader reads them from
  layer 1's data (so objects there after a screen jump count the same). A list replaces
  the tileset's files whole, each loaded even when the game believes it is loaded.

#### Layer 3 settings

Lunar Magic's "Change Layer 3 Settings" (help file: `level_layer3_settings`), kept in
the high nibbles of a level's list as the community documents them (smwspeedruns'
level data format, "ExGFX Files"): BG3 `AAAA`, SP1 `SCXX`, SP2 `HVYY`, SP3 `yyyy`,
LG1 `vvvv`, LG2 `hhhh`, LG3 `YYYY`, LG4 `yOIB`, and AN2's bit 12. Found by importing an
MWL with settings into a copy of `+ExGFX` (Lunar Magic then installs its layer 3 code),
then writing list words straight into copies of that ROM (data only) and playing levels
along set paths, printing layer 3's RAM and registers frame by frame
(examples/layer3_probe.rs `play`, `compare`), 2026-09-30.

Lunar Magic installs the code the first time it saves a level whose list has a setting
(`T` or `3` alone do not install it): `JSL`s at `$00A01F` (the end of the game's layer 3
setup, `CODE_009FB8`, where it reads the level's layer 3 setting again before loading
that setting's stripe image), `$00A153` (the overworld's load, `LDA #$06 : STA $12`),
and `$05C40C` (`CODE_05C40C`, the frame's layer 3 scroll, which runs while `$13D5` is
0), and a `JML` at `$0194B6` (`CODE_0194B4`, a sprite beyond the level's edges). A save
counts the code as installed by a `JSL` (`$22`) at `$00A01F` alone, whatever its
operand: with it a save touches none of the four sites; with anything else there (a
`JML`, the game's `LDA`) it installs its code over all four (bisected over its hook
sites, then that site's bytes, printing addresses only).

What the settings do, observed:

- Without `B` (LG4 bit 12) nothing but `O` changes the game: layer 3 keeps the level's
  own setting and scrolling, however the other nibbles are set.
- `O` (LG4 bit 14), with or without `B`: a sprite beyond the level's edges touches tile
  `$25` (air, `$1693`) instead of the game's `$00` (water).
- `B`: `$13D5` = 0, so the frame's scroll runs whatever the level's own setting is, and
  layer 3 is placed and moved by the rest. Tides stay tides (`$1403` is kept, and the
  tide's interaction follows layer 3), but their up-and-down movement stops: `B` takes
  over the frame's scroll.
- `C` (SP1 bit 14): layer 3 in colour math, `$40` bit 2 set; clear, bit 2 cleared (fog
  and fish levels too). `S` (SP1 bit 15): layer 3 moves from the main screen to the
  subscreen (`$0D9D` bit 2 to `$0D9E`), and the registers the game has written already
  (`TM`, `TS`) follow.
- `XX` (SP1 bits 12-13): X is 0, `$40`, `$80`, or `$100` pixels (0, 4, 8, 16 tiles).
  `YYyyyyYYYYy` (SP2 bits 12-13, SP3, LG3, LG4 bit 15, from the top): Y as an 11-bit
  signed number of 16-pixel tiles (-400 to 3FF).
- `Hhhhh` (SP2 bit 15, LG2) and `Vvvvv` (SP2 bit 14, LG1), each axis alike, named in
  `names.toml` (`layer3_scroll`): `00` none: layer 3 starts at the offset and the code
  leaves it alone (whatever else moves it stays moved); `01`, `02`, `03`, `18`, `19`,
  `04`, `1A`: layer 1's position shifted right 0, 1, 2, 3, 4, 5, 6 bits (arithmetically),
  plus the offset, every frame; `05`: layer 1's position plus a fifth of it (unsigned,
  rounded down) plus the offset, and one more when that 16-bit sum wraps (so a negative
  offset the position passes counts one more; fitted to the positions observed); `06`-`11`: autoscroll, the speed
  (`$40`, `$80`, `$100`, `$200`, `-$40`, `-$80`, `-$100`, `-$200`, `-$300`, `-$400`,
  `$300`, `$400`, 8.8 fixed point) added each frame to a fraction as the game's own
  layer 3 autoscroll does (`$145C` horizontal, `$145D` vertical: low byte plus speed,
  the high byte sign-extended onto the position), then layer 1's movement that frame
  (`Layer1DXPos`, `Layer1DYPos`), nothing while `$9D` is set; the start is the offset.
  `12`-`17` and `1B`-`1F`, which the dialog does not offer, moved as `01` in every test.
- `I` (LG4 bit 13): layer 3 a frame late. `$1B78`/`$1B7A` hold a position that moves as
  layer 3 would, and layer 3 (`$22`/`$24`) has the one it held the frame before (RAM).
- The load leaves, in every level of a ROM with the code: `$145E` = LG3's nibble << 4 |
  LG4's, `$145F` = LG1's << 4 | LG2's, `$7FC01A` = SP1's (bit 7 set when the list has
  `T`), `$7FC01B` = BG3's, `$7FC01C` = SP2's << 4 | SP3's. With `B` also `$145F` = H*2,
  `$1460` = V*2, `$1458`/`$145A` the speeds (0 for the other kinds), `$145C`/`$145D` 0,
  or the speed's low byte when it is negative, `$146A`/`$146C` the offsets in pixels, and
  with `I` `$1B78`/`$1B7A` = the start.
- A list with `T` (layer 3's tilemap from LT3) has the level's own layer 3 stripe image
  (a tide's, the fish, the castle windows) left out, which would otherwise be drawn over
  the tilemap Lunar Magic's graphics loader put there. Without the code in the ROM the
  stripe image is drawn.
- A slot of `$FFFF` (an empty slot in older lists) gives `F`: Kaizo Kindergarten's SP2
  and SP3 in 177 levels, so `$7FC01C` = `$FF` there; nothing else changes without `B`.
- Older versions' code, in corpus hacks saved by them, does otherwise in places: 3.0x
  keeps a position that scrolls below 0 within 11 bits (`$07FF`, where 3.10 on give
  `$FFFF`), and 2.x hacks read an empty slot's settings otherwise and keep their state
  in `$145F`-`$1460` in another form. Kobo's code does as 3.70's, which 3.10 to 3.5x
  hacks agree with on every level with settings (tests/layer3_settings.rs).
- `AAAA` (BG3 bits 12-15, "Make tides act as"), in a tide level: the rows the game fills
  with tile `$000` (water) for a tide (`CODE_00A045`: rows 16-26 of layer 2's 16 screens,
  `$7EE400` + `$1B0` a screen) take other tiles, low and high bytes: `1` `$004` in row 16
  over `$005`, `2` `$005`, `3` `$159` over `$005`, `4` `$130`, `5`-`F` `$200`-`$20A`
  (Quickie World and Baby Kaizo World 3, Lunar Magic 3.30 and 3.33, every value, and the
  same played with Kobo's code in place of their layer 3 code). Lunar Magic does it in its
  taller levels code (a `JSL` at `$00A045`, `CODE_00A045` itself), not its layer 3 code:
  a 3.70 ROM with Kobo's taller levels code leaves the rows water, `$7FC01B` = AAAA all
  the same, and QLDC 2021 `56_TheKazooBloccGosh`, which has no layer 3 code of Lunar
  Magic's, fills its tide with AAAA's tiles. At any size it fills rows 16 to the bottom of
  the screens from the one layer 2's pointers start at (the split screen with the size
  byte's `T`, else 16), the first always and then each that ends within `$3700` bytes of
  the tile planes (at the game's height, screens 16-31): found by loading a tide level at
  every size with AAAA `1` and `4` and diffing the planes, which matched cell for cell
  on four levels of three hacks (3.11-3.33) at every size that loads, `T` set and clear
  (2026-10-02; levels at size `1C` did not load in those hacks and were not compared). The rows are where layer 2
  interaction meets the tide, so `T` matters in a level without layer 2 objects too
  (Lunar Magic sets it on some; `LevelSize::split`). Kobo's taller levels code fills the
  water so (`exlevel.asm` at `$00A045`, from the block lookups' pointer bytes, since its
  pointer table is the sprite graphics buffer's by then), and its layer 3 code puts
  AAAA's tiles in the same rows. An import keeps AAAA in a tide level of a ROM with the
  taller levels code even without Lunar Magic's layer 3 code. Without a tide nothing
  changes.
- `B` in a tide level: the tide's interaction offsets (`$26`, `$28`) follow layer 3 in
  Kobo's code: layer 3's position less layer 1's (`$1462`, `$1464`), horizontally from
  layer 3's X before the frame's move, vertically from its position now, or with `I`
  the one a frame ahead (`$1B7A`, kept within the tide's bounds as the position is);
  for a horizontal autoscroll the X taken is `$80`, or 0 in a level of one screen, and
  layer 2's horizontal movement (`$17BF`, which layer 2 interaction carries the player
  and sprites by) is the autoscroll's next step, 0 while `$9D` is set (2026-10-04, the
  corpus play comparison below); a vertical autoscroll in a
  tide level starts at layer 1's position plus the offset (Lunar Magic's help: the offset
  is not a position there). Lunar Magic keeps the offsets in `$0BE8` and `$0BEA` and a
  frame ends with the previous frame's in `$26` and `$28`: horizontally from layer 3's
  position before the frame's move, vertically one autoscroll step ahead. `$28` is
  written four times a frame (a RAM watch, values only). The frame they trail by comes
  with the site after the player's collision with layer 2 (`$00E966`, "The sites a save
  keeps with the marker" below, found by putting it back to the game's bytes),
  which builds have since 2026-10-02: Super Dram World 2 v1.3's level `0D3` (3.51, a
  tide with `B`) with Kobo's layer 3 code and that plays as the hack on every frame, and
  without that it does not. Before, the 7 corpus levels played the same but where a
  sprite meets the tide's end, and on the first frame; those were checked against the
  hacks moved into a 3.70 ROM and have not been again. A tide in a vertical level, which
  only Lunar Magic supports with `B`, is refused.
- A tide's vertical position with `B` (found 2026-10-04 in QLDC 2021 `35_FrozenQuills`'s
  level `025` moved into a 3.70 ROM, then with every vertical setting and offsets from
  -12 to 30 rows, layer 1 along its height): where layer 3 moves as a tide does, with
  layer 1 (setting 1, or another of no shift) or by an autoscroll, Lunar Magic 3.70 keeps
  it not below 0 and, from `$108` on, within `$108`-`$117`, as `$108` plus the position
  less `$108` in 16 (the step of the tide's pattern); none, a fraction of layer 1, and
  1.2 times leave it as set, below 0 or past `$117`. X is not bounded (an older version
  was once seen keeping it under `$200`). Kobo's code does the same since
  (`tide_bounds`). A layer 3 axis with no scroll setting is left as it is, by Lunar
  Magic's code and since 2026-10-04 by Kobo's with `I` too (it had put back the position
  it kept a frame ahead, which undid a hack's own code moving layer 3: QLDC 2022
  `27_unipat` level `105`).
- Reviewed 2026-10-04 and kept, on condition of the play comparison that follows:
  `tides_act_as` filling the tide's rows from Kobo's layer 3 code, `advanced` with a tide
  building with Kobo's code for the offsets (at `$00E966` too), and the refusal of
  `advanced` with a tide in a vertical level. Played against the corpus (2026-10-04,
  `examples/built_play.rs`, as "Layer 2 scroll settings" has it): the 17 tide levels of 12 hacks with `tides_act_as` or `advanced`,
  each hack built by Kobo, along the seven paths, against the hack and against its
  levels moved into a 3.70 ROM. Against 3.70 they had differed in the tide's horizontal
  offset (layer 3's X after the move where 3.70 takes it before: QLDC 2021 `06_Friday`'s
  `106`; `$80` for an autoscroll in one-screen levels, where 3.70 takes 0: FrozenQuills'
  `025` of both years and MegaMarioMan9's `11F`, which put the player out of the water at
  the tide's left end), the vertical one with `I` (the shown position where 3.70 takes
  the one a frame ahead, bounded: FrozenQuills), layer 2's horizontal movement with a
  horizontal autoscroll (never set: eight levels, which the player and sprites on the
  tide are carried by), and that movement on the first collision (kept where 3.70 swaps
  it). Kobo's code does each as 3.70 since, and every frame of every level and path is
  the same, but where the transfer is not the hack (`$145E` in Kazoo Blocc Gosh's
  `030`, whose transfer has Lunar Magic's layer 3 code and its build none, `tides_act_as`
  alone needing none) and `$1456`. Against the hacks themselves, the ones from Lunar
  Magic before 3.40 (16 of the 17 levels) differ in the offsets or layer 2's movement
  from the first frames, where 3.70 does as Kobo; Super Dram World 2 v1.3's `0D3`
  (3.51) plays as the hack on every frame standing and carried. Not in the corpus, and still otherwise than 3.70 (by writing settings into
  akogare v1.2's `115` in its transfer and Kobo's build): a vertical autoscroll's offset
  a step behind 3.70's (`$0BEA`), and with layer 3 following layer 1 or autoscrolling
  vertically, the offset the load leaves (`$28` on the first frame, which the player's
  first collision keeps when it comes on that frame) from layer 3's position before
  Kobo's code has placed it. Builds take both as they are (accepted on review,
  2026-10-04; known-gaps.md).
- With every QLDC entry added, `kobos_layer3_code_plays_as_lunar_magics` takes 85 hacks
  and agrees on every level it compares (one, QLDC 2021 `51_singlepat`, left out: Asar
  would place Kobo's code in a block of the hack's). In tide levels it leaves out what a
  sprite touches (`$1693`): that comes from Lunar Magic's code at `$00E966` too, which
  stays in a hack Kobo's layer 3 code is swapped into and reads Lunar Magic's layer 3
  state; a Kobo build, with Kobo's code at both, gives the hack's values there (QLDC 2021
  `06_Friday`'s level `106`, where the swap alone did not).
- Not settled: AN2's bit 12, which no corpus ROM sets and which changed nothing seen.
  Builds refuse it (`build::check_layer3`). Reviewed 2026-10-04: the refusal stays until
  a ROM sets it; scroll settings `12`-`17` and `1B`-`1F` build as `01`; a slot of
  `$FFFF` keeps its word, and settings needing another nibble there are refused; import
  leaves the settings out of a ROM without Lunar Magic's layer 3 code but for a tide
  level's AAAA; and the level files' `[graphics.layer3]` keys stay as they are.

Kobo's (`asm/lunar-magic/layer3.asm`, installed by a build whose lists have any setting,
`build::Project::lunar_magic_layer3`, after `graphics.asm`, whose `$7FC006` it reads)
hooks the same four sites but `$00A153` (builds keep the overworld as the game has it)
and meets the check with its own `JSL` at `$00A01F`. Against Lunar Magic's
(examples/swap.rs layer3, `layer3_probe sweep`, 2026-09-30): 1,500 random
settings in random vanilla levels (not the five tide levels), along eight camera paths,
240 frames each, the same layer 3 RAM, registers, and tilemap every frame, and all work
RAM `$0000`-`$1FFF`; Kaizo Kindergarten's content (tools/lunar-magic/transfer), all 512
levels along one path and the 91 with settings along five more, the same; and `tests/layer3_settings.rs`
(`kobos_layer3_code_plays_as_lunar_magics`) plays every corpus hack's levels with settings
under both.

#### Kobo's graphics code

`asm/lunar-magic/graphics.asm`, installed by a build that uses any of the formats above
(`build::Project::lunar_magic_graphics`), meets the four checks (`$00AAD8`, `$00AA47`,
the `JSL` at `$0583B8`, `"LM"` at `$0FF15C`) and hooks `UploadSpriteGFX` (`$00A9DA`),
`UploadGFXFile` (`$00AA6B`), `PrepareGraphicsFile` (`$00BA28`, which puts a file the game
reads as 3bpp back in that layout for the letters, the overworld, and the Mode 7 bosses),
GFX33's decompression (`$00B895`, `$00B89F`), the header load (`$0583B8`, which also
leaves `$7FC006`), and the documented decompression entry `$0FF900`; objects `24` and
`25`'s dispatch entries point at an `RTS`. It leaves Lunar Magic's other 4bpp and ExGFX
sites (the overworld, cutscenes, credits, switch palace blocks) as the game has them:
builds keep the overworld's own files, and write lists `200`-`206` as Lunar Magic 3.70
writes them (the overworld's own files) so its editor and save find them. Overworld
graphics are left to the overworld work (roadmap step 4; review, 2026-10-04); an import
notes the submaps whose lists name other files (Kaizo Kindergarten's 0-4 and 6).

Against Lunar Magic's (examples/swap.rs graphics, gfx_probe `loads`, 2026-09-28):
- vanilla+LM with 4bpp files: the same VRAM after every level's load, and the same
  RAM. The player's tile words (`$6040`-`$6187`, and SP1's last tile, `$67F0`) differed
  until 2026-09-30: the game builds the player's tile pointers (`DynGfxTilePtr`,
  `DynGfxTile7FPtr` at `$0D99`) from direct page `$0C`-`$0D` before the player is set up,
  and Kobo's loader left its own scratch there. It now gives `$04`-`$0F` back as it
  found them, with `$0F` = `$FF` as the game's loop leaves it (and `$0E` = `$FF` in a
  Mode 7 room);
- `+ExGFX` with lists naming files 100 and FFF, AN2 files up to `$1A00` bytes, and
  objects `24`: the same VRAM;
- Kaizo Kindergarten's content (tools/lunar-magic/transfer), with the VRAM patch
  swapped too: the same pictures but for the 7 levels whose layer 3 settings show, and
  the same VRAM but for those and the player's tiles (both since settled). RAM differed
  in the loader's direct page and `$0D99` (above), the layer 3 settings' RAM, and
  `$7FC009`; now only in `$7FC009`.

A build stores every GFX file the game keeps as 3bpp as 4bpp, as Lunar Magic 3.70
converts it (`exgfx::stored_4bpp`: `to_lm_export`'s conversion and `GFX17`'s berry, which
the 3.21 export in `tests/fixtures/` leaves out); a project's own GFX files take 16
colours. Kobo's loader has no 3bpp case, so ExGFX or lists in a build mean 4bpp (kept so
on review, 2026-10-04: of 57 corpus ROMs, 2 have ExGFX with 3bpp files, `LAG` and one
`Super Mario World.smc`, and draw the same built as 4bpp). A vanilla 4bpp build draws all
512 levels as vanilla (render_hashes). It writes
the ExGFX files where free space allows, `$0FF600` for `80`-`FF`, the `$6E00`-byte block
(pointers for `100`-`FFF`, then 520 lists: 3.70's list with nothing set for every level
and the one after the submaps, the overworld's files for the submaps `200`-`206`) with
`$0FF873`, `$0FF937`, and `$0FF7FF` pointing into it, and the older lists at `$0FF200`.
The levels stage writes the levels' own lists. A save of such a build keeps every list,
ExGFX file, GFX file, and older list (tests/lunar_magic_save.rs). With each checked byte
put back to the clean ROM's in turn, the save still keeps them all but without the
marker, when it installs its own ExGFX code over the tables and rewrites the lists and
ExGFX pointers.

#### Still to find

- Layer 2 objects moving vertically, frame by frame (tiles changed in play match:
  "Unknowns" below). A camera that jumps further than a column a frame: found
  (2026-10-03, "Tilemap streaming" above). The game loop hook
  (`$00BA56`), the lag path (`$0081E2`), and the stripe upload (`$0085D2`) were checked
  one by one (2026-10-02, above).
- Which tiles of which files the 4bpp piece still converts from 3bpp (the help file names
  "portions of files B, F and 21"), and what it does on the overworld and in the credits.
  In levels it changes nothing Kobo's loader does not: Kaizo Kindergarten (4bpp) with
  Kobo's graphics loader swapped in loads the same VRAM on every level, the Mode 7 boss
  arenas included, but for the player's tile words (2026-10-04, `swap graphics`,
  `gfx_probe loads 0000-8000`).
- SP4's high nibble: what its bits do, and why Lunar Magic 3.70's save clears bit 12
  (Kobo's builds keep what the source says). Bit 12 changes nothing a level's load
  leaves (lunar-magic.md); play was not compared.
- AN2's bit 12, what tides act like in a taller level, and how Lunar Magic times a tide's
  interaction with `B` ("Layer 3 settings"); LT3's size 3; what a slot does
  with a file larger than the buffer (Kobo's builds refuse more than `$1000` bytes, `$1A00`
  for AN2 and `$2000` for LT3). The last two stay refused until a hack needs them (review,
  2026-10-04).
- Objects `24` and `25` in the locked corpus ROMs (Invictus, Baby Kaizo World 3, both
  Super Dram Worlds), whose object data does not parse (lunar-magic.md).
- For the sprite loader and ExAnimation agents: ExAnimation reads its source from the
  buffer at `$7EAD00`, which holds the AN2 file (or object `25`'s, or BG3's with AN2
  `7F`) after the load; taller levels need `$00BF36`, `$0580D3`, and `$05879D`, which
  Kobo's VRAM patch also hooks.

### Entrances, exits, and midway points

- Secondary entrances (data): exits `0CE`, `0CF`, `0D0`, `0D9`, `0DB`, `0DE`, `0E1`,
  `0F0`, `0F6`, `0F7`, `0FF`, which vanilla leaves with destination level 0 and stray
  position bytes, are cleared in all four tables (`$05FACE`-`$05FEF7`, 9 ranges); bit 3
  of `$05FE00` is set for exits `100`-`1FF` (`$05FF00`-`$05FFFF`), the destination's bit 8
  ("D" in the format), which vanilla leaves implicit in the submap. The save rewrote
  entry `1CB` of that table as well. Contract: Lunar Magic's exit code reads the
  destination from `$05F800` plus this bit.
- Midway points: the opcode of `STA $13CD` at `$05D9C3`, and the operand of `STA $95` and
  the `JMP CODE_05DA17` after it at `$05D9E7`, in the entrance code, and `BEQ` at
  `$00F2DB` in the midway tape's block code, which vanilla uses to skip recording a
  midway point on screen 0 (as 3.70 in 41 corpus ROMs). Documented
  (changes 2.20): separate midway coordinates, screen exits to midway entrances, the
  screen-0 fix. Observed: the entrance code leaves `$13CD` at `$1A` in 27-row levels and
  other values (`$93`, `$9A`, `$D3`, `$DA`) in taller ones, the entrance's `RL-ooooo`
  (below), which game mode `$11`'s `$009708` reads and clears ("The sites a save keeps
  with the marker"); vanilla leaves 0 there on that path.
- Exits, observed by running a Lunar Magic-saved ROM's entrance code (`CODE_05D796`) with
  chosen exit state (`expand::enter_by_exit`, examples/exit_probe.rs): the screen exit
  object keeps an exit's whole flags nibble (`0000wush`) in `$19D8` (vanilla keeps bit 0)
  and sets `$1B93` to its `s` bit. For an exit with `u`, the destination's bit 8 is its
  `h` bit and `s` alone makes it secondary; without `u`, bit 8 is 1 when the translevel
  (`$13BF`) is `$25` or more, where vanilla takes it from the player's submap. So it is
  for Yoshi's wings and the bonus game (`$1B95`, `$1425`: `CODE_05DBAC` puts `$C8` or
  `$00` in the exit table at the player's screen), whatever flags that screen's exit
  left in `$19D8` (2026-10-02, every translevel, three submaps). A secondary
  entrance's destination has its bit 8 in bit 3 of its `$05FE00` byte, and an exit's `w`
  adds `$40` to `$192A` (the entrance's action, bits 0-2; the `$40` and `$80` above them
  are Lunar Magic's flags, which the game's dispatch at `CODE_00A6CC` does not expect:
  `$40` with any action takes its water path, so Kobo's code for the main entrance
  settings strips them before the dispatch).
- Kobo's (`asm/lunar-magic/exits.asm`: `$0DA532`, `$0DA536`, a `JSL` at `$05D7CE`, a
  `JML` at `$05D7E2` past the `LDY $0E`, and extended object `02`'s entry in the
  dispatch at `$0DA115`) resolves every exit with `u` as Lunar Magic's does, for two ROMs
  and any destination, and since 2026-10-02 exits without `u`, Yoshi's wings, and the
  bonus game by the translevel too (`tests/lunar_magic_save.rs`): before, it kept the
  game's submap rule there, which a Lunar Magic save then replaced, since a save puts
  its own code at `$05D7CE`. For vanilla's overworld the two rules agree, so a build of
  vanilla levels still plays as vanilla (kept so on review, 2026-10-04). It reads a secondary entrance's four bytes through
  the pointers at `$0DE191`, `$0DE198`, `$0DE19F`, and `$05DC81`, never from bank `$05`
  itself, so the tables can be anywhere. Lunar Magic's save writes its own code from
  `$05D7E2` on, keeping the `LDY $0E` before it: in every corpus ROM the `LDY` is the
  game's, and in 36 of 47 the bytes change from `$05D7E2` (a region diff against vanilla,
  addresses only, 2026-10-03). Kobo's hook first covered `$05D7E0`, and a save left a
  `JML` to nowhere there. (The session that moved it also decoded a byte of Lunar Magic's
  just before the hook, which the move does not need: clean-room-audit.md.)
- Long screen exits and entrances past `1FF` (riff2, 147 of them), observed with
  `enter_by_exit` and level loads on riff2 and a copy with chosen bytes (2026-10-02):
  extended object `02` is five bytes, the screen as the screen exit has it (on a vertical
  layer after the game's nibble swap, so the second byte's low nibble there), the
  destination's low byte in its fourth and `nnnnwush` in its fifth. Its handler puts the
  fourth byte in `$19B8` and the fifth whole in `$19D8` for the screen and leaves `$1B93`
  alone; nothing else in RAM changes. Every exit with `u` then leads to `nnnn` as bits
  9-12, `h` as bit 8, and the low byte: a secondary one to that entrance, up to `1FFF`, a
  normal one to that "level" (`$0E`-`$0F` past `1FF`, which a Lunar Magic ROM loads as it
  is). The four tables a ROM with such entrances has are moved, each a RATS block of its
  own, behind the operands above; Lunar Magic 3.70's save sizes all six (the two further
  ones too) to the last entrance in use (riff2's `1FFF`: `$2000`; a Kobo build's `320`:
  `$321`, re-allocated), and leaves unused entrances past `1FF` with bit 3 of the
  `$05FE00` byte clear. Since 2026-10-04 Kobo's builds size them so too
  (`Project::entrance_count`, passed to `entrances.asm` as `!entrance_count`): the
  highest entrance a level defines or an exit in Lunar Magic's format (`u` and `s`, long
  or not, kept as bytes or not) names, plus one, from `$201` to `$2000`. Both observed
  sizes are that rule (riff2 defines `1FFF` and names up to `323`; the Kobo build both
  defines and names `320`); whether Lunar Magic counts an entrance only named is not
  known, and counting it keeps every exit inside the tables. A normal long exit names a
  level and moves nothing. A build with such an entrance moves all six tables, copying
  the game's, and refuses an exit past the tables it has (`build::check_exits`). Lunar
  Magic's save reads them back, every entrance's bytes the same, and still moves them,
  at the same size (`tests/lunar_magic_save.rs`). Before, builds moved them to `$2000`
  each (48 KiB) whatever was in use. riff2's build (240 levels) is byte for byte what it
  was; without levels `0D8` and `12B`, which define `1FFE` and `1FFF`, its tables hold
  `$CCE` (29,484 bytes less), and its 77 entrances past `1FF` enter as the hack's, and
  survive the save (`save-check`).
  A long exit without `u` (Super Dram World has them, beside game-format exits on the
  same screens) leaves its fifth byte in `$19D8` too, where only `wush` is read; Kobo
  keeps it as bytes and builds it so.
  riff2's 227 buildable levels enter every entrance past `1FF` and load every long exit
  as the hack saved by 3.70 does. A build that installs it writes every entrance's bit 3 as
  Lunar Magic's first save does, unused ones from `100` on included, and lets a level have
  entrances numbered in the other bank. Kaizo Kindergarten built by Kobo sends every exit
  where the hack does. Main entrances that use the per-level tables (`$05DE00`,
  `$06FC00`-`$06FFFF`) still start the player elsewhere: those tables are not carried yet.
- Entrance settings, observed by running a Lunar Magic-saved ROM's entrance code with each
  value of each settings byte (examples/entry_probe.rs: `effects` flips bits, `compare`
  runs every value in two ROMs, `batch` chosen combinations, `levels` every level as it
  stands; `KOBO_ENTRY_OW` enters from the overworld, `KOBO_ENTRY_MIDWAY` and
  `KOBO_ENTRY_SECONDARY` vary the midway and secondary tables, `KOBO_ENTRY_FULL` runs the
  whole level load; formats in the smwspeedruns level data format page). Positions are in
  pixels, rows are 16 of them.
  - `$05DE00` `IWPXXtTT`: `I` and `W` add `$80` and `$40` to `$192A` (the entrance
    action, which `$05F200` bits 5-3 set); the entrance setup at `$00A6CC` turns them into
    `$86` = `$80` (slippery) and `$85` = 1 (water) and clears them. `P` (position method 2)
    places the player by tile: in a horizontal level X = the screen's plus (`XX` bit 0,
    `$05F200` bits 2-0) and Y = (`$06FC00` bits 5-0, `$05F000` bits 3-0); in a vertical
    one X = (`XX`, `$05F200` bits 2-0) and Y = the screen's plus `$05F000` bits 3-0.
    `tTT` goes to `$0BF4` (`t` as bit 7), for Lunar Magic's sprite loader.
  - `$06FE00` `RL-ooooo`: the entrance used leaves its `R` and `L` over the level's
    `ooooo` in `$13CD`, which the game kept the midway screen in; the midway tape records
    the midway point on any screen (`$00F2DB`). `R` places layer 1 at the player's Y plus
    the signed 5-bit offset `F:ff:bb` rows (`F` from `$06FC00` bit 6, `ff` and `bb` from
    `$05F400`), not below 0. Layer 2 then: with `O` (`$06FC00` bit 7) at layer 1 plus the
    signed `ooooo` rows, `$10` meaning 0 exactly; without, so that the background's last
    row shows at the level's bottom: layer 2 = (`ooooo` + 1) × 16 − `$F0` + (layer 1 −
    the level's bottom camera position) × the vertical scroll rate (0, 1, 1/2, 1/32 for
    settings 0 to 3, the shifts arithmetic), the bottom being `$C0`, or in a vertical level
    the top of its last screen. `$1417` is then layer 2 − layer 1 × the rate.
  - `L` (face left) turns the player left and the slanted pipe's shot with him, through
    `$13CD` and game mode `$11`'s `$009708` ("The sites a save keeps with the marker");
    Kobo's loader did not run that site before 2026-10-02, so it looked like nothing.
  - `$06FA00` `SHCvvvvv`: `S` gives layer 2 separate horizontal (`hhhh`) and vertical
    (`vvvvv`) scroll settings (`$1413`, `$1414`); `C` is the editor's.
  - Vertical positioning (`$05F600` bit 5) leaves `$1412` clear when `R` is set, where the
    game's sets it to 1 before the level's own setting replaces it; and vertical scrolling
    at will (`$13F1`) starts off where layer 1 is at the level's bottom, where the game
    compares `$1C` with `$C0`.
  - Midway entrances (four tables through `read3(read3($05D9E4) + $0A)`: `IWHMXAAA`,
    `yyyyxxxx`, `RLE-ffbb`, `-FYYYYYY`): with `H`, entering past the midway point uses
    them, as the main entrance's are used, on the midway screen (`$05F400` bits 7-4 and
    `M`); with `E` as well, the entrance is that of the level `$05FC00`'s table 2 and table
    3 bit 0 name, whose settings are then read again, so redirects chain, and a cycle
    never ends (Lunar Magic's help, "Main and Midway Entrance", warns of it; Kobo's
    builds refuse one). Without `H`, the game's midway
    entrance, which in a vertical level Lunar Magic moves to Y's screen, with layers 1 and
    2 on it. An exit with `w` and without `s` leads to the destination's midway entrance
    when it has `H` and no `E`, else to its main entrance.
  - Secondary entrances: `$05FE00` `IPXXDAAA` and the tables behind `$05DC86`
    (`EFYYYYYY`) and `$05DC8B` (`RLW-----`): `I`, `P` with `XX` and `YYYYYY`, `R` with the
    offset `F:bb:ff` (`$05FA00`'s high nibble), and `W` (`$40` in `$192A`), as the main
    entrance's. `E` makes it an exit to the overworld, which the probe cannot follow and
    Kobo does not build.
  - Lunar Magic's piece for this has its restorable hooks at `$05D97D` (into
    `$05DD30`), `$05D9E3`, and `$05DA17`, and `$00A6CC` (into `$05DD00`); `$05D9A1` is
    the taller levels piece's, which also sets `$0BF4` ("Taller levels").
- Kobo's (`asm/lunar-magic/entrance.asm`, with `exits.asm`: `JSL`s at `$05DA17`, the
  check, `$05D9E3`, whose target has its midway tables' address `$0A` bytes in, and
  `$00A6CC`, and the midway tape's `BEQ` at `$00F2DB`) does all of this in one routine at
  the end of the entrance code, where the game has set everything up for the entrance
  used; `exits.asm` leaves it the secondary entrance's number and the exit's flags in
  `$0BDA`-`$0BDC`, which it clears once read. Checked with `entry_probe`: every value of every byte of Kaizo
  Kindergarten's levels `101` and `105` (horizontal), `0AD` and `15B` (vertical), `001`,
  `023`, `12A`, `1CE`, the midway tables, and secondary entrances `0C5`, `0D7`, `1BB`,
  `1C0`, through a screen exit, from the overworld past the midway point, and through an
  exit with `w`, leave the same RAM at the end of the entrance code as Lunar Magic's, but
  for `L` (and since 2026-10-01 for layer 2 scroll settings 8 to 11, `S`, and `H` too:
  "Layer 2 scroll settings" below); after the whole load, the same
  player, camera, and entrance RAM but where other pieces of Lunar Magic's differ (its
  camera's X start in vertical levels, layer 3 tides, the slanted pipe's speed). Kaizo
  Kindergarten imported and built by Kobo enters every level the same way as the hack,
  from each kind of entrance, but for a vertical level whose "No Yoshi" intro Lunar Magic
  also moves the layers for (its 3.01 fix), and Lunar Magic saves the build keeping every
  setting (2026-09-26).
- `DATA_05D710`/`DATA_05D720` (layer 2 vertical and horizontal scroll by the high nibble
  of `$05F000`; data): entries 8-11 become vertical settings 4-7 with horizontal 2.
  Lunar Magic added layer 2 scroll speeds in 3.40 (changes); a save puts its own code at
  the camera's sites (register above).
  Kobo's `entrance.asm` writes the same entries.
- `$05DD00`-`$05DD1C`: code, the target of the restorable hook at `$00A6CC` (the entrance
  setup after `CODE_00A6CC`, which checks `$1C == $C0` for vertical scrolling). After it
  the entrance's slippery and water flags are in place (`$86`, `$85`; `entry_probe`).
- `$03BCDC`-`$03BCDF`: the first four bytes of the documented screen-number routine
  (`JSL $03BCDC`: 8-bit in and out, X = the screen Mario is on, A and Y and `$00` (16-bit)
  clobbered); a save restores the rest. UberASM Tool's and GPS's teleport routines call it
  when `!EXLEVEL`.

### Layer 2 scroll settings

Lunar Magic 3.40's added layer 2 scroll settings, observed 2026-10-01 on vanilla saved
by Lunar Magic 3.70 with each value written into level `105` (and `F7`, vertical):
`exlevel_probe show` with layer 1 and 2's positions, `$1413`/`$1414`, `$1417`, and the
words below, along carried paths and with the player still, and `entry_probe batch` for
what the entrance leaves; then `exlevel_probe compare` and `entry_probe compare` of
that ROM against the same with Kobo's `exits.asm` and `entrance.asm` applied over it.

- The settings: the game's pair by `$05F000`'s high nibble through `DATA_05D710` and
  `DATA_05D720` (entries 8-11 above); with `S` (`$06FA00` bit 7), `$1413` (horizontal) is
  the nibble plus `$10` with `H` (bit 6) and `$1414` (vertical) the byte's low five bits.
  `H` without `S` changes nothing.
- What the camera (`$00F79D` on) does with each, on either axis: 0 nothing; 1 to 8 layer
  2 = layer 1 times 1, 1/2, 1/32, 1/4, 1/8, 1/16, 1/64, and 1.2 (`x * 6 / 5`, rounded
  down, exact on 602 positions), plus `$1417` vertically (horizontal 3 is 1/32 too,
  where the game's camera would halve); 9 to `$F` nothing; `$10`-`$15` and `$16`-`$1B`
  move layer 2 by themselves at 1/4, 1/2, 1, 2, 3, and 4 pixels a frame, down or right
  then up or left, following layer 1 as well: layer 2 = layer 1 + an offset that the
  speed moves (`$1452` horizontally, `$1417` vertically), with the speed in `$144A` or
  `$144C` and an 8-bit fraction in `$1443` or `$1445`, as layer 3's autoscroll keeps
  its own; `$1C`-`$1F` follow layer 1 with the offset as it is. The moving ones stand
  still while `$9D` is set, and step from game mode `$13` on (two steps before the
  level's first frame in mode `$14`, against three for every call; inferred from the
  count). The entrance sets the speed and starts the fraction at 0, or at the speed's
  low byte when it is negative.
- The entrance: with a camera relative to the player, layer 2 is placed as for the
  game's rates (the background's height, or the offset with `O`), with `$1417` = layer 2
  less layer 1 at the rate; settings from 9 on count as rate 1. Without it, the game's
  `CODE_00A796` sets `$1417` as before (every setting from 3 on as 1/8 of layer 1,
  where 3's camera takes 1/32: the game's own mismatch, kept).
- Kobo's (`entrance.asm`, 2026-10-01): the settings at the start of the entrance
  routine, the speeds, the relative camera's rates, and the camera's layer 2 in place
  of `CODE_00F79D` (dividing by 5 in software, as the SA-1 has no divider of the
  S-CPU's). Against Lunar Magic's ROM with Kobo's code swapped in: the entrance leaves
  the same RAM for every value of every settings byte of level `105` (but `$5B` and
  `$0BE7`, which come with the hook at `$05DA17`, as swapping in Kobo's showed, for other
  pieces), and the camera
  the same RAM on every frame of 240, carried right and still, for every nibble, every
  horizontal and vertical value with `S`, with `$9D` held, in the vertical level `F7`,
  and with the relative camera. A save keeps the settings and Kobo's camera with the
  marker at `$05DD7C` (register above).
- Reviewed 2026-10-04 and kept, with the corpus play comparison below: the marker at
  `$05DD7C` that a save checks before it keeps `S`, `H`, and Kobo's camera (register
  above); a moving setting stepping from game mode `$13` on (what decides it is not
  known); and in the level files, `layer2_scroll` is the horizontal setting when
  `layer2_vertical_scroll` is there, up to `$1F` (`H`), and past 15 without it refused.
- After the entrance, game mode `$11` runs `CODE_00A796` (the `JSR` at `$00970C`), which
  sets `$1417` by the game's rates (1/8 of layer 1 for every vertical setting from 3
  on). A Lunar Magic-saved ROM leaves it so only without the relative camera and for
  settings below `$10`: with the relative camera `$1417` stays the entrance's (layer 2
  less layer 1 at the rate, as above), and a moving setting starts from layer 2 less
  layer 1 (found 2026-10-04 in the corpus play comparison below, Akogare2's level `009`,
  then every nibble and every separate setting with and without `H` and the relative
  camera, written into that level in its 3.70 transfer and Kobo's build, carried right
  and up). Kobo's `$009708` code skips the `JSR` then and sets `$142A` (its one other
  effect) itself; before, every level with the relative camera and a vertical rate from
  3 on, the game's nibble 0 among them, and every moving setting, had layer 2 off by
  layer 1 at 1/8 against the rate.
- Played against the corpus (2026-10-04, `examples/built_play.rs`, docs/testing.md "A
  hack built by Kobo, played"): the 42 levels of 14 hacks with nibbles 8 to 11 or
  separate settings, each hack imported and built by Kobo (Baby Kaizo World 3's four,
  locked, left out; QLDC 2021 `34_idol`, 14 of them, builds but for nine other levels),
  along seven paths of 600 frames (standing; running and jumping right and left; carried
  right at 3 and at 8 pixels a frame and back; carried up and down), against the hack and
  against the hack's levels moved into a 3.70 ROM (`tools/lunar-magic/transfer`). It
  found the entrance's `$1417` above, the first camera's screen count and vertical
  scrolling off ("The sites a save keeps with the marker"), and a player's point out of
  the level ("Taller levels"), each Kobo's code since. Against 3.70, layer 1 and 2,
  `$1411`-`$1418`, the moving settings' RAM, `$1462`-`$1469`, and the layers' movement are
  then the same on every frame of every level and path until something else has
  differed, which is the transfer's, not Kobo's: the screen count Lunar Magic's MWL import
  re-counts (`$5D`, 11 levels; with it the camera's range), what the hack's own sprites
  and blocks touch (`$1693`, `$98`-`$9B`, the player's path after; the build carries the
  hack's PIXI insert, the transfer none of its code: where 3.70 differs first, the build
  agrees with the hack in 122 of 149 level-paths, and the rest differ from both later
  on), and `$1456` (below). Extended Interactions' `006` stops on a `BRK` in the
  transfer and `012` in the build, both in code of the hack's that neither has, and its
  `017` loads a layer 1 grid unlike the hack's and the transfer's in Kobo's build, which
  is the import's or Kobo's objects', not these settings' (not followed). Against the
  hacks themselves the camera differs where their own code or an older Lunar Magic's
  does otherwise and 3.70 agrees with Kobo: `$55`/`$56` held at 2 and `$0BE7` 0 (3.30
  and 3.31 hacks), `$1411` 0 (Extended Interactions' code), `$1413` 0 for nibble 9
  (QLDC 2021 `34_idol`), the relative camera's layer 2 a pixel apart (Extended
  Interactions `007`, 3.31), and the player's path once a hack's own code or blocks
  take part; Sakaya Sanctuary's `00E` (3.40, `S`) plays as the hack on every frame
  standing.

### The sites a save keeps with the marker

With `"LM"` at `$05DD7C` a save leaves eight sites of Lunar Magic's as it finds them
(register above). Each was put back to the game's bytes in vanilla saved by Lunar Magic
3.70 and that ROM played against the same with it (`exlevel_probe compare`, 2026-10-02):
23 levels with layer 2 interaction (`009`, `01A`, `0D4`, `111`, `1CF`, `1E2`, `00E`, `0C4`,
`1DA`), a tide (`002`, `0BE`, `0C1`, `102`, `127`), or vertical scrolling (`0C2`, `0F7`,
`108`, `0E7`, `1CE`, `12A`, `1ED`, `0DB`, `0EA`, `134`), the player run by four controller
patterns or carried along paths, 600 frames each, and every level's load.

- `$009708`, game mode `$11`'s `LDA #$20 : STA $5E` (a `JSL` in every corpus ROM from
  2.30 on): the entrance code leaves the entrance's `RL-ooooo` in `$13CD` ("Entrances"
  above); with `L` the player faces left (`$76` = 0) and `$F9` = `$C0`, else `$F9` =
  `$40`; then `$13CD` is cleared. `$5E`, the most screens the camera's first placement
  goes to, is the level's screen count (the header's, which the load later puts in
  `$5D`), not `$20`: an entrance on a level's last screen starts with the camera at the
  level's end (RAM watched in every level tried, horizontal, vertical, and taller;
  found 2026-10-04 in QLDC 2021 `34_idol`'s level `0C2`, whose camera started `$14`
  pixels past it). And with the relative camera, or a moving vertical setting, the
  game's `CODE_00A796` after the hook does not set `$1417` ("Layer 2 scroll
  settings"). In ROMs of older versions (Kaizo Kindergarten, Akogare2,
  Luminescent, Advanced Shells) a level's layer 2 comes out right only with the ROM's own
  bytes from `$009708` to `$00970F` run in place, which Kobo's loader does (smw.md).
  Before it did, a level's layer 2 came out wrong in the pictures of those hacks wherever `$13CD` had `R` (Kaizo Kindergarten
  `009`, `012`, `0A0`, `107`: 72.7-88.4% of Mesen's frame, 99.7% since).
- `$00D2B2`, the slanted pipe's `LDA #$40` (its X speed): Kobo's takes it from `$F9`.
  The player's X speed (`$7B`) leaving the pipe equals `$F9` in a Lunar Magic-saved ROM
  at `$00`, `$40`, and `$C0` (RAM, 2026-10-02), and with `L` the pipe shoots the player
  left, as Lunar Magic's help says. Kobo's bytes follow from the game's instruction there
  and that effect.
- `$00AF6C` and `$00AF71`-`$00AF76`, in the palette's fade at a level's end
  (`CODE_00AF35`): colours 1 to 15 of rows 0 to 7 fade, but for layer 3's colours in rows
  0 and 1 (row 0's 9 to 11, row 1's 9 to 11 and 13 to 15), which are set back to the
  level's (`$0703` into `$0905`); the back area colour fades. The game's fades colours 1
  to 7 of those rows and row 0's 13 to 15. Found by calling the routine
  (`expand::call_in_level`) at every step of the fade with a palette of distinct colours
  and the fade's copy filled with a marker value, which tells a colour left from one set
  or blended.
- `$00E966`, after the player's collision with layer 2 (`CODE_00E938`): layer 2's offsets
  from layer 1 (`$26`, `$28`) and its movement this frame (`$17BE`, `$17BF`) trail a
  frame. This frame's go to `$0BE8`-`$0BED` and the previous frame's come back, so that
  what runs after, and the next frame's collision, goes by them; the first time in a
  level (`$0BE7` bit 6, which the entrance code sets, then cleared) this frame's are kept
  as they are. A tide's offsets trail the same way, with layer 3's `B` or without
  (Super Dram World 2 v1.3, Lunar Magic 3.51, level `0D3`: a tide with `B`). The frame
  shows in what the player touches and his animation on moving layer 2 (`01A`, `0D4`,
  `1CF`, `1E2`, `1CE`).
- `$00F77B`, a vertical level's horizontal camera (`SEC : SBC $142C,Y`): no move when the
  player is exactly at the edge the camera goes by, as the game's horizontal levels have
  it (their `BEQ`); the game's vertical code moves a pixel there, so `$1A` and `$142A`
  came out one apart along carried paths in every vertical level tried. The scroll
  direction (`$55`, `$56`), which the horizontal code also stores, stays as it was.
- `$00F871`, vertical scrolling's `LDY #$04 : BRA` in `CODE_00F7F4` (vertical scrolling
  off: setting 2 without `$13F1`, the player above the screen's middle and not flying or
  climbing): the game's moves layer 1 up at most 6 pixels a frame and keeps it at `$C0`
  or lower, the lowest position at the game's height, so there it stays put; a Lunar
  Magic-saved ROM puts layer 1 at the lowest position the routine was given (`$04`: a
  taller level's bottom, a vertical level's last screen) wherever it was, by calls of
  `CODE_00F7F4` at every camera and player position against Kobo's (2026-10-04, found in
  SMW_2022-4-9's level `104`, 896 rows, whose camera rose from its bottom as the player
  jumped). Nothing different was seen in the levels of the game's height above, where
  both stay at `$C0`.
- `$05BCA5` (the layer 2 scroll commands' `LDA #$04 : STA $1456`, which leaves `$1456`
  0 in a level without one; only those commands read it, after setting it), and
  `$05D7B9` (the screen a screen exit is looked up by: every `$5B` from `00` to `03` and
  `80`, `81`, screens to `$21`, and both exit formats): nothing else observed, here or
  in the corpus play comparison ("Layer 2 scroll settings").

Kobo's: `entrance.asm` at `$009708`, `$00D2B2`, `$00E966`, `$00F77B`, and `$00F871`, its
entrance code setting `$0BE7` bit 6; `palette.asm` the fade's operands (`$00AF4B`, `$00AF4E`,
`$00AF6B`: the game's loop over 15 colours a row) and a `JSL` at `$00AF71` for layer 3's
colours. With them swapped into vanilla saved by Lunar Magic, every frame of the runs
above leaves the same RAM (but `$0BDA`-`$0BDC` after a level ends, `exits.asm`'s), and the
fade the same palette at every step, on LoROM and SA-1 (`tests/lunar_magic_save.rs`,
with `kobos_layer2_offset_at_the_entrance_is_lunar_magics` and
`kobos_first_camera_and_scrolling_off_are_lunar_magics` for what 2026-10-04 added); a
build's face left turns the player and the slanted pipe (`tests/project_build.rs`). The
other two keep the game's code.

### Sprites

- The loader loop (`$02A826`-`$02A83B`, from `BMI Return02A84B` to the first `INY`) now
  starts with a `JML` to a Lunar Magic block, and `INY : LDX $02` at `$02A9D7` becomes
  `JMP $A838`; `LDA $00` at `$02A968` changes opcode. Feature: the new sprite system's
  `$FF` commands and Y jumps (3.00). Contract with the tools: PIXI's and SA-1 Pack's
  loaders return to `$02A82E` as the loop head with Y at the next entry and X counted
  (`JML $02A82E`), PIXI's `SubLoadHack` returns to `$02A968`, and PIXI jumps from
  `$02A9DB` and `$02ABEF` on LoROM, so all four must stay entry points of Kobo's loop.
- `$02ABF3`, the operand of `LDX #$3F` in `CODE_02ABF2`: vanilla clears only 64 of the
  128 load flags. PIXI writes `$7F` there itself unless `!EXLEVEL` ("be able to load 128
  sprites"), so with a Lunar Magic 3 layout Kobo has to clear all 128. PIXI takes
  `!EXLEVEL` from the version string at `$0FF0B4`; the `$FF` of a Kobo build reads as a
  version past 2.53, so PIXI assembles its `!EXLEVEL` code for one.
- The group. Besides the three one-time sites above, a save installs, as one group, `JML`s
  at `$02A846` (`LoadNextSprite` and the `BNE` after it), `$02A95B` (the start of a
  horizontal level's Y, `LDA [$CE],Y : PHA : AND #$F0`), `$02AA61` (a cluster sprite's Y
  high byte in `CODE_02AAC0`, `STA $1E2A,X : DEX`), `$02AB54` and `$02ABD0` (a Boo ring's
  and a shooter's Y high byte, `AND #$01 : STA`), `JSL`s at `$02AC64` and `$02ACA4` (`LDA
  #$01 : STA $55` in the level's first spawns, `CODE_02AC5C` and `CODE_02ACA1`),
  `$02AF3D` and `$02AFA7` (`AND #$01 : STA $09` in `Load3Platforms` and `Load5Eeries`),
  and over the erase test below the level (`CLC : ADC #$50 : LDA $14D4,X : ADC #$00 :
  CMP #$02`) in each bank's `SubOffscreen` (`$01AC40`, `$02D03A`, `$03B86C`) and the
  cluster sprites' copy (`$02FED6`, which has no caller), and changes to the goal tape's
  init (`$01C08C`-`$01C093`: `STA $187B,X : AND #$01 : STA $14D4,X`) and its contact
  (`$01C0E2`, the operand of `AND #$01`). It also writes `JMP $AB78` over `JSR
  LoadShooter` at `$02A8D8` on every save, as PIXI does.
- The check: a `JSL` at `$02AF3D`, whatever it calls. A save of a ROM with one (Kobo's
  level number and sprite bank pieces, and a stub behind `$02AF3D`) left every other site
  of the group as it was; without it, it installed the whole group (found 2026-09-28 by
  comparing bytes at the sites, not printing them).

Observed with `examples/sprite_probe.rs` (lists written into a copy of vanilla saved once
by Lunar Magic, `e0`; the camera carried along set paths; `$0BF4` from the level's
`$05DE00` byte through Lunar Magic's entrance code), 2026-09-28. Rows and columns are
16-pixel tiles, `c` the camera's position (`$1A`, `$1C`) divided by 16 as `(c + offset)
>> 4`:

- Horizontal levels spawn, each call of the loader (every other frame, as the game's), in
  one pass over the list in its order: every sprite in the column the game's loader takes
  (the one the camera scrolls towards, `$55`) whose row is within the level's spawn range,
  and every sprite in the range's top row or its bottom row whose column is from `($1A -
  $30) >> 4` to `($1A + $110) >> 4`. The range, by `TT` (`$0BF4` bits 0-1): rows `($1C -
  $C0) >> 4` to `($1C + $1A0) >> 4` for 0, `$30` and `$110` for 1, `$80` and `$150` for 2;
  3 has none, and every row of the column spawns. The top of 0's range is seen only
  through its top row's respawning (a camera at `$C0` has row 0 as its top row); nothing
  below it can be checked in a level 27 rows high. Vertical levels spawn as the game's
  loader does.
- A sprite in a row pass loads at its own column: `$00` and `$01` are its X, as the game's
  loader has them for the column.
- Smart spawning (`t`, `$0BF4` bit 7): the column spawns only when it is not the one of
  the last call, and each row only when it is not the last call's; a camera that stands
  still spawns nothing again, where without it a sprite whose load flag clears (killed,
  or erased off screen) spawns again at once if its column or row is still the edge.
- Off screen in a horizontal level, a sprite (without "process off screen") is erased when
  its Y less the camera's is less than `-$D0`, `-$40`, `-$90`, `-$400F` or at least
  `$1C0`, `$130`, `$170`, `$400F` for `TT` 0 to 3 (the spawn range a tile wider above and
  two below), and every sprite at or below the level's height, as the game erases it below
  row 27. The level's first spawns leave the two bounds in `$0BF0` and `$0BF2`, which
  PIXI's `SubOffScreen` reads the same way (`routines/SubOffScreen.asm`, with
  `!EXLEVEL`); in a vertical level `TT` 0's, with `$0BF4` cleared. A level whose first
  spawns take another way leaves them zero and still keeps its sprites by `TT` (the boss
  arenas `096`, `196`, `1EB` of Luminescent and Kaizo Kindergarten), so Kobo's check
  takes them from `TT`, not from RAM. `$0BEE`-`$0BEF` hold `$FFFF`, or with smart spawning other values
  (`$90`, then `$00`, `$90`, `$40`, `$C1` for `TT` 0 to 3), which nothing known reads.
  Measured on sprites held at chosen heights too, as far as a 27-row level shows.
- A list may have 128 sprites (all 128 load flags at `$1938`); `$FF` in the old format
  ends it, in the new it is a command as [lunar-magic.md](lunar-magic.md) has them. A list
  out of screen order crashes the game (the random scenarios with unsorted lists broke to
  `BRK` and `COP`); every list of the corpus is in screen order but five levels of QLDC
  2021 `34_idol` (Lunar Magic 3.30: `016`, `08F`, `090`, `0B1`, `13F`), which stop on
  their first frame under the hack's own code too (a `BRK`, or a loop that never ends;
  Mesen could not confirm it, its script staying on the hack's title screen). Builds
  refuse such a list rather than sort it, which would change its load indexes from the
  level file's (`build::sprite_list`; kept, maintainer, 2026-10-03).
- `$14D4` is a spawned sprite's whole Y high byte in a horizontal level (the game keeps
  the extra bits in bits 2-3 as well): Y bit 4 and the Y jump times 2. The goal tape
  (sprite `7B`) gets the entry's first byte `AND #$0D` in `$187B` when it spawns, and its
  init keeps `$14D4` and `$1534` whole. In a vertical level the X high byte keeps the
  extra bits, as the game has it.
- The level's first spawns (the game's column loop from 6 columns left of the camera)
  take the rows too, at each step's camera.
- The load leaves the entry's first byte of the last sprite loaded in `$54`, which Kobo's
  loader does not; no reader of it is known, so neither it nor `$0BEE`-`$0BEF` is matched
  (reviewed, 2026-10-03).

Kobo's (`asm/lunar-magic/sprites.asm`) takes every site of the group, with the `JSL` at
`$02AF3D`, and keeps the one-time sites' entry points (`$02A82E`, `$02A968`, the `JSL` at
`$02A968` running what it replaces; `$02A846` is Kobo's step to the next sprite, which
PIXI replaces with its own), and leaves the RAM PIXI reads (`$0BF0`-`$0BF4`). It moves
`$CE` on along a list longer than 256 bytes and puts it back when the loader returns (its
`JML` at `$02A826` leaves a return through `$02A82A` on the stack), leaves `$02A95B` and
`$02AA61` as the game has them, and keeps its state in `$0BC0`-`$0BD8`, which the game
does not use. With PIXI in a ROM (its `!EXLEVEL` code), PIXI's goal tape init replaces
Kobo's at `$01C089`, keeping `$14D4` whole and taking `$187B` from its own extra bits. `tests/lunar_magic_save.rs`
(`kobos_sprite_loader_spawns_as_lunar_magics`) plays seeded random lists, levels, ranges,
and camera paths on `e0` and on `e0` with Kobo's loader in place of Lunar Magic's and
requires the same slots, load indexes, flags, and `$0BF0`-`$0BF4` on every frame; 600
such scenarios passed by hand as well (2026-09-28).

Kaizo Kindergarten (3.03), ValuableAndBeautiful, and SMW_2022-4-9, with Kobo's loader
swapped in, play all 512 levels as the hacks do along both of `sprite_probe`'s paths
(right, and up and down). Extended Interactions is SA-1; Kobo's loader against Lunar
Magic's on SA-1 is `kobos_sprite_loader_spawns_as_lunar_magics_on_sa1` ("On an SA-1
ROM").
Taller levels: Luminescent (Lunar Magic 3.30), whose 24 levels with the new sprite system
are taller, plays every level the same with Kobo's loader swapped in along both paths but
one: in level `156` (298 rows, `TT` 1 with smart spawning) 3.30's loader erases a Sumo
Brother whose Y is `$130` below the camera's a frame before Kobo's does. Moved into a 3.70
ROM (`tools/lunar-magic/transfer`), the hack plays that level the same with 3.70's
loader and with Kobo's on nine routes through it, down, up, and across (2026-09-28), so
the difference is 3.30's. Kobo matches 3.70, the version its builds target, and does not
follow older versions' differences (accepted, maintainer, 2026-10-03). A level made taller by editing an MWL's size byte for Lunar Magic to import
never built its cache (`$0CF6`) or `$0BF0` in `sprite_probe` and spawned nothing, so it
was not used. `$02AA61` was not seen to matter.

### ExAnimation

Found by transferring global animations into `vanilla+LM` (`-TransferLevelGlobalExAnim`)
and diffing (addresses and jump targets only, `romdiff.py`, `sites.py`), writing chosen
lists into copies of that ROM and playing levels through the game loop and its NMI,
recording VRAM, CGRAM, and RAM after every frame (`examples/exanim_probe.rs`), ablating
single sites, and bisecting what Lunar Magic's save and export need (scripts that print
addresses only), 2026-09-28. The data format is the community's (smwspeedruns' level
data format, "ExAnimation Data"); the behaviour below is observed, and Lunar Magic's
help ("Edit Level ExAnimated Frames") names it.

| Site | Vanilla | Feature |
|---|---|---|
| `$0583AD` | `SEP #$30 : STZ $1933` at the start of `LoadLevel` | each level's load: counters, triggers |
| `$0095B5`, `$00A2A5`, `$00A5FD` | `JSL CODE_05BB39` (the game's tile animation) in the ending, the level, and the level's setup (`CODE_00A5F9`, eight times) | each frame's animation |
| `$00A390` | `REP #$20 : LDY #$80 : STY $2115`, the start of `CODE_00A390` (the NMI's animated tile upload and colour `$64` flashing) | the uploads |
| `$008A4E` | `REP #$30 : LDX #$1FFE` in `ClearMemory` | reset |
| `$00A5E1`-`$00A5E6` | `LDA #$01EF : MVN` (the level's palette copied for fades) | changed, not a jump; see below |
| `$03BCC0`-`$03BCCF` | vanilla `$FF` | the alternative files' pointers |

- Lunar Magic installs the piece on a save of a level that has ExAnimation (or a global
  transfer): the sites above and five RATS blocks, one of them its code with the level
  table's address at `+$EA` from the `JSL` target at `$0583AD`. **The check is a `JSL`
  (`$22`) at `$00A390`**, whatever follows it: with it, a save keeps every site as it is
  and reads and writes the tables through `read3(read3($0583AE) + $EA)` even when
  `$0583AD` holds the game's bytes (and then writes through whatever those bytes point
  at, in bank `$00` for the game's); a `JML`, `JSR`, `NOP`, or the game's bytes there, and
  a save of a level with ExAnimation installs Lunar Magic's own over every site, with new
  tables (the levels' lists written before are lost). The export reads the tables the same
  way, a `JSL` or a `JML` at `$0583AD` alike; with `$FFFFFF` at `+$EA` it exports no list.
- Tables: `$03FE00`, a byte per level, `PTLG----` (the game's colour `$64` flashing, the
  game's animated tiles, the level's list, the global list, each off when set); Lunar
  Magic's first save sets every level's to `$00` but level `104`'s to `$30`, and a save
  keeps it. `read3(T+$EA)` (T the target of the `JSL` at `$0583AD`): 512 3-byte pointers
  to the levels' lists, `$0000FF` for none (the middle byte 0), in a `$600`-byte RATS
  block. `T+$5B`: the global list's bank times `$100` as a word (0: none); `T+$65`: its
  low word. Each list is a RATS block of its own, exactly its length; a save that finds a
  level's list the same as another's points both at one ("with sharing"). `$03BCC0`: 3-byte
  pointers to the uncompressed ExGFX files `60`-`63`, `$000000` (or `$FFFFFF`) for none;
  each file is a RATS block of its own.
- The format: a list is the slots used (the highest plus one), the alternative file
  (0-3, for `60`-`63`), 16 bits of custom triggers that keep their state at the load, 16
  bits of custom triggers set at the load, 16 bits of manual triggers set at the load, a
  frame for each of those (lowest first), then a 16-bit offset per slot to its entry,
  counted from the first offset (0 for an unused slot). An entry is: type, trigger,
  frames less one, two bytes of destination, then 16-bit frames: twice as many for a
  trigger with a second set, none for the rotations. Triggers with a second set, as Lunar
  Magic reads a list (it exports what it read; a list imported with another count comes
  out cut): `01`-`05`, `07`, `09`-`0E`, `20`-`2F`; without: `00`, `06`, `08`, `0F`,
  `10`-`1F`, `30`-`4F`. Every corpus list's length by this matches its RATS tag.
- Types: `01`-`08` a line of 1-8 tiles, `09`-`0E` of 12, 16, 20, 24, 28, 32; `0F` one
  2bpp tile (16 bytes); `10` two tiles stacked, `11` 16x16 (two, and two `$100` words
  below), `12` 32x16 (four and four): a VRAM word address (bit 15: the source is in the
  alternative file), each frame a RAM address in bank `$7E` or an offset into the file.
  The AN2 file is at `$7EAD00` (up to `$1A00` bytes) when the frames run, so a level
  file writes frames there as offsets into it (`an2 = true`, `source::animation`).
  `13` colours to CGRAM, `14` also into the palette (`$0703`) and its fade copy (`$0905`),
  `15` the same but not while `$1493` (the level's end) is set, `16` the back area
  colour (`$0701` and `$0903`, which the game uploads itself), `17` the same, stopping at
  the level's end: the first colour, then the colours less one (bit 7: the alternative
  file); one colour is the frame's word itself, more are read from the frame's address;
  `16` and `17` take the frame as the colour whatever the count. `18`-`1B` rotate the
  colours in both copies right (`18`, `19`) or left (`1A`, `1B`), once every frames
  times; `18` and `1A` wait for their trigger if they have one, `19` and `1B` turn the
  other way while it holds. Types `14`, `15`, and the rotations upload from the fade copy,
  so they fade with the palette.
- Triggers: `00` none; `01` blue POW (`$14AD`), `02` silver POW (`$14AE`), `03` ON/OFF
  (`$14AF` set: the second set), `04` star (`$1490`), `05` time below 100 (`$0F31`, the
  hundreds, zero), `07` five dragon coins (`$1420` at least 5): while the condition holds,
  the second set; `06` and `08` the same conditions, running once to the last frame and
  staying there; `09`-`0E` behave as always on (reserved); `0F` shares the counter of the
  first slot of its group of eight (the help's "precision timer"; types `01`-`15` then
  upload bytes of Lunar Magic's own work RAM, not the frames, the same whatever the
  frames are, so builds refuse it there, kept so on review, 2026-10-04); `10`-`1F` manual (`$7FC070+n`); `20`-`2F` custom (bit `n` of
  `$7FC0FC`, 16 bits); `30`-`4F` one-shot (bit `n` of `$7FC0F8`, 32 bits).
- Each frame, where the game runs its own animation, but not twice for one value of `$14`
  (which stands still while the game is stopped: the game's own animation is then
  skipped too; `$7FC003` holds the last value), when a list with slots runs, slots `k`, `k+8`, `k+16`, `k+24` of the global list,
  then of the level's, `k` = `$14` mod 8, each move their counter (`$7FC080+k` level,
  `$7FC0A0+k` global; `$FF` after the load) and queue an upload; the NMI makes the
  game's animated tiles, then the queue in that order (a later slot over an earlier),
  then colour `$64`. The queue stays until the next frame of the animation, so an NMI
  without one makes it again: the one after the level's setup (a game frame whose `$14`
  has not moved empties it instead, so a palette the game uploads earlier in that NMI,
  a fade at the level's end, is not covered again), after the setup has
  uploaded the whole palette, which is how a CGRAM-only colour of the setup's last
  frames shows from the first frame. The setup runs the animation eight times, one for
  each `k`, so every slot shows its first frame when the level appears.
- Counters: a plain slot counts `0`..frames-1 and shows the counter, plus the frames
  with the trigger's second set; condition one-shots count while the condition holds and
  stop on the last frame; one-shots count from `$FF` while their bit is on (holding while
  it is off) and, past the last frame, go back to `$FF` and clear the bit, the last frame
  left shown; a manual slot does nothing while the manual value equals its counter, and
  otherwise takes the value (0 for a value past the last frame, again each time) and
  shows it. A rotation's counter counts from `$FF` up to the frames less one and turns
  the colours when it reaches or passes it, back to `$FF`. By trigger (2026-10-01,
  every type and trigger probed with `exanim_probe`): a trigger holds for a rotation
  when it is a condition with a second set that holds (`01`-`05`, `07`, the custom
  bits) or is `09`-`0F`; `06`, `08`, manual, and one-shot ones never hold. `18` and
  `1A` with a trigger turn only while it holds; `19` and `1B` turn the other way while
  it holds, and otherwise step as their trigger's counter would: `06` and `08` while the
  condition holds (on and on, as the turn sets the counter back), a one-shot while its
  bit is set (which then stays set). A manual trigger does nothing while its value
  equals the counter, and otherwise sets the counter to the value less one before the
  rest (so `18` and `1A` only take the value, and `19` and `1B` turn when it is past the
  last frame).
- At the load: every counter `$FF`, one-shot bits clear, `$7FC004` `$1F`, `$7FC019`
  `$FF`; custom bits `(old AND keep) OR set`, the global list's then the level's; manual
  values set as the lists say. `$7FC004` counts every eighth frame a list runs, mod `$20`
  (so the setup's first frame makes it 0); a list with no slots (Valuable and Beautiful's
  level `11F`, which only sets manual triggers) does not count. `$7FC019` holds the `$14`
  the lists last ran for. `$7FC0C0`-`$7FC0F7` changes on frames a list runs; Kobo keeps its
  own upload queue there, in its own layout.
- `$00A5E1`: with the game's bytes back there, only a rotation or a working palette
  across colour `$F7` differs, in the fade copy past it (`$0AF5` on), where the game's
  copy stops: Lunar Magic's copy of the palette, all 256 colours, is made before the
  setup's animation runs (Valuable and Beautiful's level `133`, whose palette has colours
  past `$F7`, rotates `$F1`-`$FC`). With no animation the RAM after a load is the same
  either way.
- The player's tile words (`$6060`-`$6187` in VRAM) come out different for a frame now
  and then, as with the VRAM patch: a Lunar Magic ROM leaves other values in direct
  page `$08`-`$09`, which the game reads later; Kobo's keeps the game's.

Builds (`build::Stage::Levels`, `write_animation`) put each list in a RATS block of its
own through the tables Kobo's code keeps, write a listed level's settings byte, and the
files `60`-`63` uncompressed, each a RATS block, with `$03FDFF` = `$00` from the patch.
Kaizo Kindergarten imported and built (its layer 3 settings taken out) reads back with
every level's list and settings as the hack's (`kobo diff`), plays its 74 levels with
lists as the hack does for 64 frames but for 11 the hack's own code changes, which differ
with ExAnimation off too, and after Lunar Magic saves the build every level reads back
the same and every one of the 74 draws the same but `152`, whose picture (like `000` and
`0AD`, which have no list) changes across the save in a build without ExAnimation too.

Kobo's (`asm/lunar-magic/exanimation.asm`) takes every site but `$00A5E1`, which it
leaves as the game has it, making the copy in its own hook at `$00A5FD` instead, and
keeps its state where Lunar Magic's layout has it (the table and pointers at the same
offsets from its hook's target, the documented RAM, `$7FC003` and `$7FC019`, and its queue at
`$7FC0C0`). Against Lunar Magic's, on `vanilla+LM` with global lists transferred from
SMW_2022-4-9 (examples/swap.rs exanim), random lists of every type and
trigger, global and level, random settings, starting trigger states, and trigger,
timer, coin, stop, pause, and level-end events over 120 frames give the same VRAM (but
the player's tiles), CGRAM, palette copies, and trigger RAM on every frame of 600
scenarios (`examples/exanim_probe.rs`; `tests/lunar_magic_exanimation.rs` plays such
scenarios too), and the corpus's 32 LoROM hacks with ExAnimation (Lunar Magic 2.30 to
3.51), with Kobo's code swapped in, play 1,332 levels (every level with a list, and every
16th with a global one) the same for 64 frames each (Invictus's level `136` breaks to
`BRK` with either).

### Per-level tables

Initialised for all 512 levels; a save rewrites the saved level's entry (observed, data).

- `$05DE00`-`$05DFFF`: all `$00` (fifth secondary header byte, `IWPXXtTT`).
- `$06FC00`-`$06FDFF`: all `$00` (`OFYYYYYY`); `$06FE00`-`$06FFFF`: all `$1A` (`RL-ooooo`,
  background height 27). `$06FA00` (`SHCvvvvv`, all `$20`: auto screen count) is written by
  every save, not once. Before 3.40 `$06FA00` is `$FF`; before 3.00 only `$05DE00` exists,
  as `IWPYX---`.
- The midway tables exist only once a separate midway entrance has been set (`$05D9E3`
  stays the game's `LSR`s in a fresh install); the secondary entrances' two further tables
  are made by the first save, 510 bytes each (entrances `1FE` and `1FF` read the next
  block's tag).
- `$0EF100`-`$0EF2FF`: sprite data banks, all `$07`. `$0EF300`-`$0EF30B`: code, the
  target of the hook at `$05D8F5` (`LDA #$07 : STA $D0`), which takes the bank from
  here. `$0EF30C`-`$0EF30F` stay `$FF` for PIXI. Kobo's (`asm/lunar-magic/sprite-banks.asm`)
  is at the same place, and a save keeps its hook, its code, and the table but the saved
  level's entry (checked on vanilla with the piece alone, comparing bytes only). A
  build that installs Kobo's layout puts a changed sprite list in a RATS block and its
  bank here; an unchanged one keeps its place in bank `$07`.
- `$0EF310` (flags, `bbBBVFCT`) is rewritten by every save for every level: `$08` (`V`)
  for a vanilla background, `$18` for one whose tiles take high byte 1 (vanilla's choice
  for data at or past `$0CE8FE`), `$00` for layer 2 objects.
  `$0EF600` (custom palettes) is not written; `$FF` fill means none.

### Game loop, stripe images, and the rest

- `$008072`: `JSR RunGameMode` in the game loop becomes `JMP $BA56`, into what was vanilla
  fill (`$00BA56`-`$00BA5C`); Kobo's VRAM patch writes the same jump and its own
  `JSR RunGameMode : JML` there ("Graphics" above). It belongs to the VRAM patch, with
  `LoadScrnImage`'s first instructions (`$0085D2`-`$0085DE`), which a save installs as a
  group ("Graphics" above); changes 3.70 says stripe uploads got faster. Not in any corpus
  ROM.
- `$00FFD7`: the ROM size byte, `$0A` for 1 MiB; it follows the expansion, and PIXI, GPS,
  and UberASM Tool read it (`$0D` means an SA-1 ROM over 4 MiB).
- `$0FF035`-`$0FF083`: `$D8` then zeros after a fresh install (`$0FF035` is rewritten by
  every save, above). AddmusicK pads `$0F8000`-`$0FF050` with `$55`.
- `$0FFFE6`: set to `$01`. `$0FFFE7`-`$0FFFFF` are Lunar Magic's settings, written by every
  save (`$0FFFEB`, compression, is documented in lunar-magic.md). Unknown.

### The overworld

Being found (roadmap step 4, from 2026-10-08), and built as it is: Kobo's code for the
pieces below is `asm/lunar-magic/overworld.asm`, and `overworld::Overworld::plan` writes
the tables. The method:

- `-TransferOverworld` is the only command that makes Lunar Magic write its overworld
  layout: transferring the vanilla ROM's own overworld into a copy of the vanilla ROM
  after one level save (`e0`, as above) changes only the layout, not the content, so the
  diff (`romdiff.py`, `sites.py` against the vanilla ROM) is the layout's sites. Changing
  one thing of the game's own format in the source first, and diffing two such
  transfers, shows where the layout keeps that thing.
- `tools/oracle/dump_overworld.sh` dumps what a ROM's overworld load leaves (RAM, VRAM,
  CGRAM on the first frame of game mode `$0E`, a new game started with `$0109` held at 0
  so it goes straight there), watching only RAM; `examples/ow_probe.rs` lists the RATS
  blocks a transfer added and matches them, raw or decompressed, against such a dump.
- For the vanilla overworld, the game's code and Lunar Magic's layout leave the same
  VRAM, CGRAM, layer 1 tiles (`$7EC800`), translevels (`$7ED000`), and layer 2 tilemap
  (`$7F4000`) (2026-10-08). RAM differs only in scratch: `$7EC100`-`$7EC7FF`,
  `$7F0534`-`$7F3FFF`, `$7F81A0`-`$7FC7FF` in part, `$010D`-`$010F`, and a cloud
  sprite's random place (`$0E4E`, `$0E7E`).

The layout's sites, from the vanilla overworld's transfer (2026-10-08). Besides the
graphics pieces above (`$009471`, `$00A140`, `$049DFD`, `$0583B8`, the lists at
`$0FF15C` on), it is:

- Hooks (a `JSL` over the game's instructions): `$048509` (`LDY PlayerTurnLvl : LDA
  OWPlayerSubmap,Y`), `$048566` (`XBA : AND #$000F`), `$048E81` and `$049549` (`ASL : TAX
  : LDA LevelNames,X : STA $00 : JSR CODE_049D07`, the level name), `$048F8A` (`LDX #$07 :
  LDA OverworldLayer1Tile`), `$049199` (`CMP #$81 : BEQ`), `$04DCA5` (`INC $0F : LDA $0F :
  CMP #$6F`, the load's event loop), `$04E6C5` (`CLC : ADC #$0010`), `$04E9F7` (`BEQ : DEX
  : BPL`), `$04EDDD` (`ASL : ASL : ASL : TAY` in `CODE_04ED83`, layer 1's event change),
  `$04EEC3` (`AND #$00FF : ASL : TAX : LDA.l DATA_04E587,X`), `$04EEF1` (`CLC : ADC
  #$0020`), `$05B1A3` (`LDX #$16 : LDY #$01 : LDA DATA_05A590,X`), `$05D8B1` (`BEQ : LDA
  #$01`, in the level load). Targets at `$03BA10`-`$03BFFF`, `$05DCD0`-`$05DDFF`, and in
  RATS blocks.
- Rewritten in place: `$04D7F9`-`$04D838` (the translevel scan, `CODE_04D7F2`) and
  `$04EF27`-`$04EF3A` (the crushed tiles, `StructureCrushTile`).
- The game's instructions with new operands, tables moved or grown: layer 2's streams
  (`$04DC72`, `$04DC8D`: `OWTileNumbers` to a block of `$2F28` bytes at `$10C4B4`, still
  the game's run-length format, `CODE_04DABA`), `OWEventTileProp` (`$04DD45`),
  `OWEventTileNum` (`$04EAF5`, `$04E4AF`, `$04E4BA`), `DATA_04D85D` (`$04DA74`, `$04EC8C`,
  `$04ECBA`, `$04ECC5`, `$04ED97`, `$04EDBD`), `DATA_04D93D` (`$04EDB7`), `DATA_04DD8D`
  and `DATA_04DD8F` (`$04E49F`, `$04E709`, `$04EE3F`, `$04EE5A`), `DATA_04E5D6`
  (`$04E67C`), `DATA_04E5B6` (`$04E69C`), and the event count `$6F` (`$04D859`,
  `$04DA98`); data edits at `$048E49`, `$04A04C`, and `$04F644`.
- 23 RATS blocks (the ExGFX block among them).

What the layout keeps where (`kobo_core::overworld` reads it; tests/overworld.rs checks
it against the ROM's own load in every corpus hack that reaches an overworld):

- Layer 1's tile numbers stay at `$0CF7DF`; their high bytes (the page, 0 or 1, in RAM at
  `$7FC800`) are a `$800`-byte table, LC_LZ2, whose pointer is the low word at `$04D822` and the bank at
  `$04D827`.
- Translevels and directions are not numbered by the load as the game does but stored:
  one LC_LZ2 table of `$1000` bytes, the translevel of each layer 1 place (`$7ED000`) then
  its direction byte (`$7ED800`); pointer low word `$04D803`, bank `$04D808`.
- Layer 2 keeps the game's two run-length streams, moved into one block (the game's
  operands at `$04DC72`, `$04DC79`, `$04DC8D`).
- Level names: `$60` names of 19 tiles, the game's characters padded with `$1F`, in a
  block of `$720` bytes whose 24-bit pointer is at `$03BB57`.
- Events: `$78` of them (the operand at `$04D859`); each table the game's format, moved
  and grown, through the game's operands: layer 1 places (`$04DA74`), their VRAM (`$04EDB8`),
  the event-tile entries (`$04E49F`: the tile data offset, `$900` on for a 2x2 block, and
  the layer 2 place, 4 bytes each), the tile numbers (`$04EAF5`, raw), the properties
  (`$04DD45`, bank `$04DD4A`, the game's run-length format to a `$FFFF` word), and the
  crushed tiles, 24 (`$04E67C` events, `$04E69C` places, `$04EEC9` VRAM). The ranges of
  entries per event stay at `DATA_04E359`, and the reveal list at `DATA_04DA1D` and
  `DATA_04DA33`. The game's `BNE` at `$04DA98` is a `BRA`.
- The pointers only Lunar Magic's code reads were found by moving their tables (a
  transfer whose layer 2 is longer moves everything after it) and keeping the bytes whose
  change matched the move; the translevel pointer holds in every Lunar Magic 3 hack of the
  corpus, the page table's not in hacks of older versions (Kaizo Mario 1 and 2, Smb2dx),
  which have one page.
- The events' further tiles (smw.md, "The overworld": the game's list of 44, which
  Lunar Magic leaves in place unread) are four tables, each a RATS block, whose 24-bit
  pointers are at fixed offsets from the target of the `JSL` at `$04E9F7` (5 bytes, to
  `$04E9FB`; the game goes on at `$04E9FC`): `+$0D` the ranges (`$79` words, each
  event's first entry and the end of the last, in bytes of the word tables), `+$22` each
  entry's data, `+$28` its place, `+$34` its kind (a byte, bit 0 set for layer 2). An
  event's entries are in the order they are made, the game's list's last entry first. A
  layer 1 entry's data is the whole tile, its page in the high byte, which the load
  writes to `$7FC800` too. Found 2026-10-08: a transfer from a source whose list had one
  entry changed moved only two blocks, whose new addresses were the bytes at `$10FF52`
  and `$10FF64` of that ROM, `+$22`, `+$28`, and `+$34` from the `JSL`'s target there;
  the four offsets hold, with the opcode before each `$BF` (`LDA.l ,X`), in every Lunar
  Magic hack of the corpus, whatever bank (`$10`, `$90`, the SA-1's `$C0` on) their
  blocks are in.
- When they are made, from the memory effects of Lunar Magic-saved ROMs with events
  passed (`expand::load_overworld_passed`; editing `self.smc`'s tables to make two
  overlap): the layer 2 load makes each passed event's layer 2 entries and then its layer
  2 further tiles, event by event (`80_Fellipe R`'s event 0 blocks are under event 6's
  own entry), from `$04DCA5`, a `JSL` with the loop's branch after it; the overworld's
  load makes the layer 1 ones only, and an event's end in play (`expand::end_event`) all
  of them. The vanilla overworld's transfer loads as the game does in all three, every
  event alone and all together (2026-10-08). Kobo's code does the same, at the same two
  places, with its pointers at the four offsets.
- Level names: a hook at `$048E81` and one at `$049549` (10 bytes each, over `ASL : TAX :
  LDA LevelNames,X : STA $00 : JSR CODE_049D07`, entered with the translevel in a 16-bit
  A) put the name from the names table (`$03BB57`) in the stripe image as the game's
  routine does: 19 tiles at VRAM `$508B`, properties `$39`, the image grown by `$2A`
  bytes. The vanilla overworld's transfer and Kobo's build leave the same stripe and VRAM;
  so do Lunar Magic's transfer of Luminescent's overworld into a clean ROM and Kobo's
  build of it, where the hack itself draws at `$5088` (2026-10-08). The corpus hacks
  whose names come out otherwise draw them with code of their own: two lines from VRAM
  `$506B`, 28 tiles from `$5062`, or elsewhere (known_failures.toml).
- The level a translevel enters: the hook at `$05D8B1` (4 bytes, over the game's `BEQ :
  LDA #$01`, the submap in A, the level number's high byte left in A) takes it from the
  translevel, `$1xx` from `$25` on, on either map, unless the overworld override
  (`$0109`) names the level, which keeps the game's rule (by the submap). From the
  memory effects of `self.smc` and the corpus hacks entered from a place with each
  translevel and each bit of its direction byte, and through `$0109` with
  `TranslevelNo` set or not (`examples/ow_probe.rs`, `enter` and `enter2`, 2026-10-08):
  the direction byte and `TranslevelNo` change nothing through `$0109`. Kobo's code
  does the same (tests/overworld.rs); for the vanilla overworld both rules agree.
- Where a new game starts (the players' submaps and positions, `InitPlayerOverworldData`
  at `$009EF0`, and the 8 level tiles it opens, `InitLevelTileMovementData` at
  `$009EE0`) stays in the game's tables, which Lunar Magic's overworld editor changes in
  place; most corpus hacks start elsewhere than the game.
- A transfer also changes the byte at `$0FF092` (0 in the vanilla ROM, `$AC` after the
  vanilla overworld's transfer, `$A7` after one with that list entry changed), outside
  any RATS block; not looked into yet.


## On an SA-1 ROM

Found 2026-10-01 on the SA-1 reference ROM ([sa1.md](sa1.md): vanilla with SA-1 Pack
1.40 applied), with the same spike as on vanilla: `-ImportLevel` of level `105`'s own MWL
on a copy, then `romdiff.py` of each ROM against its saved copy and the two reports
compared, and `ramdiff.py --summary` (`--sa1` for I-RAM and BW-RAM too) over all 512
levels of each.

- The one-time set is the same: every hook site of the vanilla install is changed on the
  SA-1 ROM too, with the same kind of instruction (`JSL`, `JML`), and the same ranges
  (observed). Hook targets and the code behind them differ, as they would wherever
  Lunar Magic's blocks land; what the code does differently was not looked at (clean
  room). `$00FFD7` stays as SA-1 Pack set it (the image is already 1 MiB).
- What a level load leaves is the same as on vanilla once SA-1 Pack's RAM map is applied
  (`ram::RamMap::Sa1Pack`): the variables of `$0100`-`$1FFF` in BW-RAM (`$40010B`, the
  level number), the pointers to screen starts at `$0BF6` into `$40C800`/`$41C800`
  instead of `$7EC800`/`$7FC800`, the upload buffer pointer at `$06B5` as `$7CE8`
  (`$1CE8` in the BW-RAM window). Lunar Magic's own work RAM stays in work RAM, where
  only the S-CPU reaches it: the level flags at `$7FC00B`, the background's Map16
  addresses at `$7FBC00` and `$7FC300`, `$7F8183`-`$7F819F`. Nothing else differs in
  I-RAM or BW-RAM but the SA-1's stack (`$3700`-`$37FF`). The summaries over all 512
  levels differ only where SA-1 Pack moves sprites and objects itself (OAM, sprite
  slots).
- So Kobo's code for an SA-1 ROM is the same code with SA-1 Pack's addresses for the
  game's variables (`asm/lunar-magic/memory.asm`), and with whatever the SA-1 runs kept
  away from work RAM and the PPU. SA-1 Pack runs the level's object loader
  (`LoadLevelData`), the sprite loader, sprites, and parts of the player's code on the
  SA-1 (`boost/`); hooks inside those run there.
- The conditional Direct Map16 flags stay at `$7FC060` in work RAM: on a Lunar
  Magic-saved SA-1 ROM, `$FF` there before the load changes the conditional objects'
  tiles, and `$FF` at `$40C060`, `$41C060`, `$3060`, `$6060`, `$400060`, `$418060`, or
  `$7EC060` changes nothing (`lm_objects flags`). Lunar Magic's help says the same of the
  table, and its changes for 3.11 that it fixed SA-1 ROMs' objects reading it. Kobo's
  object code runs on the SA-1 there, so it has the S-CPU read the flag's byte through
  SA-1 Pack's call from the SA-1 (`%call_scpu`, `memory.asm`).
- Lunar Magic's help ("Use SA-1 RAM Remap for this ROM") lists the remap its code
  follows: the direct page to `$3000`, `$0100`-`$1FFF` to `$6100`-`$7FFF`, and the tile
  grid to `$40C800` and `$41C800`, which is `ram::RamMap::Sa1Pack`'s. It detects SA-1
  Pack on the first open and locks the setting on the first save.

Kobo's code on SA-1 (2026-10-01): every patch takes the game's variables through
`memory.asm`, in the convention PIXI, GPS, and UberASM Tool code is written in
(`$010B|!addr`, `$000E|!dp`, `!9E`), not through defines generated from `ram::RamMap`, so
that Kobo's patches read like the tools' (kept so on review, 2026-10-04);
`tests/install.rs` checks each against `RamMap::resolve`. A define puts the address last
(`!addr|$0BF6`) so that `!name+2` means the same in either order Asar evaluates, and an
immediate built from a define is sized (`LDA.b`). So a patch's LoROM output is the same
but for work RAM's bank byte (`$7E` for `$00`, the same memory through bank `$00`'s
mirror) and code that moved. What differs
beyond the addresses:

- What runs on the SA-1 and needs work RAM goes through the S-CPU: only the conditional
  flags. Everything else that touches work RAM or the PPU (the background, graphics,
  layer 3's setup, ExAnimation, the VRAM patch) runs on the S-CPU under SA-1 Pack as it
  does on LoROM; the emulator reports any SA-1 access to them, and none is reported.
- The uploads in the NMI or with the screen off use DMA channel 2 under SA-1 Pack, which
  keeps channel 1 for HDMA and moves the game's own uploads to 2 (`remap/dma.asm`), and
  read their buffers from bank `$00` as SA-1 Pack's do.
- The sprite loader works with SA-1 Pack's own changes to the game's loop, which run it
  with 16-bit index registers and keep the entry's offset on the stack across a spawn
  (`boost/sprite_load.asm`): the loop head folds Y into `$CE` whatever its width, Kobo
  hands a sprite to the game's code at `$02A856` with 16-bit indexes, and after a spawn
  (`$02A9D7`) drops the two bytes SA-1 Pack pushed, as its own way out at `$02A9DA`
  does. `$02ABF3` is left alone: SA-1 Pack jumps from `$02ABF2` to clear its 255 flags.
  With PIXI on SA-1, PIXI's jump at `$02A9D7` replaces Kobo's, so the goal tape's extra
  bits do not reach `$187B` through Kobo's code; Lunar Magic's loader is bypassed the
  same way there.
- Checked: all 512 levels of the SA-1 reference ROM draw with the twelve base patches
  as without them, or as the LoROM build does where the patches change a picture there
  (`012`, `0F8`, `101`). Those three are Lunar Magic 3.70's: the vanilla ROM saved once by
  Lunar Magic draws them as the build does, and not as vanilla (`012`'s first camera and
  `0F8`'s layer 2 offset are 3.70's, "The sites a save keeps with the marker"), and in
  Mesen the save's `0F8` and the build's agree but for the differences listed under
  "Graphics" (2026-10-05); `tests/install.rs`, the build tests, and the Lunar Magic save
  checks run on both; Kobo's loader against Lunar Magic's SA-1 loader in 324 seeded
  scenarios, and Kobo's ExAnimation against Lunar Magic's SA-1 code in 316
  (docs/testing.md).

## RAM after a level load

What a level load leaves in a Lunar Magic-saved vanilla ROM and not in vanilla, over all
512 levels (`ramdiff.py --summary`). The last column says which piece leaves it, where an
ablation showed it. Kobo's one-time code must leave the same wherever Lunar Magic's
restorable code, or anything else, reads it; which reads exist is the main open question.

| RAM | Levels | Value | From |
|---|---|---|---|
| `$0BF6`-`$0C55` | all | 32 3-byte pointers to screen starts in `$7EC800`: `$7EC800 + n*rows*16` | `LoadBlkPtrs` entries point here |
| `$0C56`-`$0CB5` | all | the same into `$7FC800` | |
| `$0CB6`-`$0CD5`, `$0CD6`-`$0CF5` | horizontal | low and high bytes of the same offsets (vertical levels: 0) | replaces `DATA_00BA60`/`BA9C` |
| `$0CF6`-`$0D35` | all | per screen, a 16-bit address in the sprite list (bank `$CE`), 3 bytes on per sprite | |
| from `$0D37` | all | per screen, 16-bit, the number of sprites on the screens before it | |
| `$0BE7`, `$0BEE`-`$0BF5` | all | `$40` in most levels, `$00` in some (`1A`); `FF FF 30 FF C0 01 00 00` | `$0BF0`-`$0BF4`: the sprite loader's range and `tTT` ("Sprites"); `$0BF5`: the size byte ("Taller levels"); `$0BE7` bit 6: no layer 2 interaction yet ("The sites a save keeps with the marker") |
| `$13D7`-`$13D8` | all | level height in pixels (`$01B0`, `$0100` vertical) | hook `$05D9A1` ("Taller levels") |
| `$1936`-`$1937` | all | height minus `$10`; `$01A0` vertical | hook `$05D9A1` |
| `$13CD` | all | see midway points; cleared by game mode `$11` | `$009708` |
| `$010B`-`$010C`, `$00FE`-`$00FF` | all | level number; level number + 1 | hook `$05D8E2` |
| `$7FC00B` | all | the level's `$0EF310` flags | hook `$05803B` |
| `$7EB900`/`$7EBD00` | backgrounds | tilemap low and high bytes | hook `$05803B` |
| `$7FBC00`-`$7FBF5F`, `$7FC300`-`$7FC65F` | backgrounds | per background cell (both halves, 16x27), the 16-bit address of its Map16 definition in the BG Map16 bank | hook `$05803B` |
| `$05`-`$06`, `$0A`-`$0C` | backgrounds | stride and BG Map16 table | hook `$058DA4` |
| `$1BE6`-`$1DE7`, `$0695`-`$06B6`, `$7F819F` | most | upload buffers and state | the `$0580BF` retargets |
| `$7F8183`-`$7F819F` | all | `$FF` x16 then small values (vanilla: unused) | |

`$0BF6`-`$0D75` is vanilla's `GfxDecompSP1`, the buffer for sprite tiles `4A`-`4F` and
`5A`-`5F`; the VRAM patch moves those tiles (changes 3.60 mentions them). `$6B`-`$70`
(`Map16LowPtr`/`HighPtr`) also differ, as scratch.

## Unknowns, and how to find them

- Whether anything outside the one-time code reads the RAM above: Lunar Magic's
  restorable hooks (the VRAM patch at `$00F6E4` above all) or community patches.
  Method: build Kobo's one-time set, let Lunar Magic save the build (it adds its
  restorable hooks), and compare RAM after every level load, and pictures, with a Lunar
  Magic-saved vanilla ROM (`ramdiff.py`, `render_hashes`); a mismatch names the address.
  Done for the restorable hooks (2026-10-03, a vanilla project with one Map16 page
  built and saved): all 512 pictures are the same, and what RAM differs after the load
  is each piece's own: Kobo's sprite loader keeps its state in `$0BC0`-`$0BD8` and
  leaves Lunar Magic's cache at `$0CF6`-`$0D75` and `$0BEE` as they were; `$00FE`-`$00FF`
  are cleared by Kobo's palette hook, which every build in Lunar Magic's layout has and
  which Lunar Magic's save of vanilla does not install; `$65`-`$66` and `$CE`-`$CF` point
  at level data that sits elsewhere; and direct page scratch. Community patches remain.
- Behaviour that a level load does not exercise: the overworld (`$04DCFA`, `$04E5F1`),
  which builds leave as the game has it (roadmap step 4). The rest has been played
  against Lunar Magic's code since: block contact ("Custom block actions"), scrolling
  and screen shake ("Taller levels", "Graphics"), Choc Island 2's rooms, midway points,
  goal tapes ("Sprites"), the bonus and Yoshi wings exits ("Entrances"), and tiles
  changed in play (2026-10-02: coins and blocks met by the player carried along rows of
  `101`-`107` and up columns of the vertical levels `0C2` and `12A`, with every piece of
  Kobo's in place of Lunar Magic's, leave the same tile planes and tilemaps on every
  frame).
- `$0BEE`-`$0BF5`, `$7F8183`-`$7F819F`, `$0FF035`, `$0FFFE6`: values
  observed, meaning not. Method: vary one level property at a time (height, layer 2,
  midway settings) with Lunar Magic's command line on a copy and diff RAM after load.
- What block tools other than GPS check in the help file's custom block slots, which
  3.70 does not run ("The ranges", acts-like above). Method: tool sources (Block Tool
  Super Deluxe, if any is published).
- Older versions: what 1.6x-2.x put at each piece (the corpus comparison only says equal
  to 3.70, vanilla, or other), and what makes 3.70 upgrade an older install. Kobo reads
  many versions but writes 3.70's layout, so this matters only for import.
- GUI operations (overworld save, ExAnimation, custom palettes, VRAM patch options) were
  not tried; they may write inside this set. They matter most of all the unknowns: a
  build leaves the overworld, the title screen, the credits, and messages for Lunar
  Magic's GUI to finish ([build.md](build.md#what-is-left-to-lunar-magic)), and every check
  so far ran its command line. Deferred by the maintainer at step 2's close (2026-10-05),
  to be done by hand. Method: build a project that uses every piece (Kaizo Kindergarten
  imported, or the RHR template); in Lunar Magic's GUI under Wine, on a copy, edit and
  save the overworld (a tile, a level tile's number, a path), a message, the title
  screen's demo, and the credits, then each of the other operations above; after each
  save, `kobo diff` of the build and the saved copy must show only the edit (and what
  any save changes, lunar-magic.md), `romdiff.py` which ranges of Kobo's install it
  touched, and `install-gate.py` whether a piece of Kobo's was replaced.
