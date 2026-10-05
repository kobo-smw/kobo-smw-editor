//! What changed between two versions of a level, entry by entry: for the
//! editor's view of what an edit or a commit did, and for undoing one
//! change of many.
//!
//! The lists are matched in order, the longest run of entries the two
//! have alike kept as they are (a longest common subsequence). Of what is
//! left, an entry of the old list and one of the new of the same kind (the
//! same object number, the same sprite) pair up as changed: moved,
//! resized, or set otherwise. The rest were removed or added.

use super::{Edit, ObjectLayer, insert_sprite, move_sprites};
use crate::level::objects::Object;
use crate::source::level::{Layer2, Level, Sprite};

/// One entry's change, by its index in the old list, the new, or both;
/// `at` is where in the new list the old entry belongs, among the entries
/// the two have alike.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryChange {
    Added(usize),
    Removed { old: usize, at: usize },
    Changed { old: usize, new: usize, at: usize },
}

/// What changed between two versions of a level.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct LevelDiff {
    pub layer1: Vec<EntryChange>,
    pub layer2: Vec<EntryChange>,
    pub sprites: Vec<EntryChange>,
    /// The primary header, the secondary header (the main entrance), or
    /// Lunar Magic's settings and size.
    pub header: bool,
    pub entrance: bool,
    pub settings: bool,
    /// Anything else: the secondary entrances, the palette, the graphics,
    /// the animation, layer 2's background.
    pub other: bool,
}

impl LevelDiff {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Changes, all told.
    pub fn len(&self) -> usize {
        self.layer1.len()
            + self.layer2.len()
            + self.sprites.len()
            + [self.header, self.entrance, self.settings, self.other]
                .iter()
                .filter(|&&b| b)
                .count()
    }

    pub fn objects(&self, layer: ObjectLayer) -> &[EntryChange] {
        match layer {
            ObjectLayer::One => &self.layer1,
            ObjectLayer::Two => &self.layer2,
        }
    }
}

/// What makes two objects the same kind, for pairing a changed one.
fn object_kind(object: &Object) -> (u8, u16) {
    match object {
        Object::Standard { number, .. } => (0, u16::from(*number)),
        Object::Extended { number, .. } => (1, u16::from(*number)),
        Object::ScreenExit(exit) => (2, u16::from(exit.screen)),
        Object::Lunar { number, .. } => (3, u16::from(*number)),
        Object::Unplaced(bytes) => (4, bytes.first().copied().map_or(0, u16::from)),
    }
}

/// The changes between two lists, in the order of the new one (removals
/// where they stood).
fn diff_list<T: PartialEq, K: PartialEq>(
    old: &[T],
    new: &[T],
    kind: impl Fn(&T) -> K,
) -> Vec<EntryChange> {
    // The longest common subsequence, by dynamic programming from the
    // ends: `lcs[i][j]` is its length for `old[i..]` and `new[j..]`.
    let (n, m) = (old.len(), new.len());
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if old[i] == new[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let (mut removed, mut added) = (Vec::new(), Vec::new());
    while i < n && j < m {
        if old[i] == new[j] {
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            removed.push((i, j));
            i += 1;
        } else {
            added.push(j);
            j += 1;
        }
    }
    removed.extend((i..n).map(|i| (i, m)));
    added.extend(j..m);
    let mut changes = Vec::new();
    let mut paired = vec![false; added.len()];
    for &(o, at) in &removed {
        let partner = added
            .iter()
            .enumerate()
            .find(|&(k, &a)| !paired[k] && kind(&old[o]) == kind(&new[a]));
        match partner {
            Some((k, &a)) => {
                paired[k] = true;
                changes.push(EntryChange::Changed { old: o, new: a, at });
            }
            None => changes.push(EntryChange::Removed { old: o, at }),
        }
    }
    for (k, &a) in added.iter().enumerate() {
        if !paired[k] {
            changes.push(EntryChange::Added(a));
        }
    }
    changes
}

fn layer2_objects(level: &Level) -> &[Object] {
    match &level.layer2 {
        Layer2::Objects(list) => list,
        _ => &[],
    }
}

/// What changed from `old` to `new`.
pub fn diff(old: &Level, new: &Level) -> LevelDiff {
    let sprite_kind = |s: &Sprite| s.id;
    LevelDiff {
        layer1: diff_list(&old.layer1, &new.layer1, object_kind),
        layer2: diff_list(layer2_objects(old), layer2_objects(new), object_kind),
        sprites: diff_list(&old.sprites.list, &new.sprites.list, sprite_kind),
        header: old.header != new.header,
        entrance: old.entrance != new.entrance,
        settings: old.settings != new.settings || old.size != new.size,
        other: old.entrances != new.entrances
            || old.palette != new.palette
            || old.graphics != new.graphics
            || old.animation != new.animation
            || old.animation_settings != new.animation_settings
            || (!matches!(old.layer2, Layer2::Objects(_)) && old.layer2 != new.layer2)
            || old.sprites.memory != new.sprites.memory
            || old.sprites.buoyancy != new.sprites.buoyancy
            || old.sprites.buoyancy_no_layer2 != new.sprites.buoyancy_no_layer2,
    }
}

/// What a change of a layer's objects is about, for undoing it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    Objects(ObjectLayer),
    Sprites,
}

