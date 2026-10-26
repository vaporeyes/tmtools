// ABOUTME: ReplayViewModule UI: picks the camera and draws the runs at the playhead, textured from the game assets when
// ABOUTME: an export is set (wireframe cells and boxes otherwise), with a corner HUD of the focused run's speed and inputs.

use crate::camera::{Camera, Projector};
use crate::gpu::{CarDraw, ColorVertex, DrawCallback, FrameRequest, FrameUniform, LineVertex, Renderer};
use crate::loader::{scene_key, SceneLoad};
use eframe::egui_wgpu;
use std::collections::HashMap;
use tmassets::Vehicle;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use glam::{Mat4, Quat, Vec3};
use tmreplay::gap::{position_at, rotation_at, speed_at};
use tmreplay::grid::BlockGrid;
use tmreplay::track::Channel;
use tmreplay::{format_race_time, Sample};
use tmtools_core::{Module, RenderState, Session};

// Car box in world units: half width (x), height (y), half length (z, forward)
const CAR_HALF_WIDTH: f32 = 1.0;
const CAR_HEIGHT: f32 = 1.1;
const CAR_HALF_LENGTH: f32 = 2.3;
// Blocks farther than this beyond what the camera looks at are left out, to keep the view readable and fast
const BLOCK_RANGE: f32 = 700.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CameraMode {
    Chase,
    Orbit,
    Overview,
}

// A run without a vehicle name drives the Stadium car, as the web viewer assumes
const DEFAULT_VEHICLE: &str = "StadiumCar";

pub struct ReplayViewModule {
    mode: CameraMode,
    yaw: f32,
    pitch: f32,
    distance: f32,
    show_blocks: bool,
    show_lines: bool,
    /// Posts at the focused run's checkpoints in the textured view
    show_posts: bool,
    /// Draw with the game assets when an export is set
    textured: bool,
    load: Option<SceneLoad>,
    /// The scene the GPU holds, and how it is lit
    ready: Option<ReadyScene>,
    /// Camera settings per vehicle from the export
    vehicle_info: HashMap<String, Vehicle>,
    /// Rigged vehicle models, to pose each car's wheels every frame
    vehicles: HashMap<String, std::sync::Arc<tmscene::vehicle::VehicleModel>>,
    status: Option<String>,
}

struct ReadyScene {
    key: String,
    mood: tmscene::mood::Mood,
}

impl Default for ReplayViewModule {
    fn default() -> Self {
        Self {
            mode: CameraMode::Chase,
            yaw: -1.2,
            pitch: 0.45,
            distance: 40.0,
            show_blocks: true,
            show_lines: true,
            show_posts: true,
            textured: true,
            load: None,
            ready: None,
            vehicle_info: HashMap::new(),
            vehicles: HashMap::new(),
            status: None,
        }
    }
}

fn vehicle_name(session: &Session, run: usize) -> String {
    session.ghost(run).vehicle.clone().unwrap_or_else(|| DEFAULT_VEHICLE.to_owned())
}

