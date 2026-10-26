// ABOUTME: Camera placement (eye and target) and projection of world points and segments to the screen, clipping
// ABOUTME: segments at the near plane so lines passing behind the camera still draw their visible part.

use egui::{Pos2, Rect};
use glam::{Mat4, Vec3, Vec4};

const NEAR: f32 = 0.5;
const FAR: f32 = 20_000.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    /// Vertical field of view in radians
    pub fov_y: f32,
}

impl Camera {
    /// A camera orbiting target: yaw around the up axis, pitch above the horizon, at distance
    pub fn orbit(target: Vec3, yaw: f32, pitch: f32, distance: f32) -> Self {
        let dir = Vec3::new(yaw.cos() * pitch.cos(), pitch.sin(), yaw.sin() * pitch.cos());
        Self { eye: target + dir * distance, target, fov_y: 60f32.to_radians() }
    }

    /// Behind a car heading along forward, looking a little ahead of it; ignores pitch and roll so the view stays level
    pub fn chase(position: Vec3, forward: Vec3) -> Self {
        let flat = Vec3::new(forward.x, 0.0, forward.z).try_normalize().unwrap_or(Vec3::Z);
        Self { eye: position - flat * 13.0 + Vec3::Y * 4.5, target: position + flat * 6.0 + Vec3::Y * 1.0, fov_y: 65f32.to_radians() }
    }

    /// Behind a car along its horizontal heading, raised, looking partly ahead, with the vehicle's own camera
    /// settings (distance, height, look-ahead, and field of view in degrees), as the game's chase camera does
    pub fn behind(position: Vec3, forward: Vec3, far: f32, up: f32, look_at_factor: f32, fov_degrees: f32) -> Self {
        let heading = Vec3::new(forward.x, 0.0, forward.z).try_normalize().unwrap_or(Vec3::Z);
        let eye = position - heading * far + Vec3::Y * up;
        let ahead = Vec3::new(position.x + heading.x * far, position.y + up, position.z + heading.z * far);
        let target = position.lerp(ahead, look_at_factor) - Vec3::Y;
        Self { eye, target, fov_y: fov_degrees.to_radians() }
    }

    /// The combined view and projection for a viewport of this aspect ratio, with depth from 0 (near) to 1 (far)
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let view = glam::camera::rh::view::look_at_mat4(self.eye, self.target, Vec3::Y);
        let proj = glam::camera::rh::proj::directx::perspective(self.fov_y, aspect.max(0.01), NEAR, FAR);
        proj * view
    }

    pub fn projector(&self, rect: Rect) -> Projector {
        Projector { view_proj: self.view_proj(rect.width() / rect.height().max(1.0)), rect }
    }
}

pub struct Projector {
    view_proj: Mat4,
    rect: Rect,
}

impl Projector {
    fn clip(&self, p: Vec3) -> Vec4 {
        self.view_proj * p.extend(1.0)
    }

    fn to_screen(&self, c: Vec4) -> Pos2 {
        let center = self.rect.center();
        Pos2::new(center.x + c.x / c.w * self.rect.width() / 2.0, center.y - c.y / c.w * self.rect.height() / 2.0)
    }

    /// A world point on the screen, or None when it is behind the camera
    pub fn point(&self, p: Vec3) -> Option<Pos2> {
        let c = self.clip(p);
        (c.w > NEAR).then(|| self.to_screen(c))
    }

    /// A world segment on the screen, cut at the near plane; None when all of it is behind the camera
    pub fn segment(&self, a: Vec3, b: Vec3) -> Option<[Pos2; 2]> {
        let (mut ca, mut cb) = (self.clip(a), self.clip(b));
        match (ca.w > NEAR, cb.w > NEAR) {
            (false, false) => return None,
            (true, false) => cb = ca + (cb - ca) * ((ca.w - NEAR) / (ca.w - cb.w)),
            (false, true) => ca = cb + (ca - cb) * ((cb.w - NEAR) / (cb.w - ca.w)),
            (true, true) => {}
        }
        Some([self.to_screen(ca), self.to_screen(cb)])
    }

    /// Distance from the camera along its view, for culling and fading far geometry
    pub fn depth(&self, p: Vec3) -> f32 {
        self.clip(p).w
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))
    }

    #[test]
    fn the_target_lands_in_the_middle() {
        let cam = Camera::orbit(Vec3::new(10.0, 0.0, 10.0), 0.3, 0.4, 50.0);
        let p = cam.projector(rect()).point(cam.target).unwrap();
        assert!((p.x - 400.0).abs() < 1e-3 && (p.y - 300.0).abs() < 1e-3);
    }

    #[test]
    fn points_behind_are_dropped_and_segments_cut() {
        let cam = Camera { eye: Vec3::ZERO, target: Vec3::new(0.0, 0.0, -10.0), fov_y: 1.0 };
        let pr = cam.projector(rect());
        assert!(pr.point(Vec3::new(0.0, 0.0, 10.0)).is_none());
        assert!(pr.segment(Vec3::new(0.0, 0.0, 5.0), Vec3::new(1.0, 0.0, 8.0)).is_none());
        let [a, b] = pr.segment(Vec3::new(1.0, 0.0, 5.0), Vec3::new(1.0, 0.0, -20.0)).unwrap();
        assert!(a.x.is_finite() && b.x.is_finite());
        // Higher up in the world is higher on the screen
        assert!(pr.point(Vec3::new(0.0, 1.0, -10.0)).unwrap().y < 300.0);
    }

    #[test]
    fn the_vehicle_camera_follows_the_heading_and_looks_ahead() {
        let cam = Camera::behind(Vec3::ZERO, Vec3::new(1.0, 0.5, 0.0), 4.5, 2.2, 0.88, 75.0);
        assert!((cam.eye - Vec3::new(-4.5, 2.2, 0.0)).length() < 1e-5);
        assert!((cam.target - Vec3::new(3.96, 2.2 * 0.88 - 1.0, 0.0)).length() < 1e-5);
    }

    #[test]
    fn the_chase_camera_sits_behind_and_above() {
        let cam = Camera::chase(Vec3::ZERO, Vec3::new(0.0, 0.3, 1.0));
        assert!(cam.eye.z < 0.0 && cam.eye.y > 0.0);
        assert!(cam.target.z > 0.0);
    }
}
