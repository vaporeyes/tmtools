// ABOUTME: Gbx container reading: header, header chunks, reference table, LZO body, and the chunk loop.
// ABOUTME: Covers binary Gbx files as TMF writes them; follows GBX.NET's GbxHeaderReader, GbxRefTableReader, and CMwNod.Read.

use crate::class_ids::{wrap_chunk_id, wrap_class_id};
use crate::lzo::lzo1x_decompress;
use crate::reader::{ChunkSpan, NodeSlot, Reader};
use crate::{format_err, Result};

const FACADE: u32 = 0xFACA_DE01;
const SKIP: u32 = 0x534B_4950; // "PIKS"

#[derive(Debug, Clone)]
pub struct HeaderChunk {
    /// Remapped chunk ID
    pub id: u32,
    /// Heavy chunks are skipped by the game when it only lists files
    pub heavy: bool,
    pub data: Vec<u8>,
}

/// A node that lives in another file: its index in this file and what the reference table says about it
#[derive(Debug, Clone)]
pub struct ExternalRef {
    pub node_index: i32,
    /// File name, or "resource N" for a resource ID
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct GbxFile {
    pub version: u16,
    pub class_id: u32,
    /// Raw class ID as stored, before remapping
    pub raw_class_id: u32,
    pub header_chunks: Vec<HeaderChunk>,
    pub node_count: i32,
    pub external: Vec<ExternalRef>,
    /// Compressed size when the body was LZO-compressed
    pub body_compressed_size: Option<usize>,
    /// Where the body starts in the file
    pub body_offset: usize,
    pub body: Vec<u8>,
}

impl GbxFile {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::new(bytes);
        if r.read(3)? != b"GBX" {
            return format_err("not a Gbx file");
        }
        let version = r.u16()?;
        if version < 3 {
            return format_err(format!("Gbx version {version} is not supported"));
        }
        if r.u8()? != b'B' {
            return format_err("only binary Gbx files are supported");
        }
        r.u8()?; // reference table compression
        let body_compressed = r.u8()? == b'C';
        if version >= 4 {
            r.u8()?;
        }

        let raw_class_id = r.u32()?;
        let class_id = wrap_class_id(raw_class_id);

        let mut header_chunks = Vec::new();
        if version >= 6 {
            let user_data = r.data()?;
            if !user_data.is_empty() {
                let mut u = Reader::new(user_data);
                let count = u.count()?;
                let mut entries = Vec::with_capacity(count);
                for _ in 0..count {
                    let id = wrap_chunk_id(u.u32()?);
                    let size = u.u32()?;
                    entries.push((id, size & 0x7FFF_FFFF, size & 0x8000_0000 != 0));
                }
                for (id, size, heavy) in entries {
                    header_chunks.push(HeaderChunk { id, heavy, data: u.read(size as usize)?.to_vec() });
                }
            }
        }

        let node_count = r.i32()?;
        let external = read_ref_table(&mut r, version)?;

        let (body, body_compressed_size, body_offset) = if body_compressed {
            let uncompressed = r.u32()? as usize;
            let compressed = r.u32()? as usize;
            let offset = r.pos;
            (lzo1x_decompress(r.read(compressed)?, uncompressed)?, Some(compressed), offset)
        } else {
            let offset = r.pos;
            (r.read(r.remaining())?.to_vec(), None, offset)
        };

        Ok(Self { version, class_id, raw_class_id, header_chunks, node_count, external, body_compressed_size, body_offset, body })
    }

    /// A reader over the body, positioned at the main node's first chunk, that knows which nodes are external
    pub fn body_reader(&self) -> Reader<'_> {
        let mut r = Reader::new(&self.body);
        for ext in &self.external {
            r.nodes.insert(ext.node_index, NodeSlot::External);
        }
        r
    }

    /// A fresh reader over one header chunk's data, as each header chunk has its own ID state
    pub fn header_reader(&self, chunk_id: u32) -> Option<Reader<'_>> {
        self.header_chunks.iter().find(|c| c.id == chunk_id).map(|c| Reader::new(&c.data))
    }
}

fn read_ref_table(r: &mut Reader, version: u16) -> Result<Vec<ExternalRef>> {
    let count = r.count()?;
    if count == 0 {
        return Ok(Vec::new());
    }
    r.i32()?; // ancestor level
    skip_folders(r)?;

    let mut refs = Vec::with_capacity(count);
    for _ in 0..count {
        let flags = r.i32()?;
        let is_resource = flags & 4 != 0;
        let name = if is_resource { format!("resource {}", r.i32()?) } else { r.string()? };
        let node_index = r.i32()?;
        if version >= 5 {
            r.bool()?;
        }
        if !is_resource {
            r.i32()?; // folder index
        }
        refs.push(ExternalRef { node_index, name });
    }
    Ok(refs)
}

