//! Video state captured from the game: what it uploaded, how a level's
//! layers are set up on entry, and the objects its sprite engine drew.
//! Fixed-screen boss arenas switch video modes during the frame, and
//! their collision tiles are not their visible artwork, so they carry a
//! scene of their own.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mode7 {
    pub matrix: [i16; 4],
    pub center: [u16; 2],
    pub scroll: [u16; 2],
    pub control: u8,
}

/// The video registers that say how layer 1 is read within one band of
/// a boss arena's screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Layer1Registers {
    pub mode: u8,
    pub tilemap: u8,
    /// Byte address of the character data in VRAM.
    pub character_base: u16,
    pub scroll: [u16; 2],
    pub mode7: Mode7,
}

/// First visible image row governed by a set of video registers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Band {
    pub start: usize,
    pub layer: Layer1Registers,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BossScene {
    pub bands: Vec<Band>,
    /// The arena's window: it masks BG1 and the objects, and colour math
    /// (the back area colour added to a black backdrop) is prevented
    /// outside it, so the back area shows through the window only.
    pub window: Window,
    /// The objects of the first drawing pass, front to back in screen
    /// coordinates, including sprite-based arena walls and Bowser's floor.
    pub objects: Vec<SpriteObject>,
    /// `OBSEL`: object sizes and character base.
    pub object_select: u8,
}

/// Each layer's bit in `TM`, `TS`, `TMW`, `TSW`, and `CGADSUB`, in the order the
/// renderer keeps its layers: BG1, BG2, BG3, objects.
pub const LAYER_BITS: [u8; 4] = [0x01, 0x02, 0x04, 0x10];

/// The PPU's two windows over a fixed screen: what they cover, which
/// layers they hide on each screen, and the colour window `CGWSEL` refers
/// to. Screen positions are pixels of the visible picture.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Window {
    /// Window 1's left and right edge (inclusive) per screen row, from the
    /// table the game's HDMA feeds to `WH0`/`WH1` (`$04A0`). A row with
    /// left past right is empty.
    pub rows: Vec<[u8; 2]>,
    /// Window 2's edges (`WH2`/`WH3`), the same on every row: the game
    /// has no HDMA for them.
    pub window2: [u8; 2],
    /// `TMW` and `TSW`: the layers the windows hide, where they apply, on
    /// the main screen and the subscreen. Neither has a RAM mirror.
    pub masks: [u8; 2],
    /// `W12SEL`, `W34SEL`, and `WOBJSEL` mirrors (`$41`-`$43`), a nibble
    /// each for BG1 and BG2, BG3 and BG4, objects and the colour window:
    /// bits 1 and 3 enable windows 1 and 2, bits 0 and 2 invert them.
    pub select: [u8; 3],
    /// `WBGLOG` and `WOBJLOG`: how a layer with both windows enabled
    /// combines them (OR, AND, XOR, XNOR), two bits each for BG1-BG4, and
    /// for objects and the colour window.
    pub logic: [u8; 2],
}

impl Window {
    /// Whether the windows a `select` nibble enables apply at a screen
    /// position, combined by a two-bit `logic` value.
    fn applies(&self, select: u8, logic: u8, x: usize, y: usize) -> bool {
        let within = |[left, right]: [u8; 2]| (left as usize..=right as usize).contains(&x);
        let window1 = self.rows.get(y).is_some_and(|&row| within(row)) != (select & 1 != 0);
        let window2 = within(self.window2) != (select & 4 != 0);
        match (select & 2 != 0, select & 8 != 0) {
            (false, false) => false,
            (true, false) => window1,
            (false, true) => window2,
            (true, true) => match logic & 3 {
                0 => window1 || window2,
                1 => window1 && window2,
                2 => window1 != window2,
                _ => window1 == window2,
            },
        }
    }

    /// The layers (as [`LAYER_BITS`]) hidden at a screen position on the
    /// main screen and on the subscreen.
    pub fn hidden(&self, x: usize, y: usize) -> [u8; 2] {
        let settings = [
            (self.select[0], self.logic[0]),
            (self.select[0] >> 4, self.logic[0] >> 2),
            (self.select[1], self.logic[0] >> 4),
            (self.select[2], self.logic[1]),
        ];
        let inside = LAYER_BITS
            .iter()
            .zip(settings)
            .filter(|&(_, (select, logic))| self.applies(select, logic, x, y))
            .fold(0, |bits, (bit, _)| bits | bit);
        self.masks.map(|mask| mask & inside)
    }

