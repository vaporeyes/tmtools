// ABOUTME: Block grid geometry per TMF environment: block size, height step, and ground level, to place blocks in
// ABOUTME: world units. Values come from the gbx-tools-3d asset export (collections and decoration sizes).

use crate::model::{MapBlock, ReplayMap};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockGrid {
    /// World units per block along x and z
    pub square_size: f32,
    /// World units per block step along y
    pub square_height: f32,
    /// The block y that sits at world height 0
    pub base_height: i32,
}

impl BlockGrid {
    /// The grid for a map's environment, with the base height its decoration size implies
    pub fn for_map(map: &ReplayMap) -> Self {
        let environment = map.collection.as_deref().or(map.environment.as_deref()).unwrap_or("Stadium");
        let (square_size, square_height) = match environment {
            "Speed" | "Alpine" => (32.0, 16.0),
            "Island" => (64.0, 8.0),
            "Coast" => (16.0, 4.0),
            _ => (32.0, 8.0),
        };
        Self { square_size, square_height, base_height: base_height(environment, map.size) }
    }

    /// The world-space corner of a block's cell with the lowest x, y, and z
    pub fn block_origin(&self, block: &MapBlock) -> [f32; 3] {
        [block.x as f32 * self.square_size, (block.y - self.base_height) as f32 * self.square_height, block.z as f32 * self.square_size]
    }
}

/// Ground level by environment and map size, as the decoration sizes give it; an unknown size falls back to 5
/// as the viewer does
fn base_height(environment: &str, size: Option<[i32; 3]>) -> i32 {
    let sizes: &[([i32; 3], i32)] = match environment {
        "Stadium" => &[([32, 32, 32], 0)],
        "Alpine" | "Speed" | "Rally" => &[([20, 18, 60], 3), ([30, 18, 30], 3), ([45, 18, 45], 3), ([10, 18, 150], 3), ([32, 12, 32], 0)],
        "Bay" | "Island" => &[([36, 36, 36], 4), ([45, 36, 45], 4)],
        "Coast" => &[([36, 36, 36], 3), ([45, 36, 45], 3)],
        _ => &[],
    };
    size.and_then(|s| sizes.iter().find(|(size, _)| *size == s)).map_or(5, |&(_, base)| base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_stadium_blocks_from_the_ground() {
        let map = ReplayMap { collection: Some("Stadium".into()), size: Some([32, 32, 32]), ..Default::default() };
        let grid = BlockGrid::for_map(&map);
        let block = MapBlock { name: "StadiumRoadMain".into(), x: 3, y: 2, z: 4, dir: 0, is_ground: false, variant: 0, sub_variant: 0, is_clip: false };
        assert_eq!(grid.block_origin(&block), [96.0, 16.0, 128.0]);
    }

    #[test]
    fn uses_the_decoration_size_for_ground_level() {
        let map = ReplayMap { collection: Some("Alpine".into()), size: Some([45, 18, 45]), ..Default::default() };
        assert_eq!(BlockGrid::for_map(&map), BlockGrid { square_size: 32.0, square_height: 16.0, base_height: 3 });
        let unknown = ReplayMap { collection: Some("Coast".into()), size: Some([1, 2, 3]), ..Default::default() };
        assert_eq!(BlockGrid::for_map(&unknown).base_height, 5);
    }
}
