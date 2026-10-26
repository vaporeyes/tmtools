// ABOUTME: trajmap: the Trajectory Map module. A pan-and-zoom top-down view of the map's blocks and the runs'
// ABOUTME: racing lines, with input markers placed where the car was on the tick they happened.

mod module;
mod view;

pub use module::TrajMapModule;
pub use view::{nearest_on_path, speed_color, MapView};
