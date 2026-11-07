// ABOUTME: TelemetryModule UI: plots chosen by checkboxes, stacked on a shared time or distance axis with the playhead in
// ABOUTME: each and, by distance, each rival marked where it was at the same spot. Gap series are cached per pair of runs.

use crate::series::{gap_series, step_points};
use egui::Color32;
use egui_plot::{Legend, Line, LineStyle, Plot, PlotPoints, Points, VLine};
use std::collections::HashMap;
use tmreplay::distance::{cumulative_distance, distance_at_time, time_at_distance};
use tmreplay::format_race_time;
use tmreplay::gap::{sample_at, speed_at};
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
    /// Plot against distance driven instead of time, so runs line up by where they are on the track
    by_distance: bool,
    gaps: HashMap<(RunKey, RunKey), Vec<Vec<[f64; 2]>>>,
    /// Distance driven at each sample, per run
    totals: HashMap<RunKey, Vec<f32>>,
}

impl Default for TelemetryModule {
    fn default() -> Self {
        Self { shown: [true, true, true, true, false, false], follow: false, by_distance: false, gaps: HashMap::new(), totals: HashMap::new() }
    }
}

fn run_key(session: &Session, run: usize) -> RunKey {
    let r = &session.runs()[run];
    (std::sync::Arc::as_ptr(&session.files()[r.file].bytes) as usize, r.ghost)
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
        s.insert("telemetry.by_distance".into(), self.by_distance.to_string());
    }

    fn load_settings(&mut self, s: &Settings) {
        for (i, shown) in self.shown.iter_mut().enumerate() {
            *shown = setting(s, &format!("telemetry.graph{i}")).unwrap_or(*shown);
        }
        self.follow = setting(s, "telemetry.follow").unwrap_or(self.follow);
        self.by_distance = setting(s, "telemetry.by_distance").unwrap_or(self.by_distance);
    }

    fn ui(&mut self, ui: &mut egui::Ui, session: &mut Session, _wgpu: Option<&tmtools_core::RenderState>) {
        let Some(focus) = session.focus() else {
            ui.centered_and_justified(|ui| ui.label("Open a replay to plot its telemetry."));
            return;
        };

        tmtools_core::toolbar(ui, |ui| {
            for (i, graph) in Graph::ALL.into_iter().enumerate() {
                ui.checkbox(&mut self.shown[i], graph.title());
            }
            ui.separator();
            ui.label("X axis");
            ui.selectable_value(&mut self.by_distance, false, "Time");
            ui.selectable_value(&mut self.by_distance, true, "Distance").on_hover_text("Line runs up by where they are on the track; dots mark where each rival was at the focused run's spot");
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
        for &run in &runs {
            self.totals.entry(run_key(session, run)).or_insert_with(|| cumulative_distance(&session.ghost(run).samples));
        }

        let by_distance = self.by_distance;
        let totals = &self.totals;
        // Where a moment of a run sits on the x axis: seconds, or kilometers driven
        let x_of = |run: usize, time_ms: f64| -> f64 {
            match totals.get(&run_key(session, run)) {
                Some(t) if by_distance => distance_at_time(&session.ghost(run).samples, t, time_ms as f32) as f64 / 1000.0,
                _ => time_ms / 1000.0,
            }
        };
        let focus_totals = totals.get(&run_key(session, focus)).cloned().unwrap_or_default();
        let now_x = x_of(focus, session.playback().time_ms);
        let end_x = if by_distance { focus_totals.last().copied().unwrap_or(1.0) as f64 / 1000.0 } else { session.duration_ms() / 1000.0 };
        let axis_text = move |x: f64| if by_distance { format!("{x:.2} km") } else { format_race_time((x * 1000.0) as i64) };
        let mut seek = None;

        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for graph in &graphs {
                ui.label(egui::RichText::new(graph.title()).strong());
                // Each axis keeps its own zoom, so switching doesn't carry seconds over as kilometers
                let link = if by_distance { "telemetry-distance" } else { "telemetry-time" };
                let plot = Plot::new(("telemetry", *graph as u8, by_distance))
                    .height(height)
                    .link_axis(link, [true, false])
                    .link_cursor(link, [true, false])
                    .default_x_bounds(0.0, end_x.max(if by_distance { 0.01 } else { 1.0 }))
                    .y_axis_min_width(48.0)
                    .legend(Legend::default())
                    .x_axis_formatter(move |mark, _| axis_text(mark.value))
                    .label_formatter(move |pos| {
                        let (name, p) = match pos {
                            egui_plot::HoverPosition::NearDataPoint { plot_name, position, .. } => (*plot_name, *position),
                            egui_plot::HoverPosition::Elsewhere { position } => ("", *position),
                        };
                        Some(format!("{name}\n{}\n{:.3}", axis_text(p.x), p.y))
                    });
                let follow = self.follow && session.playback().playing;
                let response = plot.show(ui, |plot_ui| {
                    self.draw(plot_ui, *graph, session, focus, &runs, &x_of);
                    plot_ui.vline(VLine::new("", now_x).color(Color32::WHITE).width(1.5));
                    if by_distance {
                        self.mark_rivals(plot_ui, *graph, session, focus, &runs, now_x * 1000.0);
                    }
                    if follow {
                        let bounds = plot_ui.plot_bounds();
                        let half = bounds.width() / 2.0;
                        plot_ui.set_plot_bounds_x(now_x - half..=now_x + half);
                    }
                    plot_ui.pointer_coordinate()
                });
                if response.response.clicked() {
                    seek = response.inner.map(|p| {
                        if by_distance {
                            time_at_distance(&session.ghost(focus).samples, &focus_totals, (p.x * 1000.0) as f32) as f64
                        } else {
                            p.x * 1000.0
                        }
                    });
                }
            }
        });

        if let Some(t) = seek {
            session.pause();
            session.seek(t);
        }
    }
}

