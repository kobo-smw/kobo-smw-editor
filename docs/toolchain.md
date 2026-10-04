# The toolchain

What the tools a build runs require of the ROM they are given, how they find space, and what
makes their output vary. From their sources (paths relative to each tool's repository), read
2026-09-25; the versions are the ones [step-2.md](step-2.md) pins. `$xxxxxx` is a SNES
address, "PC" an offset in the headerless file.

## Kobo's pinned builds

- [`kobo-smw/kobo-tools`](https://github.com/kobo-smw/kobo-tools) builds Asar 1.91 (program
  and library), PIXI 1.43, and UberASM Tool 2.1 from pinned upstream commits (its
  `pins.py`) for `linux-x64`, `windows-x64`, `macos-arm64`, and `macos-x64`, and
  publishes each as a `.tar.gz` of one folder in a release `r<N>`, with the exact source
  each was built from (`<tool>-<version>-source.tar.gz`), `SHA256SUMS`, and
  `manifest.json`. Its README has the details; its `build.py pins manifest.json` writes
  Kobo's `crates/kobo-core/src/tools/pinned.toml`, which is how Kobo moves to a new release.
- How they differ from upstream's releases: PIXI without its CFG editor (Nintendo
  resources; left out of the source archive too), with its documents as Markdown;
  UberASM Tool self-contained (no .NET to install) for x64 or arm64, without the 32-bit
  `asar.dll` its repository holds; and no Asar library in PIXI's or UberASM Tool's folder:
  Kobo lays the one it uses beside them ([`FolderTool`](../crates/kobo-core/src/tools/mod.rs)),
  so a build runs one Asar throughout. A release used without Kobo needs the library
  copied from the Asar archive, as its README says; kept so (maintainer, 2026-10-03),
  since the releases are made for Kobo, which would replace a bundled copy anyway.
