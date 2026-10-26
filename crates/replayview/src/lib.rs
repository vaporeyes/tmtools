// ABOUTME: replayview: the Replay Viewer module. Draws the map and cars with the game's meshes and textures from an asset
// ABOUTME: export on the GPU, and falls back to a wireframe view projected on the CPU when there is no export.

mod camera;
mod gpu;
mod loader;
mod module;

pub use camera::{Camera, Projector};
pub use module::ReplayViewModule;
