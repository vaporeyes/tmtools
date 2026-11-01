// ABOUTME: Generates maps from a real replay library and reads them back: every block and pillar written, none overlapping,
// ABOUTME: a start, checkpoints, speedups, floor holes, and a finish. Needs TMTOOLS_REPLAY_DIR and TMTOOLS_ASSETS_DIR; skipped without them.

use std::collections::HashSet;
use std::path::PathBuf;
use tmgen::frame::{cells, units};
use tmgen::generate::{is_checkpoint, is_finish, is_hole, is_speedup, is_start};
use tmgen::{generate, pillars, read_library, write_map, Settings};

#[test]
fn generated_maps_read_back_whole() {
    let (Some(replays), Some(assets)) = (std::env::var_os("TMTOOLS_REPLAY_DIR").map(PathBuf::from), std::env::var_os("TMTOOLS_ASSETS_DIR").map(PathBuf::from)) else {
        eprintln!("TMTOOLS_REPLAY_DIR and TMTOOLS_ASSETS_DIR are needed; skipping");
        return;
    };
    let store = tmassets::AssetStore::open(&assets).unwrap();
    let infos = store.block_infos("Stadium").unwrap();
    let library = read_library(&replays, &infos).unwrap();
    let (_, base) = library.bases.first().expect("a base map");
    // Learned again into fresh hash maps, the same seed still gives the same track
    let again = read_library(&replays, &infos).unwrap();
    for seed in 1..=3 {
        assert_eq!(generate(&library.model, Settings { seed, ..Default::default() }), generate(&again.model, Settings { seed, ..Default::default() }), "seed {seed}");
    }

    for seed in 1..=6 {
        let track = generate(&library.model, Settings { seed, ..Default::default() }).unwrap_or_else(|| panic!("seed {seed}: no track"));
        assert!(is_start(&track[0].name) && is_finish(&track.last().unwrap().name), "seed {seed}");
        assert!(track.iter().filter(|p| is_checkpoint(&p.name)).count() >= 3, "seed {seed}");

        let mut taken = HashSet::new();
        for p in &track {
            for c in cells(p.anchor, &units(infos.iter().find(|i| i.name == p.name), p.ground), p.dir) {
                assert!(taken.insert(c), "seed {seed}: {} overlaps at {c:?}", p.name);
            }
        }

        let settings = Settings { seed, ..Default::default() };
        assert!(track.iter().any(|p| is_speedup(&p.name)) && track.iter().any(|p| is_hole(&p.name)), "seed {seed}: speedups and holes");
        assert!(track.iter().all(|p| settings.style.allows(&p.name)), "seed {seed}: only the style's pieces");
        let supports = pillars(&library.model, &track, settings);
        for p in &supports {
            assert!(taken.insert(p.anchor), "seed {seed}: a pillar overlaps at {:?}", p.anchor);
        }
        let blocks: Vec<_> = track.iter().chain(&supports).cloned().collect();
        let bytes = write_map(base, &blocks, &format!("tmgen {seed}"), "tester", seed).unwrap();
        let map = tmreplay::parse_challenge(&bytes).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert_eq!((map.name.as_str(), map.author.as_str()), (format!("tmgen {seed}").as_str(), "tester"));
        assert_ne!(map.uid, tmedit::make_uid(seed), "seed {seed}: generated UIDs don't collide with other tmtools maps");
        let read: Vec<(String, [i32; 3], u8)> = map.blocks.iter().map(|b| (b.name.clone(), [b.x, b.y, b.z], b.dir)).collect();
        let written: Vec<(String, [i32; 3], u8)> = blocks.iter().map(|p| (p.name.clone(), p.anchor, p.dir)).collect();
        assert_eq!(read, written, "seed {seed}");
    }
}
