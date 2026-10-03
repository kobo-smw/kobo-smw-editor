# Marked for review

Decisions taken without the maintainer, to be reviewed in a batch. Step 2's remaining work
runs unattended (from 2026-09-27): each piece merges once CI and the local ROM, corpus, and
Lunar Magic checks pass, and review comes after. Anything a piece could not settle on its
own goes here instead of blocking it: a setting builds refuse because its behaviour could
not be found by observation, a judgement call between two defensible designs, a check that
was weakened or skipped, anything that needs a test ROM from Lunar Magic's GUI.

One entry per item, newest last, removed once reviewed (the decision then lives in the
doc it belongs to). Each says what was decided, why, where it is recorded, and what would
settle it.

## Open

- **Builds refuse layer 3's tilemap of size 3 and files too large for a slot**
  (2026-09-28, updated 2026-09-30). LT3's `DD` = 1 is settled (the whole file from
  `$5000`, as Kobo's loader had it) and builds take it; size 3 (`FF` = 3), which Lunar
  Magic's dialog does not offer and no corpus ROM uses, loads other than size 0 under
  Lunar Magic's loader (layer 3's tiles at `$4000`-`$4FFF` end up otherwise), so it is
  refused. A file over `$1000` bytes (`$1A00` for AN2, `$2000` for LT3) was not tried
  under Lunar Magic's loader, and runs over the RAM after the buffer. A file `32`-`7E`
  in a list is refused too, and objects `24` and `25` on layer 2, which neither loader
  reads. Settles it: levels made for each.
- **A build that uses any of Lunar Magic's graphics formats stores every GFX file as
  4bpp** (2026-09-28, `build::write_gfx`). Lunar Magic's check is one byte for all files,
  so once `graphics.asm` is in, every file the game keeps as 3bpp is converted as Lunar
  Magic 3.70 converts it, into free space (about 135 KiB compressed, 8 KiB more than
  the game's files, whose old copies stay), and the project's own GFX files take 16
  colours. `[graphics] bpp = 4` asks for it without ExGFX or lists. Settles it: whether
  a project should be able to keep 3bpp files with ExGFX (Lunar Magic's "use 4bpp"
  option off), which would need Kobo's code for the 3bpp case.
- **Project format for Lunar Magic's graphics** (2026-09-28, `source::project`,
  `source::level`). `[exgfx]` lists ExGFX files as indexed PNGs (4bpp, or
  `{ file, bpp = 2 }`) or `.bin` bytes; import writes a PNG when every list using the
  file loads it at one depth (4 for FG/BG/SP and the older lists, 2 for LG) and it is
  whole rows of 16 tiles, and bytes otherwise (layer 3 tilemaps, AN2 files).
  `[bypass_lists]` holds the older lists, first file first. A level's `[graphics]` names
  every slot, with `bypass`, `layer3_files`, `layer3_tilemap`, and `tilemap` (LT3's
  nibble) apart, and the layer 3 settings in `[graphics.layer3]`; it is written only when the list changes
  what the level loads. Settles it: review of the names and of PNG versus bytes.
- **Builds write the submaps' lists but have no overworld graphics code** (2026-09-28).
  Lists `200`-`206` are written as Lunar Magic 3.70 writes them (the overworld's own
  files), so its editor and save find them; Kobo leaves the overworld's, cutscenes', and
  credits' loading as the game has it, where Lunar Magic's reads those lists. Settles it:
  overworld work (roadmap step 4).
- **ExAnimation: only trigger `0F` on types `01`-`15` is refused** (2026-10-01,
  `exanimation::refusal`; replaces the 2026-09-30 entry that refused `09`-`0F` and
  rotations with manual, one-shot, or once-only triggers). Each was probed against Lunar
  Magic's code and Kobo's made to match (lunar-magic-install.md, "ExAnimation"); the
  random comparison (`tests/lunar_magic_exanimation.rs`) now draws them too and passes
  800 cases on both mappings. `0F` on a type that uploads frames uploads bytes of Lunar
  Magic's own work RAM instead, which no other code can give, so it stays refused; no
  corpus list uses it. Settles it: a hack that does.
