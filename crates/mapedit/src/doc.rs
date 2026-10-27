// ABOUTME: The map being edited, apart from any UI: block footprints from the game's block data, finding the block in
// ABOUTME: a cell, placing, moving, rotating, and deleting blocks with undo, and saving under a new UID, name, and author.

use std::collections::HashMap;
use std::sync::Arc;
use tmassets::BlockInfo;
use tmedit::{make_uid, BlockChunk, Map};
use tmreplay::MapBlock;
use tmscene::placement::occupied_cells;

pub type Cell = (i32, i32, i32);

/// The environment's blocks from the asset export
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub infos: HashMap<String, BlockInfo>,
    /// Names for the palette, sorted, without clip and terrain blocks the game places by itself
    pub names: Vec<String>,
    /// The first level above the default terrain
    pub ground_level: i32,
}

impl Catalog {
    pub fn new(infos: &[BlockInfo], ground_level: i32) -> Self {
        let mut names: Vec<String> = infos.iter().filter(|i| !i.is_default_zone && !i.name.ends_with("Clip")).map(|i| i.name.clone()).collect();
        names.sort();
        Self { infos: infos.iter().map(|i| (i.name.clone(), i.clone())).collect(), names, ground_level }
    }

    /// Whether a block placed at this level stands on the ground: it must be at ground level and have ground units
    pub fn places_on_ground(&self, name: &str, level: i32) -> bool {
        level == self.ground_level && self.infos.get(name).is_none_or(|i| !i.ground_units.is_empty() || !i.ground_variants.is_empty())
    }

    /// Terrain blocks (grass, dirt, hills) fill the ground and don't block others standing on them
    pub fn is_terrain(&self, name: &str) -> bool {
        self.infos.get(name).is_some_and(|i| i.height.is_some())
    }
}

/// What a block is drawn as on the plan
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Terrain,
    Road,
    Start,
    Finish,
    Checkpoint,
    Other,
}

pub struct Doc {
    pub map: Map,
    /// Where the map came from, for the title and the Save dialog
    pub source: String,
    /// The environment, such as "Stadium", which picks the block catalog
    pub collection: String,
    pub catalog: Option<Arc<Catalog>>,
    /// The UID saves use, made once per document so the saved map is separate from the one it came from
    pub uid: String,
    pub name: String,
    pub selected: Option<usize>,
    undo: Vec<BlockChunk>,
    redo: Vec<BlockChunk>,
    pub dirty: bool,
}

impl Doc {
    pub fn new(map: Map, source: String, seed: u64) -> Self {
        let name = map.chunk.map_name.clone();
        Self { map, source, collection: String::new(), catalog: None, uid: make_uid(seed), name, selected: None, undo: Vec::new(), redo: Vec::new(), dirty: false }
    }

    pub fn blocks(&self) -> &BlockChunk {
        &self.map.chunk
    }

    /// A block as the placement code sees it
    pub fn map_block(&self, index: usize) -> MapBlock {
        let b = &self.map.chunk.blocks[index];
        let [x, y, z] = self.map.chunk.cell(index);
        MapBlock {
            name: b.name.clone(),
            x,
            y,
            z,
            dir: b.dir,
            is_ground: b.is_ground(),
            variant: b.flags & 63,
            sub_variant: (b.flags >> 6) & 63,
            is_clip: b.is_clip(),
        }
    }

    /// Cells a block covers; one cell when the block is unknown to the catalog
    pub fn cells(&self, index: usize) -> Vec<Cell> {
        let b = self.map_block(index);
        match self.catalog.as_ref().and_then(|c| c.infos.get(&b.name)) {
            Some(info) => occupied_cells(&b, info),
            None => vec![(b.x, b.y, b.z)],
        }
    }

    /// Cells a block would cover if placed with this anchor and direction
    pub fn cells_for(&self, name: &str, cell: Cell, dir: u8, is_ground: bool) -> Vec<Cell> {
        let b = MapBlock { name: name.to_owned(), x: cell.0, y: cell.1, z: cell.2, dir, is_ground, variant: 0, sub_variant: 0, is_clip: false };
        match self.catalog.as_ref().and_then(|c| c.infos.get(name)) {
            Some(info) => occupied_cells(&b, info),
            None => vec![cell],
        }
    }

