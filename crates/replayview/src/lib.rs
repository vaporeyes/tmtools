// ABOUTME: replayview: the Replay Viewer module. A wireframe 3D view projected on the CPU, so it needs no exported
// ABOUTME: meshes or GPU pipeline; enough to watch runs move through the map from any angle.

mod camera;
mod module;

pub use camera::{Camera, Projector};
pub use module::ReplayViewModule;
