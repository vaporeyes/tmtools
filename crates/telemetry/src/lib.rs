// ABOUTME: telemetry: the Telemetry Graphs module. Plots share one time axis and cursor, so zooming or hovering one
// ABOUTME: lines up all of them; clicking any plot seeks the shared clock.

mod module;
mod series;

pub use module::TelemetryModule;
pub use series::{gap_series, step_points};
