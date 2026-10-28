// ABOUTME: Reads TMF replays (CGameCtnReplayRecord) and maps (CGameCtnChallenge): ghosts, inputs, samples, and blocks.
// ABOUTME: Port of gbx-tools-3d-node's parseReplay.ts; the same chunk readers also drive the inspector's traced walk.

use crate::inputs::parse_input;
use crate::model::{Checkpoint, Ghost, Input, MapBlock, Replay, ReplayMap};
use crate::samples::{read_old_sample_data, read_sample_data};
use gbx::{format_err, read_chunks, ChunkSpan, Flow, GbxFile, Ident, Reader, Result};

const REPLAY_RECORD: u32 = 0x0309_3000;
const CHALLENGE: u32 = 0x0304_3000;
const GHOST: u32 = 0x0309_2000;
const COLLECTOR_LIST: u32 = 0x0301_B000;
const CHALLENGE_PARAMETERS: u32 = 0x0305_B000;
const BLOCK_SKIN: u32 = 0x0305_9000;

pub fn parse_replay(bytes: &[u8]) -> Result<Replay> {
    let file = GbxFile::parse(bytes)?;
    if file.class_id != REPLAY_RECORD {
        return format_err("not a replay");
    }

    // Header chunk 0x03093000: version, then (from version 4) the map ident
    let mut map_info: Option<Ident> = None;
    if let Some(mut header) = file.header_reader(0x0309_3000) {
        let version = header.u32()?;
        if version >= 4 && version != 9999 {
            map_info = Some(header.ident()?);
        }
    }

    let body = read_replay_body(&mut file.body_reader())?;
    let challenge = body.map_bytes.as_deref().and_then(|b| parse_challenge(b).ok());

    let map = ReplayMap {
        uid: map_info.as_ref().map(|m| m.id.clone()),
        name: challenge.as_ref().map(|c| c.name.clone()),
        author: map_info.as_ref().map(|m| m.author.clone()),
        environment: map_info.as_ref().map(|m| m.collection.clone()),
        collection: challenge.as_ref().map(|c| c.collection.clone()),
        size: challenge.as_ref().map(|c| c.size),
        decoration: challenge.as_ref().map(|c| c.decoration.clone()),
        blocks: challenge.filter(|c| !c.needs_unlimiter).map(|c| c.blocks),
    };
    Ok(Replay { map, ghosts: body.ghosts, embedded_map: body.map_bytes })
}

struct ReplayBody {
    map_bytes: Option<Vec<u8>>,
    ghosts: Vec<Ghost>,
}

fn read_replay_body(r: &mut Reader) -> Result<ReplayBody> {
    let mut body = ReplayBody { map_bytes: None, ghosts: Vec::new() };
    read_chunks(r, |id, r| {
        match id {
            0x0309_3002 => body.map_bytes = Some(r.data()?.to_vec()),
            0x0309_3007 => {
                r.i32()?;
            }
            // TM Nations ESWC and earlier: a version, the ghosts, then a duration and record units
            0x0309_3004 => {
                r.i32()?;
                r.deprec_version_prefix()?;
                let count = r.count()?;
                for _ in 0..count {
                    if let Some(ghost) = r.node_ref(read_ghost)? {
                        body.ghosts.push((*ghost).clone());
                    }
                }
                r.i32()?;
                let extras = r.count()?;
                r.skip(extras * 8)?;
            }
            0x0309_3014 => {
                r.deprec_version_prefix()?;
                let count = r.count()?;
                for _ in 0..count {
                    if let Some(ghost) = r.node_ref(read_ghost)? {
                        body.ghosts.push((*ghost).clone());
                    }
                }
                r.i32()?; // always zero
                let extras = r.count()?;
                r.skip(extras * 8)?;
            }
            // The MediaTracker clip follows; nothing after it is needed
            0x0309_3015 => return Ok(Some(Flow::Stop)),
            // Older games put the clip before the inputs; a clip node can't be stepped over, so a replay with one
            // ends here without inputs
            0x0309_300C => {
                if r.i32()? != -1 {
                    return Ok(Some(Flow::Stop));
                }
            }
            // Sunrise and Nations ESWC: the first ghost's inputs, laid out as in TMF's validation chunk
            0x0309_300D => {
                let inputs = read_inputs(r)?.unwrap_or_default();
                if let Some(ghost) = body.ghosts.first_mut() {
                    ghost.inputs = inputs;
                }
                return Ok(Some(Flow::Stop));
            }
            _ => return Ok(None),
        }
        Ok(Some(Flow::Continue))
    })?;
    Ok(body)
}

