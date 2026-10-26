// ABOUTME: RunReportModule UI: the sector table for the focused run's map, the focused run's braking zones and turns
// ABOUTME: (every row jumps the clock there), and saving its inputs, a TAS script, or its telemetry to files.

use egui::{Color32, RichText};
use tmreplay::format_race_time;
use tmreplay::report::{events, EventKind};
use tmreplay::sectors::{best_sectors, boundaries, boundary_times, sector_durations, Boundary};
use tmtools_core::{color_swatch, setting, Module, RenderState, Session, Settings};

const BEST: Color32 = Color32::from_rgb(90, 220, 120);
const LOSS: Color32 = Color32::from_rgb(240, 120, 110);
const BRAKE: Color32 = Color32::from_rgb(229, 72, 77);
const STEER: Color32 = Color32::from_rgb(255, 210, 63);

/// A sector length in seconds with hundredths, as race times show it
fn seconds(ms: f32) -> String {
    format!("{:.2}", (ms / 10.0).floor() / 100.0)
}

/// "+0.12" for time lost against the best
fn loss(ms: f32) -> String {
    format!("+{:.2}", (ms / 10.0).round() / 100.0)
}

pub struct RunReportModule {
    /// Which kinds of events the braking and turns table lists: brake, left, right
    shown: [bool; 3],
    /// What the last export did
    export_status: Option<String>,
}

impl Default for RunReportModule {
    fn default() -> Self {
        Self { shown: [true; 3], export_status: None }
    }
}

#[derive(Clone, Copy)]
enum Export {
    InputsCsv,
    TasScript,
    TelemetryCsv,
}

impl Module for RunReportModule {
    fn name(&self) -> &str {
        "Run Report"
    }

    fn description(&self) -> &str {
        "Sector times with the theoretical best, every braking zone and turn, and exports of the focused run"
    }

    fn save_settings(&self, s: &mut Settings) {
        for (i, shown) in self.shown.iter().enumerate() {
            s.insert(format!("runreport.events{i}"), shown.to_string());
        }
    }

    fn load_settings(&mut self, s: &Settings) {
        for (i, shown) in self.shown.iter_mut().enumerate() {
            *shown = setting(s, &format!("runreport.events{i}")).unwrap_or(*shown);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, session: &mut Session, _wgpu: Option<&RenderState>) {
        let Some(focus) = session.focus else {
            ui.centered_and_justified(|ui| ui.label("Open a replay to see its report."));
            return;
        };
        let seek = egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                self.export_section(ui, session, focus);
                ui.add_space(16.0);
                let sector = sector_table(ui, session, focus);
                ui.add_space(16.0);
                let event = self.event_table(ui, session, focus);
                sector.or(event)
            })
            .inner;
        if let Some(t) = seek {
            session.playback.playing = false;
            session.seek(t as f64);
        }
    }
}

impl RunReportModule {
    fn export_section(&mut self, ui: &mut egui::Ui, session: &Session, focus: usize) {
        tmtools_core::toolbar(ui, |ui| {
            ui.strong("Export the focused run:");
            let has_inputs = session.runs[focus].track.is_some();
            let mut chosen = None;
            if ui.add_enabled(has_inputs, egui::Button::new("Inputs CSV...")).on_hover_text("Every recorded input event with its time, tick, and value").clicked() {
                chosen = Some(Export::InputsCsv);
            }
            if ui.add_enabled(has_inputs, egui::Button::new("TAS script...")).on_hover_text("The inputs as a TMInterface-style script: press ranges, steer and gas values, respawns").clicked() {
                chosen = Some(Export::TasScript);
            }
            if ui.button("Telemetry CSV...").on_hover_text("One row per sample: position, speed, pedals, steering, RPM, wheel contact").clicked() {
                chosen = Some(Export::TelemetryCsv);
            }
            if let Some(kind) = chosen {
                self.export_status = Some(save(session, focus, kind));
            }
            if let Some(status) = &self.export_status {
                ui.label(RichText::new(status).small().weak());
            }
        });
    }