/// The edits that take one change of `new` back to how `old` had it.
/// Indices in `new` are as the diff found them, so take one change back at
/// a time and diff again.
pub fn revert(old: &Level, new: &Level, part: Part, change: EntryChange) -> Vec<Edit> {
    match part {
        Part::Objects(layer) => {
            let (old_list, new_list) = match layer {
                ObjectLayer::One => (&old.layer1[..], &new.layer1[..]),
                ObjectLayer::Two => (layer2_objects(old), layer2_objects(new)),
            };
            match change {
                EntryChange::Added(index) => vec![Edit::RemoveObject { layer, index }],
                EntryChange::Removed { old: o, at } => old_list
                    .get(o)
                    .map(|object| Edit::InsertObject {
                        layer,
                        index: at.min(new_list.len()),
                        object: object.clone(),
                    })
                    .into_iter()
                    .collect(),
                EntryChange::Changed {
                    old: o,
                    new: index,
                    at,
                } => {
                    let Some(object) = old_list.get(o) else {
                        return Vec::new();
                    };
                    let mut edits = vec![Edit::ReplaceObject {
                        layer,
                        index,
                        object: object.clone(),
                    }];
                    // Back where it stood among the rest: `at` counts the
                    // entry itself when it is before.
                    let to = if index < at { at - 1 } else { at }.min(new_list.len() - 1);
                    if to != index {
                        edits.push(Edit::ReorderObject {
                            layer,
                            from: index,
                            to,
                        });
                    }
                    edits
                }
            }
        }
        Part::Sprites => match change {
            EntryChange::Added(index) => vec![Edit::RemoveSprite { index }],
            EntryChange::Removed { old: o, .. } => old
                .sprites
                .list
                .get(o)
                .map(|sprite| insert_sprite(new, sprite.clone()).0)
                .into_iter()
                .collect(),
            EntryChange::Changed {
                old: o, new: index, ..
            } => old
                .sprites
                .list
                .get(o)
                .and_then(|sprite| move_sprites(new, &[(index, sprite.clone())]).ok())
                .map(|(edits, _)| edits)
                .unwrap_or_default(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_match_in_order_and_pair_by_kind() {
        // a b c d -> a c' d e: b removed, c changed, e added.
        let old = [(1, 0), (2, 0), (3, 0), (4, 0)];
        let new = [(1, 0), (3, 9), (4, 0), (5, 0)];
        let changes = diff_list(&old, &new, |t| t.0);
        assert_eq!(
            changes,
            [
                EntryChange::Removed { old: 1, at: 1 },
                EntryChange::Changed {
                    old: 2,
                    new: 1,
                    at: 1
                },
                EntryChange::Added(3),
            ]
        );
        assert!(diff_list(&old, &old, |t| t.0).is_empty());
    }

    #[test]
    fn a_moved_entry_is_changed() {
        let old = [(7, 1), (8, 1)];
        let new = [(8, 1), (7, 2)];
        let changes = diff_list(&old, &new, |t| t.0);
        assert_eq!(
            changes,
            [EntryChange::Changed {
                old: 0,
                new: 1,
                at: 0
            }]
        );
    }
}
