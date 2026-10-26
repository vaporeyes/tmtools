// ABOUTME: TrajMapModule UI: blocks as grid cells shaded by height, racing lines (the focused one colored by speed or by
// ABOUTME: time gained and lost against a reference run),
// ABOUTME: input, checkpoint, and sector split markers, cars at the playhead; click seeks, or places a split when placing.

use crate::view::{gain_color, nearest_on_path, speed_color, MapView};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use tmreplay::gap::position_at;
use tmreplay::grid::BlockGrid;
use tmreplay::track::Channel;
use tmreplay::{format_race_time, Sample};
use tmtools_core::{color_swatch, Module, Session};

const BRAKE: Color32 = Color32::from_rgb(229, 72, 77);
const LIFT: Color32 = Color32::from_rgb(255, 150, 40);
const RESPAWN: Color32 = Color32::from_rgb(200, 90, 255);
const CHECKPOINT: Color32 = Color32::from_rgb(90, 180, 255);
// How close, in pixels, the pointer must be to the focused path to read it
const HOVER_PX: f32 = 14.0;

pub struct TrajMapModule {
    view: MapView,
    fitted: bool,
    follow: bool,
    show_blocks: bool,
    show_brakes: bool,
    show_lifts: bool,
    show_checkpoints: bool,
    markers_all_runs: bool,
    /// Clicks on the focused line place sector splits instead of seeking
    placing_splits: bool,
    /// Color the focused line by time gained and lost against this run instead of by speed
    reference: Option<usize>,
    gains: Option<GainCache>,
}

/// Gaps and gain rates of the focused run against the reference, kept while both stay the same
struct GainCache {
    key: (usize, usize),
    gaps: Vec<Option<f32>>,
    rates: Vec<Option<f32>>,
    /// The rate shown at full color: a high percentile, so a few spikes don't wash out the rest
    scale: f32,
}

// Gain rates are the gap's slope over this window, short enough to resolve a corner
const GAIN_WINDOW_MS: i32 = 400;

fn run_identity(session: &Session, run: usize) -> usize {
    (std::sync::Arc::as_ptr(&session.files[session.runs[run].file].bytes) as usize) ^ session.runs[run].ghost
}

impl Default for TrajMapModule {
    fn default() -> Self {
        Self {
            view: MapView::default(),
            fitted: false,
            follow: false,
            show_blocks: true,
            show_brakes: true,
            show_lifts: true,
            show_checkpoints: true,
            markers_all_runs: false,
            placing_splits: false,
            reference: None,
            gains: None,
        }
    }
}

fn xz(s: &Sample) -> [f32; 2] {
    [s.position.x, s.position.z]
}

impl Module for TrajMapModule {
    fn name(&self) -> &str {
        "Trajectory Map"
    }

    fn description(&self) -> &str {
        "Top-down racing lines over the block grid, with braking, lift, and checkpoint markers"
    }