    /// The focused run's braking zones and turns; returns a time to seek to when a row's start is clicked
    fn event_table(&mut self, ui: &mut egui::Ui, session: &Session, focus: usize) -> Option<f32> {
        ui.heading("Braking and turns");
        let Some(track) = session.runs[focus].track.as_ref() else {
            ui.label("The focused run has no recorded inputs.");
            return None;
        };
        ui.horizontal(|ui| {
            for (i, label) in ["Braking", "Left turns", "Right turns"].into_iter().enumerate() {
                ui.checkbox(&mut self.shown[i], label);
            }
        });
        ui.label(
            RichText::new(format!(
                "Inputs of one kind less than {:.2} s apart count as one event, so taps make one turn. Speeds are where the event starts, its slowest point, and where it ends.",
                tmreplay::report::MERGE_GAP_MS as f32 / 1000.0
            ))
            .small()
            .weak(),
        );

        let samples = &session.ghost(focus).samples;
        let now = session.tick_ms();
        let shown = |k: EventKind| self.shown[match k {
            EventKind::Brake => 0,
            EventKind::Left => 1,
            EventKind::Right => 2,
        }];
        let mut seek = None;
        egui::Grid::new("runreport-events").striped(true).num_columns(9).show(ui, |ui| {
            for header in ["Start", "", "Length", "Entry", "Slowest", "Exit", "Lost", "Brake ticks", "Steering"] {
                ui.strong(header);
            }
            ui.end_row();
            for e in events(track, samples).into_iter().filter(|e| shown(e.kind)) {
                let current = e.start_ms <= now && now < e.end_ms;
                let start = RichText::new(format_race_time(e.start_ms as i64));
                if ui.link(if current { start.strong() } else { start }).on_hover_text("Jump to the start of this event").clicked() {
                    seek = Some(e.start_ms as f32);
                }
                ui.colored_label(if e.kind == EventKind::Brake { BRAKE } else { STEER }, e.kind.label());
                ui.label(format!("{} ({} ticks)", seconds(e.span_ms() as f32), e.span_ms() / tmreplay::track::TICK_MS));
                ui.label(format!("{:.0}", e.entry_kmh));
                if ui.link(format!("{:.0} at {}", e.min_kmh, format_race_time(e.min_at_ms as i64))).on_hover_text("Jump to the slowest point").clicked() {
                    seek = Some(e.min_at_ms as f32);
                }
                ui.label(format!("{:.0}", e.exit_kmh));
                ui.label(format!("{:.0}", (e.entry_kmh - e.min_kmh).max(0.0)));
                ui.label(if e.brake_ticks > 0 { e.brake_ticks.to_string() } else { "-".into() });
                ui.label(if e.kind == EventKind::Brake { "-".into() } else { e.pattern() });
                ui.end_row();
            }
        });
        ui.label(RichText::new("Speeds in km/h.").small().weak());
        seek
    }
}

/// Asks where to save, writes the export, and says what happened
fn save(session: &Session, focus: usize, kind: Export) -> String {
    let ghost = session.ghost(focus);
    let map = session.replay(focus).map.name.as_deref().unwrap_or("map");
    let player = ghost.nickname.as_deref().or(ghost.login.as_deref()).unwrap_or("run");
    let stem = format!("{}-{}", tmreplay::export::file_stem(map), tmreplay::export::file_stem(player));
    let (suffix, extension, filter) = match kind {
        Export::InputsCsv => ("inputs", "csv", "CSV"),
        Export::TasScript => ("inputs", "txt", "TAS script"),
        Export::TelemetryCsv => ("telemetry", "csv", "CSV"),
    };
    let Some(path) = rfd::FileDialog::new().set_file_name(format!("{stem}-{suffix}.{extension}")).add_filter(filter, &[extension]).save_file() else {
        return "Export cancelled.".into();
    };
    let content = match (kind, session.runs[focus].track.as_ref()) {
        (Export::InputsCsv, _) => tmreplay::export::inputs_csv(&ghost.inputs),
        (Export::TasScript, Some(track)) => {
            let title = format!("{} on {}", tmreplay::strip_formatting(player), tmreplay::strip_formatting(map));
            tmreplay::export::tas_script(&ghost.inputs, track, &title)
        }
        (Export::TasScript, None) => return "This run has no inputs to export.".into(),
        (Export::TelemetryCsv, _) => tmreplay::export::telemetry_csv(&ghost.samples),
    };
    match std::fs::write(&path, content) {
        Ok(()) => format!("Saved {}", path.display()),
        Err(e) => format!("Couldn't save {}: {e}", path.display()),
    }
}

