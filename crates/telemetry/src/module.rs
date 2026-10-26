// ABOUTME: TelemetryModule UI: plots chosen by checkboxes, stacked with a shared time axis and cursor, the playhead
// ABOUTME: drawn in each, and gap series cached per pair of runs since they cost a path search per sample.

use crate::series::{gap_series, step_points};
use egui::Color32;
use egui_plot::{Legend, Line, LineStyle, Plot, PlotPoints, VLine};
use std::collections::HashMap;
use tmreplay::format_race_time;
use tmreplay::track::Channel;
use tmtools_core::{setting, Module, Session, Settings};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Graph {
    Speed,
    Gap,
    Steering,
    Pedals,
    Rpm,
    Wheels,
}

impl Graph {
    const ALL: [Graph; 6] = [Graph::Speed, Graph::Gap, Graph::Steering, Graph::Pedals, Graph::Rpm, Graph::Wheels];

    fn title(self) -> &'static str {
        match self {
            Graph::Speed => "Speed (km/h)",
            Graph::Gap => "Gap to focus (s, + behind)",
            Graph::Steering => "Steering (inputs; dashed: car)",
            Graph::Pedals => "Gas up, brake down",
            Graph::Rpm => "RPM",
            Graph::Wheels => "Wheels sliding; dashed: in the air",
        }
    }
}

/// Identifies a run by its file's bytes and ghost index, which survive files being closed around it
type RunKey = (usize, usize);

pub struct TelemetryModule {
    shown: [bool; 6],
    follow: bool,
    gaps: HashMap<(RunKey, RunKey), Vec<Vec<[f64; 2]>>>,
}

impl Default for TelemetryModule {
    fn default() -> Self {
        Self { shown: [true, true, true, true, false, false], follow: false, gaps: HashMap::new() }
    }
}

fn run_key(session: &Session, run: usize) -> RunKey {
    let r = &session.runs[run];
    (std::sync::Arc::as_ptr(&session.files[r.file].bytes) as usize, r.ghost)
}

impl Module for TelemetryModule {
    fn name(&self) -> &str {
        "Telemetry Graphs"
    }

    fn description(&self) -> &str {
        "Speed, gap, steering, pedals, RPM, and wheel contact over time for every visible run"
    }

    fn save_settings(&self, s: &mut Settings) {
        for (i, shown) in self.shown.iter().enumerate() {
            s.insert(format!("telemetry.graph{i}"), shown.to_string());
        }
        s.insert("telemetry.follow".into(), self.follow.to_string());
    }

    fn load_settings(&mut self, s: &Settings) {
        for (i, shown) in self.shown.iter_mut().enumerate() {
            *shown = setting(s, &format!("telemetry.graph{i}")).unwrap_or(*shown);
        }
        self.follow = setting(s, "telemetry.follow").unwrap_or(self.follow);
    }

    fn ui(&mut self, ui: &mut egui::Ui, session: &mut Session, _wgpu: Option<&tmtools_core::RenderState>) {
        let Some(focus) = session.focus else {
            ui.centered_and_justified(|ui| ui.label("Open a replay to plot its telemetry."));
            return;
        };

        tmtools_core::toolbar(ui, |ui| {
            for (i, graph) in Graph::ALL.into_iter().enumerate() {
                ui.checkbox(&mut self.shown[i], graph.title());
            }
            ui.separator();
            ui.checkbox(&mut self.follow, "Follow playhead");
            ui.label(egui::RichText::new("Drag to pan, scroll to zoom, double-click to reset, click to seek").weak().small());
        });
        ui.add_space(4.0);

        let graphs: Vec<Graph> = Graph::ALL.into_iter().enumerate().filter(|&(i, _)| self.shown[i]).map(|(_, g)| g).collect();
        if graphs.is_empty() {
            return;
        }
        let height = ((ui.available_height() - graphs.len() as f32 * 22.0) / graphs.len() as f32).max(90.0);
        let runs: Vec<usize> = session.visible_runs().collect();
        self.cache_gaps(session, focus, &runs);

        let now_s = session.playback.time_ms / 1000.0;
        let duration_s = session.duration_ms() / 1000.0;
        let mut seek = None;

        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for graph in &graphs {
                ui.label(egui::RichText::new(graph.title()).strong());
                let plot = Plot::new(("telemetry", *graph as u8))
                    .height(height)
                    .link_axis("telemetry-x", [true, false])
                    .link_cursor("telemetry-x", [true, false])
                    .default_x_bounds(0.0, duration_s.max(1.0))
                    .y_axis_min_width(48.0)
                    .legend(Legend::default())
                    .x_axis_formatter(|mark, _| format_race_time((mark.value * 1000.0) as i64))
                    .label_formatter(|pos| {
                        let (name, p) = match pos {
                            egui_plot::HoverPosition::NearDataPoint { plot_name, position, .. } => (*plot_name, *position),
                            egui_plot::HoverPosition::Elsewhere { position } => ("", *position),
                        };
                        Some(format!("{name}\n{}\n{:.3}", format_race_time((p.x * 1000.0) as i64), p.y))
                    });
                let follow = self.follow && session.playback.playing;
                let response = plot.show(ui, |plot_ui| {
                    self.draw(plot_ui, *graph, session, focus, &runs);
                    plot_ui.vline(VLine::new("", now_s).color(Color32::WHITE).width(1.5));
                    if follow {
                        let bounds = plot_ui.plot_bounds();
                        let half = bounds.width() / 2.0;
                        plot_ui.set_plot_bounds_x(now_s - half..=now_s + half);
                    }
                    plot_ui.pointer_coordinate()
                });
                if response.response.clicked() {
                    seek = response.inner.map(|p| p.x * 1000.0);
                }
            }
        });

        if let Some(t) = seek {
            session.playback.playing = false;
            session.seek(t);
        }
    }
}