    fn ui(&mut self, ui: &mut egui::Ui, session: &mut Session, _wgpu: Option<&tmtools_core::RenderState>) {
        let Some(focus) = session.focus else {
            ui.centered_and_justified(|ui| ui.label("Open a replay to see its racing line."));
            return;
        };

        let mut fit = !self.fitted;
        tmtools_core::toolbar(ui, |ui| {
            fit |= ui.button("Fit runs").clicked();
            ui.checkbox(&mut self.follow, "Follow focused car");
            ui.separator();
            ui.checkbox(&mut self.show_blocks, "Blocks");
            ui.checkbox(&mut self.show_brakes, "Brakes");
            ui.checkbox(&mut self.show_lifts, "Gas lifts");
            ui.checkbox(&mut self.show_checkpoints, "Checkpoints");
            ui.checkbox(&mut self.markers_all_runs, "Markers for every run");
            ui.separator();
            ui.checkbox(&mut self.placing_splits, "Place splits")
                .on_hover_text("Click the focused line to split a sector there; right-click a split to remove it. The Run Report times every sector.");
        });
        // The reference must be another visible run on the same map
        let same_map = session.runs_on_map_of(focus);
        if self.reference.is_some_and(|r| r == focus || !same_map.contains(&r)) {
            self.reference = None;
        }
        ui.horizontal(|ui| {
            ui.label("Color by");
            let selected = self.reference.map_or("speed".to_owned(), |r| format!("time vs {}", session.runs[r].label));
            egui::ComboBox::from_id_salt("trajmap-color").selected_text(selected).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.reference, None, "speed");
                for &r in same_map.iter().filter(|&&r| r != focus) {
                    ui.selectable_value(&mut self.reference, Some(r), format!("time vs {}", session.runs[r].label));
                }
            });
        });
        match self.reference {
            Some(r) => {
                let scale = self.gains(session, focus, r).scale;
                gain_legend(ui, session, r, scale);
            }
            None => legend(ui, session, focus),
        }

        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let center = rect.center();
        let runs: Vec<usize> = session.visible_runs().collect();

        if fit {
            self.fit_runs(session, &runs, rect.size());
            self.fitted = true;
        }
        if response.dragged() {
            self.view.pan(response.drag_delta());
            self.follow = false;
        }
        if response.hovered() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let anchor = response.hover_pos().unwrap_or(center);
            if scroll != 0.0 {
                self.view.zoom_about((scroll * 0.004).exp(), anchor, center);
            }
            if zoom != 1.0 {
                self.view.zoom_about(zoom, anchor, center);
            }
        }
        let now = session.playback.time_ms as f32;
        if self.follow {
            if let Some(p) = position_at(&session.ghost(focus).samples, now) {
                self.view.center = [p.x, p.z];
            }
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_gray(18));
        let to_screen = |w: [f32; 2]| self.view.to_screen(w, center);

        if self.show_blocks {
            self.draw_blocks(&painter, session, focus, &to_screen);
        }

        // Other runs first in their colors, the focused run on top colored by speed
        for &run in runs.iter().filter(|&&r| r != focus) {
            let points: Vec<Pos2> = session.ghost(run).samples.iter().map(|s| to_screen(xz(s))).collect();
            painter.add(Shape::line(points, Stroke::new(1.5, session.runs[run].color.gamma_multiply(0.8))));
        }
        let focus_samples = &session.ghost(focus).samples;
        let (min_speed, max_speed) = focus_samples.iter().fold((f32::MAX, 0.0f32), |(lo, hi), s| (lo.min(s.speed_kmh), hi.max(s.speed_kmh)));
        let gains = self.reference.and(self.gains.as_ref());
        for (i, pair) in focus_samples.windows(2).enumerate() {
            let color = match gains {
                Some(g) => g.rates.get(i + 1).copied().flatten().map_or(Color32::from_gray(90), |r| gain_color(r, g.scale)),
                None => speed_color(pair[1].speed_kmh, min_speed, max_speed),
            };
            painter.line_segment([to_screen(xz(&pair[0])), to_screen(xz(&pair[1]))], Stroke::new(3.0, color));
        }

        let marker_runs: Vec<usize> = if self.markers_all_runs { runs.clone() } else { vec![focus] };
        for &run in &marker_runs {
            self.draw_markers(&painter, session, run, &to_screen);
        }

        // Cars at the playhead
        for &run in &runs {
            if let Some(p) = position_at(&session.ghost(run).samples, now) {
                let at = to_screen([p.x, p.z]);
                let radius = if run == focus { 6.0 } else { 4.5 };
                painter.circle(at, radius, session.runs[run].color, Stroke::new(1.5, Color32::BLACK));
            }
        }

        // Sector splits, labeled by the sector each one starts
        let splits = session.splits_for(focus).to_vec();
        let ghost = session.ghost(focus);
        let boundaries = tmreplay::sectors::boundaries(ghost, &splits);
        for (i, b) in boundaries.iter().enumerate() {
            if let tmreplay::sectors::Boundary::Point(p) = b {
                let at = to_screen([p.x, p.z]);
                painter.rect_filled(Rect::from_center_size(at, Vec2::splat(9.0)), 1.0, Color32::WHITE);
                painter.text(at + Vec2::new(8.0, 8.0), Align2::LEFT_TOP, format!("S{}", i + 2), FontId::proportional(12.0), Color32::WHITE);
            }
        }
        if self.placing_splits && response.secondary_clicked() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let nearest = splits.iter().enumerate().map(|(i, p)| (i, to_screen([p.x, p.z]).distance(pointer))).min_by(|a, b| a.1.total_cmp(&b.1));
                if let Some((i, _)) = nearest.filter(|&(_, d)| d <= HOVER_PX) {
                    let key = session.map_key(focus);
                    if let Some(list) = session.splits.get_mut(&key) {
                        list.remove(i);
                    }
                    return;
                }
            }
        }

        // Hover reads the focused path; a click seeks to the point under the pointer, or places a split there
        let mut seek = None;
        let mut place = None;
        if let Some(pointer) = response.hover_pos() {
            if let Some((t, _)) = nearest_on_path(focus_samples, to_screen, pointer).filter(|&(_, d)| d <= HOVER_PX) {
                if let Some(p) = position_at(focus_samples, t) {
                    painter.circle_stroke(to_screen([p.x, p.z]), 7.0, Stroke::new(2.0, Color32::WHITE));
                }
                let tick = tmreplay::track::tick_of(t as f64);
                let speed = tmreplay::gap::speed_at(focus_samples, t).unwrap_or(0.0);
                let inputs = session.runs[focus].track.as_ref().map_or(String::new(), |track| {
                    let steer = track.steer_at(tick);
                    let gas = if track.value_at(Channel::Gas, tick) > 0.0 { "gas" } else { "no gas" };
                    let brake = if track.value_at(Channel::Brake, tick) > 0.0 { ", braking" } else { "" };
                    format!("\nsteer {:+.0}%, {gas}{brake}", steer * 100.0)
                });
                let action = if self.placing_splits { "click to split a sector here" } else { "click to seek" };
                // Against the reference: the gap here and how fast it is changing
                let versus = gains.zip(self.reference).map_or(String::new(), |(g, r)| {
                    let i = focus_samples.partition_point(|s| (s.time_ms as f32) <= t).saturating_sub(1);
                    let gap = g.gaps.get(i).copied().flatten();
                    let rate = g.rates.get(i).copied().flatten();
                    match (gap, rate) {
                        (Some(gap), Some(rate)) => {
                            let side = if gap >= 0.0 { "ahead of" } else { "behind" };
                            let trend = if rate >= 0.0 { "gaining" } else { "losing" };
                            format!("\n{:.2} s {side} {}, {trend} {:.0} ms per s", gap.abs() / 1000.0, session.runs[r].label, rate.abs())
                        }
                        _ => format!("\nno gap to {} here", session.runs[r].label),
                    }
                });
                response.clone().on_hover_text_at_pointer(format!("{}  {:.0} km/h{inputs}{versus}\n{action}", format_race_time(tick as i64), speed));
                if response.clicked() {
                    if self.placing_splits {
                        place = position_at(focus_samples, t);
                    } else {
                        seek = Some(tick as f64);
                    }
                }
            }
        }
        if let Some(p) = place {
            let key = session.map_key(focus);
            session.splits.entry(key).or_default().push(p);
        }
        if let Some(t) = seek {
            session.playback.playing = false;
            session.seek(t);
        }
    }
}

