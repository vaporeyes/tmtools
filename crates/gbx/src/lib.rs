// ABOUTME: Binary Gbx as TrackMania Forever writes it: reading the container, LZO bodies, chunk loops, and node references
// ABOUTME: (a port of gbx-tools-3d-node's gbx/ module, which follows GBX.NET), and writing files back from their parts.

pub mod class_ids;
mod file;
mod lzo;
mod reader;
mod writer;

pub use file::{read_chunks, ExternalRef, Flow, GbxFile, HeaderChunk};
pub use lzo::{lzo1x_decompress, lzo1x_store};
pub use writer::Writer;
pub use reader::{ChunkSpan, IdState, Ident, Quat, RawId, Reader, Vec3};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("read of {want} bytes at {at} runs past the end ({len})")]
    Eof { at: usize, want: usize, len: usize },
    #[error("{0}")]
    Format(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// A format error with a message
pub fn format_err<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::Format(message.into()))
}
