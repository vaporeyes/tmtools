// ABOUTME: Learns how blocks connect by following real cars through real maps: for each block a car drove through, which
// ABOUTME: way it came in, and which block it drove into next, where that block sat, and which way it entered it.

use crate::frame::{cells, origin, to_local, units, Cell};
use std::collections::HashMap;
use tmassets::BlockInfo;
use tmreplay::{MapBlock, Replay, Sample};

/// The grid: 32 units across, 8 up
const SQUARE: f32 = 32.0;
const LEVEL: f32 = 8.0;
/// Path steps between looks at which block the car is in, in world units
const STEP: f32 = 2.0;
/// Slack on top of the distance the car's speed allows between samples before a step counts as a respawn
const TELEPORT_SLACK: f32 = 3.0;

/// Which way the car was going when it entered a block, in the block's unrotated frame: 0 is +Z (the block's
/// north), then 1, 2, 3 as directions turn. Spawn is the start block, where the car begins inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Entry {
    Spawn,
    Side(u8),
}

/// A block entered a certain way: what the generator chains. The variant is part of it, as some blocks change shape
/// with it (StadiumRoadMain is a straight, a curve, or a crossing by variant).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key {
    pub name: String,
    pub ground: bool,
    pub variant: u32,
    pub entry: Entry,
}

/// A block a car drove into next, placed relative to the block it left
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Link {
    pub next: Key,
    /// The next block's origin in the left block's unrotated frame, measured from the left block's origin
    pub offset: Cell,
    /// The next block's direction relative to the left block's
    pub turn: u8,
}

/// Every connection seen, with how often
#[derive(Debug, Default, Clone)]
pub struct Model {
    pub links: HashMap<Key, HashMap<Link, u32>>,
    pub infos: HashMap<String, BlockInfo>,
    /// Runs followed and blocks they drove through
    pub runs: usize,
    pub steps: usize,
}

impl Model {
    pub fn new(infos: &[BlockInfo]) -> Self {
        Self { infos: infos.iter().map(|i| (i.name.clone(), i.clone())).collect(), ..Default::default() }
    }

    pub fn units(&self, name: &str, ground: bool) -> Vec<Cell> {
        units(self.infos.get(name), ground)
    }

    /// Follows every run of a TMF Stadium replay through its map
    pub fn learn(&mut self, replay: &Replay) {
        let map = &replay.map;
        let (Some(blocks), Some(size)) = (&map.blocks, map.size) else { return };
        // TMF's Stadium frame only; TM2 maps sit eight levels higher
        if map.collection.as_deref() != Some("Stadium") || size[1] != 32 {
            return;
        }
        let solid: Vec<&MapBlock> = blocks.iter().filter(|b| !b.is_clip && self.infos.get(&b.name).is_none_or(|i| i.height.is_none())).collect();
        let mut at: HashMap<Cell, usize> = HashMap::new();
        for (i, b) in solid.iter().enumerate() {
            for c in cells([b.x, b.y, b.z], &self.units(&b.name, b.is_ground), b.dir) {
                at.insert(c, i);
            }
        }
        for ghost in &replay.ghosts {
            let path = visited(&ghost.samples, &at);
            if path.len() < 2 {
                continue;
            }
            self.runs += 1;
            self.steps += path.len();
            let mut entry = Entry::Spawn;
            for pair in path.windows(2) {
                let (a, b) = (solid[pair[0].0], solid[pair[1].0]);
                let b_entry = Entry::Side(side(pair[1].1, b.dir));
                self.add(a, entry, b, b_entry);
                entry = b_entry;
            }
        }
    }

    fn add(&mut self, a: &MapBlock, a_entry: Entry, b: &MapBlock, b_entry: Entry) {
        let a_origin = origin([a.x, a.y, a.z], &self.units(&a.name, a.is_ground), a.dir);
        let b_origin = origin([b.x, b.y, b.z], &self.units(&b.name, b.is_ground), b.dir);
        let link = Link {
            next: Key { name: b.name.clone(), ground: b.is_ground, variant: b.variant, entry: b_entry },
            offset: to_local(a_origin, a.dir, b_origin),
            turn: (b.dir + 4 - a.dir % 4) % 4,
        };
        let key = Key { name: a.name.clone(), ground: a.is_ground, variant: a.variant, entry: a_entry };
        *self.links.entry(key).or_default().entry(link).or_insert(0) += 1;
    }
}

