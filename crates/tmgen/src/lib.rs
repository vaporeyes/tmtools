// ABOUTME: tmgen: learns how TMF Stadium blocks connect from real replays and generates maps of a style from those
// ABOUTME: connections, written as map files the game can load.

pub mod frame;
pub mod generate;
pub mod learn;
pub mod library;
pub mod write;

pub use generate::{generate, Placed, Settings, Style};
pub use learn::{Entry, Key, Link, Model};
pub use library::{read_library, Library};
pub use write::{is_base, track_uid, write_map};