    /// Whether a screen position is inside the colour window, which
    /// `CGWSEL` clips and prevents colour math against.
    pub fn color(&self, x: usize, y: usize) -> bool {
        self.applies(self.select[2] >> 4, self.logic[1] >> 2, x, y)
    }
}

/// Layer 3 as level preparation left it: where the game scrolled it for
/// the entry camera, and how it follows the camera from there. The
/// tilemap itself is in the captured VRAM.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Layer3 {
    /// `BG3HOFS`/`BG3VOFS` as the IRQ handler applies them below the
    /// status bar (`$22`/`$24`).
    pub position: [u16; 2],
    /// Layer 1 position the level was entered at (`$1A`/`$1C`).
    pub camera: [u16; 2],
    /// How far the layer moves for a 16-pixel camera move on each axis:
    /// 0 for a screen-fixed layer, 8 for half-speed parallax, 16 for a
    /// layer that stays put on the level.
    pub scroll_per_16: [i32; 2],
    /// `BG3SC`: tilemap base and size.
    pub tilemap: u8,
    /// Byte address of the 2bpp character data in VRAM.
    pub character_base: u16,
    /// `BGMODE` mirror (`$3E`): bit 3 puts high-priority layer 3 tiles
    /// in front of everything.
    pub bg_mode: u8,
}

impl Layer3 {
    /// Whether high-priority tiles go in front of everything (Mode 1's
    /// BG3 priority bit).
    pub fn high_priority_in_front(&self) -> bool {
        self.bg_mode & 0x08 != 0
    }
}

/// Screen designation and colour math as level preparation left them.
/// The level mode tables in `LoadLevel` (`LevMainScrnTbl`, `LevSubScrnTbl`,
/// `LevCGADSUBtable` at `$0581E0`-`$05823F`) choose them, and sprites such
/// as the spotlight change them afterwards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Screen {
    /// `TM` as last written: layers on the main screen (bit 0 layer 1,
    /// bit 1 layer 2, bit 2 layer 3, bit 4 objects). The game writes it
    /// from its mirror (`$0D9D`) once per level load, so a patch that
    /// writes the register afterwards is what the PPU shows; the colour
    /// math registers below go out from their mirrors every frame.
    pub main: u8,
    /// `TS` as last written (mirror `$0D9E`): layers on the subscreen.
    pub sub: u8,
    /// `CGADSUB` mirror (`$40`): which main-screen layers take part in
    /// colour math (bits as above, bit 5 the backdrop), bit 6 halves the
    /// result, bit 7 subtracts instead of adding.
    pub color_math: u8,
    /// `CGWSEL` mirror (`$44`): bit 1 uses the subscreen pixel as the
    /// operand (falling back to the fixed colour where the subscreen is
    /// transparent) instead of the fixed colour; bits 7-6 clip the main
    /// colour to black and bits 5-4 prevent colour math, each never (0),
    /// outside the colour window (1), inside it (2), or always (3).
    pub math_select: u8,
    /// `COLDATA`: the fixed colour, which the game keeps at the level's
    /// back area colour (`$0701`).
    pub fixed_color: crate::palette::Color15,
}

impl Screen {
    /// The vanilla setup of most level modes: layers 1 and 3 with objects
    /// on the main screen, layer 2 on the subscreen, added to the backdrop
    /// and layer 3 wherever they are.
    pub fn vanilla(fixed_color: crate::palette::Color15) -> Self {
        Self {
            main: 0x15,
            sub: 0x02,
            color_math: 0x24,
            math_select: 0x02,
            fixed_color,
        }
    }

    /// Whether a `CGWSEL` window-relative setting (clip or prevent, bits
    /// `10` = inside, `01` = outside, `11` = always) applies to a pixel
    /// inside or outside the colour window. Without a window every pixel
    /// is outside.
    fn window_setting_applies(setting: u8, in_window: bool) -> bool {
        match setting & 3 {
            0 => false,
            1 => !in_window,
            2 => in_window,
            _ => true,
        }
    }

    /// Whether a pixel's main-screen colour is forced to black before the
    /// math.
    pub fn clips_to_black(&self, in_window: bool) -> bool {
        Self::window_setting_applies(self.math_select >> 6, in_window)
    }

    /// Whether colour math is switched off for a pixel.
    pub fn prevents_math(&self, in_window: bool) -> bool {
        Self::window_setting_applies(self.math_select >> 4, in_window)
    }
}

/// One OAM object captured from the game's sprite engine, in level pixel
/// coordinates (which may be negative or extend past the level edge).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SpriteObject {
    pub x: i32,
    pub y: i32,
    /// OAM character number.
    pub tile: u8,
    /// OAM attribute byte: `vhoopppN` (flips, priority, palette, name
    /// table).
    pub attr: u8,
    /// Uses the larger of the two `OBSEL` sizes.
    pub large: bool,
}

