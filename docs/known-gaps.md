# Known gaps

What a rendered level does not reproduce, and what the tooling does not handle.

- Layers 2 and 3 are drawn as the entry screen shows them and continued unstretched
  (see the layer facts in [smw.md](smw.md)); parallax is not reproduced away from the entry
  screen, the status bar is left out, and an axis layer 3 does not scroll along cannot be
  followed once the camera moves.
  Decided 2026-09-24: this stays as it is. Lunar Magic simulates no parallax either; it
  tiles both layers from the level's origin at layer 1's scale. Anchoring at the entry
  frame instead is exact on the entry screen, which the video oracle checks, and shows
  what the game's load-time offsets and a hack's level-init code did to the layer; the two
  coincide when the level is entered at its origin. Origin anchoring, if a GUI wants the
  familiar picture, is a render option to add, not a change of model.
- Windows are applied only on a boss arena's fixed screen: they are screen positions, which
  a picture of a whole scrolling level has no place for. The spotlight rooms (mode `$11`)
  render uniformly dark, which is what the game shows until the light switch is hit (the
  spotlight sprite writes an empty window to `$04A0` while its `$C2` is zero), and the
  keyhole and message box effects do not appear.
- Sprites show their first drawn frame, each alone on a camera centred on it, with Mario
  off the left screen edge and no scrolling, so anything that waits for Mario or spawns
  over time (Bullet Bill shooters, generators, Lakitu's Spinies, a Magikoopa, Monty Moles
  in some hacks) is a marker or not what a player sees, and sprites that interact with one
  another only do so when they share a spot. Decided 2026-09-24: this stays as it is.
  Lunar Magic shows every sprite as a static picture at its placement (its own tile
  mappings for vanilla sprites, the `.ssc` display file's for custom ones, a numbered box
  otherwise) and never runs one, so a marker here is what the editor shows too, and a
  sprite that draws on its first frame is already better than that. Drawing the waiting
  and spawning ones one day (by giving a pass the player, or frames) would be welcome; it
  is not owed before the renderer is called complete. Off the left edge of the first screen Mario
  is on screen `$FF`, where the game never has him: a custom sprite that jumps through a
  table indexed by his screen (QLDC 2021 `79_Hwailaluta`, levels `105`, `108`, `109`;
  `34_idol`, levels `1AC`–`1AF`) fails there and is a marker; keeping
  him inside the level instead puts his objects into the capture. A sprite that waits for
  the player (the cutscene sprite of QLDC 2021 `44_Daizo Dee Von` levels `026`, `027`,
  `0C5` waits a frame at a time, then for a button) is given the vertical blanks it waits
  for, 16 of them, and then also a marker. Sprite entries a hack left beyond the level's
  width or height are skipped: no camera inside the level loads them, and the ROM's
  loader, asked for such a column, reads past the level's own data (QLDC 2021 `70_DPBOX`
  `101`, Luminescent `14A`). Custom sprites themselves come out as the emulator draws
  them: a 2026-09-24 per-sprite comparison over eight hacks found none drawn wrong
  ([testing.md](testing.md)). A sprite that stays hidden at first is drawn
  where it first appears (a Podoboo at the lava's surface). Cluster sprites are captured
  from their spawner's camera only, except the candle flames. Custom sprite loaders run as
  ROM code; the full render sweep below records sprite-pass failures even in levels whose
  loader succeeds, superseding the earlier census's claim of no such errors. The emulator
  oracle compares which sprite is in which slot on a sample of every hack's levels
  ([testing.md](testing.md)), and whole frames on a few; nothing compares each custom
  sprite's own picture. Boss arenas show the OAM of the first drawing pass instead.
- A sprite is captured alone, and some sprites look at their slot: the Yoshi's House birds
  take their colour from it, several animations their phase, and the line-guided rope its
  length. It gets the slot the ROM's loader gives it with the columns before it loaded in
  order from the entrance and every sprite of them still in place, which is what a player
  sees who walks there and leaves everything alone. One who kills or outruns sprites, or
  arrives through another entrance, may see another colour, phase, or length.
- SA-1 ROMs ([sa1.md](sa1.md)): the SA-1's timers, the second type of character conversion,
  the variable-length bit reader, and write protection are not modelled; no hack in the
  corpus uses the first three, and the last only matters to a game that relies on a write
  being refused. The full render sweep below supersedes the earlier claim that 38 of 39
  SA-1 hacks render all 512 levels: `70_DPBOX` also has fatal errors, and successful PNGs
  can carry sprite-pass warnings. The emulator oracle compares tile grids, layer 3
  tilemaps, and sprite slots on a sample of each hack's levels ([testing.md](testing.md)),
  not their pictures.
- The frame counter is `$40` on a level's first frame ([smw.md](smw.md)), one of the 256
  values a player's entry can have. Whatever the game decides by it is that value's: the
  phase of a Boo ring, which side the offscreen check looks at first, when Lakitu throws.
- Code a hack runs every frame of the level loop (a custom status bar, a power-up handed
  to the player, UberASM `main` code) has not run: a level is loaded and prepared, and its
  sprites and player are drawn, but no frame of game mode `$14` is played.
- HDMA is not run, so whatever a hack changes by scanline is missing: a gradient sky
  (Luminescent level `148` writes the fixed colour per line) comes out as the one colour the
  level's back area has. A level that enables HDMA, or touches other hardware the bus
  does not model, says so in a render warning (`Diagnostic::Unsupported`); hardware with
  nothing to model, such as a probe of open bus, is not reported.
  Decided 2026-09-24: this stays as it is. The game builds its HDMA tables each frame of
  the level loop, which the renderer never plays, and an effect is defined per scanline of
  a 224-line frame, which a whole-level picture has no place for. Lunar Magic's editor view
  shows no HDMA effects either, so the picture matches what hack authors edit against. The
  boss arena window is the one exception, since its table is written during loading and
  describes a fixed screen. Revisit only for a GUI viewport, which is a single frame.
- The GFX tooling refuses ROMs their authors locked (`GfxError::Locked`, see
  [lunar-magic.md](lunar-magic.md)): their pointer tables are not addresses. Levels are
  unaffected, since the ROM's own decompression runs for them. LC_LZ2 and LC_LZ3 have
  encoders (`compress::lz2::compress`, `compress::lz3::compress`); the LC_LZ3 one does not
  use its reversed and backwards copies (commands 5 and 6).
- A build of a hack saved by Lunar Magic 1.6x draws the vertical pipes as Lunar Magic 3.70's
  install does, each screen's in its colour set (`MAP16AppTable`), where the hack's own
  code, which a 3.70 save keeps, may draw them all in one: Kaizo Mario's level `105` has
  green pipes and its build grey and yellow ones, from the same colour tables (2026-10-04).
  Kobo writes 3.70's layout, so this is left as it is.
- A rendered level shows every ExAnimation slot's first frame: the level's setup runs the
  animation once for each of its eight phases, so the picture has what the level shows as
  it appears. There is no option to render a later frame or a triggered state. Decided on
  review (2026-10-04) to wait for a GUI need: a frame count in `RenderOptions` would run
  the game loop (`expand::play_game_loop`) that many frames before drawing.
- Two tide corners play otherwise than Lunar Magic 3.70's, in settings no corpus level has:
  with `advanced` and a vertical autoscroll, the tide's vertical interaction offset is a
  step behind; and with layer 3 following layer 1 or autoscrolling vertically, the offset
  on the level's first frame comes from layer 3's position before Kobo's code places it.
  Accepted on review (2026-10-04); lunar-magic-install.md, "Layer 3 settings", has how
  they were found.
- Lunar Magic 2.41's older ExAnimation lists (Kaizo Mario World 3, the corpus's only ROM
  with them) are not read from a ROM: where their table is had been found in a way that
  was not recorded, so the reading, and the conversion of the older format with it, were
  removed on 2026-10-03 ([clean-room-audit.md](clean-room-audit.md)). Importing that ROM
  notes that its lists were not taken; Lunar Magic's MWL export converts them to the
  current format, and an import from the MWL files keeps them. Doing it again would need
  the table found by an allowed method (a byte diff of a list Lunar Magic 2.41 writes, or
  documentation) and the older format learnt from Lunar Magic's own conversions.