fn v3(v: tmreplay::Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn car_pose(samples: &[Sample], time_ms: f32) -> Option<(Vec3, Quat)> {
    let p = position_at(samples, time_ms)?;
    let q = rotation_at(samples, time_ms)?;
    Some((v3(p), Quat::from_xyzw(q.x, q.y, q.z, q.w)))
}

impl Module for ReplayViewModule {
    fn name(&self) -> &str {
        "Replay Viewer"
    }

    fn description(&self) -> &str {
        "Basic 3D view of the runs: block cells, racing lines, and cars, with chase, orbit, and overview cameras"
    }

    fn ui(&mut self, ui: &mut egui::Ui, session: &mut Session, wgpu: Option<&RenderState>) {
        let Some(focus) = session.focus else {
            ui.centered_and_justified(|ui| ui.label("Open a replay to watch it."));
            return;
        };

        ui.horizontal(|ui| {
            ui.label("Camera");
            for (mode, label) in [(CameraMode::Chase, "Chase"), (CameraMode::Orbit, "Orbit car"), (CameraMode::Overview, "Overview")] {
                if ui.selectable_label(self.mode == mode, label).clicked() {
                    self.mode = mode;
                    self.distance = if mode == CameraMode::Overview { 900.0 } else { 40.0 };
                }
            }
            ui.separator();
            if session.assets.is_some() && wgpu.is_some() {
                ui.checkbox(&mut self.textured, "Game textures").on_hover_text("Draw the map and cars with the asset export's meshes and textures");
            }
            ui.checkbox(&mut self.show_blocks, "Blocks");
            ui.checkbox(&mut self.show_lines, "Racing lines");
            if session.assets.is_some() && wgpu.is_some() && self.textured {
                ui.checkbox(&mut self.show_posts, "Checkpoint posts");
            }
            ui.label(egui::RichText::new("Orbit and overview: drag to turn, wheel to zoom").weak().small());
        });

        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::drag());
        if self.mode != CameraMode::Chase {
            if response.dragged() {
                let d = response.drag_delta();
                self.yaw += d.x * 0.008;
                self.pitch = (self.pitch + d.y * 0.006).clamp(-0.2, 1.45);
            }
            if response.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                self.distance = (self.distance * (-scroll * 0.002).exp()).clamp(6.0, 4000.0);
            }
        }

        let now = session.playback.time_ms as f32;
        let samples = &session.ghost(focus).samples;
        let Some((car_pos, car_rot)) = car_pose(samples, now) else {
            ui.label("This run has no samples to show.");
            return;
        };
        let info = self.vehicle_info.get(&vehicle_name(session, focus));
        let camera = match (self.mode, info) {
            // The game's chase camera for this vehicle, once the export's vehicle data is loaded
            (CameraMode::Chase, Some(v)) => Camera::behind(car_pos, car_rot * Vec3::Z, v.camera_far, v.camera_up, v.camera_look_at_factor, v.camera_fov),
            (CameraMode::Chase, None) => Camera::chase(car_pos, car_rot * Vec3::Z),
            (CameraMode::Orbit, _) => Camera::orbit(car_pos, self.yaw, self.pitch, self.distance),
            (CameraMode::Overview, _) => Camera::orbit(run_center(samples), self.yaw, self.pitch.max(0.5), self.distance),
        };

        if let Some(rs) = wgpu.filter(|_| self.textured) {
            if self.draw_textured(ui, rect, session, focus, &camera, rs) {
                hud(&ui.painter_at(rect), rect, session, focus, now);
                return;
            }
        }
        let pr = camera.projector(rect);

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_rgb(24, 30, 40));
        if self.show_blocks {
            draw_blocks(&painter, &pr, session, focus, camera.eye, BLOCK_RANGE + camera.eye.distance(camera.target));
        }
        let runs: Vec<usize> = session.visible_runs().collect();
        if self.show_lines {
            for &run in &runs {
                let (color, width) = if run == focus { (session.runs[run].color, 2.5) } else { (session.runs[run].color.gamma_multiply(0.6), 1.5) };
                let points: Vec<Vec3> = session.ghost(run).samples.iter().map(|s| v3(s.position) + Vec3::Y * 0.2).collect();
                for pair in points.windows(2) {
                    if let Some(seg) = pr.segment(pair[0], pair[1]) {
                        painter.line_segment(seg, Stroke::new(width, color));
                    }
                }
            }
        }
        // Cars, farthest first so nearer ones draw over them
        let mut cars: Vec<(f32, usize, Vec3, Quat)> =
            runs.iter().filter_map(|&r| car_pose(&session.ghost(r).samples, now).map(|(p, q)| (pr.depth(p), r, p, q))).collect();
        cars.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (_, run, p, q) in cars {
            draw_car(&painter, &pr, p, q, session.runs[run].color, run == focus);
        }

        hud(&painter, rect, session, focus, now);
        if let Some(status) = &self.status {
            painter.text(rect.left_top() + Vec2::new(12.0, 12.0), Align2::LEFT_TOP, status, FontId::proportional(14.0), Color32::from_gray(220));
        }
    }
}