impl TrajMapModule {
    /// The focused run's gaps and gain rates against the reference, computed once per pair of runs
    fn gains(&mut self, session: &Session, focus: usize, reference: usize) -> &GainCache {
        let key = (run_identity(session, focus), run_identity(session, reference));
        if self.gains.as_ref().is_none_or(|g| g.key != key) {
            let focus_samples = &session.ghost(focus).samples;
            let gaps = tmreplay::gap::gaps_at_samples(focus_samples, &session.ghost(reference).samples);
            let rates = tmreplay::gap::gain_rates(focus_samples, &gaps, GAIN_WINDOW_MS);
            let mut magnitudes: Vec<f32> = rates.iter().flatten().map(|r| r.abs()).collect();
            magnitudes.sort_by(f32::total_cmp);
            let scale = magnitudes.get(magnitudes.len() * 95 / 100).copied().unwrap_or(0.0).max(20.0);
            self.gains = Some(GainCache { key, gaps, rates, scale });
        }
        self.gains.as_ref().expect("filled above")
    }

    fn fit_runs(&mut self, session: &Session, runs: &[usize], size: Vec2) {
        let mut min = [f32::MAX; 2];
        let mut max = [f32::MIN; 2];
        for s in runs.iter().flat_map(|&r| session.ghost(r).samples.iter()) {
            min = [min[0].min(s.position.x), min[1].min(s.position.z)];
            max = [max[0].max(s.position.x), max[1].max(s.position.z)];
        }
        if min[0] <= max[0] {
            self.view.fit(min, max, size);
        }
    }

    fn draw_blocks(&self, painter: &egui::Painter, session: &Session, focus: usize, to_screen: &impl Fn([f32; 2]) -> Pos2) {
        let replay = session.replay(focus);
        let Some(blocks) = &replay.map.blocks else { return };
        let grid = BlockGrid::for_map(&replay.map);
        let mut cells: Vec<_> = blocks.iter().filter(|b| !b.is_clip).map(|b| (b.y, grid.block_origin(b), b.is_ground)).collect();
        // Higher blocks drawn last, so they cover what they sit above
        cells.sort_by_key(|c| c.0);
        let top = cells.last().map_or(1, |c| c.0).max(1);
        let base = cells.first().map_or(0, |c| c.0);
        for (y, origin, ground) in cells {
            let a = to_screen([origin[0], origin[2]]);
            let b = to_screen([origin[0] + grid.square_size, origin[2] + grid.square_size]);
            let lift = (y - base) as f32 / (top - base).max(1) as f32;
            let gray = if ground { 40 } else { 55 + (lift * 70.0) as u8 };
            let cell = Rect::from_two_pos(a, b);
            painter.rect_filled(cell, 0.0, Color32::from_gray(gray));
            painter.rect_stroke(cell, 0.0, Stroke::new(1.0, Color32::from_gray(gray.saturating_add(14))), egui::StrokeKind::Inside);
        }
    }

