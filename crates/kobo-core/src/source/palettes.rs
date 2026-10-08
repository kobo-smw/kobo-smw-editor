//! The shared palettes file: the game's colour tables from `$00B0A0`
//! (`BackAreaColors` to `OWSpecialColors`, 1009 colours), which Lunar Magic
//! edits in place and exports whole as its shared palette
//! (`-ExportSharedPalette`), as a project changes them. The manifest's
//! `[palettes] shared` names it.
//!
//! ```toml
//! [background]
//! 0x05 = "#F8F8F8"  # palette 0, row 0, colour 7
//! ```
//!
//! A table per section, named as [`TABLES`] names them, keyed by the
//! colour's number in the table, in hex; a colour is `#RRGGBB` with each
//! channel the SNES 5-bit value times 8, as in a level's palette. A colour
//! the file leaves out is the clean ROM's. The trailing comments are
//! Kobo's, and say where a colour is for the tables made of palettes.

use std::collections::BTreeMap;

use toml_edit::{DocumentMut, Table};

use super::level::{color_text, parse_color};
use super::{SourceError, hex, invalid, own_line_comments};
use crate::addr::SnesAddr;
use crate::palette::Color15;
use crate::rom::{Rom, RomError};

/// Where the first table starts.
pub const START: SnesAddr = SnesAddr::new(0x00B0A0);
/// Colours in the tables, all together.
pub const COLOURS: u16 = 1009;

/// One of the game's colour tables.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SharedTable {
    /// Its section in the file.
    pub name: &'static str,
    /// The disassembly's name for it.
    pub label: &'static str,
    /// Its first colour's number among all of them.
    pub first: u16,
    pub len: u16,
    /// For a table of palettes, the colours in one of its rows, the rows
    /// in one palette, and the CGRAM column of a row's first colour, for
    /// the comments.
    pub layout: Option<(u16, u16, u16)>,
}

impl SharedTable {
    pub fn addr(&self) -> SnesAddr {
        START.add(2 * u32::from(self.first))
    }

    /// Where colour `index` of the table is, for its comment: "palette 2,
    /// row 1, colour 4" for a table of palettes, the colour numbered as in
    /// its CGRAM row.
    pub fn place(&self, index: u16) -> Option<String> {
        let (per_row, rows, column) = self.layout?;
        let palette = index / (per_row * rows);
        let row = index / per_row % rows;
        let colour = index % per_row + column;
        Some(if rows == 1 {
            format!("palette {palette}, colour {colour}")
        } else {
            format!("palette {palette}, row {row}, colour {colour}")
        })
    }
}

macro_rules! tables {
    ($(($name:literal, $label:literal, $first:literal, $len:literal, $layout:expr)),* $(,)?) => {
        [$(SharedTable { name: $name, label: $label, first: $first, len: $len, layout: $layout }),*]
    };
}

/// The tables in the order the ROM keeps them, which is each one's
/// address's order.
pub const TABLES: [SharedTable; 21] = tables![
    ("back_area", "BackAreaColors", 0, 8, None),
    ("background", "BackgroundPalettes", 8, 96, Some((6, 2, 2))),
    ("status_bar", "StatusBarColors", 104, 16, None),
    ("foreground", "ForegroundPalettes", 120, 96, Some((6, 2, 2))),
    ("standard", "StandardColors", 216, 60, None),
    ("player", "PlayerColors", 276, 40, Some((10, 1, 6))),
    ("sprite", "SpriteColors", 316, 84, Some((6, 2, 2))),
    ("bowser_end", "BowserEndPalette", 400, 12, None),
    ("overworld", "OverworldColors", 412, 168, None),
    ("overworld_standard", "OWStdColors", 580, 42, None),
    ("overworld_standard_2", "OWStdColors2", 622, 49, None),
    ("overworld_lightning", "OverworldLightning", 671, 7, None),
    ("overworld_hud", "OverworldHudColors", 678, 16, None),
    ("flashing", "FlashingColors", 694, 16, None),
    ("title_screen", "TitleScreenColors", 710, 24, None),
    ("iggy_larry_platforms", "IggyLarryPlatColors", 734, 8, None),
    ("big_crusher", "BigCrusherColors", 742, 4, None),
    ("berries", "BerryColors", 746, 21, None),
    ("bowser", "BowserColors", 767, 56, None),
    ("the_end", "TheEndColors", 823, 18, None),
    ("overworld_special", "OWSpecialColors", 841, 168, None),
];

/// The table colour `colour` is in, and its number there.
pub fn table_of(colour: u16) -> Option<(&'static SharedTable, u16)> {
    TABLES
        .iter()
        .find(|t| (t.first..t.first + t.len).contains(&colour))
        .map(|t| (t, colour - t.first))
}

/// What a project changes of the shared palettes, by colour number among
/// all of them.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct SharedPalettes {
    pub colours: BTreeMap<u16, Color15>,
}