impl SpriteObject {
    /// The object moved by (`dx`, `dy`): from screen to level coordinates,
    /// given the camera.
    pub fn translated(self, dx: i32, dy: i32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
    }

    /// VRAM byte address of the 8x8 character `column` across and `row`
    /// down the unflipped object. `object_select` is `OBSEL`, which holds
    /// the base of the two name tables; a large object's characters
    /// follow its first one across and down the 16-wide table, wrapping
    /// within it on both axes.
    pub fn character_address(&self, object_select: u8, column: usize, row: usize) -> usize {
        let tile = self.tile as usize;
        let number = (((tile & 0xF0) + row * 16) & 0xF0) | ((tile + column) & 15);
        let select = object_select as usize;
        let table = if self.attr & 1 != 0 {
            (((select >> 3) & 3) + 1) * 0x2000
        } else {
            0
        };
        (((select & 7) << 14) + table + number * OBJECT_CHARACTER_LEN) & 0xFFFF
    }
}

/// Bytes of one 8x8 object character: objects are always 4bpp.
pub const OBJECT_CHARACTER_LEN: usize = 32;

/// An 8x8 character as a sprite pass left it in VRAM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Character {
    /// VRAM byte address.
    pub address: u16,
    pub data: [u8; OBJECT_CHARACTER_LEN],
}

/// Objects drawn with characters the game uploads as it draws them, so
/// that they are not in the level's VRAM: the Podoboo has its frame copied
/// over tile `06` by the player's per-frame tile upload, and custom
/// dynamic sprites upload theirs from their own NMI code. Two such
/// sprites can use the same tile for different pictures, so each capture
/// keeps its own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DynamicObjects {
    /// Front to back, in level coordinates.
    pub objects: Vec<SpriteObject>,
    /// The characters of `objects` that differ from the level's VRAM.
    pub characters: Vec<Character>,
}

impl DynamicObjects {
    /// `vram` with this capture's characters in place.
    pub fn patched(&self, vram: &[u8]) -> Vec<u8> {
        let mut vram = vram.to_vec();
        for character in &self.characters {
            let at = character.address as usize;
            if let Some(bytes) = vram.get_mut(at..at + OBJECT_CHARACTER_LEN) {
                bytes.copy_from_slice(&character.data);
            }
        }
        vram
    }
}

/// A level sprite entry that drew nothing, for the renderer to mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UndrawnSprite {
    /// Level tile position of the entry.
    pub x: usize,
    pub y: usize,
    /// Sprite number.
    pub id: u8,
}

/// What one level sprite entry drew, for telling its part of the picture
/// from the rest: the objects are the same ones that are in the scene's
/// `objects` or `dynamic`, in level coordinates.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CapturedSprite {
    /// Level tile position of the entry.
    pub x: i32,
    pub y: i32,
    /// Sprite number.
    pub id: u8,
    pub objects: Vec<SpriteObject>,
}

/// The sprites of an ordinary level as the game draws them on their first
/// frame, front to back, plus the level sprite entries that produced no
/// graphics at all (generators, scroll commands, and the like).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpriteScene {
    pub objects: Vec<SpriteObject>,
    /// The objects again, entry by entry, for the entries that took a
    /// sprite slot; what the slotless pass drew (shooters, generators,
    /// cluster sprites) is in `objects` only.
    pub captures: Vec<CapturedSprite>,
    /// Captures drawn with characters of their own, behind `objects`.
    pub dynamic: Vec<DynamicObjects>,
    /// `OBSEL`: object sizes and character base.
    pub object_select: u8,
    /// Entries with no graphics.
    pub undrawn: Vec<UndrawnSprite>,
    /// Objects that ride on layer 2 (the castle candle flames), positioned
    /// in layer 2 pixels. The game keeps eight bits of their position, so
    /// they repeat every 256 pixels along the layer.
    pub layer2_objects: Vec<SpriteObject>,
    /// Passes the CPU core gave up on; what they would have drawn is in
    /// `undrawn` instead.
    pub diagnostics: Vec<crate::expand::Diagnostic>,
}

