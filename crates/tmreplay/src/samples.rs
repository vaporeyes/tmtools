// ABOUTME: Ghost sample data: the zlib-compressed state buffer of a TMF ghost, decoded into car samples.
// ABOUTME: Port of GBX.NET's CGameGhost.Data (ReadNew) and CSceneVehicleCar.Sample, with f32 math as there.

use crate::model::Sample;
use gbx::{format_err, Quat, Reader, Result, Vec3};
use std::f32::consts::PI;
use std::io::Read;

const CAR_CLASS_IDS: [u32; 2] = [0x0A02_B000, 0x0A10_3000];

pub struct SampleData {
    pub sample_period_ms: i32,
    pub samples: Vec<Sample>,
}

/// Raw fields of one CSceneVehicleCar sample that need unpacking
#[derive(Default)]
struct RawCar {
    position: Vec3,
    rotation: Quat,
    velocity: u32,
    angular_velocity: u32,
    rpm: u16,
    wheel_rotation: [u16; 4],
    steer: u8,
    gas: u8,
    brake: u8,
    turbo: u8,
    steer_front: u8,
    dampen_len: [u8; 4],
    u25: u8,
    u26: u8,
    u27: u8,
    dirt_blend: Option<u8>,
}

/// Largest inflated ghost data accepted; real ghosts are far smaller, and the stated size comes from the file
const MAX_INFLATED_LEN: usize = 256 << 20;

/// Zlib data that must inflate to exactly `size` bytes. Reads at most one byte past that, so a small stream can't
/// expand without limit, and refuses sizes too large to be real.
pub(crate) fn inflate_exact(compressed: &[u8], size: usize, what: &str) -> Result<Vec<u8>> {
    if size > MAX_INFLATED_LEN {
        return format_err(format!("{what} of {size} bytes is too large"));
    }
    let mut bytes = Vec::with_capacity(size);
    let read = flate2::read::ZlibDecoder::new(compressed).take(size as u64 + 1).read_to_end(&mut bytes);
    if read.is_err() || bytes.len() != size {
        return format_err(format!("{what} doesn't decompress to its stated size"));
    }
    Ok(bytes)
}

/// The time of sample `i` at a fixed period
fn fixed_step_time(i: usize, period_ms: i32) -> Result<i32> {
    i32::try_from(i).ok().and_then(|i| i.checked_mul(period_ms)).map_or_else(|| format_err("ghost sample times overflow"), Ok)
}

pub fn read_sample_data(compressed: &[u8], uncompressed_size: usize) -> Result<SampleData> {
    let bytes = inflate_exact(compressed, uncompressed_size, "ghost sample data")?;

    let mut r = Reader::new(&bytes);
    let class_id = r.u32()?;
    if class_id == u32::MAX {
        return Ok(SampleData { sample_period_ms: 0, samples: Vec::new() });
    }

    let is_fixed_time_step = r.bool()?;
    r.i32()?;
    let sample_period_ms = r.i32()?;
    let version = r.i32()?;
    let state_buffer = r.data()?;

    let count = r.count()?;
    let mut size_per_sample = -1;
    let mut sample_sizes = Vec::new();
    if count > 0 {
        r.i32()?; // first sample offset
        if count > 1 {
            size_per_sample = r.i32()?;
            // -1 means each sample has its own size; otherwise every sample takes at least a byte
            if size_per_sample == 0 || size_per_sample < -1 {
                return format_err(format!("ghost samples of {size_per_sample} bytes each"));
            }
            if size_per_sample == -1 {
                sample_sizes = (0..count - 1).map(|_| r.count()).collect::<Result<_>>()?;
            }
        }
    }
    let state_times = if is_fixed_time_step { Vec::new() } else { r.i32_array()? };
    if version >= 10 {
        r.i32()?;
    }
    if r.remaining() != 0 {
        return format_err(format!("ghost sample header has {} unread bytes", r.remaining()));
    }
    if !CAR_CLASS_IDS.contains(&class_id) && count > 0 {
        return format_err(format!("ghost samples of class 0x{class_id:08X} are not supported"));
    }

    let mut buffer = Reader::new(state_buffer);
    let mut samples = Vec::with_capacity(count.min(state_buffer.len()));
    let mut time = 0;
    for i in 0..count {
        let data = if size_per_sample != -1 {
            buffer.read(size_per_sample as usize)?
        } else if i == count - 1 {
            buffer.read(buffer.remaining())?
        } else {
            buffer.read(sample_sizes.get(i).copied().unwrap_or(0))?
        };
        time = if state_times.is_empty() {
            fixed_step_time(i, sample_period_ms)?
        } else {
            let Some(next) = time.checked_add(state_times.get(i).copied().unwrap_or(0)) else { return format_err("ghost sample times overflow") };
            next
        };
        samples.push(to_sample(time, &read_car(data, version)?));
    }

    Ok(SampleData { sample_period_ms, samples })
}

