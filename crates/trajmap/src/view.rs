// ABOUTME: The top-down view transform (world x and z to screen), zooming about the pointer, fitting bounds, the
// ABOUTME: nearest point on a run's path to the pointer, and the speed color ramp.

use egui::{Color32, Pos2, Vec2};
use tmreplay::Sample;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapView {
    /// World x and z at the center of the view
    pub center: [f32; 2],
    /// Screen pixels per world unit
    pub scale: f32,
}

impl Default for MapView {
    fn default() -> Self {
        Self { center: [512.0, 512.0], scale: 0.6 }
    }
}

impl MapView {
    /// World x and z to a screen position, for a view whose center is at screen_center
    pub fn to_screen(&self, world: [f32; 2], screen_center: Pos2) -> Pos2 {
        screen_center + Vec2::new(world[0] - self.center[0], world[1] - self.center[1]) * self.scale
    }

    pub fn to_world(&self, screen: Pos2, screen_center: Pos2) -> [f32; 2] {
        let d = (screen - screen_center) / self.scale;
        [self.center[0] + d.x, self.center[1] + d.y]
    }

    pub fn pan(&mut self, screen_delta: Vec2) {
        self.center[0] -= screen_delta.x / self.scale;
        self.center[1] -= screen_delta.y / self.scale;
    }

    /// Zooms by factor (above 1 zooms in) keeping the world point under anchor in place
    pub fn zoom_about(&mut self, factor: f32, anchor: Pos2, screen_center: Pos2) {
        let before = self.to_world(anchor, screen_center);
        self.scale = (self.scale * factor).clamp(0.02, 40.0);
        let after = self.to_world(anchor, screen_center);
        self.center[0] += before[0] - after[0];
        self.center[1] += before[1] - after[1];
    }

    /// Frames the box from min to max (world x and z) in a view of size, with a margin
    pub fn fit(&mut self, min: [f32; 2], max: [f32; 2], size: Vec2) {
        let span = Vec2::new((max[0] - min[0]).max(32.0), (max[1] - min[1]).max(32.0));
        self.center = [(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0];
        self.scale = ((size.x / span.x).min(size.y / span.y) * 0.9).clamp(0.02, 40.0);
    }
}

/// The point on the path nearest to a screen position, as (time in ms, screen distance), interpolated along the
/// segment between samples
pub fn nearest_on_path(samples: &[Sample], to_screen: impl Fn([f32; 2]) -> Pos2, pointer: Pos2) -> Option<(f32, f32)> {
    let mut best: Option<(f32, f32)> = None;
    let mut prev: Option<(Pos2, i32)> = None;
    for s in samples {
        let p = to_screen([s.position.x, s.position.z]);
        if let Some((a, ta)) = prev {
            let ab = p - a;
            let len2 = ab.length_sq();
            let along = if len2 > 0.0 { ((pointer - a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
            let distance = (a + ab * along).distance(pointer);
            if best.is_none_or(|(_, d)| distance < d) {
                best = Some((ta as f32 + along * (s.time_ms - ta) as f32, distance));
            }
        }
        prev = Some((p, s.time_ms));
    }
    best
}

/// Blue when slow through green and yellow to red when fast, for a speed between min and max
pub fn speed_color(speed: f32, min: f32, max: f32) -> Color32 {
    let t = if max > min { ((speed - min) / (max - min)).clamp(0.0, 1.0) } else { 0.5 };
    const STOPS: [(f32, [u8; 3]); 4] = [(0.0, [40, 110, 255]), (0.4, [40, 210, 120]), (0.7, [250, 220, 50]), (1.0, [240, 50, 50])];
    let i = STOPS.iter().rposition(|&(at, _)| at <= t).unwrap_or(0).min(STOPS.len() - 2);
    let (a, ca) = STOPS[i];
    let (b, cb) = STOPS[i + 1];
    let k = (t - a) / (b - a);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k).round() as u8;
    Color32::from_rgb(mix(ca[0], cb[0]), mix(ca[1], cb[1]), mix(ca[2], cb[2]))
}

/// Red when losing time through gray to green when gaining, for a rate between -max and max
pub fn gain_color(rate: f32, max: f32) -> Color32 {
    let t = if max > 0.0 { (rate / max).clamp(-1.0, 1.0) } else { 0.0 };
    let (from, to) = if t < 0.0 { ([150u8, 150, 150], [235u8, 60, 60]) } else { ([150, 150, 150], [70, 220, 110]) };
    let k = t.abs();
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * k).round() as u8;
    Color32::from_rgb(mix(from[0], to[0]), mix(from[1], to[1]), mix(from[2], to[2]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tmreplay::Vec3;

    #[test]
    fn maps_world_and_screen_both_ways() {
        let view = MapView { center: [100.0, 200.0], scale: 2.0 };
        let c = Pos2::new(400.0, 300.0);
        assert_eq!(view.to_screen([110.0, 190.0], c), Pos2::new(420.0, 280.0));
        assert_eq!(view.to_world(Pos2::new(420.0, 280.0), c), [110.0, 190.0]);
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        let mut view = MapView { center: [0.0, 0.0], scale: 1.0 };
        let c = Pos2::new(0.0, 0.0);
        let anchor = Pos2::new(50.0, -20.0);
        let before = view.to_world(anchor, c);
        view.zoom_about(2.0, anchor, c);
        let after = view.to_world(anchor, c);
        assert!((before[0] - after[0]).abs() < 1e-4 && (before[1] - after[1]).abs() < 1e-4);
        assert_eq!(view.scale, 2.0);
    }

    #[test]
    fn fits_bounds_with_a_margin() {
        let mut view = MapView::default();
        view.fit([0.0, 0.0], [100.0, 50.0], Vec2::new(200.0, 200.0));
        assert_eq!(view.center, [50.0, 25.0]);
        assert!((view.scale - 1.8).abs() < 1e-5);
    }

    #[test]
    fn finds_the_time_at_the_nearest_point() {
        let samples: Vec<Sample> = (0..3).map(|i| Sample { time_ms: i * 100, position: Vec3 { x: i as f32 * 10.0, y: 0.0, z: 0.0 }, ..Default::default() }).collect();
        let (t, d) = nearest_on_path(&samples, |w| Pos2::new(w[0], w[1]), Pos2::new(15.0, 3.0)).unwrap();
        assert!((t - 150.0).abs() < 1e-3 && (d - 3.0).abs() < 1e-4);
    }

    #[test]
    fn gain_colors_run_from_red_through_gray_to_green() {
        assert_eq!(gain_color(-50.0, 50.0), Color32::from_rgb(235, 60, 60));
        assert_eq!(gain_color(0.0, 50.0), Color32::from_rgb(150, 150, 150));
        assert_eq!(gain_color(500.0, 50.0), Color32::from_rgb(70, 220, 110));
    }

    #[test]
    fn speed_colors_run_from_blue_to_red() {
        assert_eq!(speed_color(0.0, 0.0, 100.0), Color32::from_rgb(40, 110, 255));
        assert_eq!(speed_color(100.0, 0.0, 100.0), Color32::from_rgb(240, 50, 50));
    }
}