    pub fn kind(&self, name: &str) -> Kind {
        // Slopes also have Start and End pieces
        if name.contains("StartLine") || name.contains("StartFinish") || (name.ends_with("Start") && !name.contains("Slope")) {
            Kind::Start
        } else if name.contains("Finish") {
            Kind::Finish
        } else if name.contains("Checkpoint") {
            Kind::Checkpoint
        } else if self.catalog.as_ref().is_some_and(|c| c.is_terrain(name)) {
            Kind::Terrain
        } else if self.catalog.as_ref().and_then(|c| c.infos.get(name)).is_some_and(|i| i.is_road) || name.contains("Road") {
            Kind::Road
        } else {
            Kind::Other
        }
    }

    /// Whether a block takes up space others can't share; clip markers and terrain don't
    fn is_solid(&self, index: usize) -> bool {
        let b = &self.map.chunk.blocks[index];
        !b.is_clip() && self.kind(&b.name) != Kind::Terrain
    }

    /// The solid block covering a cell, preferring the one placed last
    pub fn block_at(&self, cell: Cell) -> Option<usize> {
        (0..self.map.chunk.blocks.len()).rev().find(|&i| self.is_solid(i) && self.cells(i).contains(&cell))
    }

    /// Solid blocks other than `except` that cover any of these cells
    pub fn overlaps(&self, cells: &[Cell], except: Option<usize>) -> bool {
        (0..self.map.chunk.blocks.len()).any(|i| Some(i) != except && self.is_solid(i) && self.cells(i).iter().any(|c| cells.contains(c)))
    }

    pub fn in_bounds(&self, cells: &[Cell]) -> bool {
        let s = self.map.chunk.size;
        cells.iter().all(|&(x, y, z)| (0..s[0]).contains(&x) && (0..s[1]).contains(&y) && (0..s[2]).contains(&z))
    }