/// TM Nations ESWC and earlier (ghost chunk 0x0303F003): uncompressed sample bytes, then each sample's offset into
/// them, state times, whether the time step is fixed, the period, and the sample version. Port of GBX.NET's ReadOld
/// and ReadSamplesOld.
pub fn read_old_sample_data(r: &mut Reader) -> Result<SampleData> {
    let raw = r.data()?;
    let offsets = r.i32_array()?;
    r.i32_array()?; // state times
    r.bool()?; // fixed time step
    let sample_period_ms = r.i32()?;
    let version = r.i32()?;

    let mut samples = Vec::with_capacity(offsets.len());
    for (i, &start) in offsets.iter().enumerate() {
        let end = offsets.get(i + 1).copied().unwrap_or(raw.len() as i32);
        let Some(data) = raw.get(start.max(0) as usize..end.max(start) as usize) else {
            return format_err("ghost sample offsets run past the sample data");
        };
        samples.push(to_sample(fixed_step_time(i, sample_period_ms)?, &read_car(data, version)?));
    }
    Ok(SampleData { sample_period_ms, samples })
}

fn read_car(data: &[u8], version: i32) -> Result<RawCar> {
    let mut car = RawCar::default();
    if data.is_empty() {
        return Ok(car);
    }
    if version < 7 || version == 13 {
        return format_err(format!("car sample version {version} is not supported"));
    }

    let mut r = Reader::new(data);
    car.position = r.vec3()?;
    car.rotation = r.quat6()?;
    car.velocity = r.u32()?;
    car.angular_velocity = r.u32()?;

    r.u16()?; // speed forward
    r.u16()?; // speed sideward
    car.rpm = r.u16()?;
    car.wheel_rotation = [r.u16()?, r.u16()?, r.u16()?, r.u16()?];
    car.steer = r.u8()?;
    car.gas = r.u8()?;
    car.brake = r.u8()?;
    r.u8()?;
    if version >= 8 {
        r.u8()?;
    }
    r.u8()?;
    r.u8()?;
    car.turbo = r.u8()?;
    car.steer_front = r.u8()?;

    // Each wheel: suspension length, then its ground contact material
    for wheel in 0..4 {
        car.dampen_len[wheel] = r.u8()?;
        r.u8()?;
    }

    if version >= 8 {
        car.u25 = r.u8()?;
        car.u26 = r.u8()?;
        car.u27 = r.u8()?;
        if version >= 9 {
            car.dirt_blend = Some(r.u8()?);
            if version >= 10 {
                if r.u32()? != 0 {
                    return format_err("unexpected car sample data");
                }
                r.u8()?;
                r.u8()?;
                let u35 = r.u8()?;
                if version >= 11 {
                    let extra_bytes = match version {
                        v if v >= 16 => 5,
                        15 => 3,
                        14 => 2,
                        _ => 0,
                    };
                    r.skip(extra_bytes)?;
                    // Count is broken in specific cases like the last sample of a ghost
                    let mut extra = (u35 >> 2) & 7;
                    if version == 11 && extra > 4 {
                        extra = 4;
                    }
                    for _ in 0..extra {
                        r.vec3()?;
                        r.quat6()?;
                        r.u8()?;
                    }
                }
            }
        }
    }

    if r.remaining() != 0 {
        return format_err("not all car sample bytes were read");
    }
    Ok(car)
}

