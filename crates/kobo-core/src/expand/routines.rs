//! ROM routines the capture passes call, from the vanilla layout. Lunar
//! Magic keeps these entry points in place.

/// The reset vector, which SA-1 Pack points at its own start-up code, and
/// the main game loop the reset code ends in.
pub const RESET_VECTOR: u32 = 0x00_FFFC;
pub const GAME_LOOP: u32 = 0x00_806B;
/// `CODE_05D796`: resolves the level number and header pointers.
pub const LOAD_HEADER_POINTERS: u32 = 0x05_D796;
/// `CODE_05801E`: clears the buffers and runs `LoadLevel`.
pub const LOAD_LEVEL_DATA: u32 = 0x05_801E;
/// `PrepareGraphicsFile`: decompresses the GFX file whose number is in Y
/// into the buffer at `$7EAD00`. Returns with `RTL`.
pub const DECOMPRESS_GFX_FILE: u32 = 0x00_BA28;
/// `CODE_00B888`: decompresses GFX32/GFX33 into RAM. The game runs it
/// once during the "Nintendo Presents" screen; the animated tile
/// uploads read from that RAM.
pub const DECOMPRESS_PLAYER_GFX: u32 = 0x00_B888;
/// `ClearOutLayer3`: DMA-fills the layer 3 tilemap. Its side effect of
/// leaving the VRAM port in two-byte mode is what the upload below
/// relies on.
pub const CLEAR_LAYER3: u32 = 0x00_85FA;
/// `CODE_00A993`: uploads GFX28-GFX2B (layer 3 tiles and the status bar
/// font) to VRAM word `$4000` through the port. The game runs it once
/// during the "Nintendo Presents" screen and level loads leave that
/// region alone.
pub const UPLOAD_LAYER3_GFX: u32 = 0x00_A993;
/// `CODE_00A635`: initialises level RAM and the player's entrance.
pub const INIT_LEVEL_RAM: u32 = 0x00_A635;
/// Game mode `$11` after `CODE_00A635`: `LDA #$20 : STA $5E` (the screen
/// count's maximum) and `JSR CODE_00A796` (layer 2's first position), to
/// `GM11_LAYER2_END`. Lunar Magic puts a `JSL` to code of its own over the
/// first two, which a level's entry depends on, so the ROM's own bytes are
/// run in place, whatever they are
/// (docs/lunar-magic-install.md, "The sites a save keeps with the marker").
pub const GM11_MAX_SCREENS: u32 = 0x00_9708;
pub const GM11_LAYER2_END: u32 = 0x00_970F;
/// `GM12PrepLevel`: game mode $12. Uploads GFX, palettes, and the
/// initial tilemaps; draws boss arenas; sets up layer 3. Ends with RTS.
pub const PREPARE_LEVEL: u32 = 0x00_A59C;
/// Lunar Magic's Map16 tile pointer routine, the `$058A65` hook's target.
/// It takes the place of the game's `TAY : LDA $0FBE,Y`, so it is called
/// with a 16-bit accumulator holding the tile number times two and returns
/// what that load gave, the pointer's low word, in A; the bank it leaves in
/// direct page `$0C` (docs/lunar-magic.md).
pub const LM_MAP16_POINTER: u32 = 0x06_F540;
/// Inside `CODE_058D7A` (initial layer 2 tilemap upload), where vanilla
/// stores `#Map16BGTiles` to `$0A`. Lunar Magic 2.3 and later replace
/// the store with a `JSL` to a routine that leaves the level's BG
/// Map16 table pointer in `$0A`-`$0C`; the BG pages live in a separate
/// block from the layer 1 pages, so `LM_MAP16_POINTER` cannot find
/// them. Older versions keep the vanilla table.
pub const BG_MAP16_BASE_HOOK: u32 = 0x05_8DA4;
/// `CODE_00A1DA`: one game-mode `$14` drawing pass, which fills OAM
/// with the player, boss, and sprite-based arena walls and floor.
pub const DRAW_LEVEL_FRAME: u32 = 0x00_A1DA;
/// `ConsolidateOAM`, which every drawing pass ends by jumping to: packs
/// the per-object size bytes at `$0420` into the OAM image's last 32
/// bytes. SA-1 Pack replaces it with MaxTile's, which first rebuilds the
/// whole image from its priority buffers, so an object is only where the
/// game drew it until the frame gets here.
pub const CONSOLIDATE_OAM: u32 = 0x00_8494;
/// `CODE_02A802`: the body of `LoadSprFromLevel`, after its
/// every-other-frame check. Spawns the level sprites at the column the
/// camera position and scroll direction select. Lunar Magic reroutes
/// its inner loop but keeps this entry.
pub const SPAWN_SPRITES: u32 = 0x02_A802;
/// `MAP16AppTable`: four pointers into bank `$0D`, one per 8-column
/// stretch of the level, to alternative definitions of the vertical
/// pipe tiles `133`-`13A`. The initial tilemap upload (`CODE_0580BD`)
/// and the scroll setup (`CODE_05877E`) re-point those tiles from it
/// as each column goes up, so a pipe's colour follows its position.
pub const PIPE_POINTER_TABLE: u32 = 0x05_8776;
/// `UpdateScreenPosition`: the level loop's per-frame camera update.
/// It follows the player with layer 1 and derives the layer 2 position
/// from it and the level's layer 2 scroll settings (`$1413`/`$1414`):
/// the same position, half of it, or a fraction plus the offset
/// `CODE_00A796` worked out at load time.
pub const UPDATE_CAMERA: u32 = 0x00_F6DB;
/// `ProcScreenScrollCmds`: the level loop's per-frame layer scroll
/// update, run right after the camera update. It moves layer 2 and
/// layer 3 according to the level's scroll settings (tides, parallax
/// backgrounds, autoscroll) from the camera delta `$17BC`-`$17BD`.
pub const SCROLL_LAYERS: u32 = 0x05_BC00;
