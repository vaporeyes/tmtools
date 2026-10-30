// ABOUTME: Writes a generated track as a map file: a real TMF Stadium map's file as the base, its blocks and validation
// ABOUTME: cleared, the track's blocks added, and a new UID, name, and author. Also finds bases among replays' maps.

use crate::generate::Placed;
use tmedit::{make_uid, Map};

/// A map that can carry a generated track: a TMF Stadium map, 32 cells each way, whose later chunks can't refer to
/// block names, so its blocks can be replaced
pub fn is_base(map: &Map) -> bool {
    map.chunk.size == [32, 32, 32] && map.tail.ids_free() && map.file.header_chunks.iter().any(|c| c.id == 0x0304_3002)
}

/// A generated track's UID: the same seed always gives the same map, and none share a UID with maps made from other
/// seeds elsewhere in tmtools
pub fn track_uid(seed: u64) -> String {
    // "tmgen" mixed into the seed
    make_uid(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x746D_6765_6E00)
}

/// The map file for a track, built on base
pub fn write_map(base: &Map, track: &[Placed], name: &str, author: &str, seed: u64) -> Result<Vec<u8>, String> {
    let mut map = base.clone();
    map.clear_validation().map_err(|e| e.to_string())?;
    map.chunk.blocks.clear();
    for p in track {
        let index = map.chunk.add(&p.name, p.anchor, p.dir, p.ground).map_err(|e| format!("{} at {:?}: {e}", p.name, p.anchor))?;
        map.chunk.blocks[index].flags |= p.variant & 63;
    }
    map.set_identity(&track_uid(seed), name).map_err(|e| e.to_string())?;
    if !author.is_empty() {
        map.set_author(author).map_err(|e| e.to_string())?;
    }
    map.to_bytes(true).map_err(|e| e.to_string())
}
