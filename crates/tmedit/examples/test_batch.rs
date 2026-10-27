// ABOUTME: Writes four copies of a map (or a replay's embedded map) for checking in the game that edited files load:
// ABOUTME: unchanged (stored LZO and uncompressed), one block removed, and one block moved. Usage: test_batch <file> <out dir>.

use std::path::PathBuf;
use tmedit::{make_uid, Map};

fn special(name: &str) -> bool {
    ["Start", "Finish", "Checkpoint", "Multilap"].iter().any(|s| name.contains(s))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let (input, out) = (PathBuf::from(&args[1]), PathBuf::from(&args[2]));
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
    println!("D moves {} from {from:?} to {cell:?}", blocks[index].name);
    save("D moved", &moved, true)?;
    Ok(())
}
