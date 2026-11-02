// ABOUTME: TM2020 ghost recordings (CPlugEntRecordData): the zlib-compressed entity streams, of which the vehicle's
// ABOUTME: states (CSceneVehicleVis) become car samples. Port of GBX.NET's CPlugEntRecordData and its EntRecordDelta.

use crate::model::Sample;
use gbx::{format_err, read_chunks, Flow, Reader, Result};
use std::f32::consts::TAU;
use std::io::Read;

const ENT_RECORD_DATA: u32 = 0x0911_F000;
/// CSceneVehicleVis: the car's visual state
const VEHICLE_VIS: u32 = 0x0A01_8000;
/// Bytes a vehicle state needs for everything read from it
const VEHICLE_STATE_LEN: usize = 103;

/// One entity's recorded states, each with its time in milliseconds
struct Entity {
    class_id: u32,
    states: Vec<(i32, Vec<u8>)>,
}

/// Reads a CPlugEntRecordData node into the samples of its vehicle; the vehicle with the most states when there are
/// several
pub fn read_ent_record(class_id: u32, r: &mut Reader) -> Result<Vec<Sample>> {
    if class_id != ENT_RECORD_DATA {
        return format_err(format!("expected ghost record data, found class 0x{class_id:08X}"));
    }
    let mut samples = Vec::new();
    read_chunks(r, |id, c| {
        if id != 0x0911_F000 {
            return Ok(None);
        }
        let version = c.i32()?;
        if version < 5 {
            return format_err(format!("ghost record data version {version} is not supported"));
        }
        let uncompressed = c.count()?;
        let mut bytes = Vec::with_capacity(uncompressed);
        if flate2::read::ZlibDecoder::new(c.data()?).read_to_end(&mut bytes).is_err() || bytes.len() != uncompressed {
            return format_err("ghost record data doesn't decompress to its stated size");
        }
        let entities = read_entities(&mut Reader::new(&bytes), version)?;
        let vehicle = entities.into_iter().filter(|e| e.class_id == VEHICLE_VIS).max_by_key(|e| e.states.len());
        samples = vehicle.map(|v| v.states.iter().map(|(time, state)| vehicle_sample(*time, state)).collect::<Result<_>>()).transpose()?.unwrap_or_default();
        Ok(Some(Flow::Continue))
    })?;
    Ok(samples)
}

/// The record's entity descriptions, notice descriptions, then each entity's states
fn read_entities(r: &mut Reader, version: i32) -> Result<Vec<Entity>> {
    r.i32()?; // start
    r.i32()?; // end
    let desc_count = r.count()?;
    let mut classes = Vec::with_capacity(desc_count);
    for _ in 0..desc_count {
        classes.push(r.u32()?);
        r.i32()?; // state size
        r.i32()?; // scene UID offset
        r.i32()?; // state archive version
        r.data()?; // initial state
        r.i32()?;
    }
    let notice_count = r.count()?;
    for _ in 0..notice_count {
        r.i32()?;
        r.i32()?;
        if version >= 4 {
            r.u32()?;
        }
    }

    let mut entities = Vec::new();
    let mut has_next = r.u8()? != 0;
    while has_next {
        let kind = r.count()?;
        r.i32()?; // scene UID
        r.i32()?; // begin time
        r.i32()?; // end time
        if version >= 6 {
            r.i32()?;
        }
        let Some(&class_id) = classes.get(kind) else {
            return format_err(format!("ghost entity type {kind} has no description"));
        };
        let states = if version >= 11 { read_encoded_states(r)? } else { read_states(r)? };
        has_next = r.u8()? != 0;
        // Notices the entity raised: type, time, data
        while r.u8()? != 0 {
            r.i32()?;
            r.i32()?;
            r.data()?;
        }
        entities.push(Entity { class_id, states });
    }
    Ok(entities)
}

/// Before version 11: a flag byte before each state, then its time and bytes
fn read_states(r: &mut Reader) -> Result<Vec<(i32, Vec<u8>)>> {
    let mut states = Vec::new();
    while r.u8()? != 0 {
        let time = r.i32()?;
        states.push((time, r.data()?.to_vec()));
    }
    Ok(states)
}

/// Version 11: the count, the state size, each state's time step, then the bytes column by column: byte i of every
/// state, each stored as its difference from the same byte of the state before
fn read_encoded_states(r: &mut Reader) -> Result<Vec<(i32, Vec<u8>)>> {
    let count = r.count()?;
    if count == 0 {
        return Ok(Vec::new());
    }
    let size = r.count()?;
    let mut time = 0;
    let mut states = Vec::with_capacity(count);
    for _ in 0..count {
        time += r.i32()?;
        states.push((time, vec![0; size]));
    }
    for i in 0..size {
        let mut value = 0u8;
        for (state, &delta) in states.iter_mut().zip(r.read(count)?) {
            value = value.wrapping_add(delta);
            state.1[i] = value;
        }
    }
    Ok(states)
}

/// A vehicle state at the offsets GBX.NET reads it from
fn vehicle_sample(time_ms: i32, state: &[u8]) -> Result<Sample> {
    if state.len() < VEHICLE_STATE_LEN {
        return format_err(format!("vehicle state of {} bytes is too short", state.len()));
    }
    let unit = |b: u8| b as f32 / 255.0;
    let wheel = |at: usize| unit(state[at]) * TAU + state[at + 1] as f32 * TAU;
    let dampen = |at: usize| (unit(state[at]) - 0.5) * 4.0;

    let mut r = Reader::new(&state[47..]);
    let position = r.vec3()?;
    let rotation = r.quat6()?;
    let velocity = crate::samples::packed_vec3(r.u32()?);

    let (slip1, slip2) = (state[32], state[33]);
    let brake = unit(state[18]);
    Ok(Sample {
        time_ms,
        position,
        rotation,
        velocity,
        speed_kmh: (velocity.x * velocity.x + velocity.y * velocity.y + velocity.z * velocity.z).sqrt() * 3.6,
        // FL, FR, RR, RL
        wheel_rotation: [wheel(6), wheel(8), wheel(10), wheel(12)],
        dampen_len: [dampen(23), dampen(25), dampen(27), dampen(29)],
        // GBX.NET reads gas as byte 15 plus the brake
        gas: (unit(state[15]) + brake).min(1.0),
        brake,
        steer: (unit(state[14]) - 0.5) * 2.0,
        // The state keeps RPM in one byte; hundreds of RPM is assumed
        rpm: state[5] as f32 * 100.0,
        turbo: unit(state[21]),
        // The slip and contact bits sit where TMF's do
        sliding: crate::samples::wheel_mask(slip1 & 0x40 != 0, slip2 & 0x01 != 0, slip2 & 0x04 != 0, slip2 & 0x10 != 0),
        grounded: crate::samples::wheel_mask(slip1 & 0x80 != 0, slip2 & 0x02 != 0, slip2 & 0x08 != 0, slip2 & 0x20 != 0),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_states_are_columns_of_deltas() {
        // Two states of two bytes, 40 ms then 50 ms apart: [1, 9] then [3, 8]
        let mut bytes = Vec::new();
        for v in [2i32, 2, 40, 50] {
            bytes.extend(v.to_le_bytes());
        }
        bytes.extend([1, 2, 9, 255]);
        let states = read_encoded_states(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(states, vec![(40, vec![1, 9]), (90, vec![3, 8])]);
    }
}
