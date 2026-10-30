// ABOUTME: Generates a track from learned connections: a seeded, weighted depth-first search from a start line through
// ABOUTME: pieces of one style to a finish, with checkpoints spaced along it, every piece inside the map with clear air above.

use crate::frame::{anchor, cells, to_world, Cell};
use crate::learn::{Entry, Key, Link, Model};
use std::collections::HashSet;

/// Which blocks a style may use
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Stadium's platforms with main road, tilted road, and the pieces between them
    PlatformTech,
}

impl Style {
    pub fn allows(self, name: &str) -> bool {
        match self {
            Style::PlatformTech => ["StadiumPlatform", "StadiumRoadMain", "StadiumRoadTilt"].iter().any(|p| name.starts_with(p)),
        }
    }
}

/// What a block is for in a race
pub fn is_start(name: &str) -> bool {
    name.contains("StartLine") && !name.contains("StartFinish")
}

pub fn is_finish(name: &str) -> bool {
    name.contains("Finish") && !name.contains("StartFinish")
}

pub fn is_checkpoint(name: &str) -> bool {
    name.contains("Checkpoint")
}

#[derive(Debug, Clone, Copy)]
pub struct Settings {
    pub seed: u64,
    pub style: Style,
    /// Pieces from start to finish, roughly; the finish comes once there are at least this many
    pub length: usize,
    /// Pieces between checkpoints
    pub checkpoint_every: usize,
    /// Map size in cells
    pub size: Cell,
}

impl Default for Settings {
    fn default() -> Self {
        Self { seed: 1, style: Style::PlatformTech, length: 30, checkpoint_every: 8, size: [32, 32, 32] }
    }
}

/// A block of the generated track
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub name: String,
    pub anchor: Cell,
    pub dir: u8,
    pub ground: bool,
    pub variant: u32,
}

/// The level ground blocks stand on in a 32-tall Stadium map
const GROUND: i32 = 1;
/// Search steps across all starts before giving up
const BUDGET: usize = 300_000;
/// Starts tried before giving up
const STARTS: usize = 24;

/// A small deterministic generator (xorshift64*), so a seed always gives the same track
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number in (0, 1]
    fn unit(&mut self) -> f64 {
        ((self.next() >> 11) as f64 + 1.0) / (1u64 << 53) as f64
    }
}

struct Search<'a> {
    model: &'a Model,
    settings: Settings,
    rng: Rng,
    track: Vec<Placed>,
    /// Cells taken by blocks, and the air kept clear above them
    taken: HashSet<Cell>,
    clear: HashSet<Cell>,
    checkpoints: usize,
    since_checkpoint: usize,
    steps: usize,
}

/// Generates a track, or None when no start leads to a finish within the search budget
pub fn generate(model: &Model, settings: Settings) -> Option<Vec<Placed>> {
    let mut rng = Rng::new(settings.seed);
    let mut starts: Vec<&Key> = model.links.keys().filter(|k| k.entry == Entry::Spawn && is_start(&k.name) && settings.style.allows(&k.name)).collect();
    starts.sort();
    if starts.is_empty() {
        return None;
    }
    let mut steps = 0;
    for _ in 0..STARTS {
        let key = starts[(rng.next() % starts.len() as u64) as usize];
        let dir = (rng.next() % 4) as u8;
        let [sx, _, sz] = settings.size;
        // Somewhere central, so the track has room to wander
        let at = [sx / 4 + (rng.next() % (sx as u64 / 2)) as i32, GROUND, sz / 4 + (rng.next() % (sz as u64 / 2)) as i32];
        let mut search = Search { model, settings, rng: Rng::new(rng.next()), track: Vec::new(), taken: HashSet::new(), clear: HashSet::new(), checkpoints: 0, since_checkpoint: 0, steps };
        let units = model.units(&key.name, key.ground);
        let start_anchor = anchor(at, &units, dir);
        if !search.fits(&key.name, key.ground, start_anchor, dir) {
            continue;
        }
        search.place(key, start_anchor, dir);
        if search.extend(key, at, dir) {
            return Some(search.track);
        }
        steps = search.steps;
        if steps > BUDGET {
            break;
        }
    }
    None
}