impl TelemetryModule {
    fn cache_gaps(&mut self, session: &Session, focus: usize, runs: &[usize]) {
        // Drop pairs whose runs were closed, so a later file can never pick up a stale series
        let live: std::collections::HashSet<RunKey> = (0..session.runs.len()).map(|r| run_key(session, r)).collect();
        self.gaps.retain(|(a, b), _| live.contains(a) && live.contains(b));
        let focus_key = run_key(session, focus);
        for &run in runs.iter().filter(|&&r| r != focus) {
            let key = (focus_key, run_key(session, run));
            self.gaps.entry(key).or_insert_with(|| gap_series(&session.ghost(focus).samples, &session.ghost(run).samples));
        }
    }

    fn draw(&self, plot_ui: &mut egui_plot::PlotUi, graph: Graph, session: &Session, focus: usize, runs: &[usize]) {
        for &run in runs {
            let r = &session.runs[run];
            let ghost = session.ghost(run);
            let color = r.color;
            let name = r.label.as_str();
            let width = if run == focus { 2.0 } else { 1.3 };
            let sampled = |f: &dyn Fn(&tmreplay::Sample) -> f64| -> PlotPoints<'static> {
                ghost.samples.iter().map(|s| [s.time_ms as f64 / 1000.0, f(s)]).collect::<Vec<_>>().into()
            };
            match graph {
                Graph::Speed => plot_ui.line(Line::new(name, sampled(&|s| s.speed_kmh as f64)).color(color).width(width)),
                Graph::Gap => {
                    if run == focus {
                        plot_ui.line(Line::new(name, PlotPoints::from(vec![[0.0, 0.0], [ghost.duration_ms() as f64 / 1000.0, 0.0]])).color(color).width(1.0));
                    } else if let Some(pieces) = self.gaps.get(&(run_key(session, focus), run_key(session, run))) {
                        for piece in pieces {
                            plot_ui.line(Line::new(name, PlotPoints::from(piece.clone())).color(color).width(width));
                        }
                    }
                }
                Graph::Steering => {
                    if let Some(track) = &r.track {
                        // Right minus left, as one step line from -1 to 1
                        let mut steps: Vec<tmreplay::track::Step> = track
                            .steps(Channel::Left)
                            .iter()
                            .chain(track.steps(Channel::Right))
                            .map(|s| tmreplay::track::Step { time_ms: s.time_ms, value: track.steer_at(s.time_ms) })
                            .collect();
                        steps.sort_by_key(|s| s.time_ms);
                        steps.dedup_by_key(|s| s.time_ms);
                        plot_ui.line(Line::new(name, PlotPoints::from(step_points(&steps, track.end_ms, 1.0))).color(color).width(width));
                    }
                    if run == focus {
                        let car = format!("{name} car");
                        plot_ui.line(Line::new(car, sampled(&|s| s.steer as f64)).color(color.gamma_multiply(0.6)).style(LineStyle::dashed_dense()).width(1.0));
                    }
                }
                Graph::Pedals => {
                    if let Some(track) = &r.track {
                        plot_ui.line(Line::new(name, PlotPoints::from(step_points(track.steps(Channel::Gas), track.end_ms, 1.0))).color(color).width(width));
                        let brake = format!("{name} brake");
                        plot_ui.line(Line::new(brake, PlotPoints::from(step_points(track.steps(Channel::Brake), track.end_ms, -1.0))).color(color).width(width));
                    }
                }
                Graph::Rpm => plot_ui.line(Line::new(name, sampled(&|s| s.rpm as f64)).color(color).width(width)),
                Graph::Wheels => {
                    plot_ui.line(Line::new(name, sampled(&|s| s.sliding.count_ones() as f64)).color(color).width(width));
                    let air = format!("{name} in the air");
                    plot_ui.line(Line::new(air, sampled(&|s| (4 - s.grounded.count_ones()) as f64)).color(color).style(LineStyle::dashed_dense()).width(1.0));
                }
            }
        }
    }
}