fn read_ghost(class_id: u32, r: &mut Reader) -> Result<Ghost> {
    if class_id != GHOST {
        return format_err(format!("expected a ghost, found class 0x{class_id:08X}"));
    }
    let mut ghost = Ghost::default();
    read_chunks(r, |id, c| {
        match id {
            // CGameGhost: zlib-compressed sample data
            0x0303_F005 => {
                let uncompressed = c.count()?;
                let data = read_sample_data(c.data()?, uncompressed)?;
                ghost.sample_period_ms = Some(data.sample_period_ms);
                ghost.samples = data.samples;
            }
            // TM Nations ESWC and earlier: uncompressed sample data, then the class of the recorded vehicle
            0x0303_F003 => {
                let data = read_old_sample_data(c)?;
                ghost.sample_period_ms = Some(data.sample_period_ms);
                ghost.samples = data.samples;
            }
            0x0303_F004 => {
                c.u32()?;
            }
            0x0309_2005 => ghost.race_time_ms = c.time_nullable()?,
            // Vehicle, skin, and nickname as older games store them: TM 1.0 and PU (0x003), Sunrise and Nations
            // ESWC (0x006, one int more), United (0x00D, four ints more)
            0x0309_2003 | 0x0309_2006 | 0x0309_200D => {
                ghost.vehicle = Some(c.ident()?.id);
                c.string()?; // skin
                let ints = match id {
                    0x0309_2006 => 1,
                    0x0309_200D => 4,
                    _ => 0,
                };
                c.skip(ints * 4)?;
                ghost.nickname = Some(c.string()?);
            }
            // Old light trail color
            0x0309_2007 => {
                c.vec3()?;
            }
            0x0309_2008 | 0x0309_200A | 0x0309_200C | 0x0309_2014 => {
                c.i32()?;
            }
            0x0309_2009 => {
                c.vec3()?;
            }
            0x0309_200B => {
                let count = c.count()?;
                ghost.checkpoints = (0..count).map(|_| Ok(Checkpoint { time_ms: c.time_nullable()?, stunts_score: c.i32()? })).collect::<Result<_>>()?;
            }
            0x0309_200E | 0x0309_2010 => {
                c.id()?;
            }
            0x0309_200F => ghost.login = Some(c.string()?),
            0x0309_2012 => {
                c.u32()?;
                c.skip(16)?;
            }
            0x0309_2013 => {
                c.i32()?;
                c.i32()?;
            }
            0x0309_2015 => ghost.nickname = Some(c.id_as_string()?),
            0x0309_2017 => {
                let packs = c.count()?;
                for _ in 0..packs {
                    read_pack_desc(c)?;
                }
                ghost.nickname = Some(c.string()?);
                c.string()?; // avatar
            }
            0x0309_2018 => ghost.vehicle = Some(c.ident()?.id),
            0x0309_2019 => ghost.inputs = read_validation(c, true)?,
            0x0309_2011 => ghost.inputs = read_validation(c, false)?,
            _ => return Ok(None),
        }
        Ok(Some(Flow::Continue))
    })?;
    Ok(ghost)
}

