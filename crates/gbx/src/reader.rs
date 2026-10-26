// ABOUTME: Little-endian reader for binary Gbx data: primitives, strings, lookback IDs, idents, node references,
// ABOUTME: and an optional trace of the chunks read, for the inspector.

use crate::class_ids::{collection_name, wrap_class_id};
use crate::{format_err, Error, Result};
use std::any::Any;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Quat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

/// A model or map reference: name, collection (environment), and author
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ident {
    pub id: String,
    pub collection: String,
    pub author: String,
}

/// One chunk the chunk loop met, with byte offsets into the reader's data
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkSpan {
    /// Nesting: 0 for the main node's chunks, 1 for a node referenced from one of them, and so on
    pub depth: u32,
    pub id: u32,
    /// Where the chunk ID starts
    pub start: usize,
    /// Where the chunk's own data starts, after the ID and any skip header
    pub data_start: usize,
    pub end: usize,
    pub skippable: bool,
    /// False when the chunk was skipped without being read
    pub read: bool,
}

pub(crate) enum NodeSlot {
    /// Lives in another file; references to it read as null
    External,
    Read(Rc<dyn Any>),
}

pub struct Reader<'a> {
    bytes: &'a [u8],
    pub pos: usize,
    // Lookback strings: the version is read once per ID state, then each new string gets the next slot
    id_version: Option<i32>,
    ids: HashMap<u32, String>,
    pub(crate) nodes: HashMap<i32, NodeSlot>,
    pub(crate) trace: Option<Vec<ChunkSpan>>,
    pub(crate) depth: u32,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0, id_version: None, ids: HashMap::new(), nodes: HashMap::new(), trace: None, depth: 0 }
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }

    /// Records every chunk the chunk loop meets from now on
    pub fn start_trace(&mut self) {
        self.trace = Some(Vec::new());
    }

    pub fn take_trace(&mut self) -> Vec<ChunkSpan> {
        self.trace.take().unwrap_or_default()
    }

    pub fn read(&mut self, count: usize) -> Result<&'a [u8]> {
        if count > self.remaining() {
            return Err(Error::Eof { at: self.pos, want: count, len: self.bytes.len() });
        }
        let at = self.pos;
        self.pos += count;
        Ok(&self.bytes[at..at + count])
    }

    pub fn skip(&mut self, count: usize) -> Result<()> {
        self.read(count).map(|_| ())
    }

    /// The next four bytes as a u32, without moving
    pub fn peek_u32(&self) -> Option<u32> {
        self.bytes.get(self.pos..self.pos + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut out = [0; N];
        out.copy_from_slice(self.read(N)?);
        Ok(out)
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.array::<1>()?[0])
    }

    pub fn i16(&mut self) -> Result<i16> {
        self.array().map(i16::from_le_bytes)
    }

    pub fn u16(&mut self) -> Result<u16> {
        self.array().map(u16::from_le_bytes)
    }

    pub fn i32(&mut self) -> Result<i32> {
        self.array().map(i32::from_le_bytes)
    }

    pub fn u32(&mut self) -> Result<u32> {
        self.array().map(u32::from_le_bytes)
    }

    pub fn f32(&mut self) -> Result<f32> {
        self.array().map(f32::from_le_bytes)
    }

    /// Booleans are stored as 4-byte integers
    pub fn bool(&mut self) -> Result<bool> {
        Ok(self.u32()? != 0)
    }

    /// A count that must not be negative
    pub fn count(&mut self) -> Result<usize> {
        let n = self.i32()?;
        usize::try_from(n).or_else(|_| format_err(format!("negative count {n} at {}", self.pos - 4)))
    }

    /// Length-prefixed bytes
    pub fn data(&mut self) -> Result<&'a [u8]> {
        let n = self.count()?;
        self.read(n)
    }

    /// Length-prefixed UTF-8; invalid sequences are replaced rather than failing the file
    pub fn string(&mut self) -> Result<String> {
        let bytes = self.data()?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    pub fn int3(&mut self) -> Result<[i32; 3]> {
        Ok([self.i32()?, self.i32()?, self.i32()?])
    }

    pub fn vec3(&mut self) -> Result<Vec3> {
        Ok(Vec3 { x: self.f32()?, y: self.f32()?, z: self.f32()? })
    }

    /// Milliseconds, with -1 meaning no time
    pub fn time_nullable(&mut self) -> Result<Option<i32>> {
        let value = self.i32()?;
        Ok((value != -1).then_some(value))
    }

    pub fn i32_array(&mut self) -> Result<Vec<i32>> {
        let n = self.count()?;
        (0..n).map(|_| self.i32()).collect()
    }

    /// A packed unit vector: heading and pitch as 16-bit fractions of pi
    pub fn vec3_unit4(&mut self) -> Result<Vec3> {
        use std::f32::consts::PI;
        let heading = (self.i16()? as f32 * PI) / 32767.0;
        let pitch = (self.i16()? as f32 * (PI / 2.0)) / 32767.0;
        Ok(Vec3 { x: heading.cos() * pitch.cos(), y: heading.sin() * pitch.cos(), z: pitch.sin() })
    }

    /// A 6-byte rotation: a 16-bit angle and a packed unit axis
    pub fn quat6(&mut self) -> Result<Quat> {
        let angle = (self.u16()? as f32 * std::f32::consts::PI) / 65535.0;
        let axis = self.vec3_unit4()?;
        let s = angle.sin();
        Ok(Quat { x: axis.x * s, y: axis.y * s, z: axis.z * s, w: angle.cos() })
    }

    pub fn reset_ids(&mut self) {
        self.id_version = None;
        self.ids.clear();
    }

    fn id_index(&mut self) -> Result<u32> {
        if self.id_version.is_none() {
            let version = self.i32()?;
            if version < 3 {
                return format_err(format!("unsupported Id version {version}"));
            }
            self.id_version = Some(version);
        }
        self.u32()
    }

    fn id_string(&mut self, index: u32) -> Result<String> {
        if index & 0x3FFF_FFFF != 0 {
            return Ok(self.ids.get(&index).cloned().unwrap_or_default());
        }
        let value = self.string()?;
        let slot = index.wrapping_add(self.ids.len() as u32 + 1);
        self.ids.insert(slot, value.clone());
        Ok(value)
    }

    /// An ID read as text: numbers stay numbers
    pub fn id_as_string(&mut self) -> Result<String> {
        let index = self.id_index()?;
        if index == u32::MAX {
            return Ok(String::new());
        }
        match index & 0xC000_0000 {
            0x4000_0000 | 0x8000_0000 => self.id_string(index),
            _ => Ok(index.to_string()),
        }
    }

    /// An ID where numbers name collections (environments)
    pub fn id(&mut self) -> Result<String> {
        let index = self.id_index()?;
        if index == u32::MAX {
            return Ok(String::new());
        }
        match index & 0xC000_0000 {
            0x4000_0000 | 0x8000_0000 => self.id_string(index),
            _ => Ok(collection_name(index)),
        }
    }

    pub fn ident(&mut self) -> Result<Ident> {
        Ok(Ident { id: self.id_as_string()?, collection: self.id()?, author: self.id_as_string()? })
    }

    /// The version that prefixes old-style arrays; it doesn't change how they are read
    pub fn deprec_version_prefix(&mut self) -> Result<()> {
        self.i32().map(|_| ())
    }

    /// A reference to a node: None for null or a node in another file, the earlier node for a repeat, or a new
    /// node that `read` builds from its class ID. Every node behind references in one file must have type T.
    pub fn node_ref<T: Any>(&mut self, read: impl FnOnce(u32, &mut Self) -> Result<T>) -> Result<Option<Rc<T>>> {
        let index = self.i32()?;
        if index == -1 {
            return Ok(None);
        }
        match self.nodes.get(&index) {
            Some(NodeSlot::External) => return Ok(None),
            Some(NodeSlot::Read(node)) => {
                return node.clone().downcast::<T>().map(Some).or_else(|_| format_err(format!("node {index} is referenced as another type")));
            }
            None => {}
        }
        let class_id = wrap_class_id(self.u32()?);
        self.depth += 1;
        let node = read(class_id, self);
        self.depth -= 1;
        let node = Rc::new(node?);
        self.nodes.insert(index, NodeSlot::Read(node.clone()));
        Ok(Some(node))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(parts: &[&[u8]]) -> Vec<u8> {
        parts.concat()
    }

    #[test]
    fn reads_little_endian_primitives_and_strings() {
        let data = bytes(&[&7u32.to_le_bytes(), &(-2i16).to_le_bytes(), &1.5f32.to_le_bytes(), &3i32.to_le_bytes(), b"abc"]);
        let mut r = Reader::new(&data);
        assert_eq!(r.u32().unwrap(), 7);
        assert_eq!(r.i16().unwrap(), -2);
        assert_eq!(r.f32().unwrap(), 1.5);
        assert_eq!(r.string().unwrap(), "abc");
        assert_eq!(r.remaining(), 0);
        assert!(matches!(r.u8(), Err(Error::Eof { .. })));
    }

    #[test]
    fn lookback_ids_store_new_strings_and_reuse_them() {
        // Version 3, then a new string "Stadium" (0x40000000), then a reference to its slot, then collection 6
        let data = bytes(&[
            &3i32.to_le_bytes(),
            &0x4000_0000u32.to_le_bytes(),
            &7i32.to_le_bytes(),
            b"Stadium",
            &0x4000_0001u32.to_le_bytes(),
            &6u32.to_le_bytes(),
            &u32::MAX.to_le_bytes(),
        ]);
        let mut r = Reader::new(&data);
        assert_eq!(r.id_as_string().unwrap(), "Stadium");
        assert_eq!(r.id_as_string().unwrap(), "Stadium");
        assert_eq!(r.id().unwrap(), "Stadium");
        assert_eq!(r.id().unwrap(), "");
    }

    #[test]
    fn node_refs_read_once_and_repeat() {
        let data = bytes(&[&0i32.to_le_bytes(), &0x0309_2000u32.to_le_bytes(), &42i32.to_le_bytes(), &0i32.to_le_bytes(), &(-1i32).to_le_bytes()]);
        let mut r = Reader::new(&data);
        let first = r.node_ref(|class, r| Ok((class, r.i32()?))).unwrap().unwrap();
        assert_eq!(*first, (0x0309_2000, 42));
        let again = r.node_ref::<(u32, i32)>(|_, _| unreachable!()).unwrap().unwrap();
        assert!(Rc::ptr_eq(&first, &again));
        assert!(r.node_ref::<(u32, i32)>(|_, _| unreachable!()).unwrap().is_none());
    }
}