/// Video memory and the registers that say how to read it, as level
/// preparation left them.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct VideoMemory {
    /// VRAM: layer tiles at `$0000`, sprite tiles at `$C000`, tilemaps in
    /// between. The player's tile uploads are included, and in boss arenas
    /// the boss's.
    pub vram: Vec<u8>,
    /// Which VRAM bytes were actually written.
    pub vram_written: Vec<bool>,
    pub cgram: Vec<u8>,
    /// `BG1SC`-`BG4SC`: bits 7-2 are the tilemap's VRAM word address
    /// divided by `$400`, bit 1 selects 64 tiles tall, bit 0 selects 64
    /// tiles wide. Vanilla puts layer 1 at `$2000` and layer 2 at `$3000`,
    /// both 64x64; Lunar Magic uses `$3000` and `$3800`, 64x32.
    pub bg_sc: [u8; 4],
    /// `OBSEL`: object sizes and character base.
    pub object_select: u8,
}

impl VideoMemory {
    /// The palette as uploaded to CGRAM.
    pub fn palette(&self) -> crate::palette::Palette {
        crate::palette::Palette::from_cgram(&self.cgram)
    }
}

/// How a level is shown when it is entered.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct LevelScene {
    /// Main and sub screen designation and colour math, which decide how
    /// the layers combine into the picture.
    pub screen: Screen,
    /// Layer 1 position the level was entered at (`$1A`/`$1C`) and the
    /// layer 2 position the first camera update derived for it (`$1E`/
    /// `$20`). The two differ when the level's layer 2 scroll settings
    /// offset or slow the layer (parallax); the renderer draws layer 2
    /// where this camera sees it.
    pub camera: [u16; 2],
    pub layer2_position: [u16; 2],
    /// Layer 3 position and scroll behaviour, when the level shows
    /// layer 3 on either screen in Mode 1 (every ordinary level; boss
    /// arenas draw theirs into `boss`).
    pub layer3: Option<Layer3>,
    /// The player's OAM objects at the level's entrance, in level
    /// coordinates, as the game draws him once any entrance action (pipe,
    /// cannon pipe, door) has finished. Empty for boss arenas, whose
    /// drawing pass already includes him.
    pub player: Vec<SpriteObject>,
    /// Video-mode bands installed by the ROM's boss NMI/IRQ handlers.
    pub boss: Option<BossScene>,
}

impl LevelScene {
    /// How far layer 2 content is displaced from the layer 1 grid, in
    /// pixels: a layer 2 tile at column `c` shows at level x
    /// `c * 16 + offset[0]`. Zero when both layers scroll together.
    pub fn layer2_offset(&self) -> [i32; 2] {
        std::array::from_fn(|axis| {
            self.camera[axis].wrapping_sub(self.layer2_position[axis]) as i16 as i32
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Window 1 covers columns 2-5 of row 0 and window 2 columns 4-7.
    fn window(select: [u8; 3], logic: [u8; 2]) -> Window {
        Window {
            rows: vec![[2, 5]],
            window2: [4, 7],
            masks: [0x11, 0x02],
            select,
            logic,
        }
    }

    fn columns(hit: impl Fn(usize) -> bool) -> Vec<usize> {
        (0..10).filter(|&x| hit(x)).collect()
    }

    #[test]
    fn windows_hide_the_layers_their_screen_masks() {
        // Window 1 on BG1, inverted on BG2, window 2 on the objects.
        let w = window([0x32, 0x00, 0x08], [0; 2]);
        assert_eq!(columns(|x| w.hidden(x, 0)[0] & 0x01 != 0), [2, 3, 4, 5]);
        assert_eq!(columns(|x| w.hidden(x, 0)[0] & 0x10 != 0), [4, 5, 6, 7]);
        // BG2 is masked on the subscreen only, BG1 on the main screen only.
        assert_eq!(columns(|x| w.hidden(x, 0)[1] == 0x02), [0, 1, 6, 7, 8, 9]);
        assert!((0..10).all(|x| w.hidden(x, 0)[0] & 0x02 == 0));
        // Past the table window 1 is empty, so its inverse is everywhere.
        assert_eq!(w.hidden(3, 1), [0x00, 0x02]);
    }

    #[test]
    fn two_windows_combine_by_the_layer_logic() {
        let bg1 = |logic| {
            let w = window([0x0A, 0, 0], [logic, 0]);
            columns(|x| w.hidden(x, 0)[0] & 0x01 != 0)
        };
        assert_eq!(bg1(0), [2, 3, 4, 5, 6, 7]); // OR
        assert_eq!(bg1(1), [4, 5]); // AND
        assert_eq!(bg1(2), [2, 3, 6, 7]); // XOR
        assert_eq!(bg1(3), [0, 1, 4, 5, 8, 9]); // XNOR
        // The colour window has its own nibble and logic bits.
        let w = window([0, 0, 0xA0], [0, 0x04]);
        assert_eq!(columns(|x| w.color(x, 0)), [4, 5]);
        assert!(w.hidden(4, 0) == [0, 0]);
    }
}