/// Chunks 0x03092011 and 0x03092019: recorded inputs, then validation data
fn read_validation(r: &mut Reader, with_seed: bool) -> Result<Vec<Input>> {
    let inputs = read_inputs(r)?;
    if inputs.is_none() {
        return Ok(Vec::new());
    }

    // SGameGhostValidationData
    r.string()?;
    r.u32()?;
    r.i32()?;
    r.i32()?;
    r.string()?;
    if with_seed {
        r.i32()?;
    }
    Ok(inputs.unwrap_or_default())
}

/// The recorded inputs: None when the events duration is zero and nothing else follows
fn read_inputs(r: &mut Reader) -> Result<Option<Vec<Input>>> {
    let events_duration = r.i32()?;
    if events_duration == 0 {
        return Ok(None);
    }
    r.i32()?; // always 0
    let name_count = r.count()?;
    let names = (0..name_count).map(|_| r.id_as_string()).collect::<Result<Vec<_>>>()?;
    let count = r.count()?;
    r.i32()?; // count limit
    let mut inputs = Vec::with_capacity(count);
    for _ in 0..count {
        let time = r.i32()? - 100_000;
        let name = names.get(r.u8()? as usize).map_or("", String::as_str);
        inputs.push(parse_input(time, name, r.u32()?));
    }
    Ok(Some(inputs))
}