impl ReplayViewModule {
    /// Draws the runs with the game assets; false while the scene is loading or when there is no export, so the
    /// wireframe view is drawn instead
    fn draw_textured(&mut self, ui: &mut egui::Ui, rect: Rect, session: &Session, focus: usize, camera: &Camera, rs: &RenderState) -> bool {
        let Some(store) = session.assets.clone() else {
            self.status = None;
            return false;
        };
        let runs: Vec<usize> = session.visible_runs().collect();
        let mut vehicles: Vec<String> = runs.iter().map(|&r| vehicle_name(session, r)).collect();
        vehicles.sort();
        vehicles.dedup();
        let map = &session.replay(focus).map;
        let key = scene_key(map, &vehicles);

        // Start loading when the map or the set of vehicles changes
        let current = self.ready.as_ref().map(|r| r.key.as_str());
        if current != Some(key.as_str()) && self.load.as_ref().is_none_or(|l| l.key != key) {
            self.load = Some(SceneLoad::start(store, map.clone(), vehicles.clone()));
            self.status = Some("Loading the map with game assets...".into());
        }
        // Keep checking on the background load until it lands
        if self.load.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(loaded) = self.load.as_ref().and_then(SceneLoad::poll) {
            self.load = None;
            let mut egui_renderer = rs.renderer.write();
            let mut renderer = egui_renderer.callback_resources.remove::<Renderer>().unwrap_or_else(|| Renderer::new(&rs.device, &rs.queue));
            renderer.upload(&rs.device, &rs.queue, &loaded);
            egui_renderer.callback_resources.insert(renderer);
            let missing = loaded.map.as_ref().map_or(0, |m| m.missing.len() + m.failed.len());
            self.status = loaded.error.clone().or_else(|| (missing > 0).then(|| format!("{missing} block variants have no data in the export")));
            self.vehicle_info.extend(loaded.vehicle_info);
            self.vehicles.extend(loaded.vehicles.iter().map(|(k, v)| (k.clone(), v.clone())));
            let mood = loaded.map.as_ref().map_or_else(|| tmscene::mood::mood_for(map.decoration.as_deref(), map.collection.as_deref()), |m| m.mood);
            self.ready = Some(ReadyScene { key: loaded.key, mood });
        }
        let Some(ready) = self.ready.as_ref().filter(|r| r.key == key) else { return false };

        let now = session.playback.time_ms as f32;
        // Each car's parts in the world: its pose times each part, with the wheels posed from the samples
        let cars = runs
            .iter()
            .filter_map(|&r| {
                let samples = &session.ghost(r).samples;
                let (p, q) = car_pose(samples, now)?;
                let vehicle = vehicle_name(session, r);
                let model = self.vehicles.get(&vehicle)?;
                let pose = Mat4::from_rotation_translation(q, p);
                let wheels = tmscene::vehicle::wheel_state(samples, now);
                let parts = model.part_matrices(wheels.as_ref()).into_iter().map(|m| pose * m).collect();
                Some(CarDraw { vehicle, parts })
            })
            .collect();
        let ppp = ui.ctx().pixels_per_point();
        let mut overlay = Vec::new();
        let mut lines = Vec::new();
        if self.show_lines {
            racing_lines(session, &runs, focus, ppp, &mut lines);
        }
        if self.show_posts {
            checkpoint_posts(session, focus, &mut overlay);
        }
        let aspect = rect.width() / rect.height().max(1.0);
        let pixels = rect.size() * ppp;
        let uniform = FrameUniform::new(camera.view_proj(aspect), camera.eye, &ready.mood, [pixels.x.round(), pixels.y.round()]);

        let texture = {
            let mut egui_renderer = rs.renderer.write();
            let mut renderer = egui_renderer.callback_resources.remove::<Renderer>().unwrap_or_else(|| Renderer::new(&rs.device, &rs.queue));
            let id = renderer.ensure_targets(&rs.device, &mut egui_renderer, [pixels.x.round() as u32, pixels.y.round() as u32]);
            renderer.set_request(FrameRequest { uniform, cars, overlay, lines });
            egui_renderer.callback_resources.insert(renderer);
            id
        };
        let painter = ui.painter_at(rect);
        painter.add(egui_wgpu::Callback::new_paint_callback(rect, DrawCallback));
        painter.image(texture, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        if let Some(status) = &self.status {
            painter.text(rect.left_top() + Vec2::new(12.0, 12.0), Align2::LEFT_TOP, status, FontId::proportional(14.0), Color32::from_gray(235));
        }
        true
    }
}

/// Each visible run's path as a line a few pixels wide, lifted off the road; the focused run's is bolder
fn racing_lines(session: &Session, runs: &[usize], focus: usize, pixels_per_point: f32, out: &mut Vec<LineVertex>) {
    for &run in runs {
        let c = session.runs[run].color;
        let (alpha, width) = if run == focus { (0.9, 3.5) } else { (0.6, 2.0) };
        let color = [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, alpha];
        let points: Vec<Vec3> = session.ghost(run).samples.iter().map(|s| Vec3::new(s.position.x, s.position.y + 0.15, s.position.z)).collect();
        crate::overlay::screen_line(&points, width * pixels_per_point, color, out);
    }
}

/// Yellow posts where the focused run took each checkpoint, as the web viewer marks them
fn checkpoint_posts(session: &Session, focus: usize, out: &mut Vec<ColorVertex>) {
    let ghost = session.ghost(focus);
    for t in ghost.checkpoints.iter().filter_map(|c| c.time_ms) {
        if let Some(p) = position_at(&ghost.samples, t as f32) {
            crate::overlay::post(v3(p), [1.0, 0.65, 0.03], out);
        }
    }
}

fn run_center(samples: &[Sample]) -> Vec3 {
    let sum = samples.iter().fold(Vec3::ZERO, |acc, s| acc + v3(s.position));
    sum / samples.len().max(1) as f32
}

fn draw_blocks(painter: &egui::Painter, pr: &Projector, session: &Session, focus: usize, eye: Vec3, max_range: f32) {
    let replay = session.replay(focus);
    let Some(blocks) = &replay.map.blocks else { return };
    let grid = BlockGrid::for_map(&replay.map);
    let (s, h) = (grid.square_size, grid.square_height);
    for block in blocks.iter().filter(|b| !b.is_clip) {
        let [x, y, z] = grid.block_origin(block);
        // Outline each cell at its top; ground cells sit at their base so raised blocks stand out above them
        let top = y + if block.is_ground { 0.0 } else { h };
        let range = Vec3::new(x + s / 2.0, top, z + s / 2.0).distance(eye);
        if range > max_range {
            continue;
        }
        // Fade with distance from the camera; ground cells are fainter
        let fade = 1.0 - range / max_range;
        let alpha = ((if block.is_ground { 45.0 } else { 110.0 }) * fade) as u8;
        let stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(180, 200, 220, alpha));
        let corners = [Vec3::new(x, top, z), Vec3::new(x + s, top, z), Vec3::new(x + s, top, z + s), Vec3::new(x, top, z + s)];
        for i in 0..4 {
            if let Some(seg) = pr.segment(corners[i], corners[(i + 1) % 4]) {
                painter.line_segment(seg, stroke);
            }
        }
    }
}

