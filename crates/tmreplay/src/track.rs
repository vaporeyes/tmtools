// ABOUTME: A run's recorded inputs as step functions per channel (left, right, gas, brake), exact to the 10 ms tick,
// ABOUTME: the holds from press to release, and summary statistics. Port of the replay viewer's inputTrack.ts.

use crate::model::{Input, InputValue};

/// The physics tick, in milliseconds; inputs change only on tick boundaries
pub const TICK_MS: i32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    Left,
    Right,
    Gas,
    Brake,
}

impl Channel {
    pub const ALL: [Channel; 4] = [Channel::Left, Channel::Right, Channel::Gas, Channel::Brake];

    pub fn label(self) -> &'static str {
        match self {
            Channel::Left => "Left",
            Channel::Right => "Right",
            Channel::Gas => "Gas",
            Channel::Brake => "Brake",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// The channel's value from time_ms until the next step: 0 released, 1 fully held
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    pub time_ms: i32,
    pub value: f32,
}

/// A stretch where the channel is above zero; one still held at the end of the run ends there
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hold {
    pub start_ms: i32,
    pub end_ms: i32,
}

impl Hold {
    pub fn duration_ms(&self) -> i32 {
        self.end_ms - self.start_ms
    }

    pub fn ticks(&self) -> i32 {
        self.duration_ms() / TICK_MS
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InputTrack {
    steps: [Vec<Step>; 4],
    holds: [Vec<Hold>; 4],
    /// Raw analog steering, -65536 (full left) to 65536 (full right); None when steering was digital
    pub analog_steer: Option<Vec<(i32, i32)>>,
    pub end_ms: i32,
    /// Respawn presses, by time
    pub respawns: Vec<i32>,
}

// Full travel of an analog input
const FULL: f32 = 65536.0;

/// Inputs at the same tick replace each other; only changes become steps
fn set<T: Copy + PartialEq>(list: &mut Vec<(i32, T)>, time_ms: i32, value: T, zero: T) {
    if list.last().is_some_and(|&(t, _)| t == time_ms) {
        list.pop();
    }
    if list.last().map_or(zero, |&(_, v)| v) != value {
        list.push((time_ms, value));
    }
}

fn fraction(value: i32) -> f32 {
    (value as f32 / FULL).clamp(0.0, 1.0)
}

impl InputTrack {
    /// The run's inputs as channels, or None when it has no steering or pedal inputs
    pub fn new(inputs: &[Input], end_ms: i32) -> Option<Self> {
        let mut steps: [Vec<(i32, f32)>; 4] = Default::default();
        let mut analog: Option<Vec<(i32, i32)>> = None;
        let mut respawns = Vec::new();

        for input in inputs {
            let t = input.time_ms;
            match (input.name.as_str(), input.value) {
                ("Respawn", InputValue::Pressed(true)) => respawns.push(t),
                (name, InputValue::Pressed(pressed)) => {
                    let channel = match name {
                        "SteerLeft" => Channel::Left,
                        "SteerRight" => Channel::Right,
                        "Accelerate" => Channel::Gas,
                        "Brake" => Channel::Brake,
                        _ => continue,
                    };
                    set(&mut steps[channel.index()], t, if pressed { 1.0 } else { 0.0 }, 0.0);
                }
                ("Steer", InputValue::Analog(v)) => {
                    set(analog.get_or_insert_with(Vec::new), t, v, 0);
                    set(&mut steps[Channel::Left.index()], t, fraction(-v), 0.0);
                    set(&mut steps[Channel::Right.index()], t, fraction(v), 0.0);
                }
                // One axis for both pedals: positive accelerates, negative brakes
                ("Gas", InputValue::Analog(v)) => {
                    set(&mut steps[Channel::Gas.index()], t, fraction(v), 0.0);
                    set(&mut steps[Channel::Brake.index()], t, fraction(-v), 0.0);
                }
                ("AccelerateReal", InputValue::Analog(v)) => set(&mut steps[Channel::Gas.index()], t, fraction(v), 0.0),
                ("BrakeReal", InputValue::Analog(v)) => set(&mut steps[Channel::Brake.index()], t, fraction(v), 0.0),
                _ => {}
            }
        }

        if steps.iter().all(Vec::is_empty) {
            return None;
        }
        let steps = steps.map(|list| list.into_iter().map(|(time_ms, value)| Step { time_ms, value }).collect::<Vec<_>>());
        let holds = std::array::from_fn(|i| holds_of(&steps[i], end_ms));
        Some(Self { steps, holds, analog_steer: analog, end_ms, respawns })
    }

    pub fn steps(&self, channel: Channel) -> &[Step] {
        &self.steps[channel.index()]
    }

    pub fn holds(&self, channel: Channel) -> &[Hold] {
        &self.holds[channel.index()]
    }

    /// The value at time_ms: the latest step at or before it, 0 before the first
    pub fn value_at(&self, channel: Channel, time_ms: i32) -> f32 {
        let steps = self.steps(channel);
        let i = steps.partition_point(|s| s.time_ms <= time_ms);
        if i == 0 {
            0.0
        } else {
            steps[i - 1].value
        }
    }

    /// Raw analog steering at time_ms, when steering was analog
    pub fn analog_steer_at(&self, time_ms: i32) -> Option<i32> {
        let list = self.analog_steer.as_ref()?;
        let i = list.partition_point(|&(t, _)| t <= time_ms);
        Some(if i == 0 { 0 } else { list[i - 1].1 })
    }

    /// Steering at time_ms, -1 (full left) to 1 (full right)
    pub fn steer_at(&self, time_ms: i32) -> f32 {
        self.value_at(Channel::Right, time_ms) - self.value_at(Channel::Left, time_ms)
    }

    /// Whether the channel is held at time_ms, and since when it has been held or released (0 before the first press)
    pub fn stretch_at(&self, channel: Channel, time_ms: i32) -> Stretch {
        stretch_at(self.holds(channel), time_ms)
    }

    /// Whether any hold of the channel overlaps from_ms to to_ms (end excluded)
    pub fn held_during(&self, channel: Channel, from_ms: i32, to_ms: i32) -> bool {
        let holds = self.holds(channel);
        let i = holds.partition_point(|h| h.end_ms <= from_ms);
        holds.get(i).is_some_and(|h| h.start_ms < to_ms)
    }

    pub fn stats(&self, channel: Channel) -> HoldStats {
        HoldStats::of(self.holds(channel))
    }

    /// How many times the steering changed per second of the run, a measure of how busy the hands were
    pub fn steer_changes_per_second(&self) -> f32 {
        let changes = self.steps(Channel::Left).len() + self.steps(Channel::Right).len();
        if self.end_ms <= 0 {
            0.0
        } else {
            changes as f32 / (self.end_ms as f32 / 1000.0)
        }
    }
}

fn holds_of(steps: &[Step], end_ms: i32) -> Vec<Hold> {
    let mut holds = Vec::new();
    let mut start = None;
    for step in steps {
        match (step.value > 0.0, start) {
            (true, None) => start = Some(step.time_ms),
            (false, Some(s)) => {
                holds.push(Hold { start_ms: s, end_ms: step.time_ms });
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        holds.push(Hold { start_ms: s, end_ms: end_ms.max(s) });
    }
    holds
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stretch {
    pub held: bool,
    pub since_ms: i32,
}

pub fn stretch_at(holds: &[Hold], time_ms: i32) -> Stretch {
    let mut since_ms = 0;
    for hold in holds {
        if hold.start_ms > time_ms {
            break;
        }
        if time_ms < hold.end_ms {
            return Stretch { held: true, since_ms: hold.start_ms };
        }
        since_ms = hold.end_ms;
    }
    Stretch { held: false, since_ms }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HoldStats {
    pub count: usize,
    pub total_ms: i32,
    pub shortest_ms: Option<i32>,
    pub longest_ms: Option<i32>,
    pub mean_ms: Option<f32>,
}

impl HoldStats {
    pub fn of(holds: &[Hold]) -> Self {
        let durations = holds.iter().map(Hold::duration_ms);
        let total_ms = durations.clone().sum();
        Self {
            count: holds.len(),
            total_ms,
            shortest_ms: durations.clone().min(),
            longest_ms: durations.max(),
            mean_ms: (!holds.is_empty()).then(|| total_ms as f32 / holds.len() as f32),
        }
    }
}

/// A playback time in milliseconds as the 10 ms tick it falls in; inputs take effect from their tick
pub fn tick_of(time_ms: f64) -> i32 {
    ((time_ms + 1e-6) / TICK_MS as f64).floor() as i32 * TICK_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn button(time_ms: i32, name: &str, pressed: bool) -> Input {
        Input { time_ms, name: name.into(), value: InputValue::Pressed(pressed) }
    }

    fn analog(time_ms: i32, name: &str, value: i32) -> Input {
        Input { time_ms, name: name.into(), value: InputValue::Analog(value) }
    }

    #[test]
    fn is_none_without_steering_or_pedals() {
        assert!(InputTrack::new(&[button(0, "FakeIsRaceRunning", true)], 1000).is_none());
    }

    #[test]
    fn turns_digital_presses_into_steps_and_holds() {
        let track = InputTrack::new(
            &[button(0, "FakeIsRaceRunning", true), button(10, "Accelerate", true), button(970, "SteerRight", true), button(1090, "SteerRight", false)],
            2000,
        )
        .unwrap();
        assert!(track.analog_steer.is_none());
        assert_eq!(track.steps(Channel::Right), [Step { time_ms: 970, value: 1.0 }, Step { time_ms: 1090, value: 0.0 }]);
        assert_eq!(track.holds(Channel::Right), [Hold { start_ms: 970, end_ms: 1090 }]);
        // Gas is never released, so its hold lasts to the end of the run
        assert_eq!(track.holds(Channel::Gas), [Hold { start_ms: 10, end_ms: 2000 }]);
        assert!(track.holds(Channel::Left).is_empty());
        assert_eq!(track.holds(Channel::Right)[0].ticks(), 12);
    }

    #[test]
    fn splits_analog_steering_and_keeps_the_raw_value() {
        let track = InputTrack::new(&[analog(10, "Steer", 65536), analog(20, "Steer", 32768), analog(30, "Steer", -16384), analog(40, "Steer", 0)], 100).unwrap();
        assert_eq!(track.steps(Channel::Right), [Step { time_ms: 10, value: 1.0 }, Step { time_ms: 20, value: 0.5 }, Step { time_ms: 30, value: 0.0 }]);
        assert_eq!(track.steps(Channel::Left), [Step { time_ms: 30, value: 0.25 }, Step { time_ms: 40, value: 0.0 }]);
        assert_eq!(track.holds(Channel::Right), [Hold { start_ms: 10, end_ms: 30 }]);
        assert_eq!(track.analog_steer_at(25), Some(32768));
        assert_eq!(track.steer_at(35), -0.25);
    }

    #[test]
    fn keeps_the_last_input_of_a_tick() {
        let track = InputTrack::new(&[button(10, "Brake", true), button(10, "Brake", false), button(50, "Brake", true)], 100).unwrap();
        assert_eq!(track.steps(Channel::Brake), [Step { time_ms: 50, value: 1.0 }]);
    }

    #[test]
    fn reads_the_gas_axis_as_gas_or_brake() {
        let track = InputTrack::new(&[analog(10, "Gas", 65536), analog(20, "Gas", -32768)], 100).unwrap();
        assert_eq!(track.value_at(Channel::Gas, 10), 1.0);
        assert_eq!(track.value_at(Channel::Gas, 20), 0.0);
        assert_eq!(track.value_at(Channel::Brake, 20), 0.5);
    }

    #[test]
    fn values_hold_from_their_own_tick() {
        let track = InputTrack::new(&[button(100, "Brake", true), button(200, "Brake", false)], 1000).unwrap();
        assert_eq!(track.value_at(Channel::Brake, 90), 0.0);
        assert_eq!(track.value_at(Channel::Brake, 100), 1.0);
        assert_eq!(track.value_at(Channel::Brake, 190), 1.0);
        assert_eq!(track.value_at(Channel::Brake, 200), 0.0);
    }

    #[test]
    fn tells_how_long_a_channel_has_been_held_or_released() {
        let holds = [Hold { start_ms: 100, end_ms: 200 }, Hold { start_ms: 500, end_ms: 900 }];
        assert_eq!(stretch_at(&holds, 50), Stretch { held: false, since_ms: 0 });
        assert_eq!(stretch_at(&holds, 150), Stretch { held: true, since_ms: 100 });
        assert_eq!(stretch_at(&holds, 200), Stretch { held: false, since_ms: 200 });
        assert_eq!(stretch_at(&holds, 600), Stretch { held: true, since_ms: 500 });
        assert_eq!(stretch_at(&holds, 950), Stretch { held: false, since_ms: 900 });
    }

    #[test]
    fn finds_holds_overlapping_a_span() {
        let track = InputTrack::new(&[button(820, "Brake", true), button(840, "Brake", false)], 1000).unwrap();
        assert!(track.held_during(Channel::Brake, 800, 900));
        assert!(!track.held_during(Channel::Brake, 840, 900));
        assert!(!track.held_during(Channel::Brake, 700, 820));
    }

    #[test]
    fn summarizes_holds() {
        let stats = HoldStats::of(&[Hold { start_ms: 0, end_ms: 30 }, Hold { start_ms: 100, end_ms: 200 }]);
        assert_eq!((stats.count, stats.total_ms, stats.shortest_ms, stats.longest_ms, stats.mean_ms), (2, 130, Some(30), Some(100), Some(65.0)));
        assert_eq!(HoldStats::of(&[]).mean_ms, None);
    }

    #[test]
    fn snaps_times_to_ticks_despite_float_error() {
        assert_eq!(tick_of(970.0), 970);
        assert_eq!(tick_of(974.9), 970);
        assert_eq!(tick_of((0.1 + 0.2) * 1000.0), 300);
    }
}
