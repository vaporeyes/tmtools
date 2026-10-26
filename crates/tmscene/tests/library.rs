// ABOUTME: Parity with the web viewer's block placement on real maps, and full scene builds from a real export.
// ABOUTME: Needs TMTOOLS_REPLAY_DIR (the replays placement-expected.tsv came from) and TMTOOLS_ASSETS_DIR; skipped without them.

use std::collections::HashMap;
use std::path::PathBuf;
use tmassets::AssetStore;
use tmscene::placement::{plan_blocks, GridOptions};
use tmscene::scene::build_map;

fn dirs() -> Option<(PathBuf, PathBuf)> {
    let replays = std::env::var_os("TMTOOLS_REPLAY_DIR").map(PathBuf::from);
    let assets = std::env::var_os("TMTOOLS_ASSETS_DIR").map(PathBuf::from);
    if replays.is_none() || assets.is_none() {
        eprintln!("TMTOOLS_REPLAY_DIR and TMTOOLS_ASSETS_DIR are needed; skipping");
    }
    replays.zip(assets)
}

/// A variant group's instance count and the sums of its x, y, z, and rotation
type GroupSums = (usize, f64, f64, f64, f64);

#[test]
fn placement_matches_the_web_viewer() {
    let Some((replays, assets)) = dirs() else { return };
    let store = AssetStore::open(&assets).unwrap();

    // file -> group -> sums
    let mut expected: HashMap<String, HashMap<String, GroupSums>> = HashMap::new();
    for line in include_str!("fixtures/placement-expected.tsv").lines().filter(|l| !l.starts_with('#')) {
        let c: Vec<&str> = line.split('\t').collect();
        let n = |i: usize| c[i].parse::<f64>().unwrap();
        expected.entry(c[0].into()).or_default().insert(c[1].into(), (c[2].parse().unwrap(), n(3), n(4), n(5), n(6)));
    }

    for (file, groups) in &expected {
        let replay = tmreplay::parse_replay(&std::fs::read(replays.join(file)).unwrap()).unwrap();
        let map = &replay.map;
        let collection = store.collection(map.collection.as_deref().unwrap()).unwrap().unwrap();
        let size = map.size.unwrap();
        let base_height = store.decorations(&collection.name).unwrap().iter().find(|d| d.size == size).map_or(5, |d| d.base_height());
        let grid = GridOptions { square_size: collection.square_size, square_height: collection.square_height, base_height };
        let plan = plan_blocks(size, map.blocks.as_ref().unwrap(), &store.block_infos(&collection.name).unwrap(), grid);

        let mut actual: HashMap<String, GroupSums> = HashMap::new();
        for g in &plan.groups {
            let key = format!("{}|{}|{}|{}|{}", g.name, g.is_ground, g.variant, g.sub_variant, g.terrain_modifier.as_deref().unwrap_or(""));
            let sum = |f: fn(&tmscene::placement::Placement) -> f32| g.placements.iter().map(|p| f(p) as f64).sum::<f64>();
            actual.insert(key, (g.placements.len(), sum(|p| p.x), sum(|p| p.y), sum(|p| p.z), sum(|p| p.rotation_y)));
        }
        for m in &plan.missing {
            actual.insert(format!("missing|{m}"), (0, 0.0, 0.0, 0.0, 0.0));
        }

        assert_eq!(actual.len(), groups.len(), "{file}: group count");
        for (key, want) in groups {
            let got = actual.get(key).unwrap_or_else(|| panic!("{file}: no group {key}"));
            assert_eq!(got.0, want.0, "{file} {key}: placements");
            for (g, w) in [(got.1, want.1), (got.2, want.2), (got.3, want.3), (got.4, want.4)] {
                assert!((g - w).abs() < 1e-2, "{file} {key}: {g} vs {w}");
            }
        }
    }
}

#[test]
fn every_library_map_builds_a_scene() {
    let Some((replays, assets)) = dirs() else { return };
    let store = AssetStore::open(&assets).unwrap();
    for entry in std::fs::read_dir(&replays).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".Replay.Gbx") {
            continue;
        }
        let replay = tmreplay::parse_replay(&std::fs::read(&path).unwrap()).unwrap();
        let scene = build_map(&store, &replay.map).unwrap().expect("library maps have blocks");
        let instances: usize = scene.parts.iter().map(|p| p.instances.len()).sum();
        eprintln!("{}: {} parts, {instances} instances, {} missing, {} failed", path.display(), scene.parts.len(), scene.missing.len(), scene.failed.len());
        assert!(!scene.parts.is_empty());
        assert!(scene.failed.is_empty(), "{:?}", scene.failed);
    }
}