/// A packed vector: 16-bit log magnitude, then heading and pitch bytes
pub(crate) fn packed_vec3(packed: u32) -> Vec3 {
    let mag16 = packed as u16 as i16;
    let heading = (packed >> 16) as u8 as i8;
    let pitch = (packed >> 24) as u8 as i8;

    let mag = if mag16 == i16::MIN { 0.0 } else { (mag16 as f32 / 1000.0).exp() };
    let axis_heading = (heading as f32 * PI) / 127.0;
    let axis_pitch = (pitch as f32 * (PI / 2.0)) / 127.0;
    let cos_pitch = axis_pitch.cos();
    Vec3 { x: axis_heading.cos() * cos_pitch * mag, y: axis_heading.sin() * cos_pitch * mag, z: axis_pitch.sin() * mag }
}

pub(crate) fn wheel_mask(fl: bool, fr: bool, rr: bool, rl: bool) -> u8 {
    fl as u8 | (fr as u8) << 1 | (rr as u8) << 2 | (rl as u8) << 3
}

fn to_sample(time_ms: i32, c: &RawCar) -> Sample {
    let velocity = packed_vec3(c.velocity);
    let unit = |b: u8| b as f32 / 255.0;
    Sample {
        time_ms,
        position: c.position,
        rotation: c.rotation,
        velocity,
        angular_velocity: packed_vec3(c.angular_velocity),
        speed_kmh: (velocity.x * velocity.x + velocity.y * velocity.y + velocity.z * velocity.z).sqrt() * 3.6,
        wheel_rotation: c.wheel_rotation.map(|w| (w as f32 / 65535.0) * 1608.495),
        steer_front: (c.steer_front as f32 / 255.0) * PI * 2.0 - PI,
        dampen_len: c.dampen_len.map(|d| (d as f32 / 255.0) * 4.0 - 2.0),
        gas: unit(c.gas),
        brake: unit(c.brake),
        steer: (c.steer as f32 / 255.0) * 2.0 - 1.0,
        rpm: (c.rpm as f32 / 65535.0) * 30000.0,
        turbo: unit(c.turbo),
        dirt_blend: c.dirt_blend.map_or(0.0, unit),
        horn: (c.u25 >> 3) & 3,
        sliding: wheel_mask(c.u26 & 0x40 != 0, c.u27 & 0x01 != 0, c.u27 & 0x04 != 0, c.u27 & 0x10 != 0),
        grounded: wheel_mask(c.u26 & 0x80 != 0, c.u27 & 0x02 != 0, c.u27 & 0x08 != 0, c.u27 & 0x20 != 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    #[test]
    fn inflates_only_the_stated_size() {
        assert_eq!(inflate_exact(&zlib(b"abc"), 3, "data").unwrap(), b"abc");
        // A small stream of 64 MiB of zeros claiming 10 bytes stops after 11
        assert!(inflate_exact(&zlib(&vec![0; 64 << 20]), 10, "data").is_err());
        assert!(inflate_exact(&zlib(b"abc"), usize::MAX, "data").is_err());
    }

    #[test]
    fn rejects_samples_with_no_bytes_each() {
        // A car class, fixed time step, 50 ms, version 10, no state bytes, then i32::MAX samples of 0 bytes
        let header: Vec<u8> = [0x0A02_B000u32, 1, 0, 50, 10, 0, i32::MAX as u32, 0, 0, 0].iter().flat_map(|v| v.to_le_bytes()).collect();
        assert!(read_sample_data(&zlib(&header), header.len()).is_err());
    }

    #[test]
    fn fixed_step_times_that_overflow_are_an_error() {
        assert_eq!(fixed_step_time(3, 50).unwrap(), 150);
        assert!(fixed_step_time(usize::MAX, 50).is_err());
        assert!(fixed_step_time(50_000_000, 50).is_err());
    }

    #[test]
    fn unpacks_log_magnitude_vectors() {
        // Magnitude e^0 = 1 straight along x; the minimum magnitude code means zero
        let v = packed_vec3(0);
        assert!((v.x - 1.0).abs() < 1e-6 && v.y.abs() < 1e-6 && v.z.abs() < 1e-6);
        assert_eq!(packed_vec3(0x8000), Vec3 { x: 0.0, y: 0.0, z: 0.0 });
        // Pitch 127 points straight up
        let up = packed_vec3(0x7F00_0000);
        assert!((up.z - 1.0).abs() < 1e-6);
    }
}
