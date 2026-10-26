// ABOUTME: Plans where each map block's mesh goes: world transforms grouped by unique block variant.
// ABOUTME: Port of blockPlacement.ts (TMF rules from View3D.razor.cs: PlaceBlocks, CreateBaseZoneBlocks, GetCoveredZoneBlocks).

use std::collections::{HashMap, HashSet};
use std::f32::consts::FRAC_PI_2;
use tmassets::{BlockInfo, BlockUnit};
use tmreplay::MapBlock;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridOptions {
    pub square_size: f32,
    pub square_height: f32,
    /// Ground level of the map grid; the default terrain fills the row below it
    pub base_height: i32,
}

/// World position of one instance and its rotation around Y in radians
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub rotation_y: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VariantGroup {
    pub name: String,
    pub is_ground: bool,
    pub variant: u32,
    pub sub_variant: u32,
    /// Material variation for terrain under or next to certain blocks; the mesh is the same
    pub terrain_modifier: Option<String>,
    pub placements: Vec<Placement>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BlockPlan {
    pub groups: Vec<VariantGroup>,
    /// Blocks or variants the dataset has no mesh info for, e.g. "Road air 5/0"
    pub missing: Vec<String>,
}

type Cell = (i32, i32, i32);

/// A block to place, and which map block it is (None for generated terrain and connectors)
struct Candidate {
    block: MapBlock,
    source: Option<usize>,
}

pub fn plan_blocks(size: [i32; 3], blocks: &[MapBlock], block_infos: &[BlockInfo], grid: GridOptions) -> BlockPlan {
    let infos: HashMap<&str, &BlockInfo> = block_infos.iter().map(|i| (i.name.as_str(), i)).collect();

    let covered = covered_terrain(blocks, &infos);
    let base_zone = block_infos
        .iter()
        .find(|i| i.is_default_zone)
        .map(|zone| base_zone_blocks(size, blocks, &infos, zone, grid.base_height))
        .unwrap_or_default();
    let modifiers = terrain_modifiers(blocks, &infos);

    // The map's own clip blocks only mark where connectors go; the generated clips are drawn instead
    let candidates = base_zone
        .into_iter()
        .map(|block| Candidate { block, source: None })
        .chain(blocks.iter().enumerate().filter(|(_, b)| !b.is_clip).map(|(i, b)| Candidate { block: b.clone(), source: Some(i) }))
        .chain(clip_blocks(blocks, &infos).into_iter().map(|block| Candidate { block, source: None }));

    // Groups in first-seen order, so the plan is stable
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, (VariantGroup, Vec<MapBlock>)> = HashMap::new();
    let mut missing: Vec<String> = Vec::new();

    for Candidate { block: b, source } in candidates {
        if source.is_some_and(|i| covered.contains(&i)) {
            continue;
        }
        // TMF gives pillars many subvariants of the same mesh
        let sub_variant = if b.name.ends_with("Pillar") { 0 } else { b.sub_variant };
        let terrain_modifier = modifiers.get(&(b.x, b.z)).and_then(|m| (m.source.is_none() || m.source != source).then(|| m.name.clone()));
        // Blocks with only ground variants (Speed's terrain transitions, stacked on each other) use those even in the air
        let is_ground = b.is_ground || infos.get(b.name.as_str()).is_some_and(|i| i.air_variants.is_empty());
        let key = format!("{}|{is_ground}|{}|{sub_variant}|{}", b.name, b.variant, terrain_modifier.as_deref().unwrap_or(""));

        let entry = groups.entry(key.clone()).or_insert_with(|| {
            order.push(key);
            let group = VariantGroup { name: b.name.clone(), is_ground, variant: b.variant, sub_variant, terrain_modifier, placements: Vec::new() };
            (group, Vec::new())
        });
        entry.1.push(b);
    }

    let mut result = Vec::new();
    for key in order {
        let (mut group, members) = groups.remove(&key).expect("every key has a group");
        let Some(info) = infos.get(group.name.as_str()) else {
            if !missing.contains(&group.name) {
                missing.push(group.name);
            }
            continue;
        };
        let variants = if group.is_ground { &info.ground_variants } else { &info.air_variants };
        if !variants.iter().any(|v| v.variant == group.variant && v.sub_variant == group.sub_variant) {
            let name = format!("{} {} {}/{}", group.name, if group.is_ground { "ground" } else { "air" }, group.variant, group.sub_variant);
            if !missing.contains(&name) {
                missing.push(name);
            }
            continue;
        }

        let size = footprint(if group.is_ground { &info.ground_units } else { &info.air_units });
        let height = info.height.unwrap_or(0);
        for b in members {
            // The mesh origin is the block's north-west corner after rotation
            let (ox, oz) = match b.dir {
                1 => (size.2, 0),
                2 => (size.0, size.2),
                3 => (0, size.0),
                _ => (0, 0),
            };
            group.placements.push(Placement {
                x: (b.x + ox) as f32 * grid.square_size,
                y: (b.y - height) as f32 * grid.square_height,
                z: (b.z + oz) as f32 * grid.square_size,
                rotation_y: -(b.dir as f32) * FRAC_PI_2,
            });
        }
        result.push(group);
    }
    BlockPlan { groups: result, missing }
}

fn units<'a>(b: &MapBlock, info: &'a BlockInfo) -> &'a [BlockUnit] {
    if b.is_ground {
        &info.ground_units
    } else {
        &info.air_units
    }
}