/// Which side of a block the car came in through, from its heading on the map
fn side(heading: [f32; 2], dir: u8) -> u8 {
    // The heading in the block's frame, by the inverse of the block's turn
    let [x, z] = heading;
    let [lx, lz] = match dir % 4 {
        1 => [z, -x],
        2 => [-x, -z],
        3 => [-z, x],
        _ => [x, z],
    };
    if lz.abs() >= lx.abs() {
        if lz >= 0.0 { 0 } else { 2 }
    } else if lx < 0.0 {
        1
    } else {
        3
    }
}

/// The blocks a car drove through in order, each with the heading it had coming in; a respawn ends the path there
fn visited(samples: &[Sample], at: &HashMap<Cell, usize>) -> Vec<(usize, [f32; 2])> {
    let mut path: Vec<(usize, [f32; 2])> = Vec::new();
    let block_at = |p: [f32; 3]| {
        let (x, y, z) = ((p[0] / SQUARE).floor() as i32, (p[1] / LEVEL).floor() as i32, (p[2] / SQUARE).floor() as i32);
        // The car rides on the block below it, or inside it on slopes and in tubes
        at.get(&[x, y, z]).or_else(|| at.get(&[x, y - 1, z])).copied()
    };
    for pair in samples.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let d = [b.position.x - a.position.x, b.position.y - a.position.y, b.position.z - a.position.z];
        let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let seconds = (b.time_ms - a.time_ms).max(0) as f32 / 1000.0;
        if length > a.speed_kmh.max(b.speed_kmh) / 3.6 * seconds * 1.5 + TELEPORT_SLACK {
            break;
        }
        let steps = (length / STEP).ceil().max(1.0) as usize;
        for s in 1..=steps {
            let t = s as f32 / steps as f32;
            let p = [a.position.x + d[0] * t, a.position.y + d[1] * t, a.position.z + d[2] * t];
            if let Some(block) = block_at(p) {
                if path.last().is_none_or(|&(last, _)| last != block) {
                    path.push((block, [d[0], d[2]]));
                }
            }
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use tmreplay::{Ghost, ReplayMap, Vec3};

    fn block(name: &str, x: i32, z: i32, dir: u8) -> MapBlock {
        MapBlock { name: name.into(), x, y: 1, z, dir, is_ground: true, variant: 0, sub_variant: 0, is_clip: false }
    }

    /// A car driving north (+Z) along x = 2.5 cells from z = 0.5 to 2.5, at 36 km/h
    fn run(blocks: Vec<MapBlock>) -> Replay {
        let samples = (0..=20)
            .map(|i| Sample { time_ms: i * 1000, position: Vec3 { x: 80.0, y: 9.5, z: 16.0 + i as f32 * 3.2 }, speed_kmh: 36.0, ..Default::default() })
            .collect();
        let map = ReplayMap { collection: Some("Stadium".into()), size: Some([32, 32, 32]), blocks: Some(blocks), ..Default::default() };
        Replay { map, ghosts: vec![Ghost { samples, ..Default::default() }], embedded_map: None }
    }

    #[test]
    fn learns_each_block_driven_into_next_relative_to_the_last() {
        let mut model = Model::new(&[]);
        model.learn(&run(vec![block("Start", 2, 0, 0), block("Road", 2, 1, 0), block("Finish", 2, 2, 0)]));
        let start = Key { name: "Start".into(), ground: true, variant: 0, entry: Entry::Spawn };
        let road = Key { name: "Road".into(), ground: true, variant: 0, entry: Entry::Side(0) };
        let links = &model.links[&start];
        assert_eq!(links.len(), 1);
        let (link, count) = links.iter().next().unwrap();
        assert_eq!((link.next.clone(), link.offset, link.turn, *count), (road.clone(), [0, 0, 1], 0, 1));
        assert!(model.links[&road].keys().any(|l| l.next.name == "Finish"));
    }

    #[test]
    fn a_turned_block_sees_the_same_link_in_its_own_frame() {
        // The same road turned east: the car still drives north on the map, which is the blocks' side 3 (+X local)
        let mut model = Model::new(&[]);
        model.learn(&run(vec![block("Start", 2, 0, 1), block("Road", 2, 1, 1), block("Finish", 2, 2, 1)]));
        let start = Key { name: "Start".into(), ground: true, variant: 0, entry: Entry::Spawn };
        let (link, _) = model.links[&start].iter().next().unwrap();
        assert_eq!((link.next.entry, link.offset, link.turn), (Entry::Side(3), [1, 0, 0], 0));
    }
}
