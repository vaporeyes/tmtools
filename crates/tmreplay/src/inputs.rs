// ABOUTME: Decodes recorded input events (name and raw 32-bit data) into Inputs, and TM2020's per-tick input bit stream.
// ABOUTME: Port of GBX.NET's Input.Parse; names match its input types (Accelerate, Steer, FakeIsRaceRunning, ...).

use crate::model::{Input, InputValue};

/// Analog steering: low 16 bits are the amount, the next byte its direction
pub fn steer_value(data: u32) -> i32 {
    let dir = (data >> 16) & 0xFF;
    let value = (data & 0xFFFF) as i32;
    match dir {
        0xFF => 65536 - value,
        // Full left stores no amount
        1 => -65536,
        _ => -value * (dir as i32 + 1),
    }
}

/// Analog gas and brake use the steering encoding, negated
pub fn gas_value(data: u32) -> i32 {
    -steer_value(data)
}

pub fn parse_input(time_ms: i32, name: &str, data: u32) -> Input {
    let button = match name {
        "_FakeDontInverseAxis" => Some("FakeDontInverseAxis"),
        "_FakeFinishLine" => Some("FakeFinishLine"),
        "_FakeIsRaceRunning" => Some("FakeIsRaceRunning"),
        "Accelerate" | "Brake" | "Horn" | "Respawn" | "SteerLeft" | "SteerRight" => Some(name),
        _ => None,
    };
    if let Some(button) = button {
        return Input { time_ms, name: button.into(), value: InputValue::Pressed(data != 0) };
    }
    let analog = match name {
        "AccelerateReal" | "BrakeReal" | "Gas" => Some(gas_value(data)),
        "Steer" => Some(steer_value(data)),
        _ => None,
    };
    match analog {
        Some(value) => Input { time_ms, name: name.into(), value: InputValue::Analog(value) },
        None => Input { time_ms, name: "UnknownInput".into(), value: InputValue::None },
    }
}

/// Reads bits least significant first, as TM2020's input stream stores them; past the end reads zeros
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl BitReader<'_> {
    fn bit(&mut self) -> bool {
        let bit = self.data.get(self.pos / 8).is_some_and(|b| b & (1 << (self.pos % 8)) != 0);
        self.pos += 1;
        bit
    }

    fn number(&mut self, bits: u32) -> u64 {
        (0..bits).fold(0, |n, i| n | (self.bit() as u64) << i)
    }
}

/// TM2020 player inputs (ghost chunk 0x0309201D): a bit stream with one entry per 10 ms tick, each saying what
/// changed. Port of GBX.NET's ProcessTrackmaniaInputs for a car; `version` is the input data's (11 or 12) and
/// `start_ms` the time of the first tick.
pub fn read_tm2020_inputs(data: &[u8], ticks: usize, version: i32, start_ms: i32) -> Vec<Input> {
    let mut r = BitReader { data, pos: 0 };
    let mut inputs = Vec::new();
    let mut in_vehicle = false;
    let (mut steer, mut accel, mut brake, mut horn) = (0i8, false, false, false);
    let press = |inputs: &mut Vec<Input>, time_ms: i32, name: &str, pressed: bool| {
        inputs.push(Input { time_ms, name: name.into(), value: InputValue::Pressed(pressed) });
    };
    // Every tick takes at least two bits, so the stream can't hold more ticks than that; past it, reads are zeros
    let ticks = ticks.min(data.len().saturating_mul(4));
    for tick in 0..ticks {
        let Some(time_ms) = i32::try_from(tick).ok().and_then(|t| t.checked_mul(10)).and_then(|t| t.checked_add(start_ms)) else { break };
        if !r.bit() {
            let horn_bit = if r.bit() {
                // Only the two low state bits follow
                r.number(2) & 2 != 0
            } else {
                let states = r.number(if version == 11 { 33 } else { 34 });
                // 2 to 4: driving a car; 1 and others: on foot
                in_vehicle = matches!(states & 0xF, 2..=4);
                // The countdown's start is recorded as a respawn too
                if (states >> 31) & 1 != 0 && time_ms >= 0 {
                    press(&mut inputs, time_ms, "Respawn", true);
                }
                if (states >> 33) & 1 != 0 {
                    press(&mut inputs, time_ms, "SecondaryRespawn", true);
                }
                states & (1 << 6) != 0
            };
            if in_vehicle && horn_bit != horn {
                horn = horn_bit;
                press(&mut inputs, time_ms, "Horn", horn);
            }
        }
        // Mouse movement
        if !r.bit() {
            r.number(32);
        }
        if in_vehicle && !r.bit() {
            let value = r.number(8) as u8 as i8;
            if value != steer {
                steer = value;
                inputs.push(Input { time_ms, name: "Steer".into(), value: InputValue::Analog(steer as i32 * 65536 / 127) });
            }
            for (name, held) in [("Accelerate", &mut accel), ("Brake", &mut brake)] {
                let bit = r.bit();
                if bit != *held {
                    *held = bit;
                    press(&mut inputs, time_ms, name, bit);
                }
            }
        }
    }
    inputs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_counts_are_bounded_by_the_stream() {
        // Billions of ticks claimed over no data end at once; times near the limit stop rather than overflow
        assert!(read_tm2020_inputs(&[], usize::MAX, 12, 0).is_empty());
        assert!(read_tm2020_inputs(&[0; 64], 1000, 12, i32::MAX - 5).len() <= 1000);
    }

    #[test]
    fn decodes_steering_directions() {
        assert_eq!(steer_value(0x00FF_0000), 65536);
        assert_eq!(steer_value(0x00FF_8000), 32768);
        assert_eq!(steer_value(0x0001_0000), -65536);
        assert_eq!(steer_value(0x0000_4000), -16384);
        assert_eq!(gas_value(0x0000_4000), 16384);
    }

    #[test]
    fn names_buttons_and_analog_inputs() {
        assert_eq!(parse_input(10, "Accelerate", 1).value, InputValue::Pressed(true));
        assert_eq!(parse_input(0, "_FakeIsRaceRunning", 1).name, "FakeIsRaceRunning");
        assert_eq!(parse_input(20, "Steer", 0x00FF_0000).value, InputValue::Analog(65536));
        assert_eq!(parse_input(30, "Mystery", 1).name, "UnknownInput");
    }
}
