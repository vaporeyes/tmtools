// ABOUTME: A map file opened for editing: its block chunk (0x0304301F) parsed field by field as stored, and written back
// ABOUTME: into the body continuing the body's lookback string table, with the UID and name kept in step in the headers.

use gbx::{format_err, GbxFile, IdState, RawId, Reader, Result, Writer};
use tmreplay::{read_block_skin, reader_at_block_chunk};

use crate::tail::{drop_clips, read_tail, Tail};

const COMMON_HEADER: u32 = 0x0304_3003;
const XML_HEADER: u32 = 0x0304_3005;
const FACADE: u32 = 0xFACA_DE01;
const SKIP: u32 = 0x534B_4950; // "PIKS"
/// Flags bit 15: the block has an author and a skin node (custom sign textures)
const HAS_SKIN: u32 = 1 << 15;
/// Flags bit 12: the block stands on the ground and uses its ground variant
const GROUND: u32 = 1 << 12;
/// Flags bit 13: a clip marker, where the game connects neighbouring blocks
const CLIP: u32 = 1 << 13;

/// A block's custom skin: the author ID and the skin node's bytes, kept as read
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skin {
    pub author: RawId,
    pub node: Vec<u8>,
}

/// One block as the chunk stores it
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub name: String,
    pub dir: u8,
    /// Grid coordinates as stored; from chunk version 6, X and Z are one higher than the block's cell
    pub stored: [u8; 3],
    pub flags: u32,
    pub skin: Option<Skin>,
}

impl Block {
    pub fn is_ground(&self) -> bool {
        self.flags & GROUND != 0
    }

    pub fn is_clip(&self) -> bool {
        self.flags & CLIP != 0
    }
}

/// The block chunk's fields in the order they are stored
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockChunk {
    /// UID, collection, author
    pub map_info: [RawId; 3],
    pub map_name: String,
    pub decoration: [RawId; 3],
    pub size: [i32; 3],
    pub need_unlock: u32,
    pub version: i32,
    pub blocks: Vec<Block>,
}

impl BlockChunk {
    fn shift(&self) -> i32 {
        if self.version >= 6 {
            1
        } else {
            0
        }
    }

    /// A block's grid cell
    pub fn cell(&self, index: usize) -> [i32; 3] {
        let s = self.blocks[index].stored;
        [s[0] as i32 - self.shift(), s[1] as i32, s[2] as i32 - self.shift()]
    }

    /// Moves a block to a grid cell inside the map
    pub fn set_cell(&mut self, index: usize, cell: [i32; 3]) -> Result<()> {
        if (0..3).any(|i| cell[i] < 0 || cell[i] >= self.size[i]) {
            return format_err(format!("cell {cell:?} is outside the map ({:?})", self.size));
        }
        let shift = self.shift();
        self.blocks[index].stored = [(cell[0] + shift) as u8, cell[1] as u8, (cell[2] + shift) as u8];
        Ok(())
    }

    /// Adds a block (base variant, no skin) at a grid cell and returns its index
    pub fn add(&mut self, name: &str, cell: [i32; 3], dir: u8, is_ground: bool) -> Result<usize> {
        self.blocks.push(Block { name: name.to_owned(), dir: dir % 4, stored: [0; 3], flags: if is_ground { GROUND } else { 0 }, skin: None });
        let index = self.blocks.len() - 1;
        if let Err(e) = self.set_cell(index, cell) {
            self.blocks.pop();
            return Err(e);
        }
        Ok(index)
    }

    /// Turns a block a quarter turn clockwise
    pub fn rotate(&mut self, index: usize) {
        let b = &mut self.blocks[index];
        b.dir = (b.dir + 1) % 4;
    }

    /// Marks a block as standing on the ground or not; its variant is reset to the base one
    pub fn set_ground(&mut self, index: usize, is_ground: bool) {
        let b = &mut self.blocks[index];
        b.flags = (b.flags & !(GROUND | 0xFFF)) | if is_ground { GROUND } else { 0 };
    }