- `Tool::locate` takes a configured path first (the environment variable, then `[tools]`),
  then the pinned build for the platform: from the cache (`KOBO_TOOL_CACHE`, else
  `kobo/tools` in the user's cache directory, one folder per build named by its hash), or
  downloaded over HTTPS (`ureq`, rustls with the system's certificate store through
  `rustls-platform-verifier`, so a network that intercepts TLS with its own root works as
  it does in a browser; proxies from `HTTPS_PROXY`), checked against the pinned size and
  SHA-256 before it is unpacked, which is what vouches for the file, and
  unpacked into a scratch folder renamed into place, so a failed or concurrent fetch never
  leaves half a build. `KOBO_OFFLINE=1` turns downloads off; `kobo tools fetch` fills the
  cache ahead of time and `kobo tools` lists where each tool would come from.
- A stage's cache key takes a pinned build's hash, not its files. A configured tool's
  key takes every file of it, and `kobo build` prints a note for it: the build depends
  on the user's copy and is not reproducible elsewhere. AddmusicK and GPS always get that
  note, since Kobo can pin no build of them.
- SA-1 Pack 1.40 is pinned to its author's own release, not rebuilt: `upstream.toml`
  names `VitorVilela7/SMW-SA1-Pack`'s `v1.40` asset `SA1-Pack-140.zip` by size and
  SHA-256, and `Tool::locate` fetches, checks, and caches it as it does a kobo-tools
  build, unpacked into `sa1pack-1.40/` (the patches at the top; a configured folder may
  have them there or in `asm/`, as the repository does). Fetching it from its author is
  not redistribution, which its lack of a licence rules out (maintainer, 2026-10-03).
  The release's patches differ from the repository's tag `v1.40` only in line endings,
  and give the same bytes at 1, 6, and 8 MiB (checked 2026-10-03).
- Reproducibility of the builds themselves: the Linux ones are made in a pinned Debian 12
  image with packages from a pinned snapshot of Debian's archive, and a rebuild gives the
  same bytes; they need glibc 2.35 (Ubuntu 22.04) or later, which `build.py test` checks.
  Older systems configure their own build of a tool (accepted, maintainer, 2026-10-03;
  Debian 11, the older image first tried, has left Debian's mirrors). The Windows and macOS ones
  depend on the compilers of GitHub's runner images, which cannot be pinned: rebuilt on
  the same image they match, and a newer image may not. That is accepted (maintainer,
  2026-10-03): Kobo pins every build by hash, so what users run and what a project builds
  do not depend on it, and the Linux builds can be rebuilt and checked; pinning Xcode or
  an MSVC toolset would last only until the images drop them. kobo-tools's `verify` workflow
  rebuilds a release from its source archives and compares every file: all twelve
  builds of `r1` came out byte-identical (2026-10-03), and the Linux ones match a local
  rebuild in Docker too. `r1` was built once before Actions were turned on
  in `kobo-smw/kobo-tools`; republished by its CI (2026-10-03), its builds differ from
  that one's only in the repository named on the first line of `KOBO-TOOLS.txt`, so
  every hash changed.

## Asar 1.91, and every tool built on it

- Free space is runs of `$00`, normally skipping valid RATS blocks (with the boundary
  exception below). LoROM code goes in banks `$10`-`$3F`; data tries banks `$40` and up
  first on images over 2 MiB,
  and never crosses a bank. A patch that needs more space expands the image, 512 KiB to
  1 MiB to 2 MiB (4 MiB for data), and rewrites `$00FFD7` and the checksum
  (`libsmw.cpp:244-352`).
- `autoclean` erases the whole RATS block it finds before the old target of a pointer or
  jump it replaces, and refills it with `$00` (`libsmw.cpp:135-150`,
  `assembleblock.cpp:2081-2158`). Anything a tool repoints, or a hook site a tool takes
  over, has to lead to a block of its own: the tables at `$06F624` and `$06F63A` (GPS) and
  `$0EF30C` (PIXI), and the targets of the jumps at every tool's hook sites. The erasure
  happens in pass 1, before free space is found, so a `freecode` after the `autoclean`
  can land in the erased block, even at the same place with the same length.
- Kobo's own patches take free space through Asar like any patch, in the build's first
  stage after the base image, before Kobo or a tool has placed a block
  ([step-2.md](step-2.md)).
- On LoROM, the addresses Asar gives free space are in banks `$80` and up (`$908008` for
  PC `0x80008`), since its `pctosnes` sets bit 23 (`libsmw.h:126-134`). Kobo's own
  allocator uses `$10`-`$3F`; the bytes are the same.
- The library (`libasar`, API version 3.03) takes the image in memory through
  `asar_patch_ex` and lists every range a patch wrote, erasures included
  (`asar_getwrittenblocks`). It keeps its state in globals: one patch at a time per
  process. On Windows it runs each patch on a fiber of its own with a 4 MiB stack.
- `asar_patch_ex` copies the image into a 16 MiB buffer of its own without checking
  `buflen` against it: a longer image overruns the heap (seen with AddressSanitizer on a
  17 MiB image), so Kobo refuses one before the call (`asar::BUFFER_LEN`, 8 MiB). The
  image it returns is headerless and as long as the last write reached, not a whole
  number of banks: a 1 MiB image grown by 512 bytes looks like a headered one to a
  copier-header check, so Kobo reads it as headerless and pads it to a bank.

### Asar 1.91 RATS boundary limitation

Confirmed 2026-09-25 against Asar 1.91. On a synthetic 1 MiB LoROM, with `$FF` in the
first 512 KiB and `$00` afterwards, Kobo allocates zero-filled code blocks of `$8000`
and `$7FF8` bytes at `$118000` (PC `0x88000`) and `$128008` (PC `0x90008`). Their tags
are at PC `0x87FF8` and `0x90000`; the second block ends at PC `0x98000`, exclusive.

An Asar `freecode cleaned` request for another `$8000` bytes puts its tag at PC
`0x97FF8`, overwriting the second block's last eight zeros with `53 54 41 52 FF 7F 00 80`.
In `libsmw.cpp`, `trypcfreespace` advances to a bank edge before checking for a tag:
after skipping the first block, it jumps past the second tag and searches inside that
block. Kobo instead puts the third tag at PC `0x9FFF8`, with contents at `$148000`, and
preserves both previous blocks. Byte-identical placement is not a requirement in this case.

Reproduce the upstream behavior without a game ROM:

```sh
python3 tools/asar/rats_boundary.py ~/.local/bin/asar
```

The script constructs the two tagged blocks at the offsets above in a temporary synthetic
image and checks the overwritten bytes after running Asar. It expects the 1.91 bug and
fails if the behavior changes; it is a diagnostic, not a requirement for future Asar versions.
The always-run `rats::tests::bank_boundary_search_preserves_tagged_zeros` regression checks
Kobo's safe behavior for the same sequence, including after reloading the image and rescanning.

Kobo detects it rather than prevents it. `asar::Asar::patch` takes a `rats::Snapshot` of
every block before the patch and fails with `AsarError::Damaged` if one was changed
without being released. A block is released if its tag is gone or Asar reports writing
over it, and each of its bytes is now `$00` or inside a tagged block of the result, which
covers `autoclean` and the reuse of its space. Anything else is damage, above all a
change under a tag that still stands, as here. `tests/asar.rs` reproduces this case
through the library and checks that the guard reports the second block. The external
tools (PIXI, GPS, UberASM Tool, AddmusicK) do not report their writes, so their stages
can use the same snapshot without them; a block rewritten in place under a tag of the
same length then counts as damage. Tests of tool stages must still check that new blocks
are placed as well as that old ones are kept.

## PIXI 1.43 (GPL-3.0)

- Refuses a ROM where the pointer at `$06F624` (the acts-like table) is `$FFFFFF`, "without
  having modified a level in Lunar Magic", and one where `$00F6E4` is not `$5C`, Lunar
  Magic's VRAM patch (`src/sprite.cpp:1875-1906`). It checks only that byte; what the
  patch has to do is still to be worked out.
- Writes the sprite size table pointer at `$0EF30C` and `$42` at `$0EF30F`
  (`asm/main.asm:209-217`), clears bit 0 of `$0FFFE0` (`:49-55`), and hooks `$05D8B9` to
  store the level number in `$010B` (`:61-67`). On a LoROM image it always installs its own
  255-sprites-per-level code, with jumps at `$02A856`,
  `$02A936`, `$02A8BB`, `$02FAE9`, and `$02ABF2` (`:301-316`).
- Its tables: `STSD` and flags at `$02FFE2`-`$02FFFF`, shared routine pointers at
  `$03E05C`. A newer PIXI's marker at `$02FFE6` is refused.
- MeiMei, its sprite data remapper, uses addresses that assume a copier header
  (`src/MeiMei/MeiMei.cpp:96`) and reads the wrong bytes of a headerless image. Kobo runs
  PIXI with `-meimei-off`; its own level writer sizes sprite entries.
- The version digits at `$0FF0B4` (inside Lunar Magic's marker) turn on `!EXLEVEL` when
  above 2.53; vanilla `$FF` counts as above, `$00` would not (`asm/sa1def.asm:40-43`).
- Shared routines are listed in directory order (`src/sprite.cpp:1203`), which decides
  their slots, so output depends on the file system. `pixi_settings.json` and `plugins/`
  in the working directory change a run.
- Builds natively with CMake; configure downloads Asar and nlohmann/json by tag. The
  repository holds Nintendo data for its Windows CFG editor (SMW graphics, palettes, and a
  Map16 page under `src/CFG Editor/CFG Editor/Resources/`).
- Also refuses an image where `$029B39` is `$0000` (the More Extended Sprites patch)
  without SA-1 (`asm/sa1def.asm:118-123`).
- Kobo's sprites stage (`[pixi] dir`, `tools::run_pixi`) runs it after Map16 and before
  GPS, in a copy of its folder with the project's folder laid over it, as
  `pixi --script-mode -meimei-off -l list.txt rom.sfc`; a folder without `list.txt` is
  refused. The files it writes for Lunar Magic's sprite display (`.ssc`, `.mwt`, `.mw2`,
  `.s16`) stay in the scratch folder. The levels stage then sizes each sprite entry by
  PIXI's table (`sprites::pixi_size_table`). A Kobo build leaves `$0FF0B4` `$FF`, so PIXI
  assembles its code for Lunar Magic 3 ROMs (`!EXLEVEL` 1), the variant it also picks for
  a ROM Lunar Magic 3.70 has saved.
- A Lunar Magic save of a PIXI build (tests/lunar_magic_save.rs, 2026-09-28) keeps the
  level with its sprite's extension bytes and every site PIXI wrote (`$01C089`,
  `$02A856`, `$02A8BB`, `$02A936`, `$02ABF2`, `$02FAE9`, `$05D8B9`, `$02FFE2`, `$0EF30C`,
  `$0FFFE0`), and a goal tape placed for the secret exit (level `0EB`) still gives it.
  Kobo's sprite loader keeps Lunar Magic's sprite group out of a save; without it, the save
  installed that group over `$01C089` and the goal tape gave the normal exit.
- PIXI's 255-sprite option (on: Kobo does not pass `-d255spl`) moves the load flags to
  `$7FAF00` through `$02A856` and its other hooks, which Kobo's loader calls as the game's,
  and clears `$0FFFE0` bit 0; a build then takes levels of up to 255 sprites
  (`sprites::max_sprites`), and one without PIXI refuses 129. PIXI 1.43 refuses
  `-d255spl` ("not supported since 1.41", as it puts minor sprites' tables in the RAM
  at `$1938` the move frees), so a project cannot keep 128 (found 2026-10-03, when the
  maintainer chose a setting for it). 19 of the corpus's 28 PIXI hacks keep 128 flags at
  `$1938`; an import of one with `--pixi` notes that the build moves them, for the
  hack's own code may use `$7FAF00` or `$1938`.
- Importing a PIXI hack (2026-10-03). With the hack's PIXI folder (`kobo import hack.smc
  dir --pixi folder`), the import copies its inputs (`tools::PIXI_INPUTS`: `list.txt`, the
  sprite folders, `routines/`, `asm/ExtraDefines` and `asm/ExtraHijacks`; not PIXI's own
  patches, nor `pixi_settings.json`, whose options Kobo's run sets) into `pixi/` as
  `[pixi] dir`, runs the build as far as PIXI, and notes whether PIXI 1.43 makes the
  hack's size table from them. The pinned PIXI runs them whatever version made the
  hack, with Kobo's options, not the hack's settings: per-level sprites (`-pl`) and
  a project's own PIXI version come when a hack needs them (accepted with the
  maintainer, 2026-10-03). Without it, the import carries the insert PIXI left in the
  ROM as compiled code (`[pixi] compiled`, `pixi/compiled.toml` and a `.bin` file a
  block; `source::pixi`), which builds write back where PIXI put it. A file a block
  keeps a project's changes to it small in git (decided with the maintainer,
  2026-10-03). A project has `dir` or `compiled`, never both: PIXI would install its own
  code and tables over the compiled insert's, so moving to sources replaces every
  compiled sprite at once, which the manifest's error and the import's comment say.
  Keeping some compiled sprites beside sources is not supported (decided with the
  maintainer, 2026-10-03). The bytes are
  the hack's sprites compiled with PIXI's own (GPL) code: a project carries them from
  its user's own ROM, and Kobo never distributes them.
- The insert (`kobo_core::pixi`) is found as PIXI's own cleanup finds an earlier run's
  (`clean_hack`, `src/sprite.cpp`): `STSD` and the version byte at `$02FFE2` (`$02` for
  1.02, `$19`-`$23` for 1.2.5-1.2.15, `$30`-`$32`, then 140-143), the main block that
  `$02FFEE` points into (its code and the global sprites' 16-byte entries, init and main
  pointers at 8 and 11), the per-level tables (`$02FFEA`'s banks before 1.30, `$02FFF1`'s
  table and the two blocks its `PROT` list ends with from 1.30), the custom status
  pointers (`$02FFFD`), the shared routines' pointers (`$03E05C`, up to 310), the size
  table, each sprite type's table from the pointer after its hook (`$00A686`, `$029B1B`,
  `$029633`, `$028B6C`, `$029054`, `$0296C0`, `$0299D4`, `$02ADBA`), and every block a
  carried block's `PROT` list names. Pointers may be in the FastROM banks (`$90xxxx`).
  The fixed sites are `pixi::SITES`, the union of `main.asm` and the sprite types'
  patches from 1.02 to 1.43 (`org` lines compared across the tags); a site is carried
  where the ROM differs from the base and its bytes have the shape PIXI writes there.
  Five sites are Lunar Magic's too (`$01C089`, `$02A846`, `$02A8D8`, `$02ABF2`, and
  `$02A9D7` on SA-1; lunar-magic-install.md, "Sprites"), so their bytes may be its code,
  which an import must not copy: there a jump is carried only when its target is inside
  PIXI's main block, and other bytes only when they are PIXI's exactly. The test reads
  the target and says nothing of it; a jump that fails is left out with a note naming
  only the site (decided with the maintainer, 2026-10-03, as within the clean-room rule).
  `tests/tool_stages.rs` checks that the insert read from PIXI 1.43's output, with a
  sprite of every type and a shared routine, written onto the image PIXI ran on gives
  PIXI's image again, and that a hack imported either way plays its sprite.
- A compiled insert is a workaround for migrating a hack, not a first-party format: a
  project is meant to replace it with its sprites' sources (`[pixi] dir`), and Kobo
  favours whatever keeps such an import building over polishing it (decided with the
  maintainer, 2026-10-03).
- A compiled insert's blocks go in at their own addresses before Kobo's install
  (`Stage::SpriteBlocks`), since code is not relocatable; Kobo's patches and blocks then
  take space around them, so for such a project Asar places them around the hack's
  blocks rather than first on the bare base (accepted with the maintainer,
  2026-10-03). Its sites go in at the sprites stage, where PIXI would run. A
  build fails if a block's space is not free. The blocks leave the free space in pieces,
  so the import takes a ROM 1 MiB larger than the hack's, up to 4 MiB (accepted with the
  maintainer, 2026-10-03; a project can lower `[rom] size` once its sprites come from
  source). A block the base
  image has is never PIXI's: on SA-1, a hack's older SA-1 Pack has hooks at some of
  PIXI's sites that jump into SA-1 Pack's blocks. Where the hack's PIXI put a block that
  the base holds something else in (an SA-1 hack made with an older SA-1 Pack, whose
  blocks are smaller than 1.40's: 13 QLDC entries), the code cannot be carried; the
  import then carries PIXI's size table alone (`size_table` in `compiled.toml`), which a
  build places in free space and points `$0EF30C` at, so the levels build with their
  sprites' extension bytes but the custom sprites have no code (accepted with the
  maintainer, 2026-10-03).
- Either way, the import writes a comment before `[pixi]` in the manifest and at the top
  of `compiled.toml` saying what the insert is and why it is `.bin` files, what it cannot
  do (no editing; other tools' code not carried; with the size table alone, no sprite
  code at all), and how to move to the sprites' sources. It is the user's comment from
  then on, which `kobo fmt` keeps, so it stays with the project rather than in an
  import's output alone.
- Kobo's own Asar patches run after the carried blocks, and Asar 1.91's free space search
  goes wrong around them in two ways: inside a block that starts at a bank's start (the
  limitation below), and, on SA-1, at a free run of fewer than eight bytes before a bank's
  end, where it puts a tag and its contents across the bank's end (`trypcfreespace` checks
  only that the tag starts in the bank's last eight bytes). Every stage that runs Asar or
  a tool therefore runs with the blocks' `$00` bytes, and such short runs after them, set
  to `$FF` (`pixi::mask_blocks`), and puts them back after. What a compiled sprite reads from another
  block without a `PROT` (a table placed by hand), or calls in another tool's code
  (UberASM Tool's or GPS's shared routines), is not carried: such a sprite may break in
  play though the levels build.

## GPS 1.4.4 (no licence)

- No public repository. The 1.4.4 release (the last) is on the Wayback Machine as
  `dl.smwcentral.net/31515/GPS (V1.4.4).zip`; its `src.zip` has the source.
- Refuses a ROM whose pointer at `$06F624` is `$FFFFFF`, custom blocks on Map16 pages
  `40`+ unless `$06F63C` is not `$FF`, and a ROM with `$8B` at `$06F690` but no
  `GPS_VeRsIoN` string, as "unidentified custom block code" (`gps_src/main.cpp:544-548`,
  `822-825`, `851-853`).
- Copies the acts-like table (`$8000` bytes, two per tile, from the pointer at `$06F624`;
  pages `40`-`7F` from `$06F63A`), applies its list, and writes it back with `autoclean
  dl acts_likes_1` and `freedata`: Asar frees the block the pointer names and places the
  table again, first fit, which on a Kobo build usually lands it where it was. On images
  over 2 MiB Asar looks for data space in banks `$40` and up first, so the table moves.
  The build's blocks stage allows either for the blocks those two pointers named, and so
  cannot tell what GPS wrote inside them from the table it is meant to write.
- It depends on the shape of Lunar Magic's code in bank `$06`, not only on tables: it
  writes 16-byte entries at `$06F690`, `$06F6A0`, `$06F6B0`, `$06F6C0`, `$06F6D0`,
  `$06F6E0`, `$06F720`, `$06F730`, `$06F780`, `$06F7C0`, `$06F7D0`, and `$06F7E0`, each
  ending `JMP $F602`, and puts `JML`s over `$06F67B` and `$06F717`, whose replacements
  compare A with `#$39`, `#$EA`, and `#$82` and fall back to `$06F602` (`main.asm:7-77`).
  It does not use the documented `JSL` slots at `$06F890`-`$06F9F0`.
- Lunar Magic 3.70's first save of a Kobo build with a GPS block (a Map16 page and block
  `200` acting like `025`) keeps GPS's entries at `$06F690`-`$06F7EF`, its `JML`s at
  `$06F67B` and `$06F717`, and the acts-like table, pointer and contents (2026-09-27).
