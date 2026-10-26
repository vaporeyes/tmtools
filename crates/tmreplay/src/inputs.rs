// ABOUTME: Decodes a recorded input event (name and raw 32-bit data) into an Input.
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

#[cfg(test)]
mod tests {
    use super::*;

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