    fn checkpoint(&mut self) {
        self.undo.push(self.map.chunk.clone());
        self.redo.clear();
        self.dirty = true;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some(previous) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.map.chunk, previous));
            self.selected = self.selected.filter(|&i| i < self.map.chunk.blocks.len());
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.map.chunk, next));
            self.selected = self.selected.filter(|&i| i < self.map.chunk.blocks.len());
        }
    }

    /// Places a block when it fits inside the map without overlapping another; returns its index
    pub fn place(&mut self, name: &str, cell: Cell, dir: u8) -> Result<usize, String> {
        let on_ground = self.catalog.as_ref().is_some_and(|c| c.places_on_ground(name, cell.1));
        let cells = self.cells_for(name, cell, dir, on_ground);
        if !self.in_bounds(&cells) {
            return Err("The block doesn't fit inside the map there".into());
        }
        if self.overlaps(&cells, None) {
            return Err("Another block is in the way".into());
        }
        self.checkpoint();
        let index = self.map.chunk.add(name, [cell.0, cell.1, cell.2], dir, on_ground).map_err(|e| e.to_string())?;
        self.selected = Some(index);
        Ok(index)
    }

    pub fn delete(&mut self, index: usize) {
        self.checkpoint();
        self.map.chunk.blocks.remove(index);
        self.selected = None;
    }

    /// Moves a block by whole cells, or turns it a quarter turn clockwise, when the result still fits
    pub fn transform(&mut self, index: usize, delta: Cell, turn: bool) -> Result<(), String> {
        let b = self.map_block(index);
        let dir = if turn { (b.dir + 1) % 4 } else { b.dir };
        let anchor = (b.x + delta.0, b.y + delta.1, b.z + delta.2);
        let on_ground = self.catalog.as_ref().map_or(b.is_ground, |c| c.places_on_ground(&b.name, anchor.1));
        let cells = self.cells_for(&b.name, anchor, dir, on_ground);
        if !self.in_bounds(&cells) {
            return Err("That would leave the map".into());
        }
        if self.overlaps(&cells, Some(index)) {
            return Err("Another block is in the way".into());
        }
        self.checkpoint();
        let chunk = &mut self.map.chunk;
        chunk.set_cell(index, [anchor.0, anchor.1, anchor.2]).map_err(|e| e.to_string())?;
        if turn {
            chunk.rotate(index);
        }
        if on_ground != b.is_ground {
            chunk.set_ground(index, on_ground);
        }
        Ok(())
    }

    /// Drops the MediaTracker clips, so new block types can be added
    pub fn drop_clips(&mut self) {
        self.map.drop_clips();
        self.dirty = true;
    }

    /// The file to save: the edited blocks under this document's UID and name, and the author when one is given
    pub fn to_bytes(&self, author: &str) -> Result<Vec<u8>, String> {
        let mut map = self.map.clone();
        map.set_identity(&self.uid, &self.name).map_err(|e| e.to_string())?;
        if !author.is_empty() {
            map.set_author(author).map_err(|e| e.to_string())?;
        }
        map.to_bytes(true).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tmassets::{BlockUnit, UnitOffset};

    fn le(values: &[u32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    /// An 8 x 8 x 8 map with no blocks: an uncompressed file whose body is block chunk version 6 and the end marker
    fn empty_map() -> Map {
        let mut file = b"GBX".to_vec();
        file.extend_from_slice(&6u16.to_le_bytes());
        file.extend_from_slice(b"BUUR");
        // Class, no header chunks, one node, empty reference table
        file.extend(le(&[0x0304_3000, 0, 1, 0]));
        // Block chunk: ID version, UID "uid", collection 26, no author, empty name
        file.extend(le(&[0x0304_301F, 3, 0x4000_0000, 3]));
        file.extend_from_slice(b"uid");
        file.extend(le(&[26, u32::MAX, 0]));
        // Decoration (none, 26, none), size, need unlock, version, block count, end marker
        file.extend(le(&[u32::MAX, 26, u32::MAX, 8, 8, 8, 0, 6, 0, 0xFACA_DE01]));
        Map::parse(&file).unwrap()
    }

    fn catalog() -> Arc<Catalog> {
        let unit = |x, z| BlockUnit { offset: UnitOffset { x, y: 0, z }, ..Default::default() };
        let long = BlockInfo { name: "Long".into(), air_units: vec![unit(0, 0), unit(0, 1)], ground_units: vec![unit(0, 0), unit(0, 1)], ..Default::default() };
        let small = BlockInfo { name: "Small".into(), ..Default::default() };
        Arc::new(Catalog::new(&[long, small], 1))
    }

    #[test]
    fn places_blocks_that_fit_and_refuses_overlaps() {
        let mut doc = Doc::new(empty_map(), "test".into(), 1);
        doc.catalog = Some(catalog());
        let long = doc.place("Long", (2, 3, 2), 0).unwrap();
        assert_eq!(doc.cells(long), vec![(2, 3, 2), (2, 3, 3)]);
        assert_eq!(doc.block_at((2, 3, 3)), Some(long));
        assert!(doc.place("Small", (2, 3, 3), 0).is_err());
        assert!(doc.place("Long", (7, 3, 7), 0).is_err(), "runs off the map");
        doc.place("Small", (3, 3, 2), 0).unwrap();
        // Turning the long block a quarter turn makes it lie along X, into the small block
        assert!(doc.transform(long, (0, 0, 0), true).is_err());
        doc.transform(long, (0, 1, 0), true).unwrap();
        assert_eq!(doc.cells(long).len(), 2);
        assert_eq!(doc.map_block(long).dir, 1);
    }

    #[test]
    fn ground_flags_follow_the_level() {
        let mut doc = Doc::new(empty_map(), "test".into(), 1);
        doc.catalog = Some(catalog());
        let b = doc.place("Long", (0, 1, 0), 0).unwrap();
        assert!(doc.map_block(b).is_ground);
        doc.transform(b, (0, 2, 0), false).unwrap();
        assert!(!doc.map_block(b).is_ground);
    }

    #[test]
    fn slope_pieces_are_not_start_blocks() {
        let doc = Doc::new(empty_map(), "test".into(), 1);
        assert_eq!(doc.kind("StadiumRoadMainStartLine"), Kind::Start);
        assert_eq!(doc.kind("StadiumPlatformBiSlope2Start"), Kind::Other);
        assert_eq!(doc.kind("StadiumRoadMainFinishLine"), Kind::Finish);
    }

    #[test]
    fn undo_and_redo_restore_blocks() {
        let mut doc = Doc::new(empty_map(), "test".into(), 1);
        doc.place("Small", (1, 1, 1), 2).unwrap();
        doc.delete(0);
        assert!(doc.blocks().blocks.is_empty());
        doc.undo();
        assert_eq!(doc.blocks().blocks.len(), 1);
        doc.undo();
        assert!(doc.blocks().blocks.is_empty() && !doc.can_undo());
        doc.redo();
        assert_eq!(doc.map_block(0).dir, 2);
    }

    #[test]
    fn saves_with_the_documents_identity() {
        let mut doc = Doc::new(empty_map(), "test".into(), 1);
        doc.name = "Fresh".into();
        doc.place("Small", (1, 1, 1), 0).unwrap();
        let saved = Map::parse(&doc.to_bytes("me").unwrap()).unwrap();
        assert_eq!((saved.uid(), saved.author(), saved.chunk.map_name.as_str()), (doc.uid.as_str(), "me", "Fresh"));
        assert_eq!(saved.chunk.blocks.len(), 1);
    }
}
