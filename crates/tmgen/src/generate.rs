// ABOUTME: Generates a track from learned connections: a seeded, weighted depth-first search from a start line through a
// ABOUTME: style's pieces to a finish, with checkpoints, speedups, and floor holes spaced along it and clear air above.

use crate::frame::{anchor, cells, to_world, Cell};
use crate::learn::{Entry, Key, Link, Model};
use std::collections::HashSet;

/// Which blocks a style may use
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Stadium's platforms with main road, tilted road, and the pieces between them
    PlatformTech,
}

/// Pieces that make a track less technical: loops and walls, and the road pieces built for full speed
const TECH_BANNED: [&str; 3] = ["Loop", "Wall", "RoadMainFW"];

/// How much more or less often a tech track uses a piece than players drove it, by name, first match wins: fewer
/// banked and wide sweeping turns, more slopes and road and platform changes
const TECH_WEIGHTS: [(&str, f64); 7] = [
    ("RoadTilt", 0.35),
    ("GTCurve", 0.5),
    ("GTDiag", 0.6),
    ("Slope", 1.6),
    ("PlatformToRoad", 1.4),
    ("YShape", 1.2),
    ("Platform", 1.2),
];

impl Style {
    pub fn allows(self, name: &str) -> bool {
        match self {
            Style::PlatformTech => {
                ["StadiumPlatform", "StadiumRoadMain", "StadiumRoadTilt", "StadiumHole"].iter().any(|p| name.starts_with(p))
                    && !TECH_BANNED.iter().any(|b| name.contains(b))
            }
        }
    }

    /// The style's preference for a piece, on top of how often players drove into it
    pub fn weight(self, name: &str) -> f64 {
        match self {
            Style::PlatformTech => TECH_WEIGHTS.iter().find(|(part, _)| name.contains(part)).map_or(1.0, |&(_, w)| w),
        }
    }

