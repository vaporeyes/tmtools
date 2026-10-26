// ABOUTME: inputscope: the Input Analyzer module. A zoomable piano roll exact to the 10 ms tick, per-channel hold
// ABOUTME: statistics, and a hold table with press timing against a second run at the same spot on the track.

mod module;
mod timeview;

pub use module::InputScopeModule;
pub use timeview::TimeView;
