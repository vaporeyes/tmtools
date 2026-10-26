// ABOUTME: gbxinspect: the Gbx Inspector module. Shows a file's container structure and chunk tree, like a lump
// ABOUTME: browser for Gbx, with a hex view of whatever is selected.

mod hex;
mod module;

pub use hex::{chunk_label, hex_row};
pub use module::GbxInspectModule;