    /// Links the style never takes: a slope piece straight into the same piece turned around makes a bump, not tech
    pub fn bans(self, from: &Key, link: &Link) -> bool {
        match self {
            Style::PlatformTech => link.next.name == from.name && link.turn == 2 && from.name.contains("Slope"),
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

/// Turbo pads
pub fn is_speedup(name: &str) -> bool {
    name.contains("Turbo")
}

/// Floor pieces with a hole in them
pub fn is_hole(name: &str) -> bool {
    name.starts_with("StadiumHole")
}

/// How elevated pieces are held up
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pillars {
    /// Floating, as most players leave them
    None,
    /// Tube pillars stacked from the floor, with a tube under the piece
    Tube,
    /// Inflatable pillars stacked from the floor, with a support under the piece; the variant picks the look
    Inflatable(u32),
}

#[derive(Debug, Clone, Copy)]
pub struct Settings {
    pub seed: u64,
    pub style: Style,
    /// Pieces from start to finish, roughly; the finish comes once there are at least this many
    pub length: usize,
    /// Pieces between checkpoints
    pub checkpoint_every: usize,
    /// Turbo pads and floor holes to spread along the track
    pub speedups: usize,
    pub holes: usize,
    pub pillars: Pillars,
    /// Map size in cells
    pub size: Cell,
}

impl Default for Settings {
    fn default() -> Self {
        Self { seed: 1, style: Style::PlatformTech, length: 30, checkpoint_every: 8, speedups: 2, holes: 1, pillars: Pillars::Tube, size: [32, 32, 32] }
    }
}

/// What the track has so far, and pieces since the last of each kind
#[derive(Debug, Clone, Copy, Default)]
struct Counts {
    checkpoints: usize,
    since_checkpoint: usize,
    speedups: usize,
    since_speedup: usize,
    holes: usize,
    since_hole: usize,
}

impl Counts {
    fn after(mut self, name: &str) -> Self {
        let bump = |count: &mut usize, since: &mut usize, hit: bool| {
            if hit {
                *count += 1;
                *since = 0;
            } else {
                *since += 1;
            }
        };
        bump(&mut self.checkpoints, &mut self.since_checkpoint, is_checkpoint(name));
        bump(&mut self.speedups, &mut self.since_speedup, is_speedup(name));
        bump(&mut self.holes, &mut self.since_hole, is_hole(name));
        self
    }
}

/// A feature's weight: none once the track has enough, a lot when one is due, little in between
fn spread(count: usize, since: usize, wanted: usize, length: usize) -> f64 {
    if count >= wanted {
        0.0
    } else if since >= length / (wanted + 1) {
        40.0
    } else {
        0.05
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
/// Search steps across all starts before giving up, and for one start before trying another: a start that leads into a
/// corner is left early rather than searched to the end
const BUDGET: usize = 600_000;
const START_BUDGET: usize = 25_000;
/// Starts tried before giving up
const STARTS: usize = 48;

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
    counts: Counts,
    steps: usize,
    /// Where this start's search stops
    limit: usize,
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
        let mut search = Search { model, settings, rng: Rng::new(rng.next()), track: Vec::new(), taken: HashSet::new(), clear: HashSet::new(), counts: Counts::default(), steps, limit: (steps + START_BUDGET).min(BUDGET) };
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
        let (counts, settings) = (self.counts, self.settings);
        let due_checkpoint = counts.since_checkpoint >= settings.checkpoint_every;
        let due_finish = self.track.len() >= settings.length;
        // In a fixed order before drawing, so a seed gives the same track whatever order the map keeps them in
        let mut links: Vec<(&Link, &u32)> = links.iter().collect();
        links.sort_by(|a, b| (&a.0.next, a.0.offset, a.0.turn).cmp(&(&b.0.next, b.0.offset, b.0.turn)));
        let mut scored: Vec<(f64, Link)> = links
            .into_iter()
            .filter(|(l, _)| style.allows(&l.next.name) && !is_start(&l.next.name) && !style.bans(key, l))
            .map(|(l, &count)| {
                let mut weight = count as f64 * style.weight(&l.next.name);
                if is_speedup(&l.next.name) {
                    weight *= spread(counts.speedups, counts.since_speedup, settings.speedups, settings.length);
                }
                if is_hole(&l.next.name) {
                    weight *= spread(counts.holes, counts.since_hole, settings.holes, settings.length);
                }
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
            if self.steps > self.limit {
                return false;
            }
            let finish = is_finish(&link.next.name);
            if finish && self.counts.checkpoints < checkpoints_needed {
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
            let saved = self.counts;
            self.counts = saved.after(&link.next.name);
            if self.extend(&link.next, next_origin, next_dir) {
                return true;
            }
            self.counts = saved;
            self.unplace();
        }
        false
    }
}

/// Pillar columns under the track's elevated pieces, from the floor to just under each piece, built the way real maps
/// stack them: a ground piece, then shaft pieces, then a top piece. Every other cell gets one, so long elevated runs
/// aren't walled in, and a column goes only where every level down to the floor is free of track and its clear air.
pub fn pillars(model: &Model, track: &[Placed], settings: Settings) -> Vec<Placed> {
    let (shaft, top, variant) = match settings.pillars {
        Pillars::None => return Vec::new(),
        Pillars::Tube => ("StadiumTubePillar", "StadiumTube", 0),
        Pillars::Inflatable(v) => ("StadiumInflatablePillar", "StadiumInflatableSupport", v),
    };
    let mut taken: HashSet<Cell> = HashSet::new();
    let mut clear: HashSet<Cell> = HashSet::new();
    let mut lowest: Vec<Cell> = Vec::new();
    for p in track {
        let piece = cells(p.anchor, &model.units(&p.name, p.ground), p.dir);
        for &c in &piece {
            taken.insert(c);
            clear.insert([c[0], c[1] + 1, c[2]]);
        }
        if p.anchor[1] > GROUND {
            for &c in &piece {
                if !piece.contains(&[c[0], c[1] - 1, c[2]]) && (c[0] + c[2]) % 2 == 0 {
                    lowest.push(c);
                }
            }
        }
    }
    let mut out = Vec::new();
    for [x, y, z] in lowest {
        let column: Vec<Cell> = (GROUND..y).map(|level| [x, level, z]).collect();
        if column.iter().any(|c| taken.contains(c) || clear.contains(c)) {
            continue;
        }
        for &c in &column {
            taken.insert(c);
            let name = if c[1] == y - 1 && c[1] > GROUND { top } else { shaft };
            out.push(Placed { name: name.into(), anchor: c, dir: 0, ground: c[1] == GROUND, variant });
        }
    }
    out
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
    fn tech_bans_slope_bumps_loops_and_walls() {
        let style = Style::PlatformTech;
        let slope = key("StadiumRoadMainSlopeBase", Entry::Side(0));
        let back = |turn| Link { next: key("StadiumRoadMainSlopeBase", Entry::Side(2)), offset: [0, 0, 1], turn };
        assert!(style.bans(&slope, &back(2)));
        assert!(!style.bans(&slope, &back(0)));
        assert!(!style.allows("StadiumPlatformLoopStart") && !style.allows("StadiumPlatformWallBorder"));
        assert!(style.allows("StadiumHolePillar") && style.allows("StadiumRoadMainTurbo"));
        assert!(style.weight("StadiumRoadTiltCorner") < 1.0 && style.weight("StadiumRoadMainBiSlopeStart") > 1.0);
    }

    #[test]
    fn features_are_spread_until_there_are_enough() {
        // Two wanted on a 30-piece track: due every 10 pieces, none once there are two
        assert_eq!(spread(0, 10, 2, 30), 40.0);
        assert!(spread(0, 3, 2, 30) < 1.0);
        assert_eq!(spread(2, 20, 2, 30), 0.0);
    }

    #[test]
    fn pillars_stack_from_the_floor_and_skip_columns_through_track() {
        let model = Model::new(&[]);
        let piece = |name: &str, anchor: Cell| Placed { name: name.into(), anchor, dir: 0, ground: anchor[1] == GROUND, variant: 0 };
        // An elevated piece at level 4 over free ground, and one at level 4 over a ground piece
        let track = vec![piece("StadiumRoadMain", [2, 4, 2]), piece("StadiumRoadMain", [4, 4, 4]), piece("StadiumRoadMain", [4, 1, 4])];
        let settings = Settings { pillars: Pillars::Inflatable(3), ..Default::default() };
        let built = pillars(&model, &track, settings);
        let column: Vec<(&str, i32, bool)> = built.iter().map(|p| (p.name.as_str(), p.anchor[1], p.ground)).collect();
        assert_eq!(column, [("StadiumInflatablePillar", 1, true), ("StadiumInflatablePillar", 2, false), ("StadiumInflatableSupport", 3, false)]);
        assert!(built.iter().all(|p| p.anchor[0] == 2 && p.variant == 3));
        assert!(pillars(&model, &track, Settings { pillars: Pillars::None, ..Default::default() }).is_empty());
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