fn draw_car(painter: &egui::Painter, pr: &Projector, position: Vec3, rotation: Quat, color: Color32, focused: bool) {
    let corner = |sx: f32, y: f32, sz: f32| position + rotation * Vec3::new(sx * CAR_HALF_WIDTH, y, sz * CAR_HALF_LENGTH);
    let bottom = [corner(-1.0, 0.0, -1.0), corner(1.0, 0.0, -1.0), corner(1.0, 0.0, 1.0), corner(-1.0, 0.0, 1.0)];
    let top = bottom.map(|c| c + rotation * Vec3::Y * CAR_HEIGHT);

    // Filled roof so the car reads as a solid shape, then the edges
    let roof: Vec<Pos2> = top.iter().filter_map(|&c| pr.point(c)).collect();
    if roof.len() == 4 {
        painter.add(egui::Shape::convex_polygon(roof, color.gamma_multiply(0.55), Stroke::NONE));
    }
    let stroke = Stroke::new(if focused { 2.0 } else { 1.5 }, color);
    for i in 0..4 {
        for (a, b) in [(bottom[i], bottom[(i + 1) % 4]), (top[i], top[(i + 1) % 4]), (bottom[i], top[i])] {
            if let Some(seg) = pr.segment(a, b) {
                painter.line_segment(seg, stroke);
            }
        }
    }
    // A dot keeps the car findable when the camera is far away and the box is a few pixels
    if let Some(center) = pr.point(position + rotation * Vec3::Y * (CAR_HEIGHT / 2.0)) {
        painter.circle_filled(center, if focused { 4.0 } else { 3.0 }, color);
    }
    // A nose line marks which way the car points
    if let Some(seg) = pr.segment(corner(0.0, CAR_HEIGHT, 1.0), corner(0.0, CAR_HEIGHT, 1.0) + rotation * Vec3::Z * 2.0) {
        painter.line_segment(seg, Stroke::new(2.0, Color32::WHITE));
    }
}

