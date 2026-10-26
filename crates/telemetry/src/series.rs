// ABOUTME: Plot series built from runs: step lines for tick-exact inputs (flat until the next change) and the gap
// ABOUTME: of one run to another sampled along the focused run. Points are [seconds, value].

use tmreplay::gap::live_gap_ms;
use tmreplay::track::Step;
use tmreplay::Sample;

/// A step function as line points: each value holds flat until the next step, then jumps; ends at end_ms
pub fn step_points(steps: &[Step], end_ms: i32, scale: f64) -> Vec<[f64; 2]> {
    let mut points = vec![[0.0, 0.0]];
    let mut value = 0.0;
    for step in steps {
        let t = step.time_ms as f64 / 1000.0;
        points.push([t, value]);
        value = step.value as f64 * scale;
        points.push([t, value]);
    }
    points.push([end_ms.max(0) as f64 / 1000.0, value]);
    points
}

/// The other run's gap to the focused run in seconds at each focused sample: positive when the other car is behind.
/// Split into continuous pieces where the cars never meet, since a plot line can't hold gaps.
pub fn gap_series(focus: &[Sample], other: &[Sample]) -> Vec<Vec<[f64; 2]>> {
    let mut pieces: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut current = Vec::new();
    for s in focus {
        let t = s.time_ms as f32;
        match live_gap_ms(focus, t, other) {
            Some(g) => current.push([t as f64 / 1000.0, g as f64 / 1000.0]),
            None if !current.is_empty() => pieces.push(std::mem::take(&mut current)),
            None => {}
        }
    }
    if !current.is_empty() {
        pieces.push(current);
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;
    use tmreplay::Vec3;

    #[test]
    fn steps_hold_flat_until_the_next_change() {
        let steps = [Step { time_ms: 100, value: 1.0 }, Step { time_ms: 250, value: 0.0 }];
        assert_eq!(step_points(&steps, 1000, 1.0), vec![[0.0, 0.0], [0.1, 0.0], [0.1, 1.0], [0.25, 1.0], [0.25, 0.0], [1.0, 0.0]]);
        assert_eq!(step_points(&[], 500, -1.0), vec![[0.0, 0.0], [0.5, 0.0]]);
    }

    #[test]
    fn gaps_split_where_the_cars_never_meet() {
        let along = |z: f32, from: i32, to: i32| -> Vec<Sample> {
            (from..=to).map(|i| Sample { time_ms: i * 100, position: Vec3 { x: i as f32 * 100.0, y: 0.0, z }, ..Default::default() }).collect()
        };
        // The other car leaves the line for samples 4 to 6, far from where the focused car is then
        let mut other = along(0.0, 0, 3);
        other.extend(along(500.0, 4, 6));
        other.extend(along(0.0, 7, 10));
        let pieces = gap_series(&along(0.0, 0, 10), &other);
        assert_eq!(pieces.len(), 2);
        assert!(pieces.iter().flatten().all(|p| p[1].is_finite()));
    }

    #[test]
    fn gaps_are_in_seconds_along_the_focused_run() {
        let run = |speed: f32, ms: i32| -> Vec<Sample> {
            (0..=ms / 100).map(|i| Sample { time_ms: i * 100, position: Vec3 { x: speed * i as f32 / 10.0, y: 0.0, z: 0.0 }, ..Default::default() }).collect()
        };
        let series = gap_series(&run(10.0, 1000), &run(5.0, 2000));
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].len(), 11);
        assert!((series[0][10][1] - 1.0).abs() < 1e-4);
    }
}