- **ExAnimation project format** (2026-09-30, `source::animation`). A level's
  `[animation]` has the settings as four booleans (written only when not what a build
  gives the level: all on, level `104`'s lists off), the list's header as optional keys,
  and one slot per line: `vram` for tiles, `colour`/`colours` for colours, `delay` for a
  rotation, `frames` and, for a trigger with a second set, `triggered`, as hex words. The
  global list is its own file (`[animation] global` in the manifest), and the
  uncompressed ExGFX `60`-`63` are `.bin` files beside it (`[animation] 0x60`), not in
  `[exgfx]`, since they are not compressed or loaded into slots. Settles it: review of the
  names and of keeping `60`-`63` apart from `[exgfx]`.
- **A rendered level shows every slot's first frame** (2026-09-30). The level's setup
  runs the animation once for each of the eight phases, so a picture has what the level
  shows as it appears; there is no option to render a later frame. Settles it: a GUI need
  for it, which would add a frame count to `RenderOptions` and run the game loop that
  many frames before drawing (`expand::play_game_loop` has the loop).
- **Choc Island 2's rooms get their banks from Kobo's code** (2026-10-01,
  `choc-island.asm`). A build writes listed levels to free space, and the rooms take
  their 16-bit pointers' banks from the level the exit led to (`0CD`-`0CF`), so a build
  that wrote one of them crashed the game in the rooms. Lunar Magic's own fix (3.50,
  `$05DB5B`) installs with a save; Kobo's hooks the pointer load before it (`$05DB4B`)
  and sets the banks the game's data has, in any build that writes `0CD`-`0CF`, which
  then needs Asar. A Lunar Magic save adds its own beside Kobo's and the rooms load the
  same (Mesen, all three levels). Settles it: whether to install it only with Lunar
  Magic's layout, or to keep those levels' data in bank `$06`.
- **Taller levels: one entrance corner a pixel off** (2026-09-28). With a relative camera,
  size `1E` (15 rows) with `B` at the slowest layer 2 rate places the background one
  pixel lower than Lunar Magic's; every other size, rate, and entrance byte tried gives
  the same RAM. Settles it: more data points on sizes shorter than the screen, if anyone
  uses them.