impl Search<'_> {
    fn cells_of(&self, name: &str, ground: bool, anchor: Cell, dir: u8) -> Vec<Cell> {
        cells(anchor, &self.model.units(name, ground), dir)
    }

    /// Inside the map, at the right height for its kind, and clear of the track and the air above it
    fn fits(&self, name: &str, ground: bool, anchor: Cell, dir: u8) -> bool {
        if ground != (anchor[1] == GROUND) {
            return false;
        }
        let [sx, sy, sz] = self.settings.size;
        self.cells_of(name, ground, anchor, dir).iter().all(|&[x, y, z]| {
            (0..sx).contains(&x) && (GROUND..sy - 1).contains(&y) && (0..sz).contains(&z) && !self.taken.contains(&[x, y, z]) && !self.clear.contains(&[x, y, z]) && !self.taken.contains(&[x, y + 1, z])
        })
    }

    /// Whether a piece touches the last one placed: players sometimes jumped from one block to another a few cells
    /// away, which only works at their speed, so generated tracks stay on connected pieces
    fn touches(&self, name: &str, ground: bool, anchor: Cell, dir: u8) -> bool {
        let Some(last) = self.track.last() else { return true };
        let last_cells = self.cells_of(&last.name, last.ground, last.anchor, last.dir);
        self.cells_of(name, ground, anchor, dir).iter().any(|a| last_cells.iter().any(|b| (0..3).all(|i| (a[i] - b[i]).abs() <= 1)))
    }

    fn place(&mut self, key: &Key, anchor: Cell, dir: u8) {
        for c in self.cells_of(&key.name, key.ground, anchor, dir) {
            self.taken.insert(c);
            self.clear.insert([c[0], c[1] + 1, c[2]]);
        }
        self.track.push(Placed { name: key.name.clone(), anchor, dir, ground: key.ground, variant: key.variant });
    }

    fn unplace(&mut self) {
        let Some(p) = self.track.pop() else { return };
        for c in self.cells_of(&p.name, p.ground, p.anchor, p.dir) {
            self.taken.remove(&c);
            self.clear.remove(&[c[0], c[1] + 1, c[2]]);
        }
    }

    /// The next pieces to try after this one, best first: weighted by how often players drove that way, with
    /// checkpoints and the finish favored when they're due
    fn candidates(&mut self, key: &Key) -> Vec<Link> {
        let Some(links) = self.model.links.get(key) else { return Vec::new() };
        let style = self.settings.style;
        let due_checkpoint = self.since_checkpoint >= self.settings.checkpoint_every;
        let due_finish = self.track.len() >= self.settings.length;
        // In a fixed order before drawing, so a seed gives the same track whatever order the map keeps them in
        let mut links: Vec<(&Link, &u32)> = links.iter().collect();
        links.sort_by(|a, b| (&a.0.next, a.0.offset, a.0.turn).cmp(&(&b.0.next, b.0.offset, b.0.turn)));
        let mut scored: Vec<(f64, Link)> = links
            .into_iter()
            .filter(|(l, _)| style.allows(&l.next.name) && !is_start(&l.next.name))
            .map(|(l, &count)| {
                let mut weight = count as f64;
                if is_checkpoint(&l.next.name) {
                    weight *= if due_checkpoint { 50.0 } else { 0.05 };
                }
                if is_finish(&l.next.name) {
                    weight *= if due_finish { 500.0 } else { 0.0 };
                }
                // Efraimidis-Spirakis: a weighted random order
                let order = if weight > 0.0 { self.rng.unit().ln() / weight } else { f64::NEG_INFINITY };
                (order, l.clone())
            })
            .filter(|(order, _)| order.is_finite())
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.into_iter().map(|(_, l)| l).collect()
    }

    /// Extends the track from the piece just placed (entered as key, its origin and direction) until a finish
    fn extend(&mut self, key: &Key, origin: Cell, dir: u8) -> bool {
        let checkpoints_needed = self.settings.length / self.settings.checkpoint_every.max(1);
        for link in self.candidates(key) {
            self.steps += 1;
            if self.steps > BUDGET {
                return false;
            }
            let finish = is_finish(&link.next.name);
            if finish && self.checkpoints < checkpoints_needed {
                continue;
            }
            // A dead end, unless it finishes the track
            if !finish && !self.model.links.contains_key(&link.next) {
                continue;
            }
            if !finish && self.track.len() >= self.settings.length * 2 {
                continue;
            }
            let next_dir = (dir + link.turn) % 4;
            let next_origin = to_world(origin, dir, link.offset);
            let units = self.model.units(&link.next.name, link.next.ground);
            let next_anchor = anchor(next_origin, &units, next_dir);
            if !self.fits(&link.next.name, link.next.ground, next_anchor, next_dir) || !self.touches(&link.next.name, link.next.ground, next_anchor, next_dir) {
                continue;
            }
            self.place(&link.next, next_anchor, next_dir);
            if finish {
                return true;
            }
            let (saved_checkpoints, saved_since) = (self.checkpoints, self.since_checkpoint);
            if is_checkpoint(&link.next.name) {
                self.checkpoints += 1;
                self.since_checkpoint = 0;
            } else {
                self.since_checkpoint += 1;
            }
            if self.extend(&link.next, next_origin, next_dir) {
                return true;
            }
            self.checkpoints = saved_checkpoints;
            self.since_checkpoint = saved_since;
            self.unplace();
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learn::Link;

    fn key(name: &str, entry: Entry) -> Key {
        Key { name: name.into(), ground: true, variant: 0, entry }
    }

    /// A tiny model: a start, a road that leads to itself or a checkpoint straight ahead, and a finish
    fn model() -> Model {
        let mut model = Model::new(&[]);
        let ahead = |name: &str| Link { next: key(name, Entry::Side(0)), offset: [0, 0, 1], turn: 0 };
        let start = key("StadiumRoadMainStartLine", Entry::Spawn);
        let road = key("StadiumRoadMain", Entry::Side(0));
        let checkpoint = key("StadiumRoadMainCheckpoint", Entry::Side(0));
        model.links.entry(start).or_default().insert(ahead("StadiumRoadMain"), 1);
        for from in [road, checkpoint] {
            let links = model.links.entry(from).or_default();
            links.insert(ahead("StadiumRoadMain"), 5);
            links.insert(ahead("StadiumRoadMainCheckpoint"), 1);
            links.insert(ahead("StadiumRoadMainFinishLine"), 1);
        }
        model
    }

    #[test]
    fn builds_a_start_to_finish_track_with_spaced_checkpoints() {
        let settings = Settings { length: 10, checkpoint_every: 4, ..Default::default() };
        let track = generate(&model(), settings).expect("a track");
        assert!(is_start(&track[0].name));
        assert!(is_finish(&track.last().unwrap().name));
        assert!(track.len() > 10);
        assert!(track.iter().filter(|p| is_checkpoint(&p.name)).count() >= 2);
        // A straight line: every piece one cell ahead of the last, the way the start faces
        let step = [0, 0, 1];
        let turned = crate::frame::rotate(step, track[0].dir);
        for pair in track.windows(2) {
            let d = [pair[1].anchor[0] - pair[0].anchor[0], pair[1].anchor[1] - pair[0].anchor[1], pair[1].anchor[2] - pair[0].anchor[2]];
            assert_eq!(d, turned);
        }
    }

    #[test]
    fn the_same_seed_gives_the_same_track() {
        let settings = Settings { length: 6, checkpoint_every: 3, seed: 42, ..Default::default() };
        assert_eq!(generate(&model(), settings), generate(&model(), settings));
    }

    #[test]
    fn never_leaves_the_map() {
        // A track longer than the map is wide can't be straight, so there is none
        let settings = Settings { length: 40, checkpoint_every: 8, size: [32, 32, 32], ..Default::default() };
        assert!(generate(&model(), settings).is_none());
    }
}