/// The focused run's time, speed, and inputs in the corner: arrow keys lit to the share held, as in a TAS display
fn hud(painter: &egui::Painter, rect: Rect, session: &Session, focus: usize, now: f32) {
    let panel = Rect::from_min_size(rect.left_bottom() + Vec2::new(12.0, -96.0), Vec2::new(250.0, 84.0));
    painter.rect_filled(panel, 6.0, Color32::from_black_alpha(160));
    let speed = speed_at(&session.ghost(focus).samples, now).unwrap_or(0.0);
    let tick = tmreplay::track::tick_of(now as f64);
    let text = Color32::from_gray(230);
    painter.text(panel.left_top() + Vec2::new(10.0, 8.0), Align2::LEFT_TOP, format_race_time(tick as i64), FontId::monospace(16.0), text);
    painter.text(panel.left_top() + Vec2::new(10.0, 32.0), Align2::LEFT_TOP, format!("{speed:.0} km/h"), FontId::proportional(22.0), text);

    let Some(track) = &session.runs[focus].track else { return };
    let key = |center: Pos2, amount: f32, lit: Color32, label: &str| {
        let r = Rect::from_center_size(center, Vec2::splat(24.0));
        painter.rect_filled(r, 4.0, Color32::from_white_alpha(30));
        if amount > 0.0 {
            painter.rect_filled(r, 4.0, lit.gamma_multiply(amount.clamp(0.25, 1.0)));
        }
        painter.text(center, Align2::CENTER_CENTER, label, FontId::proportional(13.0), Color32::WHITE);
    };
    let origin = panel.right_top() + Vec2::new(-60.0, 26.0);
    key(origin, track.value_at(Channel::Gas, tick), Color32::from_rgb(63, 182, 122), "\u{25B2}");
    key(origin + Vec2::new(-27.0, 27.0), track.value_at(Channel::Left, tick), Color32::from_rgb(255, 210, 63), "\u{25C0}");
    key(origin + Vec2::new(0.0, 27.0), track.value_at(Channel::Brake, tick), Color32::from_rgb(229, 72, 77), "\u{25BC}");
    key(origin + Vec2::new(27.0, 27.0), track.value_at(Channel::Right, tick), Color32::from_rgb(255, 210, 63), "\u{25B6}");
}
