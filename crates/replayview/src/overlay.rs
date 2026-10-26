// ABOUTME: Geometry drawn over the map: racing lines as quads the shader widens to a fixed pixel width, and upright
// ABOUTME: posts at the focused run's checkpoints as colored triangles, shaded a little per side so they read as solid.

use crate::gpu::{ColorVertex, LineVertex};
use glam::Vec3;

// Checkpoint posts as the web viewer draws them: 0.25 units across and 6 tall
const POST_RADIUS: f32 = 0.25;
const POST_HEIGHT: f32 = 6.0;
const POST_SIDES: usize = 12;

/// A line through the points, width_px pixels wide on screen: two triangles per segment, widened by the shader
pub fn screen_line(points: &[Vec3], width_px: f32, color: [f32; 4], out: &mut Vec<LineVertex>) {
    for pair in points.windows(2) {
        let (a, b) = (pair[0].to_array(), pair[1].to_array());
        if a == b {
            continue;
        }
        // Each end offsets from the direction toward the other end, so the far end's sides are swapped
        let corner = |position, other, side: f32| LineVertex { position, other, params: [side, width_px], color };
        let (a_left, a_right, b_left, b_right) = (corner(a, b, 1.0), corner(a, b, -1.0), corner(b, a, -1.0), corner(b, a, 1.0));
        out.extend([a_left, a_right, b_left, a_right, b_right, b_left]);
    }
}

/// An upright post standing on base, its sides lit from one direction so the round shape shows
pub fn post(base: Vec3, color: [f32; 3], out: &mut Vec<ColorVertex>) {
    let light = Vec3::new(-0.4, 0.0, 0.6).normalize();
    for i in 0..POST_SIDES {
        let angle = |k: usize| k as f32 / POST_SIDES as f32 * std::f32::consts::TAU;
        let (a0, a1) = (angle(i), angle(i + 1));
        let normal = Vec3::new(((a0 + a1) / 2.0).cos(), 0.0, ((a0 + a1) / 2.0).sin());
        let shade = 0.7 + 0.3 * normal.dot(light).max(0.0);
        let c = [color[0] * shade, color[1] * shade, color[2] * shade, 1.0];
        let rim = |a: f32, y: f32| base + Vec3::new(a.cos() * POST_RADIUS, y, a.sin() * POST_RADIUS);
        let corners = [rim(a0, 0.0), rim(a1, 0.0), rim(a1, POST_HEIGHT), rim(a0, POST_HEIGHT)];
        for k in [0, 1, 2, 0, 2, 3] {
            out.push(ColorVertex { position: corners[k].to_array(), color: c });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_make_two_triangles_per_segment() {
        let mut out = Vec::new();
        screen_line(&[Vec3::ZERO, Vec3::X, Vec3::new(2.0, 0.0, 0.0)], 3.0, [1.0; 4], &mut out);
        assert_eq!(out.len(), 12);
        assert!(out.iter().all(|v| v.params[1] == 3.0 && v.params[0].abs() == 1.0));
        // Repeated points add nothing
        let mut none = Vec::new();
        screen_line(&[Vec3::ONE, Vec3::ONE], 3.0, [1.0; 4], &mut none);
        assert!(none.is_empty());
    }

    #[test]
    fn posts_stand_on_their_base() {
        let mut out = Vec::new();
        post(Vec3::new(5.0, 1.0, 5.0), [1.0, 0.8, 0.2], &mut out);
        assert_eq!(out.len(), POST_SIDES * 6);
        let (lo, hi) = out.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v.position[1]), hi.max(v.position[1])));
        assert_eq!((lo, hi), (1.0, 7.0));
    }
}
