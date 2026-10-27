// ABOUTME: Writes copies of a map (or a replay's embedded map) for checking in the game that edited files load: unchanged
// ABOUTME: (stored LZO and uncompressed), a block removed, moved, and new road added. Usage: test_batch <file> <out dir> [author].

use std::path::PathBuf;
use tmedit::{make_uid, Map};

fn special(name: &str) -> bool {
    ["Start", "Finish", "Checkpoint", "Multilap"].iter().any(|s| name.contains(s))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let (input, out) = (PathBuf::from(&args[1]), PathBuf::from(&args[2]));
    let author = args.get(3).cloned();
    let mut bytes = std::fs::read(&input)?;
    if let Ok(replay) = tmreplay::parse_replay(&bytes) {
        bytes = replay.embedded_map.ok_or("the replay has no embedded map")?;
    }
    let original = Map::parse(&bytes)?;
    let title = tmreplay::strip_formatting(&original.chunk.map_name).trim().to_owned();
    std::fs::create_dir_all(&out)?;
    let save = |label: &str, map: &Map, compress: bool| -> Result<(), Box<dyn std::error::Error>> {
        let path = out.join(format!("tmtools {label}.Challenge.Gbx"));
        std::fs::write(&path, map.to_bytes(compress)?)?;
        println!("{}  ({} blocks, uid {})", path.display(), map.chunk.blocks.len(), map.uid());
        Ok(())
    };
    save("A unchanged", &original, true)?;
    save("B uncompressed", &original, false)?;

    // C: drop a block whose name appears earlier too, so the string table later chunks see is unchanged
    let blocks = &original.chunk.blocks;
    let removable = (0..blocks.len())
        .rev()
        .find(|&i| !special(&blocks[i].name) && !blocks[i].name.ends_with("Clip") && blocks[..i].iter().any(|b| b.name == blocks[i].name))
        .ok_or("no block can be removed")?;
    let mut removed = original.clone();
    let gone = removed.chunk.blocks.remove(removable);
    removed.set_identity(&make_uid(1), &format!("tmtools C {title}"))?;
    if let Some(author) = &author {
        removed.set_author(author)?;
    }
    println!("C removes {} at {:?}", gone.name, original.chunk.cell(removable));
    save("C removed", &removed, true)?;

    // D: lift a block well above the track into empty air
    let mut moved = original.clone();
    let index = (0..blocks.len()).rev().find(|&i| !special(&blocks[i].name) && !blocks[i].name.ends_with("Clip")).ok_or("no block can be moved")?;
    let mut cell = original.chunk.cell(index);
    let from = cell;
    cell[1] = (cell[1] + 6).min(original.chunk.size[1] - 1);
    moved.chunk.set_cell(index, cell)?;
    moved.set_identity(&make_uid(2), &format!("tmtools D {title}"))?;
    if let Some(author) = &author {
        moved.set_author(author)?;
    }
    println!("D moves {} from {from:?} to {cell:?}", blocks[index].name);
    save("D moved", &moved, true)?;

    // E: four new road pieces in a row on the ground, in a corner no block is anchored near
    let collection = tmreplay::parse_challenge(&bytes)?.collection;
    if collection == "Stadium" {
        let mut added = original.clone();
        let size = original.chunk.size;
        let near = |x: i32, z: i32| (0..blocks.len()).any(|i| {
            let c = original.chunk.cell(i);
            (c[0] - x).abs() <= 2 && (c[2] - z).abs() <= 5 && !blocks[i].name.contains("Pillar")
        });
        let spot = (1..size[0] - 1).flat_map(|x| (1..size[2] - 5).map(move |z| (x, z))).find(|&(x, z)| !near(x, z)).ok_or("no free corner for new road")?;
        let ground = blocks.iter().enumerate().filter(|(_, b)| b.is_ground()).map(|(i, _)| original.chunk.cell(i)[1]).min().unwrap_or(1);
        for dz in 0..4 {
            added.chunk.add("StadiumRoadMain", [spot.0, ground, spot.1 + dz], 0, true)?;
        }
        added.set_identity(&make_uid(3), &format!("tmtools E {title}"))?;
        if let Some(author) = &author {
            added.set_author(author)?;
        }
        if !added.tail.ids_free() {
            added.drop_clips();
        }
        println!("E adds 4 StadiumRoadMain at x {}, z {}..{}, level {ground}", spot.0, spot.1, spot.1 + 3);
        save("E added", &added, true)?;
    }
    Ok(())
}