impl TelemetryModule {
    fn cache_gaps(&mut self, session: &Session, focus: usize, runs: &[usize]) {
        // Drop pairs whose runs were closed, so a later file can never pick up a stale series
        let live: std::collections::HashSet<RunKey> = (0..session.runs().len()).map(|r| run_key(session, r)).collect();
        self.gaps.retain(|(a, b), _| live.contains(a) && live.contains(b));
        let focus_key = run_key(session, focus);
        for &run in runs.iter().filter(|&&r| r != focus) {
            let key = (focus_key, run_key(session, run));
            self.gaps.entry(key).or_insert_with(|| gap_series(&session.ghost(focus).samples, &session.ghost(run).samples));
        }
    }

    /// Each rival's value where it was at the focused run's distance, as a dot on the cursor
    fn mark_rivals(&self, plot_ui: &mut egui_plot::PlotUi, graph: Graph, session: &Session, focus: usize, runs: &[usize], meters: f64) {
        for &run in runs.iter().filter(|&&r| r != focus) {
            let Some(totals) = self.totals.get(&run_key(session, run)) else { continue };
            let samples = &session.ghost(run).samples;
            let t = time_at_distance(samples, totals, meters as f32);
            let track = session.runs()[run].track.as_ref();
            let value = match graph {
                Graph::Speed => speed_at(samples, t).map(f64::from),
                Graph::Rpm => sample_at(samples, t).map(|s| s.rpm as f64),
                Graph::Steering => track.map(|tr| tr.steer_at(t as i32) as f64),
                Graph::Pedals => track.map(|tr| {
                    let brake = tr.value_at(Channel::Brake, t as i32);
                    if brake > 0.0 { -brake as f64 } else { tr.value_at(Channel::Gas, t as i32) as f64 }
                }),
                Graph::Gap | Graph::Wheels => None,
            };
            if let Some(y) = value {
                plot_ui.points(Points::new("", vec![[meters / 1000.0, y]]).radius(4.0).color(session.runs()[run].color));
            }
        }
    }

    fn draw(&self, plot_ui: &mut egui_plot::PlotUi, graph: Graph, session: &Session, focus: usize, runs: &[usize], x_of: &dyn Fn(usize, f64) -> f64) {
        for &run in runs {
            let r = &session.runs()[run];
            let ghost = session.ghost(run);
            let color = r.color;
            let name = r.label.as_str();
            let width = if run == focus { 2.0 } else { 1.3 };
            let sampled = |f: &dyn Fn(&tmreplay::Sample) -> f64| -> PlotPoints<'static> {
                ghost.samples.iter().map(|s| [x_of(run, s.time_ms as f64), f(s)]).collect::<Vec<_>>().into()
            };
            // Points built in seconds moved onto the axis
            let placed = |run: usize, points: Vec<[f64; 2]>| -> PlotPoints<'static> {
                points.into_iter().map(|[t, v]| [x_of(run, t * 1000.0), v]).collect::<Vec<_>>().into()
            };
            match graph {
                Graph::Speed => plot_ui.line(Line::new(name, sampled(&|s| s.speed_kmh as f64)).color(color).width(width)),
                Graph::Gap => {
                    if run == focus {
                        plot_ui.line(Line::new(name, placed(focus, vec![[0.0, 0.0], [ghost.duration_ms() as f64 / 1000.0, 0.0]])).color(color).width(1.0));
                    } else if let Some(pieces) = self.gaps.get(&(run_key(session, focus), run_key(session, run))) {
                        // Gaps are sampled along the focused run, so they sit at its distances
                        for piece in pieces {
                            plot_ui.line(Line::new(name, placed(focus, piece.clone())).color(color).width(width));
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
                        plot_ui.line(Line::new(name, placed(run, step_points(&steps, track.end_ms, 1.0))).color(color).width(width));
                    }
                    if run == focus {
                        let car = format!("{name} car");
                        plot_ui.line(Line::new(car, sampled(&|s| s.steer as f64)).color(color.gamma_multiply(0.6)).style(LineStyle::dashed_dense()).width(1.0));
                    }
                }
                Graph::Pedals => {
                    if let Some(track) = &r.track {
                        plot_ui.line(Line::new(name, placed(run, step_points(track.steps(Channel::Gas), track.end_ms, 1.0))).color(color).width(width));
                        let brake = format!("{name} brake");
                        plot_ui.line(Line::new(brake, placed(run, step_points(track.steps(Channel::Brake), track.end_ms, -1.0))).color(color).width(width));
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