fn direction(name: Option<&str>) -> Option<u8> {
    match name.unwrap_or("North") {
        "North" => Some(0),
        "East" => Some(1),
        "South" => Some(2),
        "West" => Some(3),
        _ => None,
    }
}

/// Neighbour cell offset when stepping out of a cell in each direction (North, East, South, West)
const STEP_OUT: [(i32, i32); 4] = [(0, 1), (-1, 0), (0, -1), (1, 0)];

/// Connector blocks where a block's clip meets one of the map's clip blocks, plus fillers for the clip's open sides
fn clip_blocks(blocks: &[MapBlock], infos: &HashMap<&str, &BlockInfo>) -> Vec<MapBlock> {
    let mut slot_order: Vec<Cell> = Vec::new();
    let mut slots: HashMap<Cell, &MapBlock> = HashMap::new();
    for b in blocks.iter().filter(|b| b.is_clip) {
        let cell = (b.x, b.y, b.z);
        if let std::collections::hash_map::Entry::Vacant(slot) = slots.entry(cell) {
            slot.insert(b);
            slot_order.push(cell);
        }
    }
    if slots.is_empty() {
        return Vec::new();
    }

    let mut generated = Vec::new();
    let mut placed: HashSet<(Cell, u8)> = HashSet::new();
    for b in blocks {
        let Some(info) = infos.get(b.name.as_str()) else { continue };
        if info.height.is_some() || info.is_road {
            continue;
        }
        let units = units(b, info);
        if !units.iter().any(|u| !u.clips.is_empty()) {
            continue;
        }
        let cells = occupied_cells(b, info);
        for (i, unit) in units.iter().enumerate() {
            for clip in &unit.clips {
                // Top and bottom clips have no horizontal neighbour
                let Some(clip_dir) = direction(clip.dir.as_deref()) else { continue };
                let pop_dir = (b.dir + clip_dir) % 4;
                let (cx, cy, cz) = cells[i];
                let (sx, sz) = STEP_OUT[pop_dir as usize];
                let target = (cx + sx, cy, cz + sz);
                let Some(slot) = slots.get(&target) else { continue };
                let dir = (pop_dir + 2) % 4;
                generated.push(MapBlock {
                    name: clip.id.clone(),
                    x: target.0,
                    y: target.1,
                    z: target.2,
                    dir,
                    variant: 0,
                    sub_variant: 0,
                    is_clip: false,
                    ..(*slot).clone()
                });
                placed.insert((target, dir));
            }
        }
    }

    // Ground clip blocks get the clip block's own mesh on every side nothing connected to
    for cell in slot_order {
        let slot = slots[&cell];
        if !slot.is_ground {
            continue;
        }
        for dir in 0..4 {
            if !placed.contains(&(cell, dir)) {
                generated.push(MapBlock { dir, variant: 0, sub_variant: 0, is_clip: false, ..slot.clone() });
            }
        }
    }
    generated
}

