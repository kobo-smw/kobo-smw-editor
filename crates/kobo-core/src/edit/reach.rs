//! How levels are reached, for listing them as their players meet them.
//!
//! The overworld enters a level by its translevel ([`translevel`]): those
//! are the levels a player knows by name. Every other level is a
//! sublevel, reached through a screen exit of another, as Lunar Magic's
//! users know them. [`Reach`] puts each sublevel under the first
//! overworld level whose exits lead to it, directly or through other
//! sublevels.
//!
//! [`Placeholder`] is the level data most of the game's 512 levels share:
//! the "TEST" level the game's unused numbers all point at (277 of them
//! in the vanilla ROM). A level with its objects has not been made.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use super::{ExitTarget, Workspace};
use crate::level::objects::Object;
use crate::level::{self, translevel};
use crate::rom::Rom;
use crate::source::level::{Layer2, Level};

/// An overworld level and the sublevels its exits reach.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Group {
    pub level: u16,
    /// In number order.
    pub sublevels: Vec<u16>,
}

/// The levels of a list, grouped by how they are reached.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Reach {
    /// Every overworld level listed, in number order: `000`-`024`, then
    /// `101`-`13B`.
    pub groups: Vec<Group>,
    /// Sublevels listed that no listed level's exits reach.
    pub unreached: Vec<u16>,
}

impl Reach {
    /// Groups `listed`, following the exits of the levels `level` gives
    /// (the project's as the workspace has them, the game's own for the
    /// rest). An exit through a level not listed is followed all the same.
    pub fn of(workspace: &Workspace, listed: &[u16]) -> Self {
        let levels: BTreeMap<u16, Level> = (0..0x200)
            .filter_map(|n| {
                let level = match workspace.level(n) {
                    Some(level) => level.clone(),
                    None => workspace.clean_level(n).ok()?,
                };
                Some((n, level))
            })
            .collect();
        // Secondary entrances by the level they are in, the project's
        // first; the clean ROM's tables for any no level has.
        let mut entrances: HashMap<u16, u16> = HashMap::new();
        for (&number, level) in &levels {
            for entrance in &level.entrances {
                entrances.entry(entrance.id).or_insert(number);
            }
        }
        let leads = |number: u16, level: &Level| -> Vec<u16> {
            exits(level)
                .filter_map(|exit| {
                    let target = ExitTarget::of(exit, number);
                    if target.secondary {
                        entrances
                            .get(&target.destination)
                            .copied()
                            .or_else(|| workspace.entrance_level(target.destination))
                    } else {
                        Some(target.destination)
                    }
                })
                .filter(|&to| to != number && to < 0x200)
                .collect()
        };
        let graph: BTreeMap<u16, Vec<u16>> = levels
            .iter()
            .map(|(&number, level)| (number, leads(number, level)))
            .collect();
        Self::from_graph(&graph, listed)
    }

    /// Groups `listed` by the exits in `graph`, each level's list of the
    /// levels it leads to.
    pub fn from_graph(graph: &BTreeMap<u16, Vec<u16>>, listed: &[u16]) -> Self {
        let listed: BTreeSet<u16> = listed.iter().copied().collect();
        let mut owner: BTreeMap<u16, u16> = BTreeMap::new();
        let roots: Vec<u16> = listed
            .iter()
            .copied()
            .filter(|&n| translevel(n).is_some())
            .collect();
        let mut seen: BTreeSet<u16> = roots.iter().copied().collect();
        for &root in &roots {
            let mut queue = VecDeque::from([root]);
            while let Some(level) = queue.pop_front() {
                for &to in graph.get(&level).into_iter().flatten() {
                    // Another overworld level is a group of its own.
                    if translevel(to).is_some() || !seen.insert(to) {
                        continue;
                    }
                    owner.insert(to, root);
                    queue.push_back(to);
                }
            }
        }
        let groups = roots
            .iter()
            .map(|&root| Group {
                level: root,
                sublevels: owner
                    .iter()
                    .filter(|&(n, &r)| r == root && listed.contains(n))
                    .map(|(&n, _)| n)
                    .collect(),
            })
            .collect();
        let unreached = listed
            .iter()
            .copied()
            .filter(|n| translevel(*n).is_none() && !owner.contains_key(n))
            .collect();
        Self { groups, unreached }
    }

    /// The overworld level whose group `number` is in.
    pub fn group_of(&self, number: u16) -> Option<u16> {
        self.groups
            .iter()
            .find(|g| g.level == number || g.sublevels.contains(&number))
            .map(|g| g.level)
    }
}

/// A level's screen exits, on either layer.
fn exits(level: &Level) -> impl Iterator<Item = crate::level::objects::ScreenExit> + '_ {
    let layer2: &[Object] = match &level.layer2 {
        Layer2::Objects(list) => list,
        _ => &[],
    };
    level.layer1.iter().chain(layer2).filter_map(|o| match o {
        Object::ScreenExit(exit) => Some(*exit),
        _ => None,
    })
}

/// The level data most of the game's levels share, which its unused
/// numbers point at.
#[derive(Clone, Debug)]
pub struct Placeholder {
    level: Level,
}

impl Placeholder {
    /// The clean ROM's: the layer 1 data more of its levels point at than
    /// any other, if more than one does.
    pub fn of(clean: &Rom) -> Option<Self> {
        let mut counts: BTreeMap<u32, (usize, u16)> = BTreeMap::new();
        for number in 0..0x200 {
            if let Ok(at) = level::layer1_ptr(clean, number) {
                counts.entry(at.raw()).or_insert((0, number)).0 += 1;
            }
        }
        let (count, number) = counts.values().max_by_key(|(count, _)| *count)?;
        if *count < 2 {
            return None;
        }
        let (level, _) = crate::import::read_level(clean, *number).ok()?;
        Some(Self { level })
    }

    /// Whether `level` is the placeholder: the same objects, whatever
    /// else differs (the levels that share it have backgrounds of their
    /// own).
    pub fn is(&self, level: &Level) -> bool {
        let layer2 = match &self.level.layer2 {
            Layer2::Objects(_) => level.layer2 == self.level.layer2,
            _ => !matches!(level.layer2, Layer2::Objects(_)),
        };
        level.layer1 == self.level.layer1 && layer2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sublevels_go_under_the_first_overworld_level_to_reach_them() {
        // 105 leads to 1C0, which leads to 1C1; 106 leads to 1C1 too and
        // to 105, another overworld level; 1C2 is reached by nothing.
        let graph = BTreeMap::from([
            (0x105, vec![0x1C0]),
            (0x106, vec![0x1C1, 0x105]),
            (0x1C0, vec![0x1C1, 0x105]),
            (0x1C1, vec![]),
            (0x1C2, vec![]),
        ]);
        let reach = Reach::from_graph(&graph, &[0x105, 0x106, 0x1C0, 0x1C1, 0x1C2]);
        assert_eq!(
            reach.groups,
            [
                Group {
                    level: 0x105,
                    sublevels: vec![0x1C0, 0x1C1]
                },
                Group {
                    level: 0x106,
                    sublevels: vec![]
                },
            ]
        );
        assert_eq!(reach.unreached, [0x1C2]);
        assert_eq!(reach.group_of(0x1C1), Some(0x105));
        // A level between two that is not listed is gone through.
        let reach = Reach::from_graph(&graph, &[0x105, 0x1C1]);
        assert_eq!(reach.groups[0].sublevels, [0x1C1]);
    }
}
