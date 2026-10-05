//! The hash fixtures in `tests/fixtures/`: one record a line, fields
//! split by white space, `#` comments and blank lines skipped. Hashes go
//! there, never the bytes they are of.

use std::collections::HashMap;

use sha1::{Digest, Sha1};

/// Every record's fields.
pub fn records(text: &str) -> Vec<Vec<&str>> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| l.split_whitespace().collect())
        .collect()
}

/// The records as their first field to their second: a ROM's SHA-1 to
/// its export's, say.
pub fn pairs(text: &str) -> HashMap<String, String> {
    records(text)
        .into_iter()
        .map(|r| (r[0].to_owned(), r[1].to_owned()))
        .collect()
}

/// The SHA-1 of `bytes`, in lower-case hex.
pub fn sha1_hex(bytes: &[u8]) -> String {
    Sha1::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