- Its shared routine table is at `$0CB66F`. Routine slots follow `readdir` order
  (`main.cpp:605`).
- Builds natively with `g++ -std=c++17 main.cpp asar/asardll.c -ldl`; it loads
  `./libasar.so`, and Asar 1.91 works (the same API as the 1.81 it ships).

## UberASM Tool 2.1 (GPL-3.0)

- Requires an image of 1 to 8 MiB whose internal title at PC `0x7FC0` is
  `SUPER MARIOWORLD     ` (`UberASMTool/ROM.cs:89-110`); no Lunar Magic check.
- Reads `$0FFFE0` bit 0 (clear: 255 sprites per level), so it runs after PIXI, and the
  version digits at `$0FF0B4`. Hooks `$05808C`, `$00A5EE`, `$00A242`, `$00A2EE`,
  `$00A1C3`, `$00A18F`, `$009322`, `$00804E`, `$008176`, and `$008E1A`, and rewrites
  `$05D8B7`-`$05D8DF` around PIXI's hook (`assets/asm/base/main.asm:15-101`).
- Library files are inserted in `Directory.GetFiles` order (`Library.cs:17`).
- Targets `net8.0` with `PlatformTarget` x86 and a 32-bit Windows Asar, so the published
  build does not run on Linux or macOS, where .NET has no x86 runtime. Built unmodified as
  x64 (`dotnet publish UberASMTool/UberASMTool.csproj -c Release -r linux-x64
  --self-contained false -p:PlatformTarget=x64`), it runs on Linux with `libasar.so`
  beside it, found through its `DllImport("asar")`, and gives the same bytes on every run
  (2026-09-26). A framework-dependent build needs `DOTNET_ROOT`; the .NET CLI and the
  program need `DOTNET_SYSTEM_GLOBALIZATION_INVARIANT=1` where ICU is missing. Kobo's
  pinned build is the same source published self-contained as a single file
  (`--self-contained true -p:PublishSingleFile=true`, with `InvariantGlobalization`), for
  x64, or arm64 on Apple silicon, which needs neither; trimming it warns that Pidgin, its
  parser library, is not trim-safe, so it is not trimmed. The cost is about 29 MB per
  platform against about 1 MB framework-dependent, accepted (maintainer, 2026-10-03) so
  that nothing has to be installed; since .NET 8's support ends 2026-11-10, kobo-tools
  `r2` builds it against .NET 10 (supported to 2028-11) instead. The .NET runtime bundled
  in it (MIT) is taken as the platform the program runs on, not part of its GPL
  corresponding source, so the source archive holds UberASM Tool's and Pidgin's only;
  from `r2` the notice names the runtime's exact version and its `dotnet/runtime` tag
  rather than the repository alone (maintainer, 2026-10-03). A checkout's
  `.gitkeep` files are taken for library binaries ("Zero insert size"); a release has none.
  Kobo's stage lays the project's folder over a copy of the tool's without hidden files,
  and refuses a project folder with no `list.txt`, which would run the tool's own.