    fn draw_markers(&self, painter: &egui::Painter, session: &Session, run: usize, to_screen: &impl Fn([f32; 2]) -> Pos2) {
        let samples = &session.ghost(run).samples;
        let at = |t: i32| position_at(samples, t as f32).map(|p| to_screen([p.x, p.z]));
        if let Some(track) = &session.runs[run].track {
            if self.show_brakes {
                for hold in track.holds(Channel::Brake) {
                    if let Some(p) = at(hold.start_ms) {
                        painter.circle_filled(p, 4.5, BRAKE);
                    }
                    if let Some(p) = at(hold.end_ms) {
                        painter.circle_stroke(p, 4.5, Stroke::new(1.5, BRAKE));
                    }
                }
            }
            if self.show_lifts {
                // A lift is the gap between gas holds: where the gas was released
                for hold in track.holds(Channel::Gas).iter().filter(|h| h.end_ms < track.end_ms) {
                    if let Some(p) = at(hold.end_ms) {
                        painter.add(Shape::convex_polygon(vec![p + Vec2::new(0.0, -5.0), p + Vec2::new(4.5, 3.5), p + Vec2::new(-4.5, 3.5)], LIFT, Stroke::NONE));
                    }
                }
            }
            for &t in &track.respawns {
                if let Some(p) = at(t) {
                    let s = Stroke::new(2.0, RESPAWN);
                    painter.line_segment([p + Vec2::new(-4.0, -4.0), p + Vec2::new(4.0, 4.0)], s);
                    painter.line_segment([p + Vec2::new(-4.0, 4.0), p + Vec2::new(4.0, -4.0)], s);
                }
            }
        }
        if self.show_checkpoints {
            for (i, cp) in session.ghost(run).checkpoints.iter().enumerate() {
                let Some(t) = cp.time_ms else { continue };
                if let Some(p) = at(t) {
                    let d = 5.0;
                    painter.add(Shape::convex_polygon(vec![p + Vec2::new(0.0, -d), p + Vec2::new(d, 0.0), p + Vec2::new(0.0, d), p + Vec2::new(-d, 0.0)], CHECKPOINT, Stroke::new(1.0, Color32::BLACK)));
                    painter.text(p + Vec2::new(8.0, -8.0), Align2::LEFT_BOTTOM, format!("{} {}", i + 1, format_race_time(t as i64)), FontId::proportional(11.0), CHECKPOINT);
                }
            }
        }
    }
}

fn gain_legend(ui: &mut egui::Ui, session: &Session, reference: usize, scale: f32) {
    ui.horizontal(|ui| {
        ui.label(format!("losing {scale:.0} ms/s"));
        let (rect, _) = ui.allocate_exact_size(Vec2::new(140.0, 10.0), Sense::hover());
        for i in 0..28 {
            let x0 = rect.left() + i as f32 * 5.0;
            let color = gain_color(-scale + 2.0 * scale * i as f32 / 27.0, scale);
            ui.painter().rect_filled(Rect::from_min_size(Pos2::new(x0, rect.top()), Vec2::new(5.0, 10.0)), 0.0, color);
        }
        ui.label(format!("gaining {scale:.0} ms/s against"));
        color_swatch(ui, session.runs[reference].color);
        ui.label(&session.runs[reference].label);
    });
}

fn legend(ui: &mut egui::Ui, session: &Session, focus: usize) {
    let samples = &session.ghost(focus).samples;
    let (lo, hi) = samples.iter().fold((f32::MAX, 0.0f32), |(lo, hi), s| (lo.min(s.speed_kmh), hi.max(s.speed_kmh)));
    ui.horizontal(|ui| {
        color_swatch(ui, session.runs[focus].color);
        ui.label(format!("{} km/h", lo.min(hi).round()));
        let (rect, _) = ui.allocate_exact_size(Vec2::new(140.0, 10.0), Sense::hover());
        for i in 0..28 {
            let x0 = rect.left() + i as f32 * 5.0;
            let color = speed_color(lo + (hi - lo) * i as f32 / 27.0, lo, hi);
            ui.painter().rect_filled(Rect::from_min_size(Pos2::new(x0, rect.top()), Vec2::new(5.0, 10.0)), 0.0, color);
        }
        ui.label(format!("{} km/h", hi.round()));
        ui.separator();
        ui.colored_label(BRAKE, "\u{25CF} brake  \u{25CB} release");
        ui.colored_label(LIFT, "\u{25B2} gas lift");
        ui.colored_label(RESPAWN, "x respawn");
        ui.colored_label(CHECKPOINT, "\u{25C6} checkpoint");
    });
}