## Full hack render sweep: 2026-09-22

At revision `25cca50e847ee679c500e2a287ef3e71faf3322f`, the release CLI attempted all
512 slots (`000`–`1FF`) in each of 173 distinct hack ROMs, with sprites and the player
enabled. Of 88,576 attempts, 88,522 produced PNGs and 54 failed; 209 of the produced
PNGs carried diagnostics. Ten hacks had failures or warnings; 163 had neither.
Counts of warnings here mean affected level slots, not individual failed passes.
See [testing.md](testing.md#full-hack-render-sweep) for the inputs, logs and reproduction.

These are execution results, not a visual accuracy assessment. All slots were attempted,
including unused, test and unchanged vanilla rooms. A PNG with warnings may omit or
misrender the player or sprites; a PNG without warnings can still exhibit the visual
gaps listed above. Every row was looked into on 2026-09-23, with Mesen on the levels
where a hack defect was suspected, and with instruction traces, which the clean room has
since ruled out for these ROMs ([clean-room-audit.md](clean-room-audit.md)): the table
keeps what was observed and what became of it.

| Hack | Failed slots | PNGs with warnings | Cause | Status |
| --- | ---: | ---: | --- | --- |
| QLDC 2021 `70_DPBOX` | 24 | 196 | Without the vertical blanks between the loading frames, a 64 KiB DMA from CGRAM lands on bank 0 and the load stops. `101`, `13C`: sprite entries beyond the level's 4 screens. | Fixed: the loader runs the ROM's NMI at each frame boundary ([smw.md](smw.md)); entries outside the level are skipped. All 512 slots render. |
| QLDC 2021 `34_idol` | 9 | 4 | These nine levels stop the load, and Mesen loads each of them into a garbage game mode. The four warnings (`1AC`–`1AF`) are custom sprites failing with Mario on screen `$FF`, as in `79_Hwailaluta`. | Broken in the emulator too (the nine). Known gap (the four). Unchanged. |
| QLDC 2021 `79_Hwailaluta` | 0 | 3 | A custom sprite fails with Mario on screen `$FF` (`$95`), where the sprite pass parks him, at `-64`. | Known gap (above). The sprite is a marker. |
| Invictus 1.0 | 0 | 1 | `136`: the player pass stops in the level's own per-frame code (`expand::player`). | Unchanged. |
| Luminescent v1.02 | 0 | 1 | `14A`: sprite entries beyond the level's 3 screens; the loader, asked for that column, took the list's end for a scroll sprite (`$05:BCD6`, index `$17`). | Fixed: skipped. |
| QLDC 2021 `44_Daizo Dee Von` | 0 | 3 | `026`, `027`, `0C5`: a cutscene sprite waits a frame at a time and then for a button. | Known gap (above). Waits get their vertical blanks; the button never comes. |
| QLDC 2021 `77_NerDose` | 0 | 1 | `004`: the level needs the cartridge's save RAM, which the bus did not have. | Fixed: LoROM save RAM is modelled. |
| Grand Poo World 2 | 1 | 0 | `09F`, an unused slot: the level has no background table, and the upload reads definitions from bank 0 work RAM. Mesen shows the garbage that gives. | Fixed: the definitions are read from wherever the pointer points, as the game does. |
| QLDC 2022 `04_Hwailaluta` | 2 | 0 | `09F`, `104`: as above. | Fixed. |
| QLDC 2021 `76_Bench-kun` | 18 | 0 | The Mode 7 boss rooms stop the load, explained below. | Unchanged. |

Of the 512 slots of each of these ten hacks, the ones that still fail or warn after
this are `34_idol`'s nine and `76_Bench-kun`'s eighteen, the three
`79_Hwailaluta`, four `34_idol` and three `44_Daizo Dee Von` sprite passes above, and
Invictus `136`.
The frame-boundary blanks also changed pictures the sweep counted as clean: Akogare2
`0F8` and Luminescent `0F8` were garbage or black without the upload the third blank
does, and Luminescent `161`'s shells had uninitialised colours. Every vanilla and SA-1
reference picture is byte-identical before and after.

The `76_Bench-kun` failure was taken for the hack's own on 2026-09-20, from an instruction
trace (since ruled out, [clean-room-audit.md](clean-room-audit.md)); it has not been
checked in an emulator. The sweep reproduced it in `095`, `098`–`09B`, `0CC`, `0D5`, `0D9`, `0DF`, `0E2`, `0E5`,
`195`, `198`–`19B`, `1C7`, `1DE`. Do not treat those as evidence of a new CPU-core bug.
