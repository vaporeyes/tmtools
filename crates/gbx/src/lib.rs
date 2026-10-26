// ABOUTME: Binary Gbx reading as TrackMania Forever writes it: the container, LZO bodies, chunk loops, and node references.
// ABOUTME: Port of gbx-tools-3d-node's gbx/ module, which follows GBX.NET's readers.

pub mod class_ids;
mod file;
mod lzo;
mod reader;

pub use file::{read_chunks, ExternalRef, Flow, GbxFile, HeaderChunk};
pub use lzo::lzo1x_decompress;
pub use reader::{ChunkSpan, Ident, Quat, Reader, Vec3};

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
