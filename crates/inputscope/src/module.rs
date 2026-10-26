// ABOUTME: InputScopeModule UI: the piano roll (drag to pan, wheel to zoom, click to seek to a tick), per-channel
// ABOUTME: statistics, and the hold table; a second run can be aligned by race time or by position on the track.

use crate::timeview::TimeView;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use tmreplay::format_race_time;
use std::sync::Arc;
use tmreplay::gap::{matching_press, TimeWarp};
use tmreplay::track::{tick_of, Channel, Hold, InputTrack, TICK_MS};
use tmtools_core::{color_swatch, Module, Session};

const ROW_H: f32 = 18.0;
const GROUP_GAP: f32 = 6.0;
const GUTTER: f32 = 96.0;
const AXIS_H: f32 = 20.0;
const GAS: Color32 = Color32::from_rgb(63, 182, 122);
const BRAKE: Color32 = Color32::from_rgb(229, 72, 77);
const STEER: Color32 = Color32::from_rgb(255, 210, 63);
const RESPAWN: Color32 = Color32::from_rgb(200, 90, 255);
const CHECKPOINT: Color32 = Color32::from_rgb(90, 180, 255);
// A press in the other run counts as the same press when within this much of reaching the spot
const MATCH_WINDOW_MS: f32 = 400.0;

fn channel_color(channel: Channel) -> Color32 {
    match channel {
        Channel::Left | Channel::Right => STEER,
        Channel::Gas => GAS,
        Channel::Brake => BRAKE,
    }
}

fn seconds(ms: i32) -> String {
    format!("{:.2} s", ms as f64 / 1000.0)
}

/// "0.04 earlier", "0.04 later", or "same tick" for an offset in milliseconds
fn offset_text(offset_ms: f32) -> String {
    let ticks = (offset_ms / TICK_MS as f32).round() as i32;
    match ticks {
        0 => "same tick".into(),
        t if t < 0 => format!("{:.2} earlier", -t as f32 / 100.0),
        t => format!("{:.2} later", t as f32 / 100.0),
    }
}

pub struct InputScopeModule {
    view: TimeView,
    follow: bool,
    compare: Option<usize>,
    channels: [bool; 4],
    fit_pending: bool,
    center_pending: bool,
    /// Draw the compared run where its car was on the track, through a time warp, instead of at its race time
    align_position: bool,
    /// The warp from the compared run's times to the focused run's, for a pair of runs
    warp: Option<((usize, usize), Arc<TimeWarp>)>,
}

fn run_identity(session: &Session, run: usize) -> usize {
    (Arc::as_ptr(&session.files[session.runs[run].file].bytes) as usize) ^ session.runs[run].ghost
}

impl Default for InputScopeModule {
    fn default() -> Self {
        Self { view: TimeView::default(), follow: true, compare: None, channels: [true; 4], fit_pending: false, center_pending: false, align_position: false, warp: None }
    }
}

impl Module for InputScopeModule {
    fn name(&self) -> &str {
        "Input Analyzer"
    }

    fn description(&self) -> &str {
        "Tick-exact piano roll of the inputs, hold statistics, and press timing against another run"
    }

    fn ui(&mut self, ui: &mut egui::Ui, session: &mut Session, _wgpu: Option<&tmtools_core::RenderState>) {
        let Some(focus) = session.focus else {
            ui.centered_and_justified(|ui| ui.label("Open a replay to analyze its inputs."));
            return;
        };
        if self.compare.is_some_and(|c| c >= session.runs.len() || c == focus) {
            self.compare = None;
        }

        self.toolbar(ui, session, focus);
        ui.add_space(6.0);

        let mut rows: Vec<usize> = vec![focus];
        rows.extend(self.compare);
        rows.retain(|&r| session.runs[r].track.is_some());
        if rows.is_empty() {
            ui.label("This run has no recorded inputs.");
            return;
        }

        if let Some(seek) = self.roll(ui, session, &rows) {
            session.playback.playing = false;
            session.seek(seek);
        }
        ui.add_space(8.0);

        let seek = ui
            .columns(2, |cols| {
                self.stats(&mut cols[0], session, &rows);
                self.hold_table(&mut cols[1], session, focus)
            });
        if let Some(t) = seek {
            session.playback.playing = false;
            session.seek(t as f64);
        }
    }
}