    fn write(&self, w: &mut Writer) {
        self.map_info.iter().for_each(|id| w.id_raw(id));
        w.string(&self.map_name);
        self.decoration.iter().for_each(|id| w.id_raw(id));
        w.int3(self.size);
        w.u32(self.need_unlock);
        w.i32(self.version);
        w.i32(self.blocks.len() as i32);
        for b in &self.blocks {
            w.id_text(&b.name);
            w.u8(b.dir);
            w.raw(&b.stored);
            if self.version == 0 {
                w.u16(b.flags as u16);
            } else {
                w.u32(b.flags);
            }
            if let Some(skin) = &b.skin {
                w.id_raw(&skin.author);
                w.raw(&skin.node);
            }
        }
    }
}

fn read_block_chunk(r: &mut Reader, body: &[u8]) -> Result<BlockChunk> {
    let map_info = [r.id_raw()?, r.id_raw()?, r.id_raw()?];
    let map_name = r.string()?;
    let decoration = [r.id_raw()?, r.id_raw()?, r.id_raw()?];
    let size = r.int3()?;
    let need_unlock = r.u32()?;
    let version = r.i32()?;
    let count = r.count()?;
    let mut blocks = Vec::with_capacity(count);
    for _ in 0..count {
        let name = match r.id_raw()?.text {
            Some(name) => name,
            None => return format_err("a block name isn't a string"),
        };
        let dir = r.u8()?;
        let stored = [r.u8()?, r.u8()?, r.u8()?];
        let flags = if version == 0 { r.u16()? as u32 } else { r.u32()? };
        if version >= 2 && flags & ((1 << 17) | (1 << 18) | (1 << 19) | (1 << 20)) != 0 {
            return format_err(format!("block flags 0x{flags:x} need data this editor doesn't handle"));
        }
        let skin = if flags & HAS_SKIN != 0 {
            let author = r.id_raw()?;
            let start = r.pos;
            r.node_ref(read_block_skin)?;
            Some(Skin { author, node: body[start..r.pos].to_vec() })
        } else {
            None
        };
        blocks.push(Block { name, dir, stored, flags, skin });
    }
    Ok(BlockChunk { map_info, map_name, decoration, size, need_unlock, version, blocks })
}

/// A map file with its block chunk parsed out of the body
#[derive(Debug, Clone)]
pub struct Map {
    pub file: GbxFile,
    pub chunk: BlockChunk,
    /// Where the block chunk's ID starts in the body and where its data ends
    start: usize,
    end: usize,
    /// The lookback string table at the chunk's start and end, as the original file has it
    ids_before: IdState,
    ids_after: IdState,
    /// What follows the block chunk
    pub tail: Tail,
}