/// The sector table; returns a time to seek to when a cell is clicked
fn sector_table(ui: &mut egui::Ui, session: &Session, focus: usize) -> Option<f32> {
    ui.heading("Sectors");
    let runs = session.runs_on_map_of(focus);
    let splits = session.splits_for(focus);
    let bounds = boundaries(session.ghost(focus), splits);
    if bounds.is_empty() {
        ui.label("The focused run has no checkpoints or splits to divide it into sectors.");
        return None;
    }
    ui.label(
        RichText::new(format!(
            "{} sector{} from checkpoints{}. Place more splits with Place splits on the Trajectory Map. Click a time to jump to the start of that sector.",
            bounds.len(),
            if bounds.len() == 1 { "" } else { "s" },
            if splits.is_empty() { String::new() } else { format!(" and {} placed splits", splits.len()) }
        ))
        .small()
        .weak(),
    );

    let times: Vec<Vec<Option<f32>>> = runs.iter().map(|&r| boundary_times(session.ghost(r), &bounds)).collect();
    let sectors: Vec<Vec<Option<f32>>> = times.iter().map(|t| sector_durations(t)).collect();
    let (best, theoretical) = best_sectors(&sectors);

    let mut seek = None;
    egui::Grid::new("runreport-sectors").striped(true).num_columns(bounds.len() + 2).show(ui, |ui| {
        ui.strong("Run");
        for (i, b) in bounds.iter().enumerate() {
            let ends = match b {
                Boundary::Checkpoint(c) if *c + 1 == session.ghost(focus).checkpoints.len() => "the finish".to_owned(),
                Boundary::Checkpoint(c) => format!("checkpoint {}", c + 1),
                Boundary::Point(_) => "a placed split".to_owned(),
            };
            ui.strong(format!("S{}", i + 1)).on_hover_text(format!("Ends at {ends}"));
        }
        ui.strong("Total");
        ui.end_row();

        for (row, &run) in runs.iter().enumerate() {
            ui.horizontal(|ui| {
                color_swatch(ui, session.runs[run].color);
                let label = RichText::new(&session.runs[run].label);
                ui.label(if run == focus { label.strong() } else { label });
            });
            for (i, sector) in sectors[row].iter().enumerate() {
                match (sector, best[i]) {
                    (Some(t), Some(b)) => {
                        let lost = t - b;
                        let text = if lost < 5.0 {
                            RichText::new(seconds(*t)).color(BEST).strong()
                        } else {
                            RichText::new(format!("{}  {}", seconds(*t), loss(lost))).color(LOSS)
                        };
                        let start = if i == 0 { Some(0.0) } else { times[row][i - 1] };
                        if ui.link(text).on_hover_text("Jump to the start of this sector for this run").clicked() {
                            seek = start;
                        }
                    }
                    _ => {
                        ui.label(RichText::new("-").weak());
                    }
                }
            }
            let total = session.ghost(run).race_time_ms.map_or("-".into(), |t| format_race_time(t as i64));
            ui.label(total);
            ui.end_row();
        }

        ui.label(RichText::new("Best").strong());
        for b in &best {
            ui.label(b.map_or("-".into(), seconds));
        }
        ui.label(theoretical.map_or("-".into(), |t| format_race_time(t.round() as i64)));
        ui.end_row();
    });

    if let (Some(theoretical), Some(actual)) = (theoretical, session.ghost(focus).race_time_ms) {
        let saving = actual as f32 - theoretical;
        ui.add_space(6.0);
        ui.label(format!(
            "Theoretical best {}: the best of each sector across {} run{}. The focused run is {} off it.",
            format_race_time(theoretical.round() as i64),
            runs.len(),
            if runs.len() == 1 { "" } else { "s" },
            seconds(saving.max(0.0))
        ));
    }
    seek
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sector_times_and_losses() {
        assert_eq!(seconds(12_857.0), "12.85");
        assert_eq!(loss(124.0), "+0.12");
    }
}