- Lunar Magic's first save keeps UberASM Tool's ten hooks and its code blocks.

## AddmusicK 1.0.11, AddMusicKFF (no licence)

- Requires an image over 512 KiB, and `$0E8000` either vanilla (`3E 0E`) or its own
  `@AMK` (`src/AddmusicK/AddmusicK.cpp:196-197`, `338-373`).
- Fills `$0E8000`-`$0EF0FF` and `$0F8000`-`$0FF050` with `$55` ("Lunar Magic install
  some hacks there") and puts its own code there (`asm/SNES/patch.asm:112-115`). The second
  range overlaps `$0FF035`-`$0FF050`, which Lunar Magic 3.70's first save fills. Kobo puts
  nothing in either range.
- Its own free-space scanner counts only `$00` as free, honours RATS, never crosses a bank,
  starts at PC `0x200000` and then `0x080000`, and handles only PC below `0x400000`
  (`globals.cpp:297-396`). Everything written before it must be RATS-tagged.
- Output is repeatable, including over its own output. It runs from its own folder, renames
  the ROM to `ROM~`, and reads `Addmusic_options.txt` in place of its arguments.
- Kobo's music stage runs it with `-noblock rom.sfc` in a copy of its folder, the options
  file removed so the arguments count, and the configured Asar library copied beside it
  under the name it loads (`asar.dll`, `libasar.dylib`, `libasar.so`), with the loader's
  search path (`LD_LIBRARY_PATH`, `DYLD_LIBRARY_PATH`) set to that folder. A tool folder
  that already has the library keeps its own, as UberASM Tool's does too: upstream's
  Windows releases of both are 32-bit with a 32-bit `asar.dll`, which Kobo's 64-bit
  library, the one it loads itself, cannot replace. Kobo's pinned builds of PIXI and
  UberASM Tool carry no library, so they always get Kobo's.
- Builds natively with `make`; loads `./libasar.so` or runs `asar`. The repository holds
  SMW samples, transcriptions of SMW's music and sound effects, and vanilla code bytes.

## SA-1 Pack 1.40 (no licence)

- Applies to a clean ROM only, before Lunar Magic or any tool; it detects its first run by
  `$0DA693` not being `$1E`, and on it moves every level's sprite header into bank `$07`.
  It sets `$00FFD5` to `$23`, which every other tool reads to detect SA-1, and marks
  itself at `$0084C0` (`$05A123`) and `$0084C3` (version, 140). `6mb.asm` and `8mb.asm`
  follow `sa1.asm` for larger images, and make exactly 6 and 8 MiB. Kobo runs them
  unmodified, so `6mb.asm` keeps its default `!ZSNES = 1` (ZSNES support, 128 KiB of the
  image instead of 64 bytes).
- Chooses its decompressor by `$0FFFEB` (`2`: LC_LZ3) and hooks `$00B8E3`, Lunar Magic's
  decompression site. It has to be reapplied when a ROM's compression changes.
- Contains code attributed to Lunar Magic (`asm/boost/lz3.asm`). Do not read it; the
  clean-room rule covers it.

## Callisto projects (`import::import_callisto`)

Callisto (`github.com/Underrout/callisto`, v0.6.2, no licence) builds a hack by running Lunar
Magic's command line and the tools in an order its configuration gives. Kobo does not run
it; it imports the project (`kobo import folder dir`), reading the configuration as its
documentation describes it (`kobo_core::callisto`).

- Its resources include `callisto.asm` by name (`incsrc "callisto.asm"`), which Callisto
  generates for each build, and use its macros `%incsrc_file` and `%incbin_file` to include
  a file by its path from the project's root. Callisto's documentation says only patches
  and modules can include it by name; the Romhack Races baserom's GPS blocks and UberASM
  files do too, because its setup puts Callisto's own build of Asar beside every tool, which
  finds the file anywhere. Kobo's pinned Asar does not, and GPS 1.4.4 and UberASM Tool 2.1
  pass Asar no include paths (PIXI has `--stdincludes`).
- So a project with `[callisto]` gets, for each stage that assembles (the patches and the
  four tools), a scratch copy of the files a patch may include, with Kobo's `callisto.asm`
  at its root (`build::callisto_header`: `CALLISTO_ASSEMBLING`, the version, the project's
  `callisto_header` included, and the two macros), and every `"callisto.asm"` in the copy's
  and the tool's scratch copies' `.asm` files pointed at it by its full path. A copy of the
  header beside each file would do as much, but UberASM Tool assembles every file of its
  `library/` folder, the header among them. The project's own files are never changed.
  Modules (`include_module`, `call_module`) are not carried.
- The configuration's patches run as early patches (Callisto's come after its graphics and
  Map16 and before the tools); PIXI's folder gives only its inputs, other tools' folders
  are taken whole; an initial patch is not carried (Callisto's own are ROMs Lunar Magic
  saved), nor are the overworld, title screen, and credits, which Callisto keeps as patches
  of the clean ROM. Its global ExAnimation patch, also one of the clean ROM, is applied in
  memory and its list read as a ROM import reads it.

## Across the tools

- Run twice on the same input, every tool tried gave the same bytes. Run again over its own
  output, AddmusicK repeats itself but PIXI, GPS, and SA-1 Pack move things (UberASM Tool
  was not run). A build therefore always runs a tool on the previous stage's snapshot,
  never on its own output.
- PIXI, GPS, and UberASM Tool order shared routines or library files by directory listing,
  which differs between operating systems and file systems. Fixing it takes patching the
  tools to sort, or handing them one file at a time; [step-2.md](step-2.md) accepts the
  variation for now.