impl Map {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let file = GbxFile::parse(bytes)?;
        let (chunk, start, end, ids_before, ids_after) = {
            let (mut r, start) = reader_at_block_chunk(&file)?;
            let ids_before = r.id_state();
            let chunk = read_block_chunk(&mut r, &file.body)?;
            // A misread chunk would leave the reader somewhere other than the next chunk's ID: the end marker, a map
            // chunk, or a skippable chunk (extensions use other class IDs)
            let word = |at: usize| file.body.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
            match (word(r.pos), word(r.pos + 4)) {
                (Some(next), _) if next == FACADE || next & 0xFFFF_F000 == 0x0304_3000 => {}
                (_, Some(SKIP)) => {}
                _ => return format_err("the block chunk didn't end where the next chunk starts"),
            }
            (chunk, start, r.pos, ids_before, r.id_state())
        };
        let tail = read_tail(&file.body, end, ids_after.strings.len());
        Ok(Self { file, chunk, start, end, ids_before, ids_after, tail })
    }

    pub fn uid(&self) -> &str {
        self.chunk.map_info[0].text.as_deref().unwrap_or("")
    }

    pub fn author(&self) -> &str {
        self.chunk.map_info[2].text.as_deref().unwrap_or("")
    }

    /// The block chunk's data, encoded after the chunks before it. Later chunks may refer to strings by their slot in
    /// the table, so an edit that changes the table is refused unless nothing after the chunk can do that.
    pub fn encode_chunk(&self) -> Result<Vec<u8>> {
        let mut w = Writer::with_id_state(&self.ids_before);
        self.chunk.write(&mut w);
        if w.id_state() != self.ids_after && !self.tail.ids_free() {
            return format_err(if self.tail.clip_spans.is_empty() {
                "this map has chunks after its blocks that tmtools can't read, so block types can't be added or used up"
            } else {
                "this map's MediaTracker clips may refer to block names; drop the clips to add or use up block types"
            });
        }
        Ok(w.bytes)
    }

    /// Removes the MediaTracker clips (intro, in-game, end-race, and global), which frees the block chunk to change
    /// the string table
    pub fn drop_clips(&mut self) {
        self.file.body = drop_clips(&self.file.body, &self.tail);
        self.tail = read_tail(&self.file.body, self.end, self.ids_after.strings.len());
    }

    /// The body with the block chunk re-encoded and everything else as read
    pub fn body(&self) -> Result<Vec<u8>> {
        let body = &self.file.body;
        let mut out = Vec::with_capacity(body.len());
        out.extend_from_slice(&body[..self.start + 4]);
        out.extend(self.encode_chunk()?);
        out.extend_from_slice(&body[self.end..]);
        Ok(out)
    }

    /// The whole file; `compress` wraps the body as stored LZO the way the game expects, else it is left uncompressed
    pub fn to_bytes(&self, compress: bool) -> Result<Vec<u8>> {
        Ok(self.file.write(&self.body()?, compress))
    }

    /// Gives the map a new UID and name in the block chunk, the common header, and the XML header, so the game sees
    /// it as a separate map
    pub fn set_identity(&mut self, uid: &str, name: &str) -> Result<()> {
        let old_uid = self.uid().to_owned();
        // The UID string's slot in the table now holds the new UID
        for (_, s) in self.ids_after.strings.iter_mut().filter(|(_, s)| *s == old_uid) {
            *s = uid.to_owned();
        }
        let flag = self.chunk.map_info[0].index & 0xC000_0000;
        self.chunk.map_info[0] = RawId { index: flag, text: Some(uid.to_owned()) };
        self.chunk.map_name = name.to_owned();

        for header in &mut self.file.header_chunks {
            match header.id {
                COMMON_HEADER => header.data = rewrite_common_header(&header.data, Some(uid), Some(name), None)?,
                XML_HEADER => header.data = rewrite_xml_header(&header.data, &[("uid", uid), ("name", name)])?,
                _ => {}
            }
        }
        Ok(())
    }

    /// Makes `login` the map's author, so the game lets that player open it in the editor without paying coppers
    pub fn set_author(&mut self, login: &str) -> Result<()> {
        let old = self.author().to_owned();
        let flag = self.chunk.map_info[2].index & 0xC000_0000;
        // The author's slot in the table now holds the new login; other strings never match a login
        if let Some((_, s)) = self.ids_after.strings.iter_mut().find(|(_, s)| *s == old) {
            *s = login.to_owned();
        }
        self.chunk.map_info[2] = RawId { index: flag, text: Some(login.to_owned()) };
        for header in &mut self.file.header_chunks {
            match header.id {
                COMMON_HEADER => header.data = rewrite_common_header(&header.data, None, None, Some(login))?,
                XML_HEADER => header.data = rewrite_xml_header(&header.data, &[("author", login)])?,
                _ => {}
            }
        }
        Ok(())
    }
}

