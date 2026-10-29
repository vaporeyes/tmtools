// ABOUTME: Sector splits: boundaries from checkpoints and placed points along the focused run, each run's time at every
// ABOUTME: boundary, sector durations, the best of each sector, the theoretical best, and F1-style sector colors.

use crate::gap::{pass_time_after, position_at};
use crate::model::Ghost;
use gbx::Vec3;

/// Where a sector ends
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Boundary {
    /// The run's checkpoint with this index; the last one is the finish
    Checkpoint(usize),
    /// A point placed on the track
    Point(Vec3),
}

/// Boundaries in the order the focused run reaches them; placed points it never passes are left out
pub fn boundaries(focus: &Ghost, points: &[Vec3]) -> Vec<Boundary> {
    let mut timed: Vec<(f32, Boundary)> = focus
        .checkpoints
        .iter()
        .enumerate()
        .filter_map(|(i, cp)| cp.time_ms.map(|t| (t as f32, Boundary::Checkpoint(i))))
        .collect();
    timed.extend(points.iter().filter_map(|&p| pass_time_after(&focus.samples, p, 0.0).map(|t| (t, Boundary::Point(p)))));
    timed.sort_by(|a, b| a.0.total_cmp(&b.0));
    timed.into_iter().map(|(_, b)| b).collect()
}

/// When a run reaches each boundary; None where it has no such checkpoint or never passes the point. Points are
/// matched in order, each after the previous boundary, so a crossing track isn't matched to the wrong pass.
pub fn boundary_times(ghost: &Ghost, boundaries: &[Boundary]) -> Vec<Option<f32>> {
    let mut previous = 0.0f32;
    boundaries
        .iter()
        .map(|b| {
            let time = match b {
                Boundary::Checkpoint(i) => ghost.checkpoints.get(*i).and_then(|cp| cp.time_ms).map(|t| t as f32),
                Boundary::Point(p) => pass_time_after(&ghost.samples, *p, previous),
            };
            if let Some(t) = time {
                previous = t;
            }
            time
        })
        .collect()
}

/// How long each sector took: from the start (or the previous boundary) to each boundary
pub fn sector_durations(times: &[Option<f32>]) -> Vec<Option<f32>> {
    let mut previous = Some(0.0f32);
    times
        .iter()
        .map(|&t| {
            let duration = previous.zip(t).map(|(a, b)| b - a);
            previous = t;
            duration
        })
        .collect()
}

/// The fastest time of each sector across runs, and their sum when every sector has one
pub fn best_sectors(runs: &[Vec<Option<f32>>]) -> (Vec<Option<f32>>, Option<f32>) {
    let count = runs.iter().map(Vec::len).max().unwrap_or(0);
    let best: Vec<Option<f32>> = (0..count).map(|i| runs.iter().filter_map(|r| r.get(i).copied().flatten()).min_by(f32::total_cmp)).collect();
    let total = best.iter().copied().sum::<Option<f32>>();
    (best, total.filter(|_| count > 0))
}

/// Sector times within this of the fastest, in milliseconds, count as the fastest: a tie to the hundredth shown
pub const FASTEST_SLACK_MS: f32 = 5.0;

/// F1's colors for one run per car: purple for the fastest in the sector, then against the focused run green when
/// faster and yellow when slower; the focused run's own sector is Own unless it is the fastest
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectorColor {
    Fastest,
    Faster,
    Slower,
    Own,
}

/// Each run's color in each sector, from each run's sector durations; focus is the focused run's row. Sectors without
/// a time, or without the focused run's time to compare to, have none.
pub fn sector_colors(runs: &[Vec<Option<f32>>], focus: usize) -> Vec<Vec<Option<SectorColor>>> {
    let (best, _) = best_sectors(runs);
    runs.iter()
        .enumerate()
        .map(|(row, sectors)| {
            sectors
                .iter()
                .enumerate()
                .map(|(i, time)| {
                    let (time, fastest) = (time.as_ref()?, best[i]?);
                    if time - fastest < FASTEST_SLACK_MS {
                        return Some(SectorColor::Fastest);
                    }
                    if row == focus {
                        return Some(SectorColor::Own);
                    }
                    let reference = runs.get(focus)?.get(i).copied().flatten()?;
                    Some(if *time < reference { SectorColor::Faster } else { SectorColor::Slower })
                })
                .collect()
        })
        .collect()
}

