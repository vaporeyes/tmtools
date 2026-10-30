// ABOUTME: Block geometry on the map grid: a block's cells from its units and direction, the origin of its unrotated
// ABOUTME: frame, and moving offsets between that frame and the map. Rotation follows tmscene's placement rules.

use tmassets::BlockInfo;

pub type Cell = [i32; 3];

/// A unit offset turned to a direction (0 north, 1 east, 2 south, 3 west), as the game places blocks
pub fn rotate(u: Cell, dir: u8) -> Cell {
    let [x, y, z] = u;
    match dir % 4 {
        1 => [-z, y, x],
        2 => [-x, y, -z],
        3 => [z, y, -x],
        _ => [x, y, z],
    }
}

/// The inverse of rotate: a map offset back into a block's unrotated frame
pub fn unrotate(w: Cell, dir: u8) -> Cell {
    rotate(w, (4 - dir % 4) % 4)
}

fn add(a: Cell, b: Cell) -> Cell {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: Cell, b: Cell) -> Cell {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// A block's unit offsets; one unit when the block isn't known or has a single one
pub fn units(info: Option<&BlockInfo>, is_ground: bool) -> Vec<Cell> {
    let list = info.map(|i| if is_ground { &i.ground_units } else { &i.air_units });
    match list {
        Some(units) if units.len() > 1 => units.iter().map(|u| [u.offset.x, u.offset.y, u.offset.z]).collect(),
        _ => vec![[0, 0, 0]],
    }
}

/// How far the rotated units reach below zero on X and Z; the game shifts them back so the block starts at its coordinate
fn min_shift(units: &[Cell], dir: u8) -> Cell {
    let rotated: Vec<Cell> = units.iter().map(|&u| rotate(u, dir)).collect();
    [rotated.iter().map(|r| r[0]).min().unwrap_or(0), 0, rotated.iter().map(|r| r[2]).min().unwrap_or(0)]
}

/// Where a block's unit (0, 0, 0) sits on the map, given its coordinate and direction
pub fn origin(anchor: Cell, units: &[Cell], dir: u8) -> Cell {
    sub(anchor, min_shift(units, dir))
}

/// The coordinate a block needs for its unit (0, 0, 0) to sit at origin
pub fn anchor(origin: Cell, units: &[Cell], dir: u8) -> Cell {
    add(origin, min_shift(units, dir))
}

/// The map cells a block covers
pub fn cells(anchor: Cell, units: &[Cell], dir: u8) -> Vec<Cell> {
    let o = origin(anchor, units, dir);
    units.iter().map(|&u| add(o, rotate(u, dir))).collect()
}

/// A map offset from a block's origin, in that block's unrotated frame
pub fn to_local(from_origin: Cell, dir: u8, world: Cell) -> Cell {
    unrotate(sub(world, from_origin), dir)
}

/// A point in a block's unrotated frame, back on the map
pub fn to_world(from_origin: Cell, dir: u8, local: Cell) -> Cell {
    add(from_origin, rotate(local, dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_round_trips() {
        for dir in 0..4 {
            assert_eq!(unrotate(rotate([2, 1, -3], dir), dir), [2, 1, -3]);
        }
    }

    #[test]
    fn multi_unit_blocks_start_at_their_coordinate() {
        // A block two units long on Z, turned east, lies along -X and is shifted back to start at its coordinate
        let units = [[0, 0, 0], [0, 0, 1]];
        assert_eq!(cells([5, 1, 5], &units, 1), vec![[6, 1, 5], [5, 1, 5]]);
        let o = origin([5, 1, 5], &units, 1);
        assert_eq!(anchor(o, &units, 1), [5, 1, 5]);
    }

    #[test]
    fn local_offsets_follow_the_block() {
        // One cell ahead (north, +Z) of a block facing north is one cell west (+X) of the same block turned east
        let ahead = to_world([0, 0, 0], 0, [0, 0, 1]);
        assert_eq!(ahead, [0, 0, 1]);
        assert_eq!(to_world([0, 0, 0], 1, [0, 0, 1]), [-1, 0, 0]);
        assert_eq!(to_local([0, 0, 0], 1, [-1, 0, 0]), [0, 0, 1]);
    }
}