- **Layer 2 scroll settings 8 to 11, `S`, and `H` build, and a save keeps Kobo's camera**
  (2026-10-01, `entrance.asm`, lunar-magic-install.md "Layer 2 scroll settings"; replaces
  the 2026-09-28 entry that refused them). Kobo's camera and entrance give the same RAM
  as Lunar Magic's in every case tried. Decided without review:
  - `"LM"` at `$05DD7C`, the bytes Lunar Magic's save checks before it keeps `S` and `H`.
    Without them a save drops every level's separate settings; with them it also leaves
    Kobo's entrance code and camera in place, and whatever is at eight more sites where
    it would install its own. Since 2026-10-02 builds have Kobo's code at the five whose
    Lunar Magic code changes anything (face left at `$009708` and `$00D2B2`, the end
    fade's colours at `$00AF72`, layer 2's interaction a frame behind at `$00E966`, a
    vertical level's camera at `$00F77B`), each playing as Lunar Magic's on every frame
    tried, and the game's at three where nothing different was seen (`$00F871`,
    `$05BCA5`, `$05D7BA`; lunar-magic-install.md, "The sites a save keeps with the
    marker"). Settles it: a difference at one of those three along a path not tried.
  - A moving setting steps from game mode `$13` on, which matches Lunar Magic's step count
    before a level's first frame; what decides it is not known.
  - In the level file, `layer2_scroll` is the horizontal setting when
    `layer2_vertical_scroll` is there, up to `$1F` (`H`); past 15 without it is refused.
  Settles it: a hack with these settings built and played against itself (the corpus
  sweep), or a difference along a path not tried (layer 2 objects, a scrolling sprite
  command, a level with both a moving setting and a relative camera far from its start).
- **Lunar Magic's layer 3 settings: what builds refuse, and the project format**
  (2026-09-30, `build::check_layer3`, `layer3.asm`, docs/lunar-magic-install.md "Layer 3
  settings"). Kobo's code does what Lunar Magic's was seen to do for `B` and its scroll
  settings, `X`, `Y`, `C`, `S`, `I`, and `O`, and matches it frame by frame. Builds refuse
  AN2's bit 12 (`unknown`; no effect seen, no corpus ROM sets it); `tides_act_as` and
  `advanced` in tide levels build since 2026-10-01 (next entry). Scroll settings `12`-`17` and `1B`-`1F`,
  which the dialog does not offer, build: they moved as `01` in every test. Level files
  hold the settings as `[graphics.layer3]` (`advanced`, `horizontal`, `vertical` with
  names from `names.toml`, `x` in tiles, `y` signed, `cgadsub`, `subscreen`, `sync_fix`,
  `sprites_air`, `tides_act_as`, `unknown`); a slot of `$FFFF` keeps its word, whose
  nibble Lunar Magic's code (and Kobo's) reads as `F`, so settings needing another value
  there are refused. Import leaves out the settings of a ROM without Lunar Magic's layer 3
  code (nothing reads them there), but for a tide level's AAAA, which its taller levels
  code reads. Settles it: a ROM that sets AN2's bit 12.
- **Tides: what they act like, and `advanced` with one** (2026-10-01, `layer3.asm`
  `tide_tiles` and `tide_offsets`, `build::check_layer3`, docs/lunar-magic-install.md
  "Layer 3 settings"). `tides_act_as` fills the tide's rows with its tiles as Lunar
  Magic 3.30-3.33 hacks were seen to (every value, Kobo's code in their place plays the
  same); Lunar Magic does it in its taller levels code, Kobo in its layer 3 code, in the
  rows Kobo's taller levels code fills with water at every size, by the rule found
  (2026-10-02, lunar-magic-install.md "Layer 3 settings"). `advanced` with a tide builds
  with Kobo's code computing the tide's interaction offsets from the frame's positions,
  and since 2026-10-02 with Kobo's code after the player's collision with layer 2
  (`$00E966`), which makes them trail a frame as Lunar Magic's do: Super Dram World 2
  v1.3's level `0D3` plays as the hack on every frame tried. Refused: `advanced` with a
  tide in a vertical level, which only Lunar Magic's code supports. Settles it: the 7
  corpus levels moved into a 3.70 ROM and played again (before the `$00E966` code they
  played the same but where a sprite meets the tide's end, and on the first frame).
- **SA-1: the patches name SA-1 Pack's addresses through Asar defines, not ones made from
  `ram::RamMap`** (2026-10-01, `asm/lunar-magic/memory.asm`). The plan had each patch's
  RAM "as defines through `ram::RamMap`'s SA-1 map". The patches use the community's
  convention instead (`$010B|!addr`, `$000E|!dp`, the sprite tables as `!9E`, `!14D4`),
  which PIXI, GPS, and UberASM Tool code is written in, with one file choosing the values
  by the ROM's map mode; `tests/install.rs` checks them against `RamMap::resolve`. A
  define puts the address last (`!addr|$0BF6`) so that `!name+2` means the same in
  either order Asar evaluates; an immediate built from a define is sized (`LDA.b`), since
  Asar sizes it from its literals (`$0000` on LoROM made one 16-bit). Settles it: whether
  Rust should generate the defines instead, which would keep one list but make the
  patches unlike the tools'.
- **SA-1: only SA-1 Pack 1.40 is supported** (2026-10-01, decided with the maintainer).
  Builds apply the configured SA-1 Pack, which the tests take to be 1.40; Kobo's sprite
  loader follows 1.40's changes to the game's loop. Super Diagonal Mario 2 has an older
  SA-1 Pack (no marker at `$0084C0`) with Lunar Magic 2.52; it imports, but Kobo's code
  swapped into it is not expected to match. Settles it: a hack built on an older SA-1
  Pack that someone wants to build with Kobo.
- **A screen exit in Lunar Magic's format writes its whole destination** (2026-10-02,
  `source::level`). Long screen exits name entrances up to `1FFF`, so an exit with
  `lm_format` now has `dest` as the whole number (`dest = 0x320`), where it was the low
  byte with `high = true` for bit 8; `high` stays for the game's format, whose bit 0 the
  object keeps, and is refused beside `lm_format`. Exits in the game's format are
  unchanged. No committed project had `high`. Settles it: the maintainer's taste.
- **Builds move the entrance tables to `$2000` entrances, not to the last one in use**
  (2026-10-02, `entrances.asm`). A project with an entrance or a long exit past `1FF`
  gets six tables of `$2000` (48 KiB), whatever its highest entrance; Lunar Magic sizes
  them to the last entrance in use, and its save re-allocates Kobo's so. Sizing them to
  the project would need the patch to take the size as a define. Settles it: whether
  the space matters.
- **A graphics list may name an ExGFX file the project does not have** (2026-10-02,
  `build::check_graphics_list`). The slot then loads nothing, as a slot naming a file the
  ROM lacks does under Lunar Magic's code (the same VRAM as `7F`) and Kobo's. Hacks have
  such lists (nine corpus hacks), so a build writes them as they are, where it refused
  them before; an import notes each, but a hand-written project that names a file it
  forgot to list is no longer stopped. Settles it: whether a build should refuse what
  Lunar Magic's editor allows.
- **Kobo's acts-like code runs the help file's custom block slots, which Lunar Magic 3.70
  does not** (2026-10-02, `actslike.asm`, lunar-magic-install.md "The ranges"). Every
  documented slot (`$06F890`-`$06F9FF`) holds `NOP`s and a `JMP $F602` in a Kobo build,
  so a `JSL` written there runs; 3.70 ran none written at the head, sprite, cape,
  fireball, and tongue slots, and has its own code over parts of the area. With the
  slots empty the two play the same. Settles it: whether a build should leave them
  unrun as 3.70 does, which a tool written for older Lunar Magic versions would then
  find not working, as under 3.70.
- **Exits without `u` take bit 8 from the translevel, not the submap** (2026-10-02,
  `exits.asm`, lunar-magic-install.md "Entrances, exits, and midway points"; replaces
  the earlier choice to keep the game's rule there). Lunar Magic's code does so, and its
  save puts that code at `$05D7CE` over Kobo's, so with the game's rule a saved build
  could send such an exit, or Yoshi's wings or the bonus game, to the other bank than
  before the save. For vanilla's overworld the two agree. Settles it: an overworld that
  puts a level on a submap its number does not say, built and played before and after
  a save.