/// Where a boundary is on the track, for drawing: a checkpoint is wherever the focused run was at its time
pub fn boundary_position(focus: &Ghost, boundary: &Boundary) -> Option<Vec3> {
    match boundary {
        Boundary::Point(p) => Some(*p),
        Boundary::Checkpoint(i) => focus.checkpoints.get(*i)?.time_ms.and_then(|t| position_at(&focus.samples, t as f32)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Checkpoint, Sample};

    #[test]
    fn colors_purple_for_the_fastest_then_green_or_yellow_against_the_focus() {
        use SectorColor::*;
        let runs = vec![
            vec![Some(5000.0), Some(4000.0), Some(4000.0)], // focused
            vec![Some(4900.0), Some(4200.0), Some(3900.0)],
            vec![Some(5100.0), Some(3900.0), Some(4002.0)],
        ];
        assert_eq!(
            sector_colors(&runs, 0),
            vec![vec![Some(Own), Some(Own), Some(Own)], vec![Some(Fastest), Some(Slower), Some(Fastest)], vec![Some(Slower), Some(Fastest), Some(Slower)]]
        );
        // Focusing another run moves the reference, not the fastest
        assert_eq!(sector_colors(&runs, 1)[0], vec![Some(Slower), Some(Faster), Some(Slower)]);
        // Within a hundredth of the fastest counts as the fastest
        assert_eq!(sector_colors(&[vec![Some(4000.0)], vec![Some(4003.0)]], 0)[1], vec![Some(Fastest)]);
        assert_eq!(sector_colors(&[vec![None], vec![Some(4000.0)], vec![Some(4500.0)]], 0)[2], vec![None]);
    }

    /// A run along x at a constant speed (units per second) with checkpoints at the given times
    fn run(speed: f32, duration_ms: i32, checkpoints: &[i32]) -> Ghost {
        let samples = (0..=duration_ms / 100)
            .map(|i| Sample { time_ms: i * 100, position: Vec3 { x: speed * i as f32 / 10.0, y: 0.0, z: 0.0 }, ..Default::default() })
            .collect();
        let checkpoints = checkpoints.iter().map(|&t| Checkpoint { time_ms: Some(t), stunts_score: 0 }).collect();
        Ghost { samples, checkpoints, race_time_ms: Some(duration_ms), ..Default::default() }
    }

    #[test]
    fn orders_placed_points_among_checkpoints_by_the_focused_run() {
        let focus = run(10.0, 2000, &[1000, 2000]);
        let b = boundaries(&focus, &[Vec3 { x: 15.0, y: 0.0, z: 0.0 }, Vec3 { x: 5.0, y: 0.0, z: 0.0 }, Vec3 { x: 0.0, y: 0.0, z: 900.0 }]);
        assert_eq!(b.len(), 4);
        assert_eq!(b[0], Boundary::Point(Vec3 { x: 5.0, y: 0.0, z: 0.0 }));
        assert_eq!(b[1], Boundary::Checkpoint(0));
        assert_eq!(b[2], Boundary::Point(Vec3 { x: 15.0, y: 0.0, z: 0.0 }));
        assert_eq!(b[3], Boundary::Checkpoint(1));
    }

    #[test]
    fn sectors_and_the_theoretical_best_add_up() {
        let focus = run(10.0, 2000, &[1000, 2000]);
        let slower = run(5.0, 4000, &[2000, 4000]);
        let b = boundaries(&focus, &[Vec3 { x: 5.0, y: 0.0, z: 0.0 }]);
        let times = boundary_times(&slower, &b);
        assert!((times[0].unwrap() - 1000.0).abs() < 1e-3);
        assert_eq!(times[1..], [Some(2000.0), Some(4000.0)]);

        let focus_sectors = sector_durations(&boundary_times(&focus, &b));
        let slower_sectors = sector_durations(&times);
        assert!((focus_sectors[0].unwrap() - 500.0).abs() < 1e-3);
        assert!((slower_sectors[1].unwrap() - 1000.0).abs() < 1e-3);

        // A third run fast at the start only: the best combines its first sector with the focused run's others
        let (best, total) = best_sectors(&[focus_sectors.clone(), slower_sectors, vec![Some(400.0), None, Some(2000.0)]]);
        assert_eq!(best[0], Some(400.0));
        assert!((total.unwrap() - (400.0 + 500.0 + 1000.0)).abs() < 1e-3);
    }

    #[test]
    fn missing_boundaries_leave_gaps_and_no_total() {
        assert_eq!(sector_durations(&[Some(100.0), None, Some(300.0)]), [Some(100.0), None, None]);
        let (best, total) = best_sectors(&[vec![Some(100.0), None]]);
        assert_eq!((best, total), (vec![Some(100.0), None], None));
        assert_eq!(best_sectors(&[]).1, None);
    }
}