fn skip_folders(r: &mut Reader) -> Result<()> {
    let count = r.count()?;
    for _ in 0..count {
        r.string()?;
        skip_folders(r)?;
    }
    Ok(())
}

/// What a chunk handler wants next
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    /// End the node early; the rest of the body is ignored
    Stop,
}

/// Reads a node's chunks until 0xFACADE01. `handle` reads the chunks it knows and returns None for others, which
/// are skipped when skippable; a non-skippable chunk nobody reads can't be stepped over, so it is an error.
/// Returns false when a handler stopped early.
pub fn read_chunks<'a>(r: &mut Reader<'a>, mut handle: impl FnMut(u32, &mut Reader<'a>) -> Result<Option<Flow>>) -> Result<bool> {
    loop {
        let start = r.pos;
        let raw_id = r.u32()?;
        if raw_id == FACADE {
            return Ok(true);
        }
        let id = wrap_chunk_id(raw_id);

        let skippable = r.peek_u32() == Some(SKIP);
        let skip_end = if skippable {
            r.u32()?;
            let size = r.count()?;
            Some(r.pos + size)
        } else {
            None
        };

        // Trace the span before reading, so nested chunks follow their parent
        let traced = r.trace.as_mut().map(|trace| {
            let depth = r.depth;
            trace.push(ChunkSpan { depth, id, start, data_start: r.pos, end: r.pos, skippable, read: true });
            trace.len() - 1
        });

        let flow = handle(id, r)?;
        if flow.is_none() && !skippable {
            return format_err(format!("chunk 0x{id:08X} can't be read"));
        }
        if let Some(end) = skip_end {
            if end > r.len() {
                return format_err(format!("chunk 0x{id:08X} runs past the end of the body"));
            }
            r.pos = end;
        }
        if let (Some(i), Some(trace)) = (traced, r.trace.as_mut()) {
            trace[i].end = r.pos;
            trace[i].read = flow.is_some();
        }
        if flow == Some(Flow::Stop) {
            return Ok(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le(values: &[u32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn skips_unknown_skippable_chunks_and_traces_them() {
        // A skippable chunk with 4 bytes nobody reads, a known chunk with one int, then the end marker
        let body = le(&[0x0309_3010, SKIP, 4, 99, 0x0309_3007, 5, FACADE]);
        let mut r = Reader::new(&body);
        r.start_trace();
        let mut seen = Vec::new();
        let complete = read_chunks(&mut r, |id, r| match id {
            0x0309_3007 => {
                seen.push(r.i32()?);
                Ok(Some(Flow::Continue))
            }
            _ => Ok(None),
        })
        .unwrap();
        assert!(complete);
        assert_eq!(seen, [5]);
        let trace = r.take_trace();
        assert_eq!(trace.len(), 2);
        assert_eq!((trace[0].skippable, trace[0].read, trace[0].data_start, trace[0].end), (true, false, 12, 16));
        assert_eq!((trace[1].skippable, trace[1].read, trace[1].start, trace[1].end), (false, true, 16, 24));
    }

    #[test]
    fn fails_on_unknown_chunks_that_cant_be_skipped() {
        let body = le(&[0x0309_3099, 1, FACADE]);
        let mut r = Reader::new(&body);
        assert!(read_chunks(&mut r, |_, _| Ok(None)).is_err());
    }

    #[test]
    fn stops_when_a_handler_asks() {
        let body = le(&[0x0309_3015, 0x0309_3099]);
        let mut r = Reader::new(&body);
        assert!(!read_chunks(&mut r, |_, _| Ok(Some(Flow::Stop))).unwrap());
    }

    #[test]
    fn parses_an_uncompressed_container() {
        let mut file = b"GBX".to_vec();
        file.extend_from_slice(&6u16.to_le_bytes());
        file.extend_from_slice(b"BUUR");
        file.extend(le(&[0x0309_3000]));
        // User data: one header chunk 0x03093000 with 4 bytes
        let user = le(&[1, 0x0309_3000, 4, 7]);
        file.extend(le(&[user.len() as u32]));
        file.extend(user);
        file.extend(le(&[2, 0])); // node count, empty reference table
        file.extend(le(&[FACADE]));

        let gbx = GbxFile::parse(&file).unwrap();
        assert_eq!(gbx.class_id, 0x0309_3000);
        assert_eq!(gbx.header_chunks.len(), 1);
        assert_eq!(gbx.header_reader(0x0309_3000).unwrap().u32().unwrap(), 7);
        assert_eq!(gbx.body, le(&[FACADE]));
        assert!(read_chunks(&mut gbx.body_reader(), |_, _| Ok(None)).unwrap());
    }
}