impl InputScopeModule {
    fn toolbar(&mut self, ui: &mut egui::Ui, session: &Session, focus: usize) {
        tmtools_core::toolbar(ui, |ui| {
            color_swatch(ui, session.runs[focus].color);
            ui.strong(&session.runs[focus].label);
            ui.separator();
            ui.label("Compare with");
            let selected = self.compare.map_or("nothing".to_owned(), |c| session.runs[c].label.clone());
            egui::ComboBox::from_id_salt("inputscope-compare").selected_text(selected).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.compare, None, "nothing");
                for (i, run) in session.runs.iter().enumerate() {
                    if i != focus && run.track.is_some() {
                        ui.selectable_value(&mut self.compare, Some(i), &run.label);
                    }
                }
            });
            ui.add_enabled(self.compare.is_some(), egui::Checkbox::new(&mut self.align_position, "Align by track position"))
                .on_hover_text("Draw the compared run's inputs where its car was on the track, so both drivers' inputs for the same corner line up");
            ui.separator();
            ui.checkbox(&mut self.follow, "Follow playhead");
            if ui.button("Fit run").clicked() {
                self.fit_pending = true;
            }
            if ui.button("Ticks").on_hover_text("Zoom in until every 10 ms tick is a column").clicked() {
                // A tick every 12 pixels, centered on the playhead
                self.view.zoom_about((10.0 / 12.0) / self.view.ms_per_px, 0.0);
                self.center_pending = true;
            }
            ui.label(egui::RichText::new("Drag to pan (stops following), wheel to zoom, click to seek").weak().small());
        });
    }

    /// Draws the piano roll; returns a time to seek to when clicked
    fn roll(&mut self, ui: &mut egui::Ui, session: &Session, runs: &[usize]) -> Option<f64> {
        let height = runs.len() as f32 * (4.0 * ROW_H + GROUP_GAP) + AXIS_H;
        let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click_and_drag());
        let lane = Rect::from_min_max(Pos2::new(rect.left() + GUTTER, rect.top()), rect.right_bottom());
        let width = lane.width() as f64;

        if self.fit_pending {
            self.view.fit(session.ghost(runs[0]).duration_ms() as f64, width);
            self.fit_pending = false;
            self.follow = false;
        }
        if self.center_pending {
            self.view.start_ms = session.playback.time_ms - width / 2.0 * self.view.ms_per_px;
            self.center_pending = false;
        }
        if response.dragged() {
            self.view.pan_px(response.drag_delta().x as f64);
            self.follow = false;
        }
        if response.hovered() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
            let anchor = response.hover_pos().map_or(width / 2.0, |p| (p.x - lane.left()) as f64);
            if scroll.y != 0.0 {
                self.view.zoom_about((-scroll.y as f64 * 0.004).exp(), anchor);
            }
            if zoom != 1.0 {
                self.view.zoom_about(1.0 / zoom as f64, anchor);
            }
            if scroll.x != 0.0 {
                self.view.pan_px(scroll.x as f64);
            }
        }
        // Following keeps the playhead in view however it moves: playing, stepping, or seeking from another module
        if self.follow {
            self.view.follow(session.playback.time_ms, width);
        }

        // With position alignment, the compared run's times map onto the focused run's through the warp
        let warp = self.warp_for(session, runs);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_gray(22));
        let x = |t: f64| lane.left() + self.view.x_of(t) as f32;
        let (from, to) = (self.view.start_ms, self.view.time_at(width));
        let small = FontId::proportional(10.5);
        let rows_bottom = rect.bottom() - AXIS_H;

        // Grid: every tick when they are far enough apart, labeled steps above that
        let step = self.view.label_step_ms(70.0);
        if self.view.tick_px() >= 4.0 {
            let mut t = (from / 10.0).ceil() as i64 * 10;
            while (t as f64) <= to {
                painter.line_segment([Pos2::new(x(t as f64), rect.top()), Pos2::new(x(t as f64), rows_bottom)], Stroke::new(1.0, Color32::from_gray(34)));
                t += 10;
            }
        }
        let mut t = (from / step as f64).ceil() as i64 * step;
        while (t as f64) <= to {
            let px = x(t as f64);
            painter.line_segment([Pos2::new(px, rect.top()), Pos2::new(px, rows_bottom)], Stroke::new(1.0, Color32::from_gray(60)));
            if t >= 0 {
                painter.text(Pos2::new(px, rows_bottom + 3.0), Align2::CENTER_TOP, format_race_time(t), small.clone(), Color32::from_gray(170));
            }
            t += step;
        }

        let mut top = rect.top();
        for &run in runs {
            let track = session.runs[run].track.as_ref().expect("rows have tracks");
            let warp = warp.as_ref().filter(|_| run != runs[0]);
            let to_view = |t: f64| warp.map_or(t, |w| w.to_focus(t as f32) as f64);
            let x = |t: f64| x(to_view(t));
            let (from, to) = warp.map_or((from, to), |w| (w.to_other(from as f32) as f64, w.to_other(to as f32) as f64));
            let group = Rect::from_min_max(Pos2::new(rect.left(), top), Pos2::new(rect.right(), top + 4.0 * ROW_H));
            painter.rect_filled(Rect::from_min_size(group.min, Vec2::new(4.0, group.height())), 0.0, session.runs[run].color);

            for (row, channel) in Channel::ALL.into_iter().enumerate() {
                let row_rect = Rect::from_min_size(Pos2::new(lane.left(), top + row as f32 * ROW_H), Vec2::new(lane.width(), ROW_H));
                if row % 2 == 0 {
                    painter.rect_filled(row_rect, 0.0, Color32::from_white_alpha(6));
                }
                painter.text(Pos2::new(rect.left() + 10.0, row_rect.center().y), Align2::LEFT_CENTER, channel.label(), small.clone(), Color32::from_gray(200));
                draw_channel(&painter, track, channel, row_rect, &x, from, to, &small);
            }

            for &respawn in &track.respawns {
                let px = x(respawn as f64);
                painter.line_segment([Pos2::new(px, group.top()), Pos2::new(px, group.bottom())], Stroke::new(2.0, RESPAWN));
            }
            for cp in session.ghost(run).checkpoints.iter().filter_map(|c| c.time_ms) {
                let px = x(cp as f64);
                painter.line_segment([Pos2::new(px, group.top()), Pos2::new(px, group.bottom())], Stroke::new(1.0, CHECKPOINT));
            }
            top += 4.0 * ROW_H + GROUP_GAP;
        }

        // The current tick spans from the playhead's tick to the next
        let tick = session.tick_ms() as f64;
        let tick_rect = Rect::from_min_max(Pos2::new(x(tick), rect.top()), Pos2::new(x(tick + TICK_MS as f64).max(x(tick) + 1.0), rows_bottom));
        painter.rect_filled(tick_rect, 0.0, Color32::from_white_alpha(28));
        painter.line_segment([Pos2::new(x(session.playback.time_ms), rect.top()), Pos2::new(x(session.playback.time_ms), rect.bottom())], Stroke::new(1.5, Color32::WHITE));

        // Hover: the tick under the pointer and every input's state there
        if let Some(pos) = response.hover_pos().filter(|p| p.x >= lane.left()) {
            let at = tick_of(self.view.time_at((pos.x - lane.left()) as f64).max(0.0));
            painter.line_segment([Pos2::new(x(at as f64), rect.top()), Pos2::new(x(at as f64), rows_bottom)], Stroke::new(1.0, Color32::from_white_alpha(90)));
            response.clone().on_hover_ui_at_pointer(|ui| {
                ui.strong(format!("{}  (tick {})", format_race_time(at as i64), at / TICK_MS));
                for &run in runs {
                    let track = session.runs[run].track.as_ref().expect("rows have tracks");
                    // A warped run is read where its car was at this spot
                    let (run_at, note) = match warp.as_ref().filter(|_| run != runs[0]) {
                        Some(w) => {
                            let t = tick_of(w.to_other(at as f32).max(0.0) as f64);
                            (t, format!(" (at {} on its clock)", format_race_time(t as i64)))
                        }
                        None => (at, String::new()),
                    };
                    ui.horizontal(|ui| {
                        color_swatch(ui, session.runs[run].color);
                        ui.label(format!("{}{note}", hover_summary(track, run_at)));
                    });
                }
            });
        }

        if response.clicked() {
            let pos = response.interact_pointer_pos()?;
            if pos.x >= lane.left() {
                return Some(tick_of(self.view.time_at((pos.x - lane.left()) as f64).max(0.0)) as f64);
            }
        }
        None
    }

    /// The warp for the compared run when aligning by position; None otherwise or when the runs never meet
    fn warp_for(&mut self, session: &Session, runs: &[usize]) -> Option<Arc<TimeWarp>> {
        if !self.align_position || runs.len() < 2 {
            return None;
        }
        let key = (run_identity(session, runs[0]), run_identity(session, runs[1]));
        if self.warp.as_ref().is_none_or(|(k, _)| *k != key) {
            let warp = TimeWarp::new(&session.ghost(runs[0]).samples, &session.ghost(runs[1]).samples);
            self.warp = Some((key, Arc::new(warp)));
        }
        self.warp.as_ref().map(|(_, w)| w.clone()).filter(|w| !w.is_empty())
    }

    fn stats(&mut self, ui: &mut egui::Ui, session: &Session, runs: &[usize]) {
        ui.heading("Holds");
        for &run in runs {
            let track = session.runs[run].track.as_ref().expect("rows have tracks");
            ui.horizontal(|ui| {
                color_swatch(ui, session.runs[run].color);
                ui.label(&session.runs[run].label);
            });
            egui::Grid::new(("inputscope-stats", run)).striped(true).num_columns(6).show(ui, |ui| {
                for header in ["", "Holds", "Total", "Shortest", "Longest", "Mean"] {
                    ui.strong(header);
                }
                ui.end_row();
                for channel in Channel::ALL {
                    let s = track.stats(channel);
                    ui.colored_label(channel_color(channel), channel.label());
                    ui.label(s.count.to_string());
                    ui.label(seconds(s.total_ms));
                    ui.label(s.shortest_ms.map_or("-".into(), seconds));
                    ui.label(s.longest_ms.map_or("-".into(), seconds));
                    ui.label(s.mean_ms.map_or("-".into(), |m| format!("{:.3} s", m / 1000.0)));
                    ui.end_row();
                }
            });
            let steering = if track.analog_steer.is_some() { "analog" } else { "digital" };
            ui.label(format!(
                "Steering: {steering}, {:.1} changes per second. Respawns: {}.",
                track.steer_changes_per_second(),
                track.respawns.len()
            ));
            ui.add_space(8.0);
        }
    }

    /// The focused run's holds in time order; returns a time to seek to when a row's time is clicked
    fn hold_table(&mut self, ui: &mut egui::Ui, session: &Session, focus: usize) -> Option<i32> {
        let track = session.runs[focus].track.as_ref()?;
        let other = self.compare.and_then(|c| session.runs[c].track.as_ref().map(|t| (c, t)));
        ui.horizontal(|ui| {
            ui.heading("Hold table");
            for (i, channel) in Channel::ALL.into_iter().enumerate() {
                ui.checkbox(&mut self.channels[i], channel.label());
            }
        });
        if other.is_some() {
            ui.label(egui::RichText::new("Press and release vs other: when the other run did the same at the spot where this run did it").small().weak());
        }

        let mut holds: Vec<(Channel, Hold)> = Channel::ALL
            .into_iter()
            .enumerate()
            .filter(|&(i, _)| self.channels[i])
            .flat_map(|(_, c)| track.holds(c).iter().map(move |&h| (c, h)))
            .collect();
        holds.sort_by_key(|&(c, h)| (h.start_ms, c as u8));

        let focus_samples = &session.ghost(focus).samples;
        let mut seek = None;
        egui::ScrollArea::vertical().id_salt("inputscope-holds").auto_shrink(false).show(ui, |ui| {
            let columns = if other.is_some() { 7 } else { 5 };
            egui::Grid::new("inputscope-hold-grid").striped(true).num_columns(columns).show(ui, |ui| {
                for header in ["", "Pressed", "Released", "Length", "Ticks"] {
                    ui.strong(header);
                }
                if other.is_some() {
                    ui.strong("Press vs other");
                    ui.strong("Release vs other");
                }
                ui.end_row();
                let now = session.tick_ms();
                for (channel, hold) in holds {
                    let current = hold.start_ms <= now && now < hold.end_ms;
                    let name = egui::RichText::new(channel.label()).color(channel_color(channel));
                    ui.label(if current { name.strong().underline() } else { name });
                    if ui.link(format_race_time(hold.start_ms as i64)).clicked() {
                        seek = Some(hold.start_ms);
                    }
                    if ui.link(format_race_time(hold.end_ms as i64)).clicked() {
                        seek = Some(hold.end_ms);
                    }
                    ui.label(seconds(hold.duration_ms()));
                    ui.label(hold.ticks().to_string());
                    if let Some((c, other_track)) = other {
                        let other_samples = &session.ghost(c).samples;
                        let presses = other_track.holds(channel).iter().map(|h| h.start_ms);
                        let releases = other_track.holds(channel).iter().map(|h| h.end_ms);
                        let press = matching_press(focus_samples, hold.start_ms, other_samples, presses, MATCH_WINDOW_MS);
                        let release = matching_press(focus_samples, hold.end_ms, other_samples, releases, MATCH_WINDOW_MS);
                        ui.label(press.map_or("-".into(), |m| offset_text(m.offset_ms)));
                        ui.label(release.map_or("-".into(), |m| offset_text(m.offset_ms)));
                    }
                    ui.end_row();
                }
            });
        });
        seek
    }
}