/// Header 0x03043003 starts with a version byte, the map ident (UID, collection, author), and the name; the rest is
/// kept as stored. Fields given as None keep their stored values.
fn rewrite_common_header(data: &[u8], uid: Option<&str>, name: Option<&str>, author: Option<&str>) -> Result<Vec<u8>> {
    let mut r = Reader::new(data);
    let version = r.u8()?;
    let keep = |id: RawId, text: Option<&str>| match text {
        Some(text) => RawId { index: id.index & 0xC000_0000, text: Some(text.to_owned()) },
        None => id,
    };
    let stored_uid = keep(r.id_raw()?, uid);
    let collection = r.id_raw()?;
    let stored_author = keep(r.id_raw()?, author);
    let stored_name = r.string()?;
    let mut w = Writer::new();
    w.u8(version);
    w.id_raw(&stored_uid);
    w.id_raw(&collection);
    w.id_raw(&stored_author);
    w.string(name.unwrap_or(&stored_name));
    w.raw(&data[r.pos..]);
    Ok(w.bytes)
}

/// Header 0x03043005 is one XML string; its ident element carries the UID, name, and author
fn rewrite_xml_header(data: &[u8], attrs: &[(&str, &str)]) -> Result<Vec<u8>> {
    let xml = Reader::new(data).string()?;
    let Some(ident_at) = xml.find("<ident ") else { return format_err("the XML header has no ident element") };
    let ident_end = ident_at + xml[ident_at..].find('>').unwrap_or(xml.len() - ident_at);
    let mut ident = xml[ident_at..ident_end].to_owned();
    for &(attr, value) in attrs {
        let key = format!(" {attr}=\"");
        let Some(at) = ident.find(&key) else { return format_err(format!("the XML ident has no {attr}")) };
        let value_at = at + key.len();
        let value_end = value_at + ident[value_at..].find('"').unwrap_or(0);
        ident.replace_range(value_at..value_end, &xml_escape(value));
    }
    let mut w = Writer::new();
    w.string(&format!("{}{ident}{}", &xml[..ident_at], &xml[ident_end..]));
    Ok(w.bytes)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// A 27-character UID like the ones the game makes, derived from a seed
pub fn make_uid(seed: u64) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut state = seed;
    (0..27)
        .map(|_| {
            // splitmix64
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            ALPHABET[((z ^ (z >> 31)) % ALPHABET.len() as u64) as usize] as char
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_the_xml_ident() {
        let mut w = Writer::new();
        w.string(r#"<header type="challenge"><ident uid="old" name="Old" author="me"/><desc envir="Stadium"/></header>"#);
        let out = rewrite_xml_header(&w.bytes, &[("uid", "fresh"), ("name", "A & B")]).unwrap();
        assert_eq!(
            Reader::new(&out).string().unwrap(),
            r#"<header type="challenge"><ident uid="fresh" name="A &amp; B" author="me"/><desc envir="Stadium"/></header>"#
        );
    }

    #[test]
    fn rewrites_the_common_header_and_keeps_the_rest() {
        let mut w = Writer::new();
        w.u8(5);
        w.id_text("old");
        w.id_raw(&RawId { index: 26, text: None });
        w.id_text("me");
        w.string("Old");
        w.raw(&[1, 2, 3]);
        let out = rewrite_common_header(&w.bytes, Some("fresh"), Some("New"), None).unwrap();
        let mut r = Reader::new(&out);
        assert_eq!(r.u8().unwrap(), 5);
        let ident = r.ident().unwrap();
        assert_eq!((ident.id.as_str(), ident.author.as_str()), ("fresh", "me"));
        assert_eq!(r.string().unwrap(), "New");
        assert_eq!(r.read(3).unwrap(), [1, 2, 3]);
    }

    #[test]
    fn uids_look_like_the_games() {
        let uid = make_uid(7);
        assert_eq!(uid.len(), 27);
        assert!(uid.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(uid, make_uid(8));
    }
}
