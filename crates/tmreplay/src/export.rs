// ABOUTME: Exports for other tools: a run's input events as CSV, its inputs as a TMInterface-style TAS script, and its
// ABOUTME: samples (position, speed, pedals, steering, RPM, wheel contact) as CSV.

use crate::model::{Input, InputValue, Sample};
use crate::track::{Channel, InputTrack, TICK_MS};
use std::fmt::Write;

/// Every recorded input event: time, tick, input name, and value (1 or 0 for buttons, the raw amount for analog)
pub fn inputs_csv(inputs: &[Input]) -> String {
    let mut out = String::from("time_ms,tick,input,value\n");
    for i in inputs {
        let value = match i.value {
            InputValue::Pressed(p) => (p as i32).to_string(),
            InputValue::Analog(v) => v.to_string(),
            InputValue::None => String::new(),
        };
        let _ = writeln!(out, "{},{},{},{value}", i.time_ms, i.time_ms / TICK_MS, i.name);
    }
    out
}

/// A time as TMInterface scripts write it: seconds with hundredths
fn script_time(ms: i32) -> String {
    format!("{}.{:02}", ms / 1000, (ms % 1000) / 10)
}

/// The run's inputs as a TMInterface-style script: digital holds as "start-end press key" ranges (a hold still down at
/// the end of the run is a lone press), analog steering and gas as raw "steer" and "gas" values, and respawns as
/// "press enter". Lines are in time order; `#` lines are comments.
pub fn tas_script(inputs: &[Input], track: &InputTrack, title: &str) -> String {
    let mut lines: Vec<(i32, String)> = Vec::new();
    let analog_steer = track.analog_steer.is_some();
    for (channel, key) in [(Channel::Gas, "up"), (Channel::Brake, "down"), (Channel::Left, "left"), (Channel::Right, "right")] {
        // Analog steering is written as raw values below, not as left and right holds
        if analog_steer && matches!(channel, Channel::Left | Channel::Right) {
            continue;
        }
        let digital = track.steps(channel).iter().all(|s| s.value == 0.0 || s.value == 1.0);
        if !digital {
            continue;
        }
        for hold in track.holds(channel) {
            let released = track.steps(channel).iter().any(|s| s.time_ms == hold.end_ms && s.value == 0.0);
            let line = if released {
                format!("{}-{} press {key}", script_time(hold.start_ms), script_time(hold.end_ms))
            } else {
                format!("{} press {key}", script_time(hold.start_ms))
            };
            lines.push((hold.start_ms, line));
        }
    }
    for i in inputs {
        match (i.name.as_str(), i.value) {
            ("Steer", InputValue::Analog(v)) => lines.push((i.time_ms, format!("{} steer {v}", script_time(i.time_ms)))),
            ("Gas", InputValue::Analog(v)) => lines.push((i.time_ms, format!("{} gas {v}", script_time(i.time_ms)))),
            ("Respawn", InputValue::Pressed(true)) => lines.push((i.time_ms, format!("{} press enter", script_time(i.time_ms)))),
            _ => {}
        }
    }
    lines.sort_by_key(|(t, _)| *t);
    let mut out = format!("# {title}\n# Exported by tmtools from the replay's recorded inputs\n");
    for (_, line) in lines {
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// One row per sample: time, position, speed, pedals and steering as the car recorded them, RPM, turbo, and how many
/// wheels slide and touch the ground
pub fn telemetry_csv(samples: &[Sample]) -> String {
    let mut out = String::from("time_ms,x,y,z,speed_kmh,gas,brake,steer,rpm,turbo,wheels_sliding,wheels_grounded\n");
    for s in samples {
        let p = s.position;
        let _ = writeln!(
            out,
            "{},{:.3},{:.3},{:.3},{:.2},{:.3},{:.3},{:.3},{:.0},{:.3},{},{}",
            s.time_ms,
            p.x,
            p.y,
            p.z,
            s.speed_kmh,
            s.gas,
            s.brake,
            s.steer,
            s.rpm,
            s.turbo,
            s.sliding.count_ones(),
            s.grounded.count_ones()
        );
    }
    out
}

/// A file name part made of letters, digits, and dashes, from formatted player or map names
pub fn file_stem(text: &str) -> String {
    let plain = crate::strip_formatting(text);
    let mut out = String::new();
    for c in plain.chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let trimmed = out.trim_end_matches('-').to_owned();
    if trimmed.is_empty() {
        "run".into()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gbx::Vec3;

    fn button(time_ms: i32, name: &str, pressed: bool) -> Input {
        Input { time_ms, name: name.into(), value: InputValue::Pressed(pressed) }
    }

    #[test]
    fn inputs_become_csv_rows() {
        let csv = inputs_csv(&[button(970, "SteerRight", true), Input { time_ms: 20, name: "Steer".into(), value: InputValue::Analog(-1200) }]);
        assert_eq!(csv, "time_ms,tick,input,value\n970,97,SteerRight,1\n20,2,Steer,-1200\n");
    }

    #[test]
    fn digital_holds_become_ranges_and_open_holds_lone_presses() {
        let inputs = [button(10, "Accelerate", true), button(970, "SteerRight", true), button(1090, "SteerRight", false), button(5000, "Respawn", true)];
        let track = InputTrack::new(&inputs, 12_850).unwrap();
        let script = tas_script(&inputs, &track, "Run on Map");
        assert_eq!(script, "# Run on Map\n# Exported by tmtools from the replay's recorded inputs\n0.01 press up\n0.97-1.09 press right\n5.00 press enter\n");
    }

    #[test]
    fn analog_steering_is_written_as_raw_values() {
        let inputs = [Input { time_ms: 330, name: "Steer".into(), value: InputValue::Analog(63249) }, button(10, "Accelerate", true)];
        let track = InputTrack::new(&inputs, 1000).unwrap();
        let script = tas_script(&inputs, &track, "t");
        assert!(script.contains("0.33 steer 63249\n"));
        assert!(!script.contains("press right"));
    }

    #[test]
    fn telemetry_has_one_row_per_sample() {
        let s = Sample { time_ms: 100, position: Vec3 { x: 1.0, y: 2.0, z: 3.0 }, speed_kmh: 57.5, gas: 1.0, sliding: 0b0011, grounded: 0b1111, ..Default::default() };
        let csv = telemetry_csv(&[s]);
        assert_eq!(csv.lines().count(), 2);
        assert_eq!(csv.lines().nth(1).unwrap(), "100,1.000,2.000,3.000,57.50,1.000,0.000,0.000,0,0.000,2,4");
    }

    #[test]
    fn file_stems_drop_formatting_and_symbols() {
        assert_eq!(file_stem("$f45Alex$fffbros! ツ <3"), "Alexbros-ツ-3");
        assert_eq!(file_stem("$o$n$s$fffInterlock #USM"), "Interlock-USM");
        assert_eq!(file_stem("$$"), "run");
    }
}