struct Modifier {
    name: String,
    /// The map block that set it; None when set by a block-level modifier
    source: Option<usize>,
}

/// Terrain modifier for each grid column
fn terrain_modifiers(blocks: &[MapBlock], infos: &HashMap<&str, &BlockInfo>) -> HashMap<(i32, i32), Modifier> {
    let mut result = HashMap::new();
    for (i, b) in blocks.iter().enumerate() {
        let Some(info) = infos.get(b.name.as_str()) else { continue };
        let units = units(b, info);
        if !units.iter().any(|u| u.terrain_modifier.is_some()) {
            continue;
        }
        for (unit, (x, _, z)) in units.iter().zip(occupied_cells(b, info)) {
            if let Some(m) = &unit.terrain_modifier {
                result.insert((x, z), Modifier { name: normalize_modifier(m), source: Some(i) });
            }
        }
    }
    // Block-level modifiers (dirt) win over unit ones (fabric)
    for b in blocks {
        if let Some(m) = infos.get(b.name.as_str()).and_then(|i| i.terrain_modifier.as_deref()) {
            result.insert((b.x, b.z), Modifier { name: normalize_modifier(m), source: None });
        }
    }
    result
}

/// Block data spells some modifiers in lower case ("fabric"); material data uses "Fabric"
fn normalize_modifier(name: &str) -> String {
    let mut chars = name.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

/// Block size in grid cells, from its unit offsets
fn footprint(units: &[BlockUnit]) -> (i32, i32, i32) {
    if units.len() <= 1 {
        return (1, 1, 1);
    }
    let max = |f: fn(&BlockUnit) -> i32| units.iter().map(f).max().unwrap_or(0) + 1;
    (max(|u| u.offset.x), max(|u| u.offset.y), max(|u| u.offset.z))
}

fn rotate_unit((x, y, z): Cell, dir: u8) -> Cell {
    match dir {
        1 => (-z, y, x),
        2 => (-x, y, -z),
        3 => (z, y, -x),
        _ => (x, y, z),
    }
}

/// Grid cells covered by a block, with its rotated units shifted back to start at the block coordinate
fn occupied_cells(b: &MapBlock, info: &BlockInfo) -> Vec<Cell> {
    let units = units(b, info);
    if units.len() <= 1 {
        return vec![(b.x, b.y, b.z)];
    }
    let rotated: Vec<Cell> = units.iter().map(|u| rotate_unit((u.offset.x, u.offset.y, u.offset.z), b.dir)).collect();
    let min_x = rotated.iter().map(|r| r.0).min().unwrap_or(0);
    let min_z = rotated.iter().map(|r| r.2).min().unwrap_or(0);
    rotated.into_iter().map(|(x, y, z)| (b.x + x - min_x, b.y + y, b.z + z - min_z)).collect()
}

/// Indices of terrain blocks hidden under a non-terrain block standing on the ground
fn covered_terrain(blocks: &[MapBlock], infos: &HashMap<&str, &BlockInfo>) -> HashSet<usize> {
    let mut ground_cells = HashSet::new();
    for b in blocks {
        if let Some(info) = infos.get(b.name.as_str()).filter(|i| i.height.is_none()) {
            ground_cells.extend(occupied_cells(b, info));
        }
    }
    blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| b.is_ground && infos.get(b.name.as_str()).is_some_and(|i| i.height.is_some()) && ground_cells.contains(&(b.x, b.y + 1, b.z)))
        .map(|(i, _)| i)
        .collect()
}

