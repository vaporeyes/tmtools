// ABOUTME: tmedit: opens TMF map files for editing (blocks, UID, name) and writes them back so the game reads them,
// ABOUTME: re-encoding only the block chunk and the identity headers and keeping every other byte as read.

mod map;
mod tail;

pub use map::{make_uid, Block, BlockChunk, Map, Skin};
pub use tail::Tail;
