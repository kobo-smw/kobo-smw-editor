//! The player carried along a path, for the probes that play a level
//! (`gfx_probe`, `layer3_probe`): legs `dx,dy@frames/dx,dy@frames/...`,
//! each moving the player by (dx, dy) pixels a frame for its frames, the
//! last for good. Included with `#[path]`.

use kobo_core::ram::{Ram, RamAddr};

/// The player's position on `frame` of `path` from `start`, and whether a
/// leg moves vertically.
pub fn place(path: &str, frame: u32, start: (u16, u16)) -> ((u16, u16), bool) {
    let (mut x, mut y) = (start.0 as i32, start.1 as i32);
    let mut left = frame as i32;
    let mut vertical = false;
    for leg in path.split('/') {
        let (step, n) = match leg.split_once('@') {
            Some((s, n)) => (s, n.parse::<i32>().unwrap()),
            None => (leg, i32::MAX),
        };
        let (dx, dy) = step
            .split_once(',')
            .map(|(a, b)| (a.parse::<i32>().unwrap(), b.parse::<i32>().unwrap()))
            .unwrap_or((0, 0));
        vertical |= dy != 0;
        let k = left.min(n);
        x += dx * k;
        y += dy * k;
        left -= k;
        if left == 0 {
            break;
        }
    }
    ((x.max(0) as u16, y.max(0) as u16), vertical)
}

/// Puts the player where `path` has them on `frame`, with no speed, and
/// lets the camera scroll vertically if a leg moves that way;
/// `invulnerable` keeps a path through enemies or lava going.
pub fn steer(path: &str, frame: u32, ram: &mut Ram, start: (u16, u16), invulnerable: bool) {
    let ((x, y), vertical) = place(path, frame, start);
    ram.set_u16(RamAddr::new(0x7E_0094), x);
    ram.set_u16(RamAddr::new(0x7E_0096), y);
    ram.set_u16(RamAddr::new(0x7E_00D1), x);
    ram.set_u16(RamAddr::new(0x7E_00D3), y);
    ram.set_u8(RamAddr::new(0x7E_007B), 0);
    ram.set_u8(RamAddr::new(0x7E_007D), 0);
    if invulnerable {
        ram.set_u8(RamAddr::new(0x7E_1497), 0x7F);
    }
    if vertical {
        ram.set_u8(RamAddr::new(0x7E_1412), 1);
        ram.set_u8(RamAddr::new(0x7E_1404), 1);
        ram.set_u8(RamAddr::new(0x7E_13F1), 1);
    }
}
