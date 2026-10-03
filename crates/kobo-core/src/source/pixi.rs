//! A hack's PIXI insert carried as compiled code (`[pixi] compiled`), for
//! a project imported without its sprites' sources ([`crate::pixi`]).
//!
//! ```toml
//! version = 0x32  # PIXI 1.32
//! sprites_255 = true
//!
//! [sites]
//! 0x02A963 = "5C 00 80 94 EA"
//!
//! [blocks]
//! 0x948008 = "compiled/948008.bin"
//! ```
//!
//! Sites are bytes at fixed addresses, written as they are; blocks are
//! files of the bytes a RATS block holds, named by the address of their
//! first byte and relative to this file, which a build writes back there
//! with their tags. It is not source: the bytes are what PIXI assembled
//! for the hack.

use std::collections::BTreeMap;
use std::path::PathBuf;

use toml_edit::DocumentMut;

use super::{
    Comments, SourceError, Writer, hex, hex_bytes, invalid, parse_hex_bytes, read_comments,
};
use crate::addr::SnesAddr;
use crate::pixi::version_name;

/// The file's content: the insert, with its blocks named by file.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Compiled {
    pub version: u8,
    pub sprites_255: bool,
    pub sites: BTreeMap<SnesAddr, Vec<u8>>,
    pub blocks: BTreeMap<SnesAddr, PathBuf>,
    /// PIXI's size table alone, which a build places where it has room
    /// (`pixi::Insert::size_table`).
    pub size_table: Option<PathBuf>,
}

/// The file a block is kept in, relative to the insert's file.
pub fn block_file(at: SnesAddr) -> PathBuf {
    PathBuf::from("compiled").join(format!("{:06X}.bin", at.raw()))
}

fn key(at: SnesAddr) -> String {
    hex(at.raw(), 6)
}

fn address(at: &str, key: &str) -> Result<SnesAddr, SourceError> {
    key.strip_prefix("0x")
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .filter(|&n| n <= 0xFF_FFFF)
        .map(SnesAddr::new)
        .ok_or_else(|| invalid(at, "an address is 0x000000 to 0xFFFFFF"))
}

impl Compiled {
    /// Writes the file in Kobo's format, with the user's comments.
    pub fn to_toml(&self, comments: &Comments) -> String {
        let mut out = Writer::new(comments);
        out.line(format_args!(
            "version = {}  # PIXI {}",
            hex(self.version as u32, 2),
            version_name(self.version)
        ));
        if self.sprites_255 {
            out.line("sprites_255 = true");
        }
        if let Some(file) = &self.size_table {
            let path = toml_edit::Value::from(file.to_string_lossy().replace('\\', "/"));
            out.line(format_args!("size_table = {path}"));
        }
        if !self.sites.is_empty() {
            out.table("sites");
            for (&at, bytes) in &self.sites {
                out.key("sites", &key(at), hex_bytes(bytes));
            }
        }
        if !self.blocks.is_empty() {
            out.table("blocks");
            for (&at, file) in &self.blocks {
                let path = toml_edit::Value::from(file.to_string_lossy().replace('\\', "/"));
                out.key("blocks", &key(at), path);
            }
        }
        out.finish()
    }

    /// Reads the file, with the own-line comments to keep.
    pub fn from_toml(text: &str) -> Result<(Self, Comments), SourceError> {
        let doc: DocumentMut = text.parse()?;
        let comments = read_comments(&doc);
        let mut out = Self::default();
        let mut version = None;
        for (name, item) in doc.iter() {
            match name {
                "version" => {
                    version = Some(
                        item.as_integer()
                            .and_then(|n| u8::try_from(n).ok())
                            .ok_or_else(|| {
                                invalid("version", "is PIXI's version byte, 0x00 to 0xFF")
                            })?,
                    );
                }
                "size_table" => {
                    let file = item
                        .as_str()
                        .ok_or_else(|| invalid("size_table", "must be a file path"))?;
                    out.size_table = Some(PathBuf::from(file));
                }
                "sprites_255" => {
                    out.sprites_255 = item
                        .as_bool()
                        .ok_or_else(|| invalid("sprites_255", "must be true or false"))?;
                }
                "sites" | "blocks" => {
                    let table = item
                        .as_table()
                        .ok_or_else(|| invalid(name, "must be a table"))?;
                    for (k, value) in table.iter() {
                        let at = format!("{name}.{k}");
                        let addr = address(&at, k)?;
                        let text = value
                            .as_str()
                            .ok_or_else(|| invalid(&at, "must be a string"))?;
                        if name == "sites" {
                            let bytes = parse_hex_bytes(&at, text)?;
                            if bytes.is_empty() {
                                return Err(invalid(&at, "has no bytes"));
                            }
                            out.sites.insert(addr, bytes);
                        } else {
                            out.blocks.insert(addr, PathBuf::from(text));
                        }
                    }
                }
                _ => return Err(invalid(name, "is not a key of a compiled PIXI insert")),
            }
        }
        out.version = version.ok_or_else(|| invalid("version", "is missing"))?;
        Ok((out, comments))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let compiled = Compiled {
            version: 0x32,
            sprites_255: true,
            sites: BTreeMap::from([
                (SnesAddr::new(0x02A963), vec![0x5C, 0x00, 0x80, 0x94, 0xEA]),
                (SnesAddr::new(0x0EF30C), vec![0x08, 0x80, 0x94, 0x42]),
            ]),
            blocks: BTreeMap::from([(
                SnesAddr::new(0x948008),
                block_file(SnesAddr::new(0x948008)),
            )]),
            size_table: None,
        };
        let mut comments = Comments::default();
        comments.add(Comments::TOP, ["# Carried from a hack.".to_string()]);
        let text = compiled.to_toml(&comments);
        assert_eq!(
            text,
            "# Carried from a hack.\n\nversion = 0x32  # PIXI 1.32\nsprites_255 = true\n\n\
             [sites]\n0x02A963 = \"5C 00 80 94 EA\"\n0x0EF30C = \"08 80 94 42\"\n\n\
             [blocks]\n0x948008 = \"compiled/948008.bin\"\n"
        );
        let (again, kept) = Compiled::from_toml(&text).unwrap();
        assert_eq!(again, compiled);
        assert_eq!(again.to_toml(&kept), text);
    }

    #[test]
    fn refusals() {
        let bad = |text: &str| Compiled::from_toml(text).is_err();
        assert!(bad(""));
        assert!(bad("version = 0x100\n"));
        assert!(bad("version = 1\nextra = 1\n"));
        assert!(bad("version = 1\n[sites]\n0x1000000 = \"00\"\n"));
        assert!(bad("version = 1\n[sites]\n0x02A963 = \"\"\n"));
        assert!(bad("version = 1\n[sites]\n0x02A963 = \"5C0\"\n"));
        assert!(!bad("version = 1\n"));
    }
}
