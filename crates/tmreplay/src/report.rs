// ABOUTME: Braking zones and turns found in a run's tick-exact inputs, with the speeds through each (entry, minimum,
// ABOUTME: exit), braking within it, and how the steering was done (one hold, a series of taps, or analog amounts).

use crate::gap::speed_at;
use crate::model::Sample;
use crate::track::{Channel, Hold, InputTrack, TICK_MS};

/// Holds of the same input closer than this belong to one turn or braking zone, so taps count as one event
pub const MERGE_GAP_MS: i32 = 250;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Brake,
    Left,
    Right,
}

impl EventKind {
    pub fn label(self) -> &'static str {
        match self {
            EventKind::Brake => "Brake",
            EventKind::Left => "Left",
            EventKind::Right => "Right",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub kind: EventKind,
    pub start_ms: i32,
    pub end_ms: i32,
    /// Holds merged into the event; more than one means taps
    pub holds: usize,
    /// Time the input was actually held, out of the event's span
    pub held_ms: i32,
    /// Largest share held, 0 to 1; below 1 only for analog inputs
    pub peak: f32,
    pub entry_kmh: f32,
    pub min_kmh: f32,
    pub min_at_ms: i32,
    pub exit_kmh: f32,
    /// Ticks of braking within the event (for a brake event, all of its held ticks)
    pub brake_ticks: i32,
}

impl Event {
    pub fn span_ms(&self) -> i32 {
        self.end_ms - self.start_ms
    }

    /// "hold", "3 taps, 62% held", or "analog, peak 87%"
    pub fn pattern(&self) -> String {
        if self.peak < 0.999 {
            format!("analog, peak {:.0}%", self.peak * 100.0)
        } else if self.holds > 1 {
            format!("{} taps, {:.0}% held", self.holds, self.held_ms as f32 * 100.0 / self.span_ms().max(1) as f32)
        } else {
            "hold".into()
        }
    }
}

/// Holds merged where the gap between them is under MERGE_GAP_MS, as (start, end, holds merged, held ms)
fn merge(holds: &[Hold]) -> Vec<(i32, i32, usize, i32)> {
    let mut out: Vec<(i32, i32, usize, i32)> = Vec::new();
    for h in holds {
        match out.last_mut() {
            Some(last) if h.start_ms - last.1 < MERGE_GAP_MS => {
                last.1 = h.end_ms;
                last.2 += 1;
                last.3 += h.duration_ms();
            }
            _ => out.push((h.start_ms, h.end_ms, 1, h.duration_ms())),
        }
    }
    out
}

/// The run's braking zones and turns in time order
pub fn events(track: &InputTrack, samples: &[Sample]) -> Vec<Event> {
    let mut out = Vec::new();
    for (channel, kind) in [(Channel::Brake, EventKind::Brake), (Channel::Left, EventKind::Left), (Channel::Right, EventKind::Right)] {
        for (start, end, holds, held) in merge(track.holds(channel)) {
            let steps = track.steps(channel);
            let peak = steps.iter().filter(|s| s.time_ms >= start && s.time_ms < end).map(|s| s.value).fold(0.0f32, f32::max);
            let speed = |t: i32| speed_at(samples, t as f32).unwrap_or(0.0);
            // The slowest point: samples inside the span, and its ends
            let (min_at_ms, min_kmh) = samples
                .iter()
                .filter(|s| s.time_ms > start && s.time_ms < end)
                .map(|s| (s.time_ms, s.speed_kmh))
                .chain([(start, speed(start)), (end, speed(end))])
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .expect("the ends are always there");
            let brake_ms: i32 = track
                .holds(Channel::Brake)
                .iter()
                .map(|h| (h.end_ms.min(end) - h.start_ms.max(start)).max(0))
                .sum();
            out.push(Event {
                kind,
                start_ms: start,
                end_ms: end,
                holds,
                held_ms: held,
                peak,
                entry_kmh: speed(start),
                min_kmh,
                min_at_ms,
                exit_kmh: speed(end),
                brake_ticks: brake_ms / TICK_MS,
            });
        }
    }
    out.sort_by_key(|e| (e.start_ms, e.kind as u8));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Input, InputValue};

    fn button(time_ms: i32, name: &str, pressed: bool) -> Input {
        Input { time_ms, name: name.into(), value: InputValue::Pressed(pressed) }
    }

    /// Speed falls from 200 km/h to 100 at 1000 ms, then rises back to 200 at 2000 ms
    fn samples() -> Vec<Sample> {
        (0..=20).map(|i| Sample { time_ms: i * 100, speed_kmh: 100.0 + (i as f32 - 10.0).abs() * 10.0, ..Default::default() }).collect()
    }

    #[test]
    fn taps_close_together_make_one_turn() {
        let inputs = [
            button(400, "SteerRight", true),
            button(500, "SteerRight", false),
            button(600, "SteerRight", true),
            button(800, "SteerRight", false),
            button(1500, "SteerRight", true),
            button(1600, "SteerRight", false),
        ];
        let track = InputTrack::new(&inputs, 2000).unwrap();
        let found = events(&track, &samples());
        assert_eq!(found.len(), 2);
        let first = &found[0];
        assert_eq!((first.kind, first.start_ms, first.end_ms, first.holds, first.held_ms), (EventKind::Right, 400, 800, 2, 300));
        assert_eq!(first.pattern(), "2 taps, 75% held");
        assert_eq!(found[1].pattern(), "hold");
    }

    #[test]
    fn braking_zones_report_speeds_and_ticks() {
        let inputs = [button(600, "Brake", true), button(1200, "Brake", false), button(700, "SteerLeft", true), button(1100, "SteerLeft", false)];
        let track = InputTrack::new(&inputs, 2000).unwrap();
        let found = events(&track, &samples());
        let brake = found.iter().find(|e| e.kind == EventKind::Brake).unwrap();
        assert_eq!((brake.entry_kmh, brake.min_kmh, brake.min_at_ms, brake.exit_kmh), (140.0, 100.0, 1000, 120.0));
        assert_eq!(brake.brake_ticks, 60);
        let turn = found.iter().find(|e| e.kind == EventKind::Left).unwrap();
        assert_eq!(turn.brake_ticks, 40);
        // Brake starts first, so it is listed first
        assert_eq!(found[0].kind, EventKind::Brake);
    }

    #[test]
    fn analog_steering_reports_its_peak() {
        let inputs = [Input { time_ms: 100, name: "Steer".into(), value: InputValue::Analog(-32768) }, Input { time_ms: 300, name: "Steer".into(), value: InputValue::Analog(0) }];
        let track = InputTrack::new(&inputs, 2000).unwrap();
        let found = events(&track, &samples());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].pattern(), "analog, peak 50%");
    }
}
