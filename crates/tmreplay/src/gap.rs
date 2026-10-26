// ABOUTME: Positions along a run between samples, and the live gap between two runs: when the other car passed the
// ABOUTME: spot where the focused car is. Port of the replay viewer's compare.ts.

use crate::model::Sample;
use gbx::{Quat, Vec3};

// The other car must pass within this many world units of the spot to count as passing it
const MAX_MATCH_DISTANCE: f32 = 24.0;
// Look for the pass near the same race time first, so loops and crossings match the right lap
const SEARCH_WINDOW_MS: f32 = 15_000.0;

fn lerp(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    Vec3 { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t, z: a.z + (b.z - a.z) * t }
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3 { x: a.x - b.x, y: a.y - b.y, z: a.z - b.z }
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

/// The samples around time_ms and how far between them it is; held at the ends
fn bracket(samples: &[Sample], time_ms: f32) -> Option<(usize, usize, f32)> {
    let last = samples.len().checked_sub(1)?;
    let i = samples.partition_point(|s| (s.time_ms as f32) <= time_ms);
    if i == 0 {
        return Some((0, 0, 0.0));
    }
    if i > last {
        return Some((last, last, 0.0));
    }
    let (a, b) = (&samples[i - 1], &samples[i]);
    let span = (b.time_ms - a.time_ms) as f32;
    Some((i - 1, i, if span > 0.0 { (time_ms - a.time_ms as f32) / span } else { 0.0 }))
}

/// The car's position at time_ms, between samples
pub fn position_at(samples: &[Sample], time_ms: f32) -> Option<Vec3> {
    let (a, b, t) = bracket(samples, time_ms)?;
    Some(lerp(samples[a].position, samples[b].position, t))
}

/// The car's rotation at time_ms, normalized linear blend between samples (they are 100 ms apart, so close)
pub fn rotation_at(samples: &[Sample], time_ms: f32) -> Option<Quat> {
    let (a, b, t) = bracket(samples, time_ms)?;
    let (qa, mut qb) = (samples[a].rotation, samples[b].rotation);
    // Take the short way round
    if qa.x * qb.x + qa.y * qb.y + qa.z * qb.z + qa.w * qb.w < 0.0 {
        qb = Quat { x: -qb.x, y: -qb.y, z: -qb.z, w: -qb.w };
    }
    let q = Quat { x: qa.x + (qb.x - qa.x) * t, y: qa.y + (qb.y - qa.y) * t, z: qa.z + (qb.z - qa.z) * t, w: qa.w + (qb.w - qa.w) * t };
    let len = (q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w).sqrt().max(1e-9);
    Some(Quat { x: q.x / len, y: q.y / len, z: q.z / len, w: q.w / len })
}

/// Speed in km/h at time_ms, between samples
pub fn speed_at(samples: &[Sample], time_ms: f32) -> Option<f32> {
    let (a, b, t) = bracket(samples, time_ms)?;
    Some(samples[a].speed_kmh + (samples[b].speed_kmh - samples[a].speed_kmh) * t)
}

/// The latest sample at or before time_ms
pub fn sample_at(samples: &[Sample], time_ms: f32) -> Option<&Sample> {
    let i = samples.partition_point(|s| (s.time_ms as f32) <= time_ms);
    samples.get(i.saturating_sub(1))
}

/// When the other car passed the spot where the focused car is at focus_time_ms, relative to the focused car:
/// negative when it got there earlier (ahead), positive when later (behind). None when it never came close.
/// After the focused car finishes, its finish spot is used, so the gap settles on the finish difference.
pub fn live_gap_ms(focus: &[Sample], focus_time_ms: f32, other: &[Sample]) -> Option<f32> {
    if focus.is_empty() || other.len() < 2 {
        return None;
    }
    let t = focus_time_ms.min(focus.last()?.time_ms as f32);
    let spot = position_at(focus, t)?;

    let near = closest_pass(other, spot, t - SEARCH_WINDOW_MS, t + SEARCH_WINDOW_MS);
    let best = match near {
        Some(n) if n.1 <= MAX_MATCH_DISTANCE => Some(n),
        _ => closest_pass(other, spot, f32::NEG_INFINITY, f32::INFINITY),
    };
    best.filter(|b| b.1 <= MAX_MATCH_DISTANCE).map(|b| b.0 - t)
}

/// How the other run made the same press at the same place: when the focused car pressed at press_ms, the other
/// car reached that spot at some time; the other run's press nearest that time, within window_ms, is its match
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatchedPress {
    /// When the other car reached the spot where the focused car pressed
    pub other_at_spot_ms: f32,
    /// The other run's press
    pub other_press_ms: i32,
    /// Negative when the other run pressed before reaching the spot (earlier on the track), positive when after
    pub offset_ms: f32,
}

pub fn matching_press(focus: &[Sample], press_ms: i32, other: &[Sample], other_presses: impl IntoIterator<Item = i32>, window_ms: f32) -> Option<MatchedPress> {
    let gap = live_gap_ms(focus, press_ms as f32, other)?;
    let at_spot = press_ms as f32 + gap;
    other_presses
        .into_iter()
        .map(|p| (p, p as f32 - at_spot))
        .filter(|(_, offset)| offset.abs() <= window_ms)
        .min_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        .map(|(other_press_ms, offset_ms)| MatchedPress { other_at_spot_ms: at_spot, other_press_ms, offset_ms })
}

/// The point on the path nearest the spot, as (time, distance), searching segments that start in [from, to]
fn closest_pass(samples: &[Sample], spot: Vec3, from: f32, to: f32) -> Option<(f32, f32)> {
    let mut best: Option<(f32, f32)> = None;
    for pair in samples.windows(2) {
        let (t0, t1) = (pair[0].time_ms as f32, pair[1].time_ms as f32);
        if t1 < from || t0 > to {
            continue;
        }
        let (a, b) = (pair[0].position, pair[1].position);
        let ab = sub(b, a);
        let len2 = dot(ab, ab);
        let along = if len2 > 0.0 { (dot(sub(spot, a), ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let p = lerp(a, b, along);
        let d = sub(p, spot);
        let distance = dot(d, d).sqrt();
        if best.is_none_or(|(_, bd)| distance < bd) {
            best = Some((t0 + along * (t1 - t0), distance));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A car along x at a constant speed (units per second), sampled every 100 ms
    fn run(speed: f32, duration_ms: i32) -> Vec<Sample> {
        (0..=duration_ms / 100)
            .map(|i| {
                let t = i * 100;
                Sample { time_ms: t, position: Vec3 { x: speed * t as f32 / 1000.0, y: 0.0, z: 0.0 }, speed_kmh: speed * 3.6, ..Default::default() }
            })
            .collect()
    }

    #[test]
    fn interpolates_between_samples_and_holds_at_the_ends() {
        let samples = run(10.0, 1000);
        assert!((position_at(&samples, 150.0).unwrap().x - 1.5).abs() < 1e-5);
        assert_eq!(position_at(&samples, 5000.0).unwrap().x, 10.0);
        assert_eq!(position_at(&samples, -10.0).unwrap().x, 0.0);
        assert_eq!(sample_at(&samples, 150.0).unwrap().time_ms, 100);
    }

    #[test]
    fn the_gap_is_positive_when_the_other_car_is_behind() {
        let focus = run(10.0, 2000);
        let slower = run(5.0, 4000);
        assert!((live_gap_ms(&focus, 1000.0, &slower).unwrap() - 1000.0).abs() < 1e-2);
        assert!((live_gap_ms(&slower, 2000.0, &focus).unwrap() + 1000.0).abs() < 1e-2);
    }

    #[test]
    fn matches_presses_at_the_same_place_on_the_track() {
        // The slower car reaches x = 10 (where the focused car pressed at 1000 ms) at 2000 ms
        let focus = run(10.0, 2000);
        let slower = run(5.0, 4000);
        let matched = matching_press(&focus, 1000, &slower, [500, 1960, 3000], 300.0).unwrap();
        assert!((matched.other_at_spot_ms - 2000.0).abs() < 1e-2);
        assert_eq!(matched.other_press_ms, 1960);
        assert!((matched.offset_ms + 40.0).abs() < 1e-2);
        assert_eq!(matching_press(&focus, 1000, &slower, [2500], 300.0), None);
    }

    #[test]
    fn there_is_no_gap_when_the_cars_never_meet() {
        let focus = run(10.0, 2000);
        let mut far = run(10.0, 2000);
        for s in &mut far {
            s.position.z = 500.0;
        }
        assert_eq!(live_gap_ms(&focus, 1000.0, &far), None);
    }
}