/// Every shared colour as `rom` has it.
pub fn read_all(rom: &Rom) -> Result<Vec<Color15>, RomError> {
    let bytes = rom.read(START, 2 * usize::from(COLOURS))?;
    Ok(bytes
        .chunks(2)
        .map(|w| Color15(u16::from_le_bytes([w[0], w[1]])))
        .collect())
}

impl SharedPalettes {
    pub fn is_empty(&self) -> bool {
        self.colours.is_empty()
    }

    /// The colours `rom` has that `clean` does not.
    pub fn changes(rom: &Rom, clean: &Rom) -> Result<Self, RomError> {
        let (theirs, ours) = (read_all(rom)?, read_all(clean)?);
        let colours = (0..COLOURS)
            .zip(theirs.iter().zip(&ours))
            .filter(|(_, (a, b))| a != b)
            .map(|(n, (a, _))| (n, *a))
            .collect();
        Ok(Self { colours })
    }

    /// Writes the colours where the game keeps them, as Lunar Magic does.
    pub fn write(&self, rom: &mut Rom) -> Result<(), RomError> {
        for (&n, colour) in &self.colours {
            rom.write_u16(START.add(2 * u32::from(n)), colour.0)?;
        }
        Ok(())
    }

    /// Writes the file in Kobo's format, with `top`, the comments before
    /// its first table.
    pub fn to_toml(&self, top: &[String]) -> String {
        let mut out = String::new();
        for line in top {
            out += &format!("{line}\n");
        }
        if !top.is_empty() {
            out.push('\n');
        }
        let mut first = true;
        for table in &TABLES {
            let colours: Vec<(u16, Color15)> = self
                .colours
                .range(table.first..table.first + table.len)
                .map(|(&n, &c)| (n - table.first, c))
                .collect();
            if colours.is_empty() {
                continue;
            }
            if !first {
                out.push('\n');
            }
            first = false;
            out += &format!("[{}]\n", table.name);
            for (index, colour) in colours {
                let line = format!("{} = \"{}\"", hex(u32::from(index), 2), color_text(colour));
                match table.place(index) {
                    Some(place) => out += &format!("{line}  # {place}\n"),
                    None => out += &format!("{line}\n"),
                }
            }
        }
        out
    }

    /// Reads a shared palettes file, with the comments before its first
    /// table.
    pub fn from_toml(text: &str) -> Result<(Self, Vec<String>), SourceError> {
        let doc: DocumentMut = text.parse()?;
        let mut out = Self::default();
        let mut top = None;
        for (key, item) in doc.iter() {
            let table = TABLES
                .iter()
                .find(|t| t.name == key)
                .ok_or_else(|| invalid(key, "is not one of the game's colour tables"))?;
            let entries: &Table = item
                .as_table()
                .ok_or_else(|| invalid(key, "must be a table of colours"))?;
            if top.is_none() {
                top = Some(own_line_comments(
                    entries
                        .decor()
                        .prefix()
                        .and_then(|p| p.as_str())
                        .unwrap_or(""),
                    false,
                ));
            }
            for (index, colour) in entries.iter() {
                let at = format!("{key}.{index}");
                let n = index
                    .strip_prefix("0x")
                    .and_then(|h| u16::from_str_radix(h, 16).ok())
                    .filter(|&n| n < table.len)
                    .ok_or_else(|| {
                        invalid(
                            &at,
                            format!(
                                "a colour of {key} is 0x00 to {}",
                                hex(u32::from(table.len - 1), 2)
                            ),
                        )
                    })?;
                let text = colour
                    .as_str()
                    .ok_or_else(|| invalid(&at, "must be a colour \"#RRGGBB\""))?;
                out.colours.insert(table.first + n, parse_color(&at, text)?);
            }
        }
        Ok((out, top.unwrap_or_default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_cover_every_colour_in_order() {
        let mut next = 0;
        for table in &TABLES {
            assert_eq!(table.first, next, "{}", table.name);
            next += table.len;
        }
        assert_eq!(next, COLOURS);
        // The disassembly's addresses.
        let at = |name: &str| TABLES.iter().find(|t| t.name == name).unwrap().addr().raw();
        assert_eq!(at("background"), 0x00B0B0);
        assert_eq!(at("sprite"), 0x00B318);
        assert_eq!(at("berries"), 0x00B674);
        assert_eq!(at("overworld_special"), 0x00B732);
    }

    #[test]
    fn a_file_reads_back_as_written() {
        let mut palettes = SharedPalettes::default();
        palettes.colours.insert(8 + 5, Color15::from_rgb5(31, 0, 8));
        palettes.colours.insert(841, Color15::from_rgb5(1, 2, 3));
        let text = palettes.to_toml(&["# Mine.".into()]);
        assert!(
            text.contains("[background]\n0x05 = \"#F80040\"  # palette 0, row 0, colour 7\n"),
            "{text}"
        );
        let (read, top) = SharedPalettes::from_toml(&text).unwrap();
        assert_eq!(read, palettes);
        assert_eq!(top, ["# Mine."]);
        assert!(SharedPalettes::from_toml("[background]\n0x60 = \"#000000\"\n").is_err());
        assert!(SharedPalettes::from_toml("[colours]\n").is_err());
    }
}