/// A skin or file reference; only its size matters here
fn read_pack_desc(r: &mut Reader) -> Result<()> {
    let version = r.u8()?;
    if version >= 3 {
        r.skip(32)?;
    }
    let path = r.string()?;
    if (!path.is_empty() && version >= 1) || version >= 3 {
        r.string()?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Challenge {
    pub uid: String,
    pub name: String,
    pub author: String,
    pub collection: String,
    pub size: [i32; 3],
    pub decoration: String,
    pub blocks: Vec<MapBlock>,
    /// TMUnlimiter maps hide their real blocks in an extension chunk and leave one placeholder block whose name
    /// tells players to install the mod; their blocks can't be placed without Unlimiter's offsets
    pub needs_unlimiter: bool,
}

pub fn parse_challenge(bytes: &[u8]) -> Result<Challenge> {
    let file = GbxFile::parse(bytes)?;
    if file.class_id != CHALLENGE {
        return format_err("not a map");
    }
    read_challenge_body(&mut file.body_reader())?.map_or_else(|| format_err("the map has no block data"), Ok)
}

/// A reader over a map's body positioned at the data of its block chunk (0x0304301F), carrying the lookback strings
/// and nodes of the chunks before it, and the offset where that chunk's ID starts
pub fn reader_at_block_chunk(file: &GbxFile) -> Result<(Reader<'_>, usize)> {
    if file.class_id != CHALLENGE {
        return format_err("not a map");
    }
    let mut r = file.body_reader();
    let mut start = None;
    read_chunks(&mut r, |id, r| {
        match id {
            0x0304_300D => {
                r.ident()?;
            }
            0x0304_3011 => {
                r.node_ref(read_challenge_parameters)?;
                r.node_ref(read_challenge_parameters)?;
                r.i32()?;
            }
            // The block chunk can't be skipped, so its data starts right after its ID
            0x0304_301F => {
                start = Some(r.pos - 4);
                return Ok(Some(Flow::Stop));
            }
            _ => return Ok(None),
        }
        Ok(Some(Flow::Continue))
    })?;
    match start {
        Some(start) => Ok((r, start)),
        None => format_err("the map has no block data"),
    }
}

fn read_challenge_body(r: &mut Reader) -> Result<Option<Challenge>> {
    let mut challenge = None;
    read_chunks(r, |id, r| {
        match id {
            0x0304_300D => {
                r.ident()?;
            }
            0x0304_3011 => {
                r.node_ref(read_challenge_parameters)?;
                r.node_ref(read_challenge_parameters)?;
                r.i32()?; // kind
            }
            0x0304_301F => {
                let map_info = r.ident()?;
                let name = r.string()?;
                let decoration = r.ident()?;
                let size = r.int3()?;
                r.bool()?; // need unlock
                let version = r.i32()?;
                let count = r.count()?;
                let blocks = (0..count).map(|_| read_block(r, version)).collect::<Result<Vec<_>>>()?;
                let needs_unlimiter = blocks.len() == 1 && blocks[0].name.starts_with("TMUnlimiter");
                challenge = Some(Challenge {
                    uid: map_info.id,
                    name,
                    author: map_info.author,
                    collection: map_info.collection,
                    size,
                    decoration: decoration.id,
                    blocks,
                    needs_unlimiter,
                });
                // Everything needed is read; MediaTracker clips and other chunks follow
                return Ok(Some(Flow::Stop));
            }
            _ => return Ok(None),
        }
        Ok(Some(Flow::Continue))
    })?;
    Ok(challenge)
}

/// The two parameter nodes of chunk 0x03043011; nothing in them is kept
fn read_challenge_parameters(class_id: u32, r: &mut Reader) -> Result<()> {
    match class_id {
        COLLECTOR_LIST => read_chunks(r, |id, c| {
            if id != 0x0301_B000 {
                return Ok(None);
            }
            let count = c.count()?;
            for _ in 0..count {
                c.ident()?;
                c.i32()?;
            }
            Ok(Some(Flow::Continue))
        }),
        CHALLENGE_PARAMETERS => read_chunks(r, |id, c| {
            let size = match id {
                0x0305_B000 => 32,
                0x0305_B001 => {
                    for _ in 0..4 {
                        c.string()?;
                    }
                    0
                }
                0x0305_B002 => 64,
                0x0305_B003 => 24,
                0x0305_B004 => 20,
                0x0305_B005 => 12,
                0x0305_B006 => c.count()? * 4,
                0x0305_B007 => 0,
                0x0305_B008 => 8,
                _ => return Ok(None),
            };
            c.skip(size)?;
            Ok(Some(Flow::Continue))
        }),
        _ => format_err(format!("map parameters of class 0x{class_id:08X} can't be read")),
    }
    .map(|_| ())
}

fn read_block(r: &mut Reader, version: i32) -> Result<MapBlock> {
    let name = r.id()?;
    let dir = r.u8()?;
    let (x, y, z) = (r.u8()? as i32, r.u8()? as i32, r.u8()? as i32);
    let flags = if version == 0 { r.u16()? as u32 } else { r.u32()? };

    if flags & (1 << 15) != 0 {
        r.id()?; // author
        r.node_ref(read_block_skin)?;
    }
    if version >= 2 && flags & ((1 << 17) | (1 << 18) | (1 << 19) | (1 << 20)) != 0 {
        return format_err(format!("block flags 0x{flags:x} need data this reader doesn't handle"));
    }

    // From version 6, coordinates are stored one higher on X and Z
    let shift = if version >= 6 { 1 } else { 0 };
    Ok(MapBlock {
        name,
        x: x - shift,
        y,
        z: z - shift,
        dir,
        is_ground: flags & (1 << 12) != 0,
        variant: flags & 63,
        sub_variant: (flags >> 6) & 63,
        is_clip: flags & (1 << 13) != 0,
    })
}

/// A block skin node (custom textures on signs): read only to step over it
pub fn read_block_skin(class_id: u32, r: &mut Reader) -> Result<()> {
    if class_id != BLOCK_SKIN {
        return format_err(format!("block skin of class 0x{class_id:08X} can't be read"));
    }
    read_chunks(r, |id, c| {
        match id {
            0x0305_9000 => {
                c.string()?;
                c.string()?;
            }
            0x0305_9001 => {
                c.string()?;
                read_pack_desc(c)?;
            }
            0x0305_9002 => {
                c.string()?;
                read_pack_desc(c)?;
                read_pack_desc(c)?;
            }
            0x0305_9003 => {
                c.i32()?;
                read_pack_desc(c)?;
            }
            _ => return Ok(None),
        }
        Ok(Some(Flow::Continue))
    })
    .map(|_| ())
}

/// The chunks met while reading a file's body, for inspection
#[derive(Debug, Clone, Default)]
pub struct BodyTrace {
    pub spans: Vec<ChunkSpan>,
    /// Where the walk ended, when it didn't reach the end of the main node
    pub stopped_at: Option<usize>,
    /// Why the walk ended early: a reader stopped on purpose, or an error
    pub note: Option<String>,
}

/// Walks a file's body with the readers this crate has for its class, recording every chunk met. Replays and maps
/// use their full readers; other classes get only their skippable chunks stepped over.
pub fn trace_body(file: &GbxFile) -> BodyTrace {
    let mut r = file.body_reader();
    r.start_trace();
    let result = match file.class_id {
        REPLAY_RECORD => read_replay_body(&mut r).map(|_| ()),
        CHALLENGE => read_challenge_body(&mut r).map(|_| ()),
        _ => read_chunks(&mut r, |_, _| Ok(None)).map(|_| ()),
    };
    let mut trace = BodyTrace { spans: r.take_trace(), ..Default::default() };
    let at = r.pos;
    match result {
        Ok(_) if at < file.body.len() && trace.spans.last().is_some_and(|s| s.depth == 0) => {
            trace.stopped_at = Some(at);
            trace.note = Some("Stopped on purpose: the rest of the body (MediaTracker clips and later chunks) isn't needed for analysis".into());
        }
        Ok(_) => {}
        Err(e) => {
            trace.stopped_at = Some(at);
            trace.note = Some(e.to_string());
        }
    }
    trace
}

#[cfg(test)]
mod tests {
    use super::*;
    use gbx::Writer;

    /// A Nations ESWC replay body: ghosts in 0x03093004 with old-style samples and player info, a null clip, then
    /// the inputs in 0x0309300D
    fn eswc_body() -> Vec<u8> {
        let mut w = Writer::new();
        w.u32(0x0309_3004);
        w.i32(0); // version
        w.i32(10); // array version
        w.i32(1);
        w.i32(1); // node index
        w.u32(GHOST);
        w.u32(0x0303_F003);
        w.data(&[]); // two empty samples
        w.i32(2);
        w.i32(0);
        w.i32(0);
        w.i32(0); // no state times
        w.bool(true);
        w.i32(100);
        w.i32(8);
        w.u32(0x0309_2006);
        w.id_text("StadiumCar");
        w.id_text("Vehicles");
        w.id_text("Nadeo");
        w.string("");
        w.i32(0);
        w.string("plastorex");
        w.u32(0xFACA_DE01);
        w.i32(7500);
        w.i32(0); // no record units
        w.u32(0x0309_300C);
        w.i32(-1);
        w.u32(0x0309_300D);
        w.i32(7390);
        w.i32(0);
        w.i32(1);
        w.id_text("Accelerate");
        w.i32(1);
        w.i32(1);
        w.i32(100_010);
        w.u8(0);
        w.u32(1);
        w.bytes
    }

    #[test]
    fn reads_nations_eswc_replays() {
        let body = eswc_body();
        let replay = read_replay_body(&mut Reader::new(&body)).unwrap();
        let ghost = &replay.ghosts[0];
        assert_eq!((ghost.nickname.as_deref(), ghost.vehicle.as_deref()), (Some("plastorex"), Some("StadiumCar")));
        assert_eq!((ghost.samples.len(), ghost.sample_period_ms, ghost.samples[1].time_ms), (2, Some(100), 100));
        assert_eq!(ghost.inputs.len(), 1);
        assert_eq!((ghost.inputs[0].time_ms, ghost.inputs[0].name.as_str()), (10, "Accelerate"));
    }

    #[test]
    fn eswc_replays_with_a_clip_stop_before_the_inputs() {
        let mut body = eswc_body();
        let clip = body.windows(4).position(|w| w == 0x0309_300Cu32.to_le_bytes()).unwrap();
        body[clip + 4..clip + 8].copy_from_slice(&5i32.to_le_bytes());
        let replay = read_replay_body(&mut Reader::new(&body)).unwrap();
        assert!(replay.ghosts[0].inputs.is_empty());
    }
}
