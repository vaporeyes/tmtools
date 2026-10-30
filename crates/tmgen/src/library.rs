// ABOUTME: Reads a folder of replays for generating: learns block connections from every run, and keeps the maps that
// ABOUTME: can serve as a base for a generated track, by mood.

use crate::learn::Model;
use crate::write::is_base;
use std::path::Path;
use tmassets::BlockInfo;
use tmedit::Map;

pub struct Library {
    pub model: Model,
    /// Base maps with their mood (decoration), such as "Day" or "Sunset"
    pub bases: Vec<(String, Map)>,
    /// Replays read
    pub replays: usize,
}

/// Every .Replay.Gbx in dir; files that can't be read are skipped
pub fn read_library(dir: &Path, infos: &[BlockInfo]) -> std::io::Result<Library> {
    let mut library = Library { model: Model::new(infos), bases: Vec::new(), replays: 0 };
    let mut paths: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths.iter().filter(|p| p.to_string_lossy().to_lowercase().ends_with(".replay.gbx")) {
        let Ok(replay) = std::fs::read(path).map_err(|_| ()).and_then(|b| tmreplay::parse_replay(&b).map_err(|_| ())) else { continue };
        library.replays += 1;
        library.model.learn(&replay);
        let Some(map) = replay.embedded_map.as_deref().and_then(|b| Map::parse(b).ok()) else { continue };
        if is_base(&map) && replay.map.collection.as_deref() == Some("Stadium") {
            let mood = replay.map.decoration.clone().unwrap_or_default();
            library.bases.push((mood, map));
        }
    }
    Ok(library)
}