/// Default terrain for every ground cell that no block occupies
fn base_zone_blocks(size: [i32; 3], blocks: &[MapBlock], infos: &HashMap<&str, &BlockInfo>, zone: &BlockInfo, base_height: i32) -> Vec<MapBlock> {
    let ground_height = base_height + 1;
    let mut occupied = HashSet::new();
    for b in blocks {
        match infos.get(b.name.as_str()) {
            // Terrain and unknown blocks fill their column themselves
            None => {
                occupied.insert((b.x, b.z));
            }
            Some(info) if info.height.is_some() => {
                occupied.insert((b.x, b.z));
            }
            Some(info) => occupied.extend(occupied_cells(b, info).into_iter().filter(|c| c.1 == ground_height).map(|c| (c.0, c.2))),
        }
    }
    let mut out = Vec::new();
    for x in 0..size[0] {
        for z in 0..size[2] {
            if !occupied.contains(&(x, z)) {
                out.push(MapBlock {
                    name: zone.name.clone(),
                    x,
                    y: ground_height - 1,
                    z,
                    dir: 0,
                    is_ground: true,
                    variant: 0,
                    sub_variant: 0,
                    is_clip: false,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tmassets::{Clip, UnitOffset, Variant};

    const GRID: GridOptions = GridOptions { square_size: 32.0, square_height: 8.0, base_height: 0 };

    fn block(name: &str, x: i32, y: i32, z: i32) -> MapBlock {
        MapBlock { name: name.into(), x, y, z, dir: 0, is_ground: false, variant: 0, sub_variant: 0, is_clip: false }
    }

    fn unit(x: i32, z: i32) -> BlockUnit {
        BlockUnit { offset: UnitOffset { x, y: 0, z }, ..Default::default() }
    }

    fn v(variant: u32, sub_variant: u32) -> Variant {
        Variant { variant, sub_variant }
    }

    fn road() -> BlockInfo {
        BlockInfo { name: "Road".into(), air_units: vec![unit(0, 0)], ground_units: vec![unit(0, 0)], air_variants: vec![v(0, 0)], ground_variants: vec![v(0, 0)], ..Default::default() }
    }

    fn grass() -> BlockInfo {
        BlockInfo { name: "Grass".into(), ground_units: vec![unit(0, 0)], ground_variants: vec![v(0, 0)], height: Some(0), is_default_zone: true, ..Default::default() }
    }

    // 2 wide (x) by 3 deep (z)
    fn wide() -> BlockInfo {
        BlockInfo { name: "Wide".into(), air_units: vec![unit(0, 0), unit(1, 0), unit(0, 2), unit(1, 2)], air_variants: vec![v(0, 0)], ..Default::default() }
    }

    fn group<'a>(plan: &'a BlockPlan, name: &str) -> &'a VariantGroup {
        plan.groups.iter().find(|g| g.name == name).unwrap_or_else(|| panic!("no group {name}"))
    }

    #[test]
    fn places_a_single_block_at_its_coordinate_times_the_block_size() {
        let plan = plan_blocks([4, 8, 4], &[block("Road", 1, 3, 2)], &[road()], GRID);
        assert_eq!(plan.groups.len(), 1);
        assert_eq!(plan.groups[0].placements, [Placement { x: 32.0, y: 24.0, z: 64.0, rotation_y: -0.0 }]);
    }

    #[test]
    fn offsets_rotated_blocks_by_their_rotated_footprint() {
        for (dir, (x, z)) in [(0u8, (0.0, 0.0)), (1, (96.0, 0.0)), (2, (64.0, 96.0)), (3, (0.0, 64.0))] {
            let plan = plan_blocks([8, 8, 8], &[MapBlock { dir, ..block("Wide", 0, 1, 0) }], &[wide()], GRID);
            assert_eq!(plan.groups[0].placements[0], Placement { x, y: 8.0, z, rotation_y: -(dir as f32) * FRAC_PI_2 }, "dir {dir}");
        }
    }

    #[test]
    fn fills_empty_ground_with_the_default_zone_one_row_below() {
        let plan = plan_blocks([2, 8, 2], &[MapBlock { is_ground: true, ..block("Road", 0, 1, 0) }], &[road(), grass()], GRID);
        let g = group(&plan, "Grass");
        assert!(g.is_ground);
        let cells: Vec<_> = g.placements.iter().map(|p| (p.x, p.y, p.z)).collect();
        assert_eq!(cells, [(0.0, 0.0, 32.0), (32.0, 0.0, 0.0), (32.0, 0.0, 32.0)]);
    }

    #[test]
    fn drops_terrain_directly_under_a_non_terrain_block() {
        let blocks = [MapBlock { is_ground: true, ..block("Grass", 0, 0, 0) }, MapBlock { is_ground: true, ..block("Grass", 1, 0, 0) }, MapBlock { is_ground: true, ..block("Road", 0, 1, 0) }];
        let plan = plan_blocks([2, 8, 1], &blocks, &[road(), BlockInfo { is_default_zone: false, ..grass() }], GRID);
        assert_eq!(group(&plan, "Grass").placements.iter().map(|p| p.x).collect::<Vec<_>>(), [32.0]);
    }

    #[test]
    fn raises_terrain_by_its_height() {
        let hill = BlockInfo { name: "Hill".into(), height: Some(2), is_default_zone: false, ..grass() };
        let plan = plan_blocks([1, 8, 1], &[MapBlock { is_ground: true, ..block("Hill", 0, 3, 0) }], &[hill], GRID);
        assert_eq!(plan.groups[0].placements[0].y, 8.0);
    }

    #[test]
    fn collapses_pillar_subvariants_into_one_mesh() {
        let pillar = BlockInfo { name: "TubePillar".into(), air_variants: vec![v(0, 0), v(0, 3)], ..road() };
        let blocks = [block("TubePillar", 0, 1, 0), MapBlock { sub_variant: 3, ..block("TubePillar", 1, 1, 0) }];
        let plan = plan_blocks([2, 8, 1], &blocks, &[pillar], GRID);
        assert_eq!(plan.groups.len(), 1);
        assert_eq!((plan.groups[0].sub_variant, plan.groups[0].placements.len()), (0, 2));
    }

    #[test]
    fn skips_clips_and_reports_what_the_dataset_lacks() {
        let blocks = [MapBlock { is_clip: true, ..block("Road", 0, 1, 0) }, block("Unknown", 0, 1, 0), MapBlock { variant: 5, ..block("Road", 0, 1, 0) }];
        let mut plan = plan_blocks([1, 8, 1], &blocks, &[road()], GRID);
        assert!(plan.groups.is_empty());
        plan.missing.sort();
        assert_eq!(plan.missing, ["Road air 5/0", "Unknown"]);
    }

    fn hub() -> BlockInfo {
        let clips = vec![Clip { id: "HubClip".into(), dir: None }, Clip { id: "HubClip".into(), dir: Some("East".into()) }];
        BlockInfo { name: "Hub".into(), ground_units: vec![BlockUnit { clips, ..unit(0, 0) }], ground_variants: vec![v(0, 0)], ..Default::default() }
    }

    fn clip_info(name: &str) -> BlockInfo {
        BlockInfo { name: name.into(), ground_units: vec![unit(0, 0)], ground_variants: vec![v(0, 0)], ..Default::default() }
    }

    #[test]
    fn places_connectors_where_clips_meet_and_fills_open_sides() {
        let blocks = [MapBlock { is_ground: true, ..block("Hub", 1, 1, 1) }, MapBlock { is_ground: true, is_clip: true, ..block("GrassClip", 1, 1, 2) }];
        let plan = plan_blocks([4, 8, 4], &blocks, &[hub(), clip_info("HubClip"), clip_info("GrassClip")], GRID);
        let rotations = |name: &str| {
            let mut r: Vec<f32> = group(&plan, name).placements.iter().map(|p| p.rotation_y).collect();
            r.sort_by(f32::total_cmp);
            r
        };
        // The north clip pops into (1, 1, 2) facing back south; the east clip finds no clip block
        assert_eq!(rotations("HubClip"), [-2.0 * FRAC_PI_2]);
        // The clip block itself is not drawn, but its remaining three sides get ground fillers
        assert_eq!(rotations("GrassClip"), [-3.0 * FRAC_PI_2, -FRAC_PI_2, -0.0]);
    }

    #[test]
    fn road_blocks_generate_no_clips() {
        let blocks = [MapBlock { is_ground: true, ..block("Hub", 1, 1, 1) }, MapBlock { is_clip: true, ..block("GrassClip", 1, 1, 2) }];
        let plan = plan_blocks([4, 8, 4], &blocks, &[BlockInfo { is_road: true, ..hub() }, clip_info("HubClip"), clip_info("GrassClip")], GRID);
        assert!(plan.groups.iter().all(|g| g.name != "HubClip"));
    }

    #[test]
    fn terrain_modifiers_mark_columns_but_not_the_block_that_sets_them() {
        let dirt = BlockInfo { name: "Dirt".into(), terrain_modifier: Some("TerrainModifierDirt".into()), is_default_zone: false, ..grass() };
        let plan = plan_blocks([2, 8, 1], &[MapBlock { is_ground: true, ..block("Dirt", 0, 1, 0) }], &[dirt, grass()], GRID);
        assert_eq!(group(&plan, "Dirt").terrain_modifier.as_deref(), Some("TerrainModifierDirt"));
        assert_eq!(group(&plan, "Grass").terrain_modifier, None);

        let fabric_units = vec![
            BlockUnit { terrain_modifier: Some("Fabric".into()), ..unit(0, 0) },
            BlockUnit { terrain_modifier: Some("fabric".into()), ..unit(1, 0) },
        ];
        let fabric = BlockInfo { name: "Fabric".into(), air_units: fabric_units, air_variants: vec![v(0, 0)], ..Default::default() };
        let blocks = [block("Fabric", 0, 2, 0), MapBlock { is_ground: true, ..block("Grass", 0, 0, 0) }, MapBlock { is_ground: true, ..block("Grass", 1, 0, 0) }];
        let plan = plan_blocks([3, 8, 1], &blocks, &[fabric, BlockInfo { is_default_zone: false, ..grass() }], GRID);
        assert_eq!(group(&plan, "Fabric").terrain_modifier, None);
        let grass_groups: Vec<_> = plan.groups.iter().filter(|g| g.name == "Grass").collect();
        assert_eq!(grass_groups.len(), 1);
        assert_eq!((grass_groups[0].terrain_modifier.as_deref(), grass_groups[0].placements.len()), (Some("Fabric"), 2));
    }

    #[test]
    fn ground_only_blocks_use_ground_variants_in_the_air() {
        let rock = BlockInfo { name: "RockToRock".into(), ground_units: vec![unit(0, 0)], ground_variants: vec![v(5, 2)], height: Some(4), ..Default::default() };
        let plan = plan_blocks([32, 32, 32], &[MapBlock { variant: 5, sub_variant: 2, ..block("RockToRock", 3, 12, 3) }], &[rock], GRID);
        assert!(plan.missing.is_empty());
        assert!(plan.groups[0].is_ground);

        let road = BlockInfo { air_variants: vec![v(0, 0)], ..road() };
        let plan = plan_blocks([32, 32, 32], &[MapBlock { variant: 7, ..block("Road", 3, 12, 3) }], &[road], GRID);
        assert_eq!(plan.missing, ["Road air 7/0"]);
    }
}
