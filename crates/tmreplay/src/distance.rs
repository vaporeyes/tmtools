// ABOUTME: Distance a car has driven along its path, in world units (meters in TrackMania), up to any moment of the run.
// ABOUTME: Respawns teleport the car, so steps much longer than its speed allows are left out; also distance back to time.

use crate::model::Sample;

/// Slack on top of the distance the car's speed allows between two samples before a step counts as a teleport
const TELEPORT_SLACK_M: f32 = 3.0;

fn step_length(a: &Sample, b: &Sample) -> f32 {
    let (dx, dy, dz) = (b.position.x - a.position.x, b.position.y - a.position.y, b.position.z - a.position.z);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// Whether the step from a to b is driving rather than a respawn teleport: no longer than the faster of the two
/// speeds allows over the time between them, with some slack
fn is_driven(a: &Sample, b: &Sample) -> bool {
    let seconds = (b.time_ms - a.time_ms).max(0) as f32 / 1000.0;
    let reach = a.speed_kmh.max(b.speed_kmh) / 3.6 * seconds * 1.5 + TELEPORT_SLACK_M;
    step_length(a, b) <= reach
}

/// Distance driven from the start to time_ms, in meters; the step under way at time_ms counts in proportion
pub fn distance_at(samples: &[Sample], time_ms: f32) -> f32 {
    let mut total = 0.0;
    for pair in samples.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let (t0, t1) = (a.time_ms as f32, b.time_ms as f32);
        if t0 >= time_ms {
            break;
        }
        if !is_driven(a, b) {
            continue;
        }
        let share = if t1 <= time_ms || t1 <= t0 { 1.0 } else { (time_ms - t0) / (t1 - t0) };
        total += step_length(a, b) * share;
    }
    total
}

/// Distance driven over the whole run, in meters
pub fn total_distance(samples: &[Sample]) -> f32 {
    samples.windows(2).filter(|p| is_driven(&p[0], &p[1])).map(|p| step_length(&p[0], &p[1])).sum()
}

/// Distance driven at each sample, in meters: the running total distance_at gives at the sample times
pub fn cumulative_distance(samples: &[Sample]) -> Vec<f32> {
    let mut totals = Vec::with_capacity(samples.len());
    let mut total = 0.0;
    for (i, s) in samples.iter().enumerate() {
        if i > 0 && is_driven(&samples[i - 1], s) {
            total += step_length(&samples[i - 1], s);
        }
        totals.push(total);
    }
    totals
}

/// Distance driven at time_ms from the running totals of cumulative_distance, in meters; a quick lookup for drawing
pub fn distance_at_time(samples: &[Sample], totals: &[f32], time_ms: f32) -> f32 {
    let Some(last) = samples.len().checked_sub(1) else { return 0.0 };
    let next = samples.partition_point(|s| (s.time_ms as f32) <= time_ms);
    if next == 0 {
        return 0.0;
    }
    if next > last {
        return totals[last];
    }
    let (t0, t1) = (samples[next - 1].time_ms as f32, samples[next].time_ms as f32);
    let share = if t1 > t0 { (time_ms - t0) / (t1 - t0) } else { 1.0 };
    totals[next - 1] + (totals[next] - totals[next - 1]) * share
}

/// When the car had driven this far, in milliseconds: the first moment it got there, so a respawn that stands still
/// doesn't move it. Clamped to the run.
pub fn time_at_distance(samples: &[Sample], totals: &[f32], meters: f32) -> f32 {
    let Some(last) = samples.len().checked_sub(1) else { return 0.0 };
    if meters <= 0.0 {
        return samples[0].time_ms as f32;
    }
    if meters >= totals[last] {
        return samples[last].time_ms as f32;
    }
    // The first sample at or past the distance
    let at = totals.partition_point(|&d| d < meters);
    let (d0, d1) = (totals[at - 1], totals[at]);
    let (t0, t1) = (samples[at - 1].time_ms as f32, samples[at].time_ms as f32);
    if d1 > d0 {
        t0 + (meters - d0) / (d1 - d0) * (t1 - t0)
    } else {
        t1
    }
}

/// "850 m" below a kilometer, "1.24 km" from there
pub fn format_distance(meters: f32) -> String {
    if meters < 1000.0 {
        format!("{:.0} m", meters)
    } else {
        format!("{:.2} km", meters / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gbx::Vec3;

    /// A car along x at 36 km/h (10 m/s), sampled every 100 ms
    fn run(duration_ms: i32) -> Vec<Sample> {
        (0..=duration_ms / 100)
            .map(|i| Sample { time_ms: i * 100, position: Vec3 { x: i as f32, y: 0.0, z: 0.0 }, speed_kmh: 36.0, ..Default::default() })
            .collect()
    }

    #[test]
    fn sums_the_path_and_counts_the_current_step_in_part() {
        let samples = run(2000);
        assert!((total_distance(&samples) - 20.0).abs() < 1e-4);
        assert!((distance_at(&samples, 1000.0) - 10.0).abs() < 1e-4);
        assert!((distance_at(&samples, 1050.0) - 10.5).abs() < 1e-4);
        assert_eq!(distance_at(&samples, 0.0), 0.0);
        assert!((distance_at(&samples, 9000.0) - 20.0).abs() < 1e-4);
    }

    #[test]
    fn running_totals_match_distance_at_and_invert() {
        let samples = run(2000);
        let totals = cumulative_distance(&samples);
        assert_eq!(totals.len(), samples.len());
        for t in [0.0, 50.0, 1000.0, 1050.0, 1999.0, 5000.0] {
            assert!((distance_at_time(&samples, &totals, t) - distance_at(&samples, t)).abs() < 1e-4, "{t}");
        }
        assert!((time_at_distance(&samples, &totals, 10.5) - 1050.0).abs() < 1e-3);
        assert_eq!(time_at_distance(&samples, &totals, -1.0), 0.0);
        assert_eq!(time_at_distance(&samples, &totals, 99.0), 2000.0);
    }

    #[test]
    fn leaves_out_respawn_teleports() {
        let mut samples = run(1000);
        // A respawn moves the car 500 m back in one sample, then it drives on from there
        for s in samples.iter_mut().skip(6) {
            s.position.x -= 500.0;
        }
        assert!((total_distance(&samples) - 9.0).abs() < 1e-3);
    }

    #[test]
    fn formats_meters_and_kilometers() {
        assert_eq!(format_distance(849.6), "850 m");
        assert_eq!(format_distance(1236.0), "1.24 km");
    }
}