/// Fills a channel's row: each step to the share held, then each hold's length where it fits
#[allow(clippy::too_many_arguments)]
fn draw_channel(painter: &egui::Painter, track: &InputTrack, channel: Channel, row: Rect, x: &impl Fn(f64) -> f32, from: f64, to: f64, font: &FontId) {
    let color = channel_color(channel);
    let steps = track.steps(channel);
    let first = steps.partition_point(|s| (s.time_ms as f64) <= from).saturating_sub(1);
    let end_of_run = track.end_ms;
    for (i, step) in steps.iter().enumerate().skip(first) {
        if step.time_ms as f64 > to {
            break;
        }
        if step.value <= 0.0 {
            continue;
        }
        let next = steps.get(i + 1).map_or(end_of_run, |s| s.time_ms);
        let left = x(step.time_ms as f64).max(row.left());
        let right = x(next as f64).min(row.right());
        if right <= left {
            continue;
        }
        let bar = (row.height() - 4.0) * step.value;
        painter.rect_filled(Rect::from_min_max(Pos2::new(left, row.bottom() - 2.0 - bar), Pos2::new(right, row.bottom() - 2.0)), 1.0, color);
    }

    for hold in track.holds(channel) {
        if (hold.end_ms as f64) < from || (hold.start_ms as f64) > to {
            continue;
        }
        let left = x(hold.start_ms as f64).max(row.left());
        let right = x(hold.end_ms as f64).min(row.right());
        let text = format!("{:.2}", hold.duration_ms() as f64 / 1000.0);
        let galley = painter.layout_no_wrap(text, font.clone(), Color32::WHITE);
        if right - left < galley.size().x + 6.0 {
            continue;
        }
        let pos = Pos2::new((left + right) / 2.0 - galley.size().x / 2.0, row.center().y - galley.size().y / 2.0);
        // A dark plate keeps the length readable over full and partial bars alike
        painter.rect_filled(Rect::from_min_size(pos - Vec2::new(2.0, 0.0), galley.size() + Vec2::new(4.0, 0.0)), 2.0, Color32::from_black_alpha(170));
        painter.galley(pos, galley, Color32::WHITE);
    }
}

fn hover_summary(track: &InputTrack, at: i32) -> String {
    let steer = track.steer_at(at);
    let side = if steer > 0.0 { "right" } else if steer < 0.0 { "left" } else { "center" };
    let steer_text = match track.analog_steer_at(at) {
        Some(raw) => format!("steer {side} {}% ({raw})", (steer.abs() * 100.0).round()),
        None => format!("steer {side}"),
    };
    let pedal = |c: Channel| {
        let s = track.stretch_at(c, at);
        let state = if s.held { "on" } else { "off" };
        format!("{} {state} {}", c.label().to_lowercase(), seconds(at - s.since_ms))
    };
    format!("{steer_text}, {}, {}", pedal(Channel::Gas), pedal(Channel::Brake))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_read_in_ticks() {
        assert_eq!(offset_text(-40.0), "0.04 earlier");
        assert_eq!(offset_text(3.0), "same tick");
        assert_eq!(offset_text(120.0), "0.12 later");
    }
}
