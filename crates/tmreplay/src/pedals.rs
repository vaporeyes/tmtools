// ABOUTME: A run's path split by what the driver's feet were doing: on the gas, braking, or coasting, from the exact
// ABOUTME: inputs (or the car's samples when a run has none), cut where each press and release happens.

use crate::gap::position_at;
use crate::track::{Channel, InputTrack};
use crate::{Sample, Vec3};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pedal {
    Gas,
    Brake,
    /// Neither pedal
    Coast,
}

/// The pedals from time_ms until the next change; braking wins when both are pressed
fn pedal_at(samples: &[Sample], track: Option<&InputTrack>, time_ms: i32) -> Pedal {
    let (gas, brake) = match track {
        Some(t) => (t.value_at(Channel::Gas, time_ms), t.value_at(Channel::Brake, time_ms)),
        // The sample at or before the time
        None => {
            let i = samples.partition_point(|s| s.time_ms <= time_ms).saturating_sub(1);
            samples.get(i).map_or((0.0, 0.0), |s| (s.gas.round(), s.brake.round()))
        }
    };
    if brake > 0.0 {
        Pedal::Brake
    } else if gas > 0.0 {
        Pedal::Gas
    } else {
        Pedal::Coast
    }
}

/// The path in stretches of one pedal state each, every stretch a line of points through the samples and the exact
/// spots where the pedals changed
pub fn pedal_stretches(samples: &[Sample], track: Option<&InputTrack>) -> Vec<(Pedal, Vec<Vec3>)> {
    let (Some(first), Some(last)) = (samples.first(), samples.last()) else { return Vec::new() };
    let mut times: Vec<i32> = samples.iter().map(|s| s.time_ms).collect();
    if let Some(t) = track {
        for channel in [Channel::Gas, Channel::Brake] {
            times.extend(t.steps(channel).iter().map(|s| s.time_ms).filter(|&ms| ms > first.time_ms && ms < last.time_ms));
        }
    }
    times.sort_unstable();
    times.dedup();

    let mut stretches: Vec<(Pedal, Vec<Vec3>)> = Vec::new();
    for pair in times.windows(2) {
        let pedal = pedal_at(samples, track, pair[0]);
        let (Some(a), Some(b)) = (position_at(samples, pair[0] as f32), position_at(samples, pair[1] as f32)) else { continue };
        match stretches.last_mut() {
            Some((current, points)) if *current == pedal => points.push(b),
            _ => stretches.push((pedal, vec![a, b])),
        }
    }
    stretches
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Input, InputValue};

    /// A car along x at 1 unit per 100 ms for a second
    fn samples() -> Vec<Sample> {
        (0..=10).map(|i| Sample { time_ms: i * 100, position: Vec3 { x: i as f32, y: 0.0, z: 0.0 }, ..Default::default() }).collect()
    }

    fn press(time_ms: i32, name: &str, pressed: bool) -> Input {
        Input { time_ms, name: name.into(), value: InputValue::Pressed(pressed) }
    }

    #[test]
    fn splits_the_path_exactly_where_the_pedals_change() {
        // Gas from the start, a brake tap from 0.25 s to 0.42 s (the gas held through it), gas off at 0.7 s
        let inputs = [press(0, "Accelerate", true), press(250, "Brake", true), press(420, "Brake", false), press(700, "Accelerate", false)];
        let track = InputTrack::new(&inputs, 1000).unwrap();
        let stretches = pedal_stretches(&samples(), Some(&track));
        let summary: Vec<(Pedal, f32, f32)> = stretches.iter().map(|(p, pts)| (*p, pts[0].x, pts.last().unwrap().x)).collect();
        assert_eq!(summary, [(Pedal::Gas, 0.0, 2.5), (Pedal::Brake, 2.5, 4.2), (Pedal::Gas, 4.2, 7.0), (Pedal::Coast, 7.0, 10.0)]);
    }

    #[test]
    fn falls_back_to_the_samples_without_inputs() {
        let mut s = samples();
        for x in s.iter_mut().take(5) {
            x.gas = 1.0;
        }
        let stretches = pedal_stretches(&s, None);
        assert_eq!(stretches.iter().map(|(p, _)| *p).collect::<Vec<_>>(), [Pedal::Gas, Pedal::Coast]);
    }
}
